//! Generating plants: lifecycle, ramping and daily commitment.

mod commitment;
mod plant;
mod solar;

pub use commitment::{BoundaryStart, CommitmentAction, CommitmentSchedule, commitment_actions};
pub use plant::{Plant, PlantStatus};
pub use solar::{draw_clearness, solar_available, sun_factor};
