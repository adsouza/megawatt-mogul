//! Persistence values and port; no filesystem, codec or wall clock in the application.
use crate::{GameCheckpoint, RestoreError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SaveId(String);
impl SaveId {
    pub fn new(value: String) -> Result<Self, SaveError> {
        if value.len() == 32
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            Ok(Self(value))
        } else {
            Err(SaveError::InvalidId)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveRevision(pub [u8; 32]);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayCompatibility {
    pub build: String,
    pub platform: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    Manual,
    Automatic,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveMetadata {
    pub slot: SaveId,
    pub run: SaveId,
    pub kind: SaveKind,
    pub name: String,
    pub saved_at: u64,
}
impl SaveMetadata {
    pub fn validate(&self) -> Result<(), SaveError> {
        let trimmed = self.name.trim();
        if trimmed != self.name
            || trimmed.is_empty()
            || trimmed.chars().count() > 80
            || trimmed.chars().any(char::is_control)
        {
            Err(SaveError::InvalidSaveName)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SaveBundle {
    pub compatibility: ReplayCompatibility,
    pub metadata: SaveMetadata,
    pub checkpoint: GameCheckpoint,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SaveEntry {
    pub region: domain::ids::RegionId,
    pub region_name_key: String,
    pub metadata: SaveMetadata,
    pub summary: crate::CheckpointSummary,
    pub revision: SaveRevision,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SaveListing {
    pub id: Option<SaveId>,
    pub file_label: String,
    pub modified_at: u64,
    pub entry: Result<SaveEntry, SaveError>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveTarget {
    Create,
    Replace(SaveRevision),
    ConfirmDurability(SaveRevision),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReceipt {
    pub metadata: SaveMetadata,
    pub revision: SaveRevision,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SaveError {
    #[error("invalid slot id")]
    InvalidId,
    #[error("invalid save name")]
    InvalidSaveName,
    #[error("save format {recorded} unsupported (supported {supported})")]
    SaveFormatVersionMismatch { recorded: u32, supported: u32 },
    #[error("save build differs")]
    ReplayBuildMismatch,
    #[error("save platform differs")]
    PlatformMismatch,
    #[error("save checksum or length differs")]
    ChecksumMismatch,
    #[error("save limit exceeded")]
    SaveTooLarge,
    #[error("malformed save: {0}")]
    Malformed(String),
    #[error("slot changed")]
    SlotChanged,
    #[error("slot busy")]
    SlotBusy,
    #[error("durability unconfirmed for {slot:?}")]
    DurabilityUnconfirmed {
        slot: SaveId,
        revision: SaveRevision,
    },
    #[error("cancelled")]
    Cancelled,
    #[error("{stage}: {message}")]
    Io {
        stage: &'static str,
        message: String,
    },
    #[error(transparent)]
    Restore(#[from] RestoreError),
}

pub trait SaveStore: Send + Sync {
    fn compatibility(&self) -> ReplayCompatibility;
    fn allocate_id(&self) -> SaveId;
    fn now(&self) -> u64;
    fn list(&self) -> Result<Vec<SaveListing>, SaveError>;
    fn read(&self, id: &SaveId) -> Result<(SaveBundle, crate::LoadedRegion), SaveError>;
    fn write(
        &self,
        bundle: &SaveBundle,
        target: &SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError>;
}
