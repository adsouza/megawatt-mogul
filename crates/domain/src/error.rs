//! Domain error type.
//!
//! Messages are developer-facing diagnostics only. The domain never produces
//! player-facing text; localisation happens in an outer layer.

use thiserror::Error;

use crate::ids::{LineId, NodeId, PlantId};
use crate::time::SimDuration;
use crate::units::Megawatts;

/// Every recoverable failure the domain can report. A command returning `Err`
/// leaves the session unchanged.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DomainError {
    /// A ratio was NaN or outside `0.0..=1.0`.
    #[error("invalid ratio: {value}")]
    InvalidRatio { value: f64 },
    /// A month number was outside `1..=12`.
    #[error("invalid month: {month}")]
    InvalidMonth { month: u8 },
    /// The operation needs a plant whose output the operator can choose.
    #[error("plant {plant:?} is not dispatchable")]
    PlantNotDispatchable { plant: PlantId },
    /// The plant must be offline for this operation.
    #[error("plant {plant:?} is not offline")]
    PlantNotOffline { plant: PlantId },
    /// The plant must be online for this operation.
    #[error("plant {plant:?} is not online")]
    PlantNotOnline { plant: PlantId },
    /// The plant must be starting or online for this operation.
    #[error("plant {plant:?} is not running")]
    PlantNotRunning { plant: PlantId },
    /// A requested target is below the plant's minimum stable output.
    #[error("target below minimum stable output {min:?} for plant {plant:?}")]
    BelowMinimumStableOutput { plant: PlantId, min: Megawatts },
    /// A requested target is above the plant's capacity.
    #[error("target above capacity {max:?} for plant {plant:?}")]
    AboveCapacity { plant: PlantId, max: Megawatts },
    /// A commitment off-gap is too short to ramp down from capacity and start again.
    #[error("off-gap {gap:?} shorter than shutdown {shutdown:?} plus start-up {startup:?}")]
    CommitmentInfeasible {
        gap: SimDuration,
        shutdown: SimDuration,
        startup: SimDuration,
    },
    /// A node id is already present in the network.
    #[error("duplicate node {node:?}")]
    DuplicateNode { node: NodeId },
    /// A line referenced a node the network does not contain.
    #[error("unknown node {node:?}")]
    UnknownNode { node: NodeId },
    /// A line's endpoints were the same node.
    #[error("line from node {node:?} to itself")]
    LineToSelf { node: NodeId },
    /// The two nodes are already joined by a line, in either direction.
    #[error("nodes {from:?} and {to:?} are already joined by a line")]
    DuplicateLine { from: NodeId, to: NodeId },
    /// A line id is already present in the network.
    #[error("duplicate line id {line:?}")]
    DuplicateLineId { line: LineId },
}
