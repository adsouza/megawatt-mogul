//! Generating plants: lifecycle, ramping and daily commitment.

mod battery;
mod battery_schedule;
mod commitment;
mod hydro_schedule;
mod plant;
mod solar;
mod wind;

pub use battery::{BatteryFlow, BatteryMode, BatteryState, BatteryTotals};
pub(crate) use battery_schedule::fill_minutes;
pub use commitment::{BoundaryStart, CommitmentAction, CommitmentSchedule, commitment_actions};
pub use hydro_schedule::{HydroFill, water_fill};
pub use plant::{Plant, PlantStatus};
pub use solar::{
    SunAngles, declination_deg, draw_clearness, solar_available, sun_angles, sun_factor,
};
pub(crate) use solar::{
    annual_sun_integral, annual_sun_integral_with_clearness, astronomical_sun, mean_clearness,
};

pub use wind::{wind_available, wind_step};
