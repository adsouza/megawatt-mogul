//! Region tuning parameters and the starting scenario.

use super::plant_spec::PlantKind;
use crate::ids::{CellCoord, CityId};
use crate::time::SimMonth;
use crate::units::{Hertz, Megawatts, Money, Price, Ratio};

/// A demand centre.
#[derive(Debug, Clone, PartialEq)]
pub struct CityDef {
    pub id: CityId,
    pub name_key: String,
    pub cell: CellCoord,
    pub base_load: Megawatts,
}

/// Demand multipliers. Spring and autumn use `shoulder`.
#[derive(Debug, Clone, PartialEq)]
pub struct DemandShape {
    pub winter: [f64; 24],
    pub summer: [f64; 24],
    pub shoulder: [f64; 24],
    pub monthly: [f64; 12],
    pub forecast_error: Ratio,
    pub error_persistence: f64,
}

/// Solar resource parameters. `daylight` holds (sunrise, sunset) hours per month.
#[derive(Debug, Clone, PartialEq)]
pub struct SolarParams {
    pub daylight: [(f64, f64); 12],
    pub clearness_mean: [f64; 12],
    pub clearness_sd: f64,
}

/// Hydro resource parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroParams {
    pub monthly_flow: [f64; 12],
}

/// Frequency dynamics and protection thresholds. Thresholds are deviations from nominal.
#[derive(Debug, Clone, PartialEq)]
pub struct FrequencyParams {
    pub nominal: Hertz,
    pub k_inertia: f64,
    pub k_damping: f64,
    pub normal_band: Hertz,
    pub shed_below: Hertz,
    pub blackout_below: Hertz,
    pub blackout_after_shed_ticks: u32,
    pub curtail_above: Hertz,
    /// Deviation above which generation can trip on over-frequency protection.
    pub trip_above: Hertz,
    /// Consecutive ticks above `trip_above` before the trip fires.
    pub trip_after_ticks: u32,
    /// Frequency step between successive under-frequency load-shed stages.
    pub shed_stage_spacing: Hertz,
    pub shed_step: Ratio,
}

/// The rate regulator's rules.
#[derive(Debug, Clone, PartialEq)]
pub struct RegulatorParams {
    pub starting_tariff: Price,
    pub reference_tariff: Price,
    pub base_return: Ratio,
    pub min_return: Ratio,
    pub max_return: Ratio,
    pub reliability_target: Ratio,
    pub return_per_reliability: f64,
    pub return_per_cost_improvement: f64,
    pub reserve_margin: Ratio,
    pub strike_threshold: Ratio,
    pub strikes_to_revoke: u8,
    pub clean_months_to_expire: u8,
    /// Deviation from nominal beyond which a tick counts as a frequency violation.
    pub frequency_band: Hertz,
    /// Share of a month's ticks in violation that earns a frequency strike.
    pub frequency_violation_share: Ratio,
}

/// Customer satisfaction model parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct SatisfactionParams {
    pub starting: f64,
    pub reliability_floor: Ratio,
    pub price_sensitivity: f64,
    pub smoothing: Ratio,
}

/// Transmission line construction parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct LineParams {
    pub cost_per_cell: Money,
    pub cells_per_month: u32,
    pub book_life_months: u32,
}

/// A plant the player owns at the start of the game.
#[derive(Debug, Clone, PartialEq)]
pub struct StartPlantDef {
    pub kind: PlantKind,
    pub cell: CellCoord,
}

/// The starting scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct StartDef {
    pub month: SimMonth,
    pub cash: Money,
    pub substation: CellCoord,
    pub connected_cities: Vec<CityId>,
    pub fleet: Vec<StartPlantDef>,
}
