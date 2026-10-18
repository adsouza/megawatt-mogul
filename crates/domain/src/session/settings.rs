//! Per-game settings chosen when a session is created.

use crate::grid::NetworkModel;

/// How faithfully the simulation models the physical grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimulationFidelity {
    /// The power-flow model; copper plate by default.
    pub network: NetworkModel,
}

/// Settings fixed for the lifetime of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameSettings {
    pub fidelity: SimulationFidelity,
}
