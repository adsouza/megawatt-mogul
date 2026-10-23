//! Generating plants: lifecycle, ramping and daily commitment.

mod battery;
mod battery_schedule;
mod commitment;
mod hydro_schedule;
mod plant;
mod solar;

pub use battery::{BatteryFlow, BatteryMode, BatteryState, BatteryTotals};
pub(crate) use battery_schedule::fill_minutes;
pub use commitment::{BoundaryStart, CommitmentAction, CommitmentSchedule, commitment_actions};
pub use hydro_schedule::{HydroFill, water_fill};
pub use plant::{Plant, PlantStatus};
pub use solar::{draw_clearness, solar_available, sun_factor};
