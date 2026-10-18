//! Ports: the interfaces adapters implement for region data and the command log.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domain::ids::RegionId;
use domain::region::{RegionDefinition, RegionError};
use domain::session::GameSettings;
use thiserror::Error;

use crate::command::Command;

/// Version of the command-log format. Bump when the header or record layout
/// changes in a way old readers cannot handle.
pub const LOG_FORMAT_VERSION: u32 = 1;

/// An opaque digest of a region's exact source bytes. Adapters compute it;
/// the application only stores and compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionFingerprint([u8; 32]);

impl RegionFingerprint {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A selectable region, for menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSummary {
    pub id: RegionId,
    pub name_key: String,
}

/// Why a region could not be listed or loaded.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RegionLoadError {
    #[error("region {0:?} not found")]
    NotFound(RegionId),
    #[error("i/o error loading region {region:?}: {message}")]
    Io { region: RegionId, message: String },
    #[error("parse error in region {region:?}: {message}")]
    Parse { region: RegionId, message: String },
    #[error("region {region:?} is invalid: {error}")]
    Invalid {
        region: RegionId,
        error: RegionError,
    },
    /// The source was requested as one region but declares another id, e.g. a
    /// renamed file or an archive paired with the wrong log.
    #[error("region id mismatch: requested {requested:?}, source declares {found:?}")]
    IdMismatch {
        requested: RegionId,
        found: RegionId,
    },
}

/// One region, loaded once. Adapters guarantee all three fields come from the
/// same single read of the source.
#[derive(Debug, Clone)]
pub struct LoadedRegion {
    pub definition: Arc<RegionDefinition>,
    pub source_bytes: Vec<u8>,
    pub fingerprint: RegionFingerprint,
}

/// Where regions come from.
pub trait RegionCatalog {
    fn list(&self) -> Result<Vec<RegionSummary>, RegionLoadError>;
    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError>;
}

/// Everything needed to reproduce a game from its commands.
#[derive(Debug, Clone, PartialEq)]
pub struct LogHeader {
    pub log_format_version: u32,
    pub sim_rules_version: u32,
    pub settings: GameSettings,
    /// For display only; the fingerprint identifies the region content.
    pub region: RegionId,
    pub seed: u64,
    pub region_fingerprint: RegionFingerprint,
}

/// One entry of the command log.
#[derive(Debug, Clone, PartialEq)]
pub enum LogRecord {
    Header(LogHeader),
    Command(Command),
}

/// A command-log sink failed.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("command log error: {message}")]
pub struct CommandLogError {
    pub message: String,
}

/// Where commands are recorded.
pub trait CommandLogSink {
    /// Archives the region source and records the single header. Must be
    /// called once, before `append` accepts anything.
    fn initialize(
        &mut self,
        header: &LogHeader,
        region_source: &[u8],
    ) -> Result<(), CommandLogError>;

    /// Records a later entry; fails before `initialize` has succeeded.
    fn append(&mut self, record: &LogRecord) -> Result<(), CommandLogError>;
}

#[derive(Debug, Default)]
struct InMemoryLog {
    initialized: bool,
    records: Vec<LogRecord>,
    region_source: Vec<u8>,
}

/// A command log held in memory. Clones share the same storage, so a test can
/// hand one clone to a `Game` and inspect another.
#[derive(Debug, Clone, Default)]
pub struct InMemoryCommandLog {
    inner: Arc<Mutex<InMemoryLog>>,
}

impl InMemoryCommandLog {
    fn lock(&self) -> MutexGuard<'_, InMemoryLog> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every record so far: the header first, then the commands.
    pub fn records(&self) -> Vec<LogRecord> {
        self.lock().records.clone()
    }

    /// The archived region source bytes.
    pub fn region_source(&self) -> Vec<u8> {
        self.lock().region_source.clone()
    }
}

impl CommandLogSink for InMemoryCommandLog {
    fn initialize(
        &mut self,
        header: &LogHeader,
        region_source: &[u8],
    ) -> Result<(), CommandLogError> {
        let mut log = self.lock();
        if log.initialized {
            return Err(CommandLogError {
                message: "log already initialised".to_owned(),
            });
        }
        log.initialized = true;
        log.region_source = region_source.to_vec();
        log.records.push(LogRecord::Header(header.clone()));
        Ok(())
    }

    fn append(&mut self, record: &LogRecord) -> Result<(), CommandLogError> {
        let mut log = self.lock();
        if !log.initialized {
            return Err(CommandLogError {
                message: "log not initialised".to_owned(),
            });
        }
        log.records.push(record.clone());
        Ok(())
    }
}
