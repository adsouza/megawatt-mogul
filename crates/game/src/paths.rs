//! Where the game reads assets and writes logs. Paths always reach the game as arguments or
//! resources; only `main.rs` reads the process's command line.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use bevy::prelude::Resource;

/// The directory holding `regions/` and `locales/`.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct AssetRoot(pub PathBuf);

/// The directory command logs are written to.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct LogDir(pub PathBuf);

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

/// The command-line flags: `--assets <dir>` and `--log-dir <dir>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub assets: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
}

impl CliArgs {
    /// Parses the arguments after the program name.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, CliError> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let (flag, slot) = match arg.to_str() {
                Some("--assets") => ("--assets", &mut parsed.assets),
                Some("--log-dir") => ("--log-dir", &mut parsed.log_dir),
                _ => return Err(CliError::Unknown(arg)),
            };
            *slot = Some(args.next().ok_or(CliError::MissingValue(flag))?.into());
        }
        Ok(parsed)
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
    /// An argument that is not a known flag.
    Unknown(OsString),
}

impl CliError {
    pub const USAGE: &'static str = "usage: game [--assets <dir>] [--log-dir <dir>]";
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue(flag) => write!(f, "{flag} needs a directory"),
            Self::Unknown(arg) => write!(f, "unknown argument {}", arg.to_string_lossy()),
        }
    }
}

impl std::error::Error for CliError {}
