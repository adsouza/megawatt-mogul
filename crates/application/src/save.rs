//! Persistence values and port; no filesystem, codec or wall clock in the application.
use crate::{CommandLogSink, Game, GameCheckpoint, LoadedRegion, RestoreError};
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
/// The simulation build and target platform that wrote a save. Diagnostics only: loading accepts
/// a save from any build or platform whose state verifies (`SaveBundle::restore`).
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
    /// The build that wrote this save. A writer stamps its own; a decoded bundle carries the one
    /// recorded in the file, which may be another build's or platform's.
    pub compatibility: ReplayCompatibility,
    pub metadata: SaveMetadata,
    pub checkpoint: GameCheckpoint,
}
impl SaveBundle {
    /// Restores and verifies this save on the `running` build, as [`Game::restore`] does, whichever
    /// build recorded it. Portable state bypasses historical replay. A mismatched boundary returns no game.
    /// If the save came from another build or platform, that failure is
    /// [`SaveError::OtherVersionDiverged`], so the player learns why; other failures keep their
    /// own cause.
    pub fn restore(
        self,
        region: LoadedRegion,
        running: &ReplayCompatibility,
        sink: Box<dyn CommandLogSink>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Game, SaveError> {
        let recorded = self.compatibility;
        Game::restore(region, self.checkpoint, sink, cancelled).map_err(|error| {
            if error.is_verification_failure() && &recorded != running {
                SaveError::OtherVersionDiverged { recorded, error }
            } else {
                SaveError::Restore(error)
            }
        })
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SaveEntry {
    pub region: domain::ids::RegionId,
    pub region_name_key: String,
    pub metadata: SaveMetadata,
    pub summary: crate::CheckpointSummary,
    pub revision: SaveRevision,
    /// The build and platform that wrote the save, for diagnostics.
    pub compatibility: ReplayCompatibility,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SaveListing {
    pub id: Option<SaveId>,
    pub file_label: String,
    pub modified_at: u64,
    /// Digest of the listed bytes, retained even when a safely identified file cannot decode.
    pub revision: Option<SaveRevision>,
    /// Card metadata only; a readable card does not guarantee that the full game can restore.
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
    #[error("deletion durability unconfirmed for {slot:?}")]
    DeleteDurabilityUnconfirmed { slot: SaveId },
    #[error("cancelled")]
    Cancelled,
    #[error("{stage}: {message}")]
    Io {
        stage: &'static str,
        message: String,
    },
    #[error(transparent)]
    Restore(#[from] RestoreError),
    /// A save recorded by another build or platform whose replay here failed verification:
    /// `error` is one of [`RestoreError::is_verification_failure`]'s. Nothing was installed.
    #[error(
        "save from build {} on {} did not verify here: {error}",
        recorded.build,
        recorded.platform
    )]
    OtherVersionDiverged {
        recorded: ReplayCompatibility,
        error: RestoreError,
    },
    /// A save recorded by another build or platform whose payload parsed but which this build
    /// can't decode, such as an embedded region or command in a schema this build doesn't read.
    /// `cause` is the decoder's message. The same build's failure is `Malformed`.
    #[error(
        "save from build {} on {} could not be decoded here: {cause}",
        recorded.build,
        recorded.platform
    )]
    OtherVersionUnreadable {
        recorded: ReplayCompatibility,
        cause: String,
    },
}

pub trait SaveStore: Send + Sync {
    fn compatibility(&self) -> ReplayCompatibility;
    fn allocate_id(&self) -> SaveId;
    fn now(&self) -> u64;
    /// Lightweight metadata hints, possibly cached. Selected reads/restores validate fresh bytes.
    fn list(&self) -> Result<Vec<SaveListing>, SaveError>;
    fn read(&self, id: &SaveId) -> Result<(SaveBundle, crate::LoadedRegion), SaveError>;
    fn read_cancellable(
        &self,
        id: &SaveId,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(SaveBundle, crate::LoadedRegion), SaveError> {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let result = self.read(id);
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        result
    }
    /// Re-check a preloaded source before publishing it. File stores can hash the bytes without
    /// decoding or replaying; other adapters may use their normal listing revision.
    fn revision(&self, id: &SaveId) -> Result<SaveRevision, SaveError> {
        self.list()?
            .into_iter()
            .find(|listing| listing.id.as_ref() == Some(id))
            .ok_or(SaveError::SlotChanged)?
            .entry
            .map(|entry| entry.revision)
    }
    /// Change only the display name, preserving the recorded game, stamp and saved-at time.
    /// Reject a stale revision; publication has the same guarantees as `write`.
    fn rename(
        &self,
        id: &SaveId,
        revision: SaveRevision,
        name: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError>;
    /// Delete only these confirmed bytes under the slot lock. An already absent file is an
    /// idempotent retry; a replacement must still match `revision`. Success confirms directory
    /// durability; a post-removal sync failure is `DeleteDurabilityUnconfirmed`.
    fn delete(
        &self,
        id: &SaveId,
        revision: SaveRevision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), SaveError>;
    fn write(
        &self,
        bundle: &SaveBundle,
        target: &SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError>;
}
