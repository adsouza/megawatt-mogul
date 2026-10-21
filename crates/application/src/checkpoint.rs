//! Pure accepted-command checkpoints. Adapters verify bytes before providing a loaded region.
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
            Self::CommandCountMismatch
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
