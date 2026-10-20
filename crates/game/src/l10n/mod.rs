//! Localisation (spec §9): Fluent bundles, and the text for typed events, errors and lessons.
//!
//! The domain and application layers produce typed values only. Everything the player reads comes
//! from the `.ftl` files under `<assets>/locales/en-US/`, through [`Localizer`]. [`Localize`]
//! turns each typed value into text, with an exhaustive `match` per enum, so a new variant fails
//! to compile until it has a translation.

pub mod format;
pub mod keys;
mod localize;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use bevy::prelude::*;
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentError, FluentResource};
use unic_langid::LanguageIdentifier;

pub use self::localize::{Localize, Toast};
use crate::paths::AssetRoot;
use crate::state::{BootChecklistAppExt, BootError, finish_boot};

/// The locale this build ships: Milestone 1 is en-US only (spec §3.1).
pub const LOCALE: &str = "en-US";

/// A value passed into a Fluent message. Fluent's own `NUMBER` needs ICU data this crate set
/// doesn't ship, so numbers the player reads arrive as [`Text`](Self::Text) from
/// [`format`]. [`Number`](Self::Number) is for plural selectors and counts.
#[derive(Debug, Clone, PartialEq)]
pub enum FluentArg {
    Text(String),
    Number(f64),
}

impl From<String> for FluentArg {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for FluentArg {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<f64> for FluentArg {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

impl From<u32> for FluentArg {
    fn from(number: u32) -> Self {
        Self::Number(f64::from(number))
    }
}

/// Why the translations could not be loaded. Boot fails with this text; the game never panics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum L10nError {
    /// The locale directory or a file in it could not be read.
    Io { path: PathBuf, message: String },
    /// The locale directory holds no `.ftl` files.
    NoFiles { dir: PathBuf },
    /// A file has syntax errors.
    Parse { file: String, errors: Vec<String> },
    /// Two files define the same message.
    Duplicate { file: String, id: String },
}

impl fmt::Display for L10nError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => {
                write!(
                    f,
                    "cannot read translations at {}: {message}",
                    path.display()
                )
            }
            Self::NoFiles { dir } => write!(f, "no .ftl files in {}", dir.display()),
            Self::Parse { file, errors } => {
                write!(f, "{file} has syntax errors: {}", errors.join("; "))
            }
            Self::Duplicate { file, id } => {
                write!(f, "{file} defines {id}, which another file already defines")
            }
        }
    }
}

impl std::error::Error for L10nError {}

/// The loaded translations. A Bevy resource, so presenters read it as `Res<Localizer>`.
///
/// Lookups never fail: a missing key (or a message that cannot be formatted) returns the key
/// itself and logs one warning per key, so a missing translation is visible but never a crash.
#[derive(Resource)]
pub struct Localizer {
    bundle: FluentBundle<FluentResource>,
    /// Keys that fell back, each with why, reported once.
    problems: Mutex<BTreeMap<String, String>>,
}

impl Localizer {
    /// Loads every `.ftl` file directly under `dir`, in file-name order.
    pub fn load(dir: &Path) -> Result<Self, L10nError> {
        let io_error = |path: &Path, error: std::io::Error| L10nError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        };
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).map_err(|error| io_error(dir, error))? {
            let path = entry.map_err(|error| io_error(dir, error))?.path();
            if path.extension().is_some_and(|ext| ext == "ftl") && path.is_file() {
                files.push(path);
            }
        }
        files.sort();
        if files.is_empty() {
            return Err(L10nError::NoFiles {
                dir: dir.to_path_buf(),
            });
        }
        let mut sources = Vec::with_capacity(files.len());
        for path in files {
            let source = fs::read_to_string(&path).map_err(|error| io_error(&path, error))?;
            let name = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            sources.push((name, source));
        }
        Self::from_sources(sources)
    }

    /// Builds the translations from `(file name, source)` pairs.
    pub fn from_sources(
        sources: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, L10nError> {
        let locale: LanguageIdentifier = LOCALE
            .parse()
            .expect("the locale constant is a valid language identifier");
        let mut bundle = FluentBundle::new_concurrent(vec![locale]);
        // Isolation marks wrap each argument in invisible characters that egui draws as boxes.
        bundle.set_use_isolating(false);
        for (file, source) in sources {
            let resource =
                FluentResource::try_new(source).map_err(|(_, errors)| L10nError::Parse {
                    file: file.clone(),
                    errors: errors.iter().map(ToString::to_string).collect(),
                })?;
            if let Err(errors) = bundle.add_resource(resource) {
                let id = errors.into_iter().find_map(|error| match error {
                    FluentError::Overriding { id, .. } => Some(id),
                    _ => None,
                });
                return Err(L10nError::Duplicate {
                    file,
                    id: id.unwrap_or_default(),
                });
            }
        }
        Ok(Self {
            bundle,
            problems: Mutex::new(BTreeMap::new()),
        })
    }

    /// Whether `key` is a message in the loaded translations.
    pub fn has(&self, key: &str) -> bool {
        self.bundle.has_message(key)
    }

    /// The text of `key`, or the key itself if it has no translation.
    pub fn text(&self, key: &str) -> String {
        self.args(key, &[])
    }

    /// The text of `key` with `args`, or the key itself if it has no translation.
    pub fn args(&self, key: &str, args: &[(&str, FluentArg)]) -> String {
        let Some(pattern) = self
            .bundle
            .get_message(key)
            .and_then(|message| message.value())
        else {
            self.note(key, "no such message".to_owned());
            return key.to_owned();
        };
        let mut fluent_args = FluentArgs::new();
        for (name, arg) in args {
            match arg {
                FluentArg::Text(text) => fluent_args.set(*name, text.as_str()),
                FluentArg::Number(number) => fluent_args.set(*name, *number),
            }
        }
        let mut errors = Vec::new();
        let text = self
            .bundle
            .format_pattern(pattern, Some(&fluent_args), &mut errors);
        if !errors.is_empty() {
            let detail: Vec<String> = errors.iter().map(ToString::to_string).collect();
            self.note(key, detail.join("; "));
        }
        text.into_owned()
    }

    /// The keys that fell back so far, each with why: a missing message, or a message that could
    /// not be formatted (a missing argument, say). Empty when every lookup was clean.
    pub fn problems(&self) -> Vec<(String, String)> {
        self.lock_problems()
            .iter()
            .map(|(key, why)| (key.clone(), why.clone()))
            .collect()
    }

    fn note(&self, key: &str, why: String) {
        let mut problems = self.lock_problems();
        if !problems.contains_key(key) {
            tracing::warn!(key, why = why.as_str(), "translation problem");
            problems.insert(key.to_owned(), why);
        }
    }

    fn lock_problems(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.problems.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Loads the [`Localizer`] during `Boot`. Boot waits for it; if the files are missing or broken,
/// the failure becomes a [`BootError`] (which the boot screen shows) instead of a panic.
pub struct LocalizationPlugin;

impl Plugin for LocalizationPlugin {
    fn build(&self, app: &mut App) {
        app.require_for_boot::<Localizer>().add_systems(
            Update,
            load_localizer
                .before(finish_boot)
                .run_if(resource_exists::<AssetRoot>)
                .run_if(not(resource_exists::<Localizer>))
                .run_if(not(resource_exists::<BootError>)),
        );
    }
}

fn load_localizer(world: &mut World) {
    let dir = world.resource::<AssetRoot>().0.join("locales").join(LOCALE);
    match Localizer::load(&dir) {
        Ok(localizer) => {
            info!("loaded {LOCALE} translations");
            world.insert_resource(localizer);
        }
        Err(error) => {
            error!("{error}");
            world.insert_resource(BootError(error.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn localizer(sources: &[(&str, &str)]) -> Result<Localizer, L10nError> {
        Localizer::from_sources(
            sources
                .iter()
                .map(|(name, source)| ((*name).to_owned(), (*source).to_owned())),
        )
    }

    fn one(source: &str) -> Localizer {
        localizer(&[("test.ftl", source)]).unwrap()
    }

    #[test]
    fn text_and_args_resolve_without_bidi_isolation_marks() {
        let l10n = one("hello = Hello\ngreet = Hi { $name }, { $place }\n");

        assert_eq!(l10n.text("hello"), "Hello");
        assert_eq!(
            l10n.args(
                "greet",
                &[("name", "Ada".into()), ("place", "Grid".to_owned().into())]
            ),
            "Hi Ada, Grid"
        );
    }

    #[test]
    fn number_args_drive_plural_selectors() {
        let l10n = one("n = { $count ->\n    [one] one thing\n   *[other] { $count } things\n }\n");

        assert_eq!(l10n.args("n", &[("count", 1.0.into())]), "one thing");
        assert_eq!(l10n.args("n", &[("count", 3.0.into())]), "3 things");
    }

    #[test]
    fn missing_key_returns_the_key() {
        let l10n = one("hello = Hello\n");

        assert_eq!(l10n.text("no-such-key"), "no-such-key");
        assert_eq!(
            l10n.args("no-such-key", &[("x", 1.0.into())]),
            "no-such-key"
        );
        assert!(!l10n.has("no-such-key"));
        assert!(l10n.has("hello"));
    }

    #[test]
    fn a_missing_key_is_reported_once() {
        let l10n = one("hello = Hello\n");
        for _ in 0..3 {
            l10n.text("no-such-key");
        }

        let problems = l10n.problems();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].0, "no-such-key");
    }

    #[test]
    fn a_missing_argument_is_a_problem_but_still_renders() {
        let l10n = one("greet = Hi { $name }\n");

        let text = l10n.text("greet");

        assert!(text.starts_with("Hi "), "{text}");
        assert_eq!(l10n.problems().len(), 1);
    }

    #[test]
    fn a_clean_lookup_reports_no_problems() {
        let l10n = one("hello = Hello\n");
        l10n.text("hello");
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn syntax_errors_fail_the_load_and_name_the_file() {
        let error = localizer(&[("ok.ftl", "fine = Fine\n"), ("bad.ftl", "broken = { \n")])
            .err()
            .expect("bad ftl must not load");

        assert!(matches!(&error, L10nError::Parse { file, .. } if file == "bad.ftl"));
        assert!(error.to_string().contains("bad.ftl"), "{error}");
    }

    #[test]
    fn a_message_defined_in_two_files_fails_the_load() {
        let error = localizer(&[("a.ftl", "same = A\n"), ("b.ftl", "same = B\n")])
            .err()
            .expect("duplicate ids must not load");

        assert!(matches!(&error, L10nError::Duplicate { id, .. } if id == "same"));
    }

    #[test]
    fn load_reads_every_ftl_file_and_ignores_the_rest() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.ftl"), "from-a = A\n").unwrap();
        std::fs::write(dir.path().join("b.ftl"), "from-b = B\n").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not-ftl = nope\n").unwrap();

        let l10n = Localizer::load(dir.path()).unwrap();

        assert_eq!(l10n.text("from-a"), "A");
        assert_eq!(l10n.text("from-b"), "B");
        assert!(!l10n.has("not-ftl"));
    }

    #[test]
    fn load_fails_when_the_directory_is_missing() {
        let dir = TempDir::new().unwrap();

        let error = Localizer::load(&dir.path().join("nope")).err().unwrap();

        assert!(matches!(error, L10nError::Io { .. }), "{error}");
    }

    #[test]
    fn load_fails_when_the_directory_has_no_ftl_files() {
        let dir = TempDir::new().unwrap();

        let error = Localizer::load(dir.path()).err().unwrap();

        assert!(matches!(error, L10nError::NoFiles { .. }), "{error}");
    }

    #[test]
    fn the_localizer_can_be_a_bevy_resource() {
        fn assert_resource<T: bevy::prelude::Resource>() {}
        assert_resource::<Localizer>();
    }
}
