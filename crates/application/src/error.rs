//! Application-level errors.

use domain::error::DomainError;
use thiserror::Error;

use crate::ports::{CommandLogError, RegionFingerprint, RegionLoadError};

/// Why a command (or starting a game) failed.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CommandError {
    /// The domain rejected the command; the session is unchanged.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// The region could not be loaded when starting a new game.
    #[error(transparent)]
    Region(#[from] RegionLoadError),
    /// The command log could not be initialised when starting a new game.
    #[error(transparent)]
    Log(#[from] CommandLogError),
}

/// Why a replay was refused or failed.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReplayError {
    #[error("log format version {recorded} unsupported (supported {supported})")]
    LogFormatVersionMismatch { recorded: u32, supported: u32 },
    #[error("simulation rules version {recorded} incompatible (engine {supported})")]
    RulesVersionMismatch { recorded: u32, supported: u32 },
    #[error("region fingerprint mismatch: expected {expected:?}, actual {actual:?}")]
    RegionFingerprintMismatch {
        expected: RegionFingerprint,
        actual: RegionFingerprint,
    },
    #[error("replayed command failed: {0}")]
    Command(#[from] CommandError),
}
