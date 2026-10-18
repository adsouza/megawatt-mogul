#![forbid(unsafe_code)]

//! Application layer: the write side of CQRS (commands, the command pipeline
//! and the `Game` service) plus the ports that adapters implement.

pub mod command;
pub mod error;
pub mod game;
pub mod pipeline;
pub mod ports;

pub use command::{Command, CommandHandler};
pub use error::{CommandError, ReplayError};
pub use game::{Game, auto_select_region, replay};
pub use pipeline::{
    CommandLogLayer, CoreHandler, InvariantCheckLayer, TracingLayer, build_pipeline,
};
pub use ports::{
    CommandLogError, CommandLogSink, InMemoryCommandLog, LOG_FORMAT_VERSION, LoadedRegion,
    LogHeader, LogRecord, RegionCatalog, RegionFingerprint, RegionLoadError, RegionSummary,
};
