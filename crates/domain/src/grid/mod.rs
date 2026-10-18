//! The transmission grid: topology and connectivity.

pub mod network;

pub use network::{Line, LineStatus, Network, Node, NodeKind, distance_cells};
