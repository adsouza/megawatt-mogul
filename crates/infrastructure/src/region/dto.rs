//! Serde DTOs mirroring the RON region file. They carry raw numbers only;
//! `mapping` turns them into validated domain types. Unknown fields are
//! rejected so a typo in a hand-authored file is an error, not a silent default.

use serde::{Deserialize, Serialize};

/// A whole region file. Self-contained: no includes, no external inputs.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegionFile {
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub region_schema: Option<u32>,
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
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub wind_potential: Option<Field>,
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub wind: Option<WindDto>,
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
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GridSize {
    pub width: u16,
    pub height: u16,
}

/// A resource layer: a constant base plus Gaussian blobs, clamped to `[0, 1]`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub base: f64,
    pub blobs: Vec<Blob>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Blob {
    pub lon: f64,
    pub lat: f64,
    pub radius_deg: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum PlantKindDto {
    Nuclear,
    Coal,
    Ccgt,
    Peaker,
    Solar,
    Hydro,
    Battery,
    Wind,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub enum SitingDto {
    Anywhere,
    CoolingWater(f64),
    HydroSite(f64),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    #[serde(default)]
    pub battery: Option<BatteryDto>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DemandDto {
    pub winter: Vec<f64>,
    pub summer: Vec<f64>,
    pub shoulder: Vec<f64>,
    pub monthly: Vec<f64>,
    pub forecast_error: f64,
    pub error_persistence: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SolarDto {
    /// (sunrise, sunset) hours, one pair per month.
    pub daylight: Vec<(f64, f64)>,
    pub clearness_mean: Vec<f64>,
    pub clearness_sd: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindDto {
    pub monthly: Vec<f64>,
    pub persistence: f64,
    pub innovation_sd: f64,
    pub initial_sd: f64,
    pub band: f64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WindExtension {
    pub potential: Field,
    pub wind: WindDto,
    pub plant: PlantSpecDto,
    pub charge_end_hour: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HydroDto {
    pub monthly_flow: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegulatorDto {
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub storage_accreditation: Option<Vec<(f64, f64)>>,
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub satisfaction_target: Option<f64>,
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub return_per_satisfaction_point: Option<f64>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SatisfactionDto {
    pub starting: f64,
    pub reliability_floor: f64,
    pub price_sensitivity: f64,
    pub smoothing: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinesDto {
    pub cost_per_cell_usd: i64,
    pub cells_per_month: u32,
    pub book_life_months: u32,
}

/// The starting scenario. The home city is an id; fleet points are (lon, lat).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartDto {
    pub year: i32,
    pub month: u8,
    pub cash_usd: i64,
    pub home_city: u32,
    pub connected_cities: Vec<u32>,
    pub fleet: Vec<(PlantKindDto, f64, f64)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatteryDto {
    pub charge_mw: f64,
    pub energy_mwh: f64,
    pub charge_efficiency: f64,
    pub discharge_efficiency: f64,
    pub discharge_start_hour: u8,
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub charge_end_hour: Option<u8>,
}

// Missing fields stay distinguishable from authored values while RON uses ordinary
// `region_schema: 2` / `satisfaction_target: 90.0` syntax, including on DTO round trips.
mod present_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<T>, D::Error> {
        T::deserialize(d).map(Some)
    }
    pub fn serialize<T: Serialize, S: Serializer>(
        value: &Option<T>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => value.serialize(s),
            None => s.serialize_none(),
        }
    }
}

/// Schema-4 site irradiance: geographic Gaussian field or exact row-major cells.
/// The wire forms are mutually exclusive and keep existing Gaussian RON valid.
#[derive(Debug, Clone)]
pub enum IrradianceV4 {
    Gaussian(Field),
    Cells(Vec<f64>),
}

impl<'de> Deserialize<'de> for IrradianceV4 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename = "Field", deny_unknown_fields)]
        struct Source {
            #[serde(default, deserialize_with = "present_option::deserialize")]
            base: Option<f64>,
            #[serde(default, deserialize_with = "present_option::deserialize")]
            blobs: Option<Vec<Blob>>,
            #[serde(default, deserialize_with = "present_option::deserialize")]
            cells: Option<Vec<f64>>,
        }
        use serde::de::Error;
        let source = Source::deserialize(deserializer)?;
        match (source.base, source.blobs, source.cells) {
            (Some(base), Some(blobs), None) => Ok(Self::Gaussian(Field { base, blobs })),
            (None, None, Some(cells)) => Ok(Self::Cells(cells)),
            (_, _, Some(_)) => Err(D::Error::custom(
                "irradiance requires either base and blobs or cells, never both",
            )),
            (None, _, None) => Err(D::Error::missing_field("base")),
            (_, None, None) => Err(D::Error::missing_field("blobs")),
        }
    }
}

impl Serialize for IrradianceV4 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        match self {
            Self::Gaussian(field) => field.serialize(serializer),
            Self::Cells(cells) => {
                let mut source = serializer.serialize_struct("Field", 1)?;
                source.serialize_field("cells", cells)?;
                source.end()
            }
        }
    }
}

/// Strict schema-4 source; no legacy locale or tuning defaults.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegionFileV4 {
    pub region_schema: u32,
    pub id: String,
    pub name_key: String,
    pub bounds: Bounds,
    pub grid: GridSize,
    /// Land polygons as (lon, lat) rings.
    pub land: Vec<Vec<(f64, f64)>>,
    /// Polygons cut out of the land (lakes, bays).
    pub exclude: Vec<Vec<(f64, f64)>>,
    pub height: Field,
    pub irradiance: IrradianceV4,
    pub cooling_water: Field,
    pub hydro_potential: Field,
    pub wind_potential: Field,
    pub wind: WindDto,
    pub catalog: Vec<PlantSpecDtoV4>,
    pub cities: Vec<CityDtoV4>,
    pub system_zone: u16,
    pub time_zones: Vec<TimeZoneDto>,
    pub climates: Vec<ClimateDto>,
    pub demand_noise: DemandNoiseDto,
    pub calendar: CalendarDto,
    pub currency: CurrencyDto,
    #[serde(
        default,
        with = "present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub atlas: Option<AtlasDto>,
    pub solar: SolarDtoV4,
    pub hydro: HydroDto,
    pub frequency: FrequencyDto,
    pub regulator: RegulatorDtoV4,
    pub satisfaction: SatisfactionDto,
    pub lines: LinesDtoV4,
    pub start: StartDtoV4,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlantSpecDtoV4 {
    pub kind: PlantKindDto,
    pub capacity_mw: f64,
    pub min_stable: f64,
    pub ramp_mw_per_min: f64,
    pub startup_hours: f64,
    pub startup_cost: i64,
    pub marginal_cost_per_mwh: f64,
    pub fixed_om_per_month: i64,
    pub capital_cost: i64,
    pub build_months: u32,
    pub capacity_credit: f64,
    pub book_life_months: u32,
    pub siting: SitingDto,
    #[serde(default)]
    pub battery: Option<BatteryDto>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CityDtoV4 {
    pub id: u32,
    pub zone: u16,
    pub climate: u16,
    pub name_key: String,
    pub lon: f64,
    pub lat: f64,
    pub base_load_mw: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SolarDtoV4 {
    pub clearness_mean: Vec<f64>,
    pub clearness_sd: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegulatorDtoV4 {
    pub storage_accreditation: Vec<(f64, f64)>,
    pub satisfaction_target: f64,
    pub return_per_satisfaction_point: f64,
    pub starting_tariff_per_mwh: f64,
    pub reference_tariff_per_mwh: f64,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinesDtoV4 {
    pub cost_per_cell: i64,
    pub cells_per_month: u32,
    pub book_life_months: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartDtoV4 {
    #[serde(default)]
    pub line_names: Vec<(u32, String)>,
    pub year: i32,
    pub month: u8,
    pub cash: i64,
    pub home_city: u32,
    pub connected_cities: Vec<u32>,
    pub fleet: Vec<(PlantKindDto, f64, f64)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeZoneDto {
    pub id: u16,
    pub name_key: String,
    pub standard_offset_minutes: i32,
    pub dst_months: Vec<bool>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateDto {
    pub id: u16,
    pub shapes: Vec<(String, Vec<f64>)>,
    pub month_shapes: Vec<String>,
    pub monthly: Vec<f64>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DemandNoiseDto {
    pub forecast_error: f64,
    pub error_persistence: f64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarDto {
    pub seasons: Vec<SeasonDto>,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub enum SeasonDto {
    Winter,
    Spring,
    Summer,
    Autumn,
    Monsoon,
    PostMonsoon,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CurrencyDto {
    Usd,
    Eur,
    Cny,
    Inr,
    Rub,
    Brl,
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AtlasDto {
    pub colour: String,
}

impl<'de> Deserialize<'de> for AtlasDto {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename = "AtlasDto", deny_unknown_fields)]
        struct Wire {
            pub colour: String,
            // Archived schema-4 sources used illustration letters. Accept and discard them
            // without changing archived bytes or exposing them to presentation consumers.
            #[serde(default, rename = "letter")]
            _legacy_letter: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            colour: wire.colour,
        })
    }
}
#[derive(Debug, Clone)]
pub enum ParsedRegionFile {
    Legacy(RegionFile),
    V4(RegionFileV4),
}
impl ParsedRegionFile {
    pub fn id(&self) -> &str {
        match self {
            Self::Legacy(f) => &f.id,
            Self::V4(f) => &f.id,
        }
    }
    pub fn name_key(&self) -> &str {
        match self {
            Self::Legacy(f) => &f.name_key,
            Self::V4(f) => &f.name_key,
        }
    }
}
pub(super) struct SchemaProbe {
    pub region_schema: Option<u32>,
}

impl<'de> Deserialize<'de> for SchemaProbe {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Fields {
            #[serde(default, with = "present_option")]
            region_schema: Option<u32>,
        }
        struct ProbeVisitor;
        impl<'de> serde::de::Visitor<'de> for ProbeVisitor {
            type Value = SchemaProbe;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a region struct")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let fields =
                    Fields::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(SchemaProbe {
                    region_schema: fields.region_schema,
                })
            }
        }
        // Probe only the schema without imposing a helper's struct name. The full
        // versioned DTO then validates its own name, fields, and complete syntax.
        deserializer.deserialize_any(ProbeVisitor)
    }
}
