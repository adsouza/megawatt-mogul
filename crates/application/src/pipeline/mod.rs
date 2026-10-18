//! The command pipeline: each cross-cutting concern is a layer implementing
//! [`CommandHandler`], assembled in one place by [`build_pipeline`].

mod command_log_layer;
mod core;
mod invariant_layer;
mod tracing_layer;

pub use command_log_layer::CommandLogLayer;
pub use core::CoreHandler;
pub use invariant_layer::InvariantCheckLayer;
pub use tracing_layer::TracingLayer;

use crate::command::CommandHandler;
use crate::ports::CommandLogSink;

/// Tracing → InvariantCheck (debug builds only) → CommandLog → Core.
pub fn build_pipeline(sink: Box<dyn CommandLogSink>) -> Box<dyn CommandHandler> {
    let handler: Box<dyn CommandHandler> = Box::new(CommandLogLayer::new(CoreHandler, sink));
    #[cfg(debug_assertions)]
    let handler: Box<dyn CommandHandler> = Box::new(InvariantCheckLayer::new(handler));
    Box::new(TracingLayer::new(handler))
}
