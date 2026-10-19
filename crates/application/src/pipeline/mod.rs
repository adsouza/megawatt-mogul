//! The command pipeline: each cross-cutting concern is a layer implementing
//! [`CommandHandler`], assembled in one place by [`build_pipeline`].

mod command_log_layer;
mod core;
mod invariant_layer;
mod tracing_layer;

pub use command_log_layer::{CommandLogLayer, LogHealth};
pub use core::CoreHandler;
pub use invariant_layer::InvariantCheckLayer;
pub use tracing_layer::TracingLayer;

use crate::command::CommandHandler;
use crate::ports::CommandLogSink;

/// Tracing → InvariantCheck (debug builds only) → CommandLog → Core, with the
/// [`LogHealth`] the command-log layer reports its first failure into.
pub fn build_pipeline(sink: Box<dyn CommandLogSink>) -> (Box<dyn CommandHandler>, LogHealth) {
    let log_layer = CommandLogLayer::new(CoreHandler, sink);
    let health = log_layer.health();
    let handler: Box<dyn CommandHandler> = Box::new(log_layer);
    #[cfg(debug_assertions)]
    let handler: Box<dyn CommandHandler> = Box::new(InvariantCheckLayer::new(handler));
    (Box::new(TracingLayer::new(handler)), health)
}
