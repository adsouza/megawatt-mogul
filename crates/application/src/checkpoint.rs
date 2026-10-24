//! Portable state and accepted-command checkpoints. Adapters verify bytes before loading regions.
use domain::session::{DayPhase, GameSession, SessionStatus};
use domain::time::SimMonth;
use thiserror::Error;

use crate::{Command, CommandError, CommandLogError, LogHeader, ReplayError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointSummary {
    pub month: SimMonth,
    pub tick: u32,
    pub phase: DayPhase,
    pub status: SessionStatus,
}

impl CheckpointSummary {
    pub fn of(session: &GameSession) -> Self {
        Self {
            month: session.clock().month(),
            tick: session.clock().tick(),
            phase: session.phase(),
            status: session.status(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameCheckpoint {
    pub snapshot: Option<PortableState>,
    pub header: LogHeader,
    pub region_source: Vec<u8>,
    pub commands: Vec<Command>,
    pub command_count: u64,
    pub state_hash: u64,
    pub summary: CheckpointSummary,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum RestoreError {
    #[error(transparent)]
    Replay(#[from] ReplayError),
    #[error("region source differs from checkpoint")]
    RegionSourceMismatch,
    #[error("region identity differs from checkpoint")]
    RegionIdMismatch,
    #[error("command count mismatch")]
    CommandCountMismatch,
    #[error("state hash mismatch")]
    StateHashMismatch,
    #[error("summary mismatch")]
    SummaryMismatch,
    #[error("command {index} failed: {error}")]
    ReplayCommandFailed { index: usize, error: CommandError },
    #[error("invalid or unsupported portable state")]
    InvalidSnapshot,
    #[error("restore cancelled")]
    Cancelled,
    #[error(transparent)]
    Log(#[from] CommandLogError),
}

impl RestoreError {
    /// Whether the replay ran and failed to reproduce the recorded boundary: its command count,
    /// end-state hash or summary differs, or a recorded command was refused. A save from another
    /// build that fails this way is one that build can't reproduce here.
    ///
    /// A decoder that finds a save's recorded count differing from its command list returns
    /// `CommandCountMismatch` before any replay: no build writes such a file (`encode_save`
    /// refuses it), so it is a writer bug or a damaged file, and keeps the same-build copy.
    pub fn is_verification_failure(&self) -> bool {
        match self {
            Self::InvalidSnapshot
            | Self::CommandCountMismatch
            | Self::StateHashMismatch
            | Self::SummaryMismatch
            | Self::ReplayCommandFailed { .. } => true,
            Self::Replay(_)
            | Self::RegionSourceMismatch
            | Self::RegionIdMismatch
            | Self::Cancelled
            | Self::Log(_) => false,
        }
    }
}

/// Versioned complete state. The journal remains an audit trail; it is not rerun to restore this
/// boundary. New rules apply only to future commands. Schema upgrades are separate from rules.
#[derive(Debug, Clone, PartialEq)]
pub struct PortableState {
    pub origin: Option<MigrationOrigin>,
    pub schema: u32,
    pub session: Vec<u8>,
    pub projections: Vec<u8>,
    pub journal_hash: u64,
}
impl PortableState {
    pub fn rules_compatible(recorded: u32, running: u32) -> bool {
        (8..=running).contains(&recorded)
    }
    /// Covers both playable state and history, with explicit boundaries between their bytes.
    pub fn state_hash(&self) -> u64 {
        self.schema
            .to_le_bytes()
            .into_iter()
            .chain((self.session.len() as u64).to_le_bytes())
            .chain(self.session.iter().copied())
            .chain((self.projections.len() as u64).to_le_bytes())
            .chain(self.projections.iter().copied())
            .fold(0xcbf2_9ce4_8422_2325, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    }
}
pub fn portable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOrigin {
    pub slot: crate::SaveId,
    pub revision: crate::SaveRevision,
    pub rules: u32,
    pub state_hash: u64,
    pub command_count: u64,
}
