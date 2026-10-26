#![forbid(unsafe_code)]

//! Application layer: the write side of CQRS (commands, the command pipeline
//! and the `Game` service), the read-side projections, and the ports that
//! adapters implement.

pub mod checkpoint;
pub mod command;
pub mod error;
pub mod game;
pub mod pipeline;
pub mod ports;
pub mod projections;
pub mod save;
pub use save::*;

pub use checkpoint::{
    CheckpointSummary, GameCheckpoint, MigrationOrigin, PortableState, RestoreError, portable_hash,
};
pub use command::{Command, CommandHandler};
pub use error::{CommandError, ReplayError};
pub use game::{Game, PendingRecording, auto_select_region, replay};
pub use pipeline::{
    CommandLogLayer, CoreHandler, InvariantCheckLayer, LogHealth, TracingLayer, build_pipeline,
};
pub use ports::{
    CommandLogError, CommandLogSink, InMemoryCommandLog, LOG_FORMAT_VERSION, LoadedRegion,
    LogHeader, LogRecord, RegionBundleProblem, RegionCatalog, RegionFingerprint, RegionListing,
    RegionLoadError, RegionSummary, SidecarProblem,
};
