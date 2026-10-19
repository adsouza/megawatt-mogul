//! Serde DTOs mirroring the RON region file. They carry raw numbers only;
//! `mapping` turns them into validated domain types. Unknown fields are
//! rejected so a typo in a hand-authored file is an error, not a silent default.

use serde::Deserialize;

/// A whole region file. Self-contained: no includes, no external inputs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionFile {
    pub id: String,
    pub name_key: String,
    pub bounds: Bounds,
    pub grid: GridSize,
    /// Land polygons as (lon, lat) rings.
    pub land: Vec<Vec<(f64, f64)>>,
    /// Polygons cut out of the land (lakes, bays).
    pub exclude: Vec<Vec<(f64, f64)>>,
    pub height: Field,
    pub irradiance: Field,
    pub cooling_water: Field,
    pub hydro_potential: Field,
    pub catalog: Vec<PlantSpecDto>,
    pub cities: Vec<CityDto>,
    pub demand: DemandDto,
    pub solar: SolarDto,
    pub hydro: HydroDto,
    pub frequency: FrequencyDto,
    pub regulator: RegulatorDto,
    pub satisfaction: SatisfactionDto,
    pub lines: LinesDto,
    pub start: StartDto,
}

/// Geographic extent of the grid in degrees.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridSize {
    pub width: u16,
    pub height: u16,
}

/// A resource layer: a constant base plus Gaussian blobs, clamped to `[0, 1]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub base: f64,
    pub blobs: Vec<Blob>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blob {
    pub lon: f64,
    pub lat: f64,
    pub radius_deg: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum PlantKindDto {
    Nuclear,
    Coal,
    Ccgt,
    Peaker,
    Solar,
    Hydro,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum SitingDto {
    Anywhere,
    CoolingWater(f64),
    HydroSite(f64),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlantSpecDto {
    pub kind: PlantKindDto,
    pub capacity_mw: f64,
    pub min_stable: f64,
    pub ramp_mw_per_min: f64,
    pub startup_hours: f64,
    pub startup_cost_usd: i64,
    pub marginal_cost_usd_per_mwh: f64,
    pub fixed_om_usd_per_month: i64,
    pub capital_cost_usd: i64,
    pub build_months: u32,
    pub capacity_credit: f64,
    pub book_life_months: u32,
    pub siting: SitingDto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CityDto {
    pub id: u32,
    pub name_key: String,
    pub lon: f64,
    pub lat: f64,
    pub base_load_mw: f64,
}

/// Fixed-length arrays are sequences here; `mapping` checks the lengths
/// (24 hours, 12 months).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemandDto {
    pub winter: Vec<f64>,
    pub summer: Vec<f64>,
    pub shoulder: Vec<f64>,
    pub monthly: Vec<f64>,
    pub forecast_error: f64,
    pub error_persistence: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolarDto {
    /// (sunrise, sunset) hours, one pair per month.
    pub daylight: Vec<(f64, f64)>,
    pub clearness_mean: Vec<f64>,
    pub clearness_sd: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HydroDto {
    pub monthly_flow: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrequencyDto {
    pub nominal_hz: f64,
    pub k_inertia: f64,
    pub k_damping: f64,
    pub normal_band_hz: f64,
    pub shed_below_hz: f64,
    pub blackout_below_hz: f64,
    pub blackout_after_shed_ticks: u32,
    pub curtail_above_hz: f64,
    pub trip_above_hz: f64,
    pub trip_after_ticks: u32,
    pub shed_stage_spacing_hz: f64,
    pub shed_step: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegulatorDto {
    pub starting_tariff_usd_per_mwh: f64,
    pub reference_tariff_usd_per_mwh: f64,
    pub base_return: f64,
    pub min_return: f64,
    pub max_return: f64,
    pub reliability_target: f64,
    pub return_per_reliability: f64,
    pub return_per_cost_improvement: f64,
    pub reserve_margin: f64,
    pub strike_threshold: f64,
    pub strikes_to_revoke: u8,
    pub clean_months_to_expire: u8,
    pub frequency_band_hz: f64,
    pub frequency_violation_share: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SatisfactionDto {
    pub starting: f64,
    pub reliability_floor: f64,
    pub price_sensitivity: f64,
    pub smoothing: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinesDto {
    pub cost_per_cell_usd: i64,
    pub cells_per_month: u32,
    pub book_life_months: u32,
}

/// The starting scenario. Points are (lon, lat); `mapping` snaps them to cells.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartDto {
    pub year: i32,
    pub month: u8,
    pub cash_usd: i64,
    pub substation: (f64, f64),
    pub connected_cities: Vec<u32>,
    pub fleet: Vec<(PlantKindDto, f64, f64)>,
}
