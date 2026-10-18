//! Domain error type.
//!
//! Messages are developer-facing diagnostics only. The domain never produces
//! player-facing text; localisation happens in an outer layer.

use thiserror::Error;

/// Every recoverable failure the domain can report. A command returning `Err`
/// leaves the session unchanged.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DomainError {
    /// A ratio was NaN or outside `0.0..=1.0`.
    #[error("invalid ratio: {value}")]
    InvalidRatio { value: f64 },
}
