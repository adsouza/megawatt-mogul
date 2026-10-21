//! The command line: where the game reads assets and writes logs, and the launch smoke's frame
//! limit. Paths always reach the game as arguments or resources; only `main.rs` reads the
//! process's command line.

use std::ffi::OsString;
use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

/// The directory holding `regions/` and `locales/`.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct AssetRoot(pub PathBuf);

/// The directory command logs are written to.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct LogDir(pub PathBuf);

#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct SaveDir(pub PathBuf);

/// The asset root: `arg` if given, otherwise the first existing directory of `./assets`,
/// `<exe dir>/assets` and the workspace's `assets/` (for `cargo run`).
///
/// If none exists, returns `./assets`, so the error that follows names the conventional place.
pub fn resolve_asset_root(arg: Option<PathBuf>) -> PathBuf {
    if let Some(explicit) = arg {
        return explicit;
    }
    let working_dir = PathBuf::from("assets");
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("assets")));
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    [Some(working_dir.clone()), beside_exe, Some(workspace)]
        .into_iter()
        .flatten()
        .find(|candidate| candidate.is_dir())
        .unwrap_or(working_dir)
}

/// The command-line flags: `--assets <dir>` and `--log-dir <dir>`, plus the hidden developer flag
/// `--exit-after-frames <n>` (see [`ExitAfterFrames`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub assets: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
    pub save_dir: Option<PathBuf>,
    pub exit_after_frames: Option<NonZeroU32>,
}

impl CliArgs {
    /// Parses the arguments after the program name.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let flag = match arg.to_str() {
                Some("--assets") => "--assets",
                Some("--log-dir") => "--log-dir",
                Some("--save-dir") => "--save-dir",
                Some("--exit-after-frames") => "--exit-after-frames",
                _ => return Err(CliError::Unknown(arg)),
            };
            let value = args.next().ok_or(CliError::MissingValue(flag))?;
            match flag {
                "--assets" => parsed.assets = Some(value.into()),
                "--log-dir" => parsed.log_dir = Some(value.into()),
                "--save-dir" => parsed.save_dir = Some(value.into()),
                _ => {
                    let count = value.to_str().and_then(|text| text.parse().ok());
                    let count = count.ok_or(CliError::NotACount { flag, value })?;
                    parsed.exit_after_frames = Some(count);
                }
            }
        }
        Ok(parsed)
    }

    pub fn save_dir(&self) -> PathBuf {
        self.save_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("saves"))
    }

    /// The log directory: `--log-dir`, or `./logs`.
    pub fn log_dir(&self) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("logs"))
    }
}

/// A command line that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// The flag was last, with no value after it.
    MissingValue(&'static str),
    /// The flag needs a whole number of at least 1.
    NotACount { flag: &'static str, value: OsString },
    /// An argument that is not a known flag.
    Unknown(OsString),
}

impl CliError {
    pub const USAGE: &'static str =
        "usage: game [--assets <dir>] [--log-dir <dir>] [--save-dir <dir>]";
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(flag) => write!(f, "{flag} needs a value"),
            Self::NotACount { flag, value } => write!(
                f,
                "{flag} needs a whole number of at least 1, not {}",
                value.to_string_lossy()
            ),
            Self::Unknown(arg) => write!(f, "unknown argument {}", arg.to_string_lossy()),
        }
    }
}

impl std::error::Error for CliError {}

/// The launch smoke (`--exit-after-frames <n>`): the app exits cleanly, with `AppExit::Success`,
/// at the end of its `n`th frame. In a window each frame is rendered, so a run that gets there
/// has opened the window, initialised the egui pass and drawn the menu.
///
/// A developer flag, left out of [`CliError::USAGE`]. It can't run under nextest: winit needs the
/// main thread on macOS, and CI has no display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitAfterFrames(pub NonZeroU32);

impl Plugin for ExitAfterFrames {
    fn build(&self, app: &mut App) {
        let limit = self.0.get();
        app.add_systems(
            Last,
            move |mut frames: Local<u32>, mut exit: MessageWriter<AppExit>| {
                *frames += 1;
                if *frames == limit {
                    exit.write(AppExit::Success);
                }
            },
        );
    }
}
