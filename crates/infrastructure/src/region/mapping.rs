//! Maps the serde DTOs onto the domain's parts and lets the domain validate
//! them. Everything the domain would otherwise trust (finiteness, units that
//! must not overflow, array lengths) is checked here, so hostile or garbled
//! files produce errors rather than panics.

use domain::ids::{CellCoord, CityId, RegionId};
use domain::region::{
    CityDef, DemandShape, FrequencyParams, HydroParams, LineParams, PlantKind, PlantSpec,
    RegionDefinition, RegionDefinitionParts, RegionError, RegulatorParams, SatisfactionParams,
    SitingRule, SolarParams, StartDef, StartPlantDef,
};
use domain::time::{SimDuration, SimMonth, TICKS_PER_HOUR};
use domain::units::{Hertz, Megawatts, Money, Price, RampRate, Ratio};

use super::dto::*;
use super::raster::{rasterize, to_cell};

type Result<T> = std::result::Result<T, RegionError>;

/// Largest grid accepted, in cells. Regions are authored at roughly 100 x 60;
/// the cap keeps a typo (or hostile file) from allocating gigabytes.
pub const MAX_CELLS: usize = 262_144;

/// Rasterises the terrain, maps every DTO and validates the result through
/// the domain constructor. Fails with the first problem found.
pub fn map_region(file: RegionFile) -> Result<RegionDefinition> {
    check_geometry(&file)?;
    let parts = RegionDefinitionParts {
        id: RegionId::new(&file.id),
        name_key: file.name_key.clone(),
        terrain: rasterize(&file),
        catalog: file
            .catalog
            .iter()
            .map(map_spec)
            .collect::<Result<Vec<_>>>()?,
        cities: file
            .cities
            .iter()
            .map(|c| map_city(&file, c))
            .collect::<Result<Vec<_>>>()?,
        demand: map_demand(&file.demand)?,
        solar: map_solar(&file.solar)?,
        hydro: HydroParams {
            monthly_flow: array(&file.hydro.monthly_flow, "hydro.monthly_flow")?,
        },
        frequency: map_frequency(&file.frequency)?,
        regulator: map_regulator(&file.regulator)?,
        satisfaction: map_satisfaction(&file.satisfaction)?,
        lines: map_lines(&file.lines)?,
        start: map_start(&file)?,
    };
    RegionDefinition::try_from(parts)
}

fn invalid(field: &'static str) -> RegionError {
    RegionError::InvalidShape { field }
}

fn finite(value: f64, field: &'static str) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field))
    }
}

fn non_negative(value: f64, field: &'static str) -> Result<f64> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(invalid(field))
    }
}

fn ratio(value: f64, field: &'static str) -> Result<Ratio> {
    Ratio::new(value).map_err(|_| invalid(field))
}

/// Whole dollars to exact money, rejecting amounts that overflow cents.
fn dollars(value: i64, field: &'static str) -> Result<Money> {
    value
        .checked_mul(100)
        .map(Money::from_cents)
        .ok_or_else(|| invalid(field))
}

/// A fixed-length array from a sequence of the wrong-or-right length.
fn array<const N: usize>(values: &[f64], field: &'static str) -> Result<[f64; N]> {
    let array: [f64; N] = values.try_into().map_err(|_| invalid(field))?;
    for value in array {
        finite(value, field)?;
    }
    Ok(array)
}

fn check_geometry(file: &RegionFile) -> Result<()> {
    let b = &file.bounds;
    let ordered = b.west < b.east && b.south < b.north;
    let ok = [b.west, b.east, b.south, b.north]
        .iter()
        .all(|v| v.is_finite())
        && ordered;
    if !ok {
        return Err(invalid("bounds"));
    }
    let cells = usize::from(file.grid.width) * usize::from(file.grid.height);
    if cells == 0 || cells > MAX_CELLS {
        return Err(invalid("grid"));
    }
    let ring_ok = |rings: &[Vec<(f64, f64)>]| {
        rings
            .iter()
            .flatten()
            .all(|&(lon, lat)| lon.is_finite() && lat.is_finite())
    };
    if !ring_ok(&file.land) {
        return Err(invalid("land"));
    }
    if !ring_ok(&file.exclude) {
        return Err(invalid("exclude"));
    }
    for (field, name) in [
        (&file.height, "height"),
        (&file.irradiance, "irradiance"),
        (&file.cooling_water, "cooling_water"),
        (&file.hydro_potential, "hydro_potential"),
    ] {
        check_field(field, name)?;
    }
    Ok(())
}

fn check_field(field: &Field, name: &'static str) -> Result<()> {
    let blobs_ok = field.blobs.iter().all(|b| {
        b.lon.is_finite()
            && b.lat.is_finite()
            && b.value.is_finite()
            && b.radius_deg.is_finite()
            && b.radius_deg > 0.0
    });
    if field.base.is_finite() && blobs_ok {
        Ok(())
    } else {
        Err(invalid(name))
    }
}

/// `to_cell` clamps to the grid, which would silently turn a mistyped
/// coordinate into an edge cell. Authored points must lie inside the bounds
/// (inclusive: `lon == east` maps to the last column). NaN fails too.
fn within_lon(file: &RegionFile, lon: f64, field: &'static str) -> Result<()> {
    if (file.bounds.west..=file.bounds.east).contains(&lon) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn within_lat(file: &RegionFile, lat: f64, field: &'static str) -> Result<()> {
    if (file.bounds.south..=file.bounds.north).contains(&lat) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn map_kind(kind: PlantKindDto) -> PlantKind {
    match kind {
        PlantKindDto::Nuclear => PlantKind::Nuclear,
        PlantKindDto::Coal => PlantKind::Coal,
        PlantKindDto::Ccgt => PlantKind::Ccgt,
        PlantKindDto::Peaker => PlantKind::Peaker,
        PlantKindDto::Solar => PlantKind::Solar,
        PlantKindDto::Hydro => PlantKind::Hydro,
    }
}

fn map_siting(siting: SitingDto) -> Result<SitingRule> {
    Ok(match siting {
        SitingDto::Anywhere => SitingRule::Anywhere,
        SitingDto::CoolingWater(min) => SitingRule::CoolingWater {
            min: ratio(min, "catalog.siting")?,
        },
        SitingDto::HydroSite(min) => SitingRule::HydroSite {
            min: ratio(min, "catalog.siting")?,
        },
    })
}

fn startup_duration(hours: f64) -> Result<SimDuration> {
    let ticks = (hours * f64::from(TICKS_PER_HOUR)).round();
    if (0.0..=f64::from(u32::MAX)).contains(&ticks) {
        Ok(SimDuration::from_ticks(ticks as u32))
    } else {
        Err(invalid("catalog.startup_hours"))
    }
}

fn map_spec(spec: &PlantSpecDto) -> Result<PlantSpec> {
    Ok(PlantSpec {
        kind: map_kind(spec.kind),
        capacity: Megawatts::new(non_negative(spec.capacity_mw, "catalog.capacity_mw")?),
        min_stable: ratio(spec.min_stable, "catalog.min_stable")?,
        ramp: RampRate::mw_per_minute(non_negative(
            spec.ramp_mw_per_min,
            "catalog.ramp_mw_per_min",
        )?),
        startup: startup_duration(spec.startup_hours)?,
        startup_cost: dollars(spec.startup_cost_usd, "catalog.startup_cost_usd")?,
        marginal_cost: Price::per_mwh(finite(
            spec.marginal_cost_usd_per_mwh,
            "catalog.marginal_cost_usd_per_mwh",
        )?),
        fixed_om_per_month: dollars(
            spec.fixed_om_usd_per_month,
            "catalog.fixed_om_usd_per_month",
        )?,
        capital_cost: dollars(spec.capital_cost_usd, "catalog.capital_cost_usd")?,
        build_months: spec.build_months,
        capacity_credit: ratio(spec.capacity_credit, "catalog.capacity_credit")?,
        book_life_months: spec.book_life_months,
        siting: map_siting(spec.siting)?,
    })
}

fn map_city(file: &RegionFile, city: &CityDto) -> Result<CityDef> {
    within_lon(file, city.lon, "cities.lon")?;
    within_lat(file, city.lat, "cities.lat")?;
    Ok(CityDef {
        id: CityId(city.id),
        name_key: city.name_key.clone(),
        cell: to_cell(file, city.lon, city.lat),
        base_load: Megawatts::new(non_negative(city.base_load_mw, "cities.base_load_mw")?),
    })
}

fn map_demand(demand: &DemandDto) -> Result<DemandShape> {
    Ok(DemandShape {
        winter: array(&demand.winter, "demand.winter")?,
        summer: array(&demand.summer, "demand.summer")?,
        shoulder: array(&demand.shoulder, "demand.shoulder")?,
        monthly: array(&demand.monthly, "demand.monthly")?,
        forecast_error: ratio(demand.forecast_error, "demand.forecast_error")?,
        error_persistence: finite(demand.error_persistence, "demand.error_persistence")?,
    })
}

fn map_solar(solar: &SolarDto) -> Result<SolarParams> {
    let daylight: [(f64, f64); 12] = solar
        .daylight
        .as_slice()
        .try_into()
        .map_err(|_| invalid("solar.daylight"))?;
    for (rise, set) in daylight {
        finite(rise, "solar.daylight")?;
        finite(set, "solar.daylight")?;
    }
    Ok(SolarParams {
        daylight,
        clearness_mean: array(&solar.clearness_mean, "solar.clearness_mean")?,
        clearness_sd: finite(solar.clearness_sd, "solar.clearness_sd")?,
    })
}

fn map_frequency(f: &FrequencyDto) -> Result<FrequencyParams> {
    let hz = |value, field| finite(value, field).map(Hertz::new);
    Ok(FrequencyParams {
        nominal: hz(f.nominal_hz, "frequency.nominal_hz")?,
        k_inertia: finite(f.k_inertia, "frequency.k_inertia")?,
        k_damping: finite(f.k_damping, "frequency.k_damping")?,
        normal_band: hz(f.normal_band_hz, "frequency.normal_band_hz")?,
        shed_below: hz(f.shed_below_hz, "frequency.shed_below_hz")?,
        blackout_below: hz(f.blackout_below_hz, "frequency.blackout_below_hz")?,
        blackout_after_shed_ticks: f.blackout_after_shed_ticks,
        curtail_above: hz(f.curtail_above_hz, "frequency.curtail_above_hz")?,
        trip_above: hz(f.trip_above_hz, "frequency.trip_above_hz")?,
        trip_after_ticks: f.trip_after_ticks,
        shed_step: ratio(f.shed_step, "frequency.shed_step")?,
    })
}

fn map_regulator(r: &RegulatorDto) -> Result<RegulatorParams> {
    let price = |value, field| finite(value, field).map(Price::per_mwh);
    Ok(RegulatorParams {
        starting_tariff: price(
            r.starting_tariff_usd_per_mwh,
            "regulator.starting_tariff_usd_per_mwh",
        )?,
        reference_tariff: price(
            r.reference_tariff_usd_per_mwh,
            "regulator.reference_tariff_usd_per_mwh",
        )?,
        base_return: ratio(r.base_return, "regulator.base_return")?,
        min_return: ratio(r.min_return, "regulator.min_return")?,
        max_return: ratio(r.max_return, "regulator.max_return")?,
        reliability_target: ratio(r.reliability_target, "regulator.reliability_target")?,
        return_per_reliability: finite(
            r.return_per_reliability,
            "regulator.return_per_reliability",
        )?,
        return_per_cost_improvement: finite(
            r.return_per_cost_improvement,
            "regulator.return_per_cost_improvement",
        )?,
        reserve_margin: ratio(r.reserve_margin, "regulator.reserve_margin")?,
        strike_threshold: ratio(r.strike_threshold, "regulator.strike_threshold")?,
        strikes_to_revoke: r.strikes_to_revoke,
        clean_months_to_expire: r.clean_months_to_expire,
        frequency_band: finite(r.frequency_band_hz, "regulator.frequency_band_hz")
            .map(Hertz::new)?,
        frequency_violation_share: ratio(
            r.frequency_violation_share,
            "regulator.frequency_violation_share",
        )?,
    })
}

fn map_satisfaction(s: &SatisfactionDto) -> Result<SatisfactionParams> {
    Ok(SatisfactionParams {
        starting: finite(s.starting, "satisfaction.starting")?,
        reliability_floor: ratio(s.reliability_floor, "satisfaction.reliability_floor")?,
        price_sensitivity: finite(s.price_sensitivity, "satisfaction.price_sensitivity")?,
        smoothing: ratio(s.smoothing, "satisfaction.smoothing")?,
    })
}

fn map_lines(l: &LinesDto) -> Result<LineParams> {
    Ok(LineParams {
        cost_per_cell: dollars(l.cost_per_cell_usd, "lines.cost_per_cell_usd")?,
        cells_per_month: l.cells_per_month,
        book_life_months: l.book_life_months,
    })
}

fn map_start(file: &RegionFile) -> Result<StartDef> {
    let start = &file.start;
    let at = |lon: f64, lat: f64, field| -> Result<CellCoord> {
        within_lon(file, lon, field)?;
        within_lat(file, lat, field)?;
        Ok(to_cell(file, lon, lat))
    };
    Ok(StartDef {
        month: SimMonth::new(start.year, start.month).map_err(|_| invalid("start.month"))?,
        cash: dollars(start.cash_usd, "start.cash_usd")?,
        substation: at(start.substation.0, start.substation.1, "start.substation")?,
        connected_cities: start.connected_cities.iter().copied().map(CityId).collect(),
        fleet: start
            .fleet
            .iter()
            .map(|&(kind, lon, lat)| {
                Ok(StartPlantDef {
                    kind: map_kind(kind),
                    cell: at(lon, lat, "start.fleet")?,
                })
            })
            .collect::<Result<_>>()?,
    })
}
