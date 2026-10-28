//! Maps the serde DTOs onto the domain's parts and lets the domain validate
//! them. Everything the domain would otherwise trust (finiteness, units that
//! must not overflow, array lengths) is checked here, so hostile or garbled
//! files produce errors rather than panics.

use domain::ids::{CellCoord, CityId, RegionId};
use domain::region::{
    Calendar, CityDef, CityLocale, Climate, ClimateId, DemandNoiseParams, FrequencyParams,
    GeoBounds, HydroParams, LineParams, PlantKind, PlantSpec, RegionClock, RegionDefinition,
    RegionDefinitionParts, RegionError, RegulatorParams, SatisfactionParams, SitingRule,
    SolarModel, SolarParams, StartDef, StartPlantDef, StorageAccreditation, TimeZone, WindParams,
    ZoneId,
};
use domain::time::{SimDuration, SimMonth, TICKS_PER_HOUR};
use domain::units::{Hertz, Megawatts, Money, Price, RampRate, Ratio};

use super::dto::*;
use super::raster::{Geography, Irradiance, rasterize_geometry, to_cell_geometry};

type Result<T> = std::result::Result<T, RegionError>;

/// Largest grid accepted, in cells. Regions are authored at roughly 100 x 60;
/// the cap keeps a typo (or hostile file) from allocating gigabytes.
pub const MAX_CELLS: usize = 262_144;

/// Rasterises the terrain, maps every DTO and validates the result through
/// the domain constructor. Fails with the first problem found.
pub fn map_region(mut file: RegionFile) -> Result<RegionDefinition> {
    match file.region_schema {
        None | Some(2) => super::legacy::extend_wind(&mut file)?,
        Some(3) => {}
        Some(schema) => return Err(RegionError::UnsupportedRegionSchema(schema)),
    }
    let wind = file
        .wind
        .as_ref()
        .ok_or(RegionError::MissingField { field: "wind" })?;
    let potential = file
        .wind_potential
        .as_ref()
        .ok_or(RegionError::MissingField {
            field: "wind_potential",
        })?;
    check_field(potential, "wind_potential")?;
    check_geometry(file.geography())?;
    let locale = super::legacy::legacy_locale(&file)?;
    let mut parts = RegionDefinitionParts {
        id: RegionId::new(&file.id),
        name_key: file.name_key.clone(),
        currency: locale.currency,
        geo: GeoBounds {
            west: file.bounds.west,
            east: file.bounds.east,
            south: file.bounds.south,
            north: file.bounds.north,
        },
        clock: locale.clock,
        city_locales: locale.city_locales,
        start_line_names: Default::default(),
        areas: None,
        hvdc: None,
        economic_zones: None,
        terrain: rasterize_geometry(file.geography()),
        catalog: file
            .catalog
            .iter()
            .map(map_spec)
            .collect::<Result<Vec<_>>>()?,
        cities: file
            .cities
            .iter()
            .map(|c| map_city(file.geography(), c))
            .collect::<Result<Vec<_>>>()?,
        climates: locale.climates,
        demand_noise: DemandNoiseParams {
            forecast_error: ratio(file.demand.forecast_error, "demand.forecast_error")?,
            error_persistence: finite(file.demand.error_persistence, "demand.error_persistence")?,
        },
        calendar: locale.calendar,
        solar: map_solar(&file.solar)?,
        wind: WindParams {
            monthly: array(&wind.monthly, "wind.monthly")?,
            persistence: wind.persistence,
            innovation_sd: wind.innovation_sd,
            initial_sd: wind.initial_sd,
            band: ratio(wind.band, "wind.band")?,
        },
        geothermal: None,
        hydro: HydroParams {
            monthly_flow: array(&file.hydro.monthly_flow, "hydro.monthly_flow")?,
        },
        frequency: map_frequency(&file.frequency)?,
        regulator: map_regulator(&file.regulator, file.region_schema)?,
        satisfaction: map_satisfaction(&file.satisfaction)?,
        lines: map_lines(&file.lines)?,
        start: map_start(file.geography(), &file.start)?,
    };
    parts.catalog.push(super::legacy::geothermal_stub()?);
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

/// Whole major currency units to exact money, rejecting minor-unit overflow.
fn major_units(value: i64, field: &'static str) -> Result<Money> {
    value
        .checked_mul(100)
        .map(Money::from_cents)
        .ok_or_else(|| invalid(field))
}

/// A fixed-length array from a sequence of the wrong-or-right length.
pub(super) fn array<const N: usize>(values: &[f64], field: &'static str) -> Result<[f64; N]> {
    let array: [f64; N] = values.try_into().map_err(|_| invalid(field))?;
    for value in array {
        finite(value, field)?;
    }
    Ok(array)
}

fn check_geometry(file: Geography<'_>) -> Result<()> {
    let b = file.bounds;
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
    if !ring_ok(file.land) {
        return Err(invalid("land"));
    }
    if !ring_ok(file.exclude) {
        return Err(invalid("exclude"));
    }
    check_field(file.height, "height")?;
    match file.irradiance {
        Irradiance::Gaussian(field) => check_field(field, "irradiance")?,
        Irradiance::Cells(values) => {
            if values.len() != cells
                || values
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(invalid("irradiance.cells"));
            }
        }
    }
    for (field, name) in [
        (file.cooling_water, "cooling_water"),
        (file.hydro_potential, "hydro_potential"),
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
fn within_lon(file: Geography<'_>, lon: f64, field: &'static str) -> Result<()> {
    if (file.bounds.west..=file.bounds.east).contains(&lon) {
        Ok(())
    } else {
        Err(invalid(field))
    }
}

fn within_lat(file: Geography<'_>, lat: f64, field: &'static str) -> Result<()> {
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
        PlantKindDto::Battery => PlantKind::Battery,
        PlantKindDto::Wind => PlantKind::Wind,
        PlantKindDto::Geothermal => PlantKind::Geothermal,
    }
}

fn map_siting(siting: SitingDto) -> Result<SitingRule> {
    Ok(match siting {
        SitingDto::GeothermalSite(min) => SitingRule::GeothermalSite {
            min: ratio(min, "catalog.siting")?,
        },
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
        startup_cost: major_units(spec.startup_cost_usd, "catalog.startup_cost_usd")?,
        marginal_cost: Price::per_mwh(finite(
            spec.marginal_cost_usd_per_mwh,
            "catalog.marginal_cost_usd_per_mwh",
        )?),
        fixed_om_per_month: major_units(
            spec.fixed_om_usd_per_month,
            "catalog.fixed_om_usd_per_month",
        )?,
        capital_cost: major_units(spec.capital_cost_usd, "catalog.capital_cost_usd")?,
        build_months: spec.build_months,
        capacity_credit: ratio(spec.capacity_credit, "catalog.capacity_credit")?,
        book_life_months: spec.book_life_months,
        siting: map_siting(spec.siting)?,
        battery: spec.battery.as_ref().map(map_battery).transpose()?,
    })
}

fn map_battery(b: &BatteryDto) -> Result<domain::region::BatterySpec> {
    Ok(domain::region::BatterySpec {
        charge_power: Megawatts::new(non_negative(b.charge_mw, "battery.charge_mw")?),
        energy_capacity: domain::units::MegawattHours::new(non_negative(
            b.energy_mwh,
            "battery.energy_mwh",
        )?),
        charge_efficiency: ratio(b.charge_efficiency, "battery.charge_efficiency")?,
        discharge_efficiency: ratio(b.discharge_efficiency, "battery.discharge_efficiency")?,
        discharge_start_hour: b.discharge_start_hour,
        charge_end_hour: b.charge_end_hour.ok_or(RegionError::MissingField {
            field: "battery.charge_end_hour",
        })?,
    })
}

fn map_city(file: Geography<'_>, city: &CityDto) -> Result<CityDef> {
    within_lon(file, city.lon, "cities.lon")?;
    within_lat(file, city.lat, "cities.lat")?;
    Ok(CityDef {
        id: CityId(city.id),
        name_key: city.name_key.clone(),
        cell: to_cell_geometry(file, city.lon, city.lat),
        base_load: Megawatts::new(non_negative(city.base_load_mw, "cities.base_load_mw")?),
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
        model: SolarModel::DaylightTable(daylight),
        clearness_mean: array(&solar.clearness_mean, "solar.clearness_mean")?,
        clearness_sd: finite(solar.clearness_sd, "solar.clearness_sd")?,
        annual_site_clearness: false,
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
        shed_stage_spacing: hz(f.shed_stage_spacing_hz, "frequency.shed_stage_spacing_hz")?,
        shed_step: ratio(f.shed_step, "frequency.shed_step")?,
    })
}

fn map_regulator(r: &RegulatorDto, schema: Option<u32>) -> Result<RegulatorParams> {
    use super::legacy;
    let missing = |field| RegionError::MissingField { field };
    let (ladder, target, per_point) = match schema {
        None => {
            if r.storage_accreditation.is_some()
                || r.satisfaction_target.is_some()
                || r.return_per_satisfaction_point.is_some()
            {
                return Err(missing("region_schema"));
            }
            (
                legacy::A12_STORAGE_ACCREDITATION.as_slice(),
                legacy::A12_SATISFACTION_TARGET,
                legacy::A12_RETURN_PER_SATISFACTION_POINT,
            )
        }
        Some(2 | 3) => (
            r.storage_accreditation
                .as_deref()
                .ok_or_else(|| missing("regulator.storage_accreditation"))?,
            r.satisfaction_target
                .ok_or_else(|| missing("regulator.satisfaction_target"))?,
            r.return_per_satisfaction_point
                .ok_or_else(|| missing("regulator.return_per_satisfaction_point"))?,
        ),
        Some(schema) => return Err(RegionError::UnsupportedRegionSchema(schema)),
    };
    let storage_accreditation = StorageAccreditation::new(
        ladder
            .iter()
            .map(|&(h, c)| ratio(c, "regulator.storage_accreditation").map(|credit| (h, credit)))
            .collect::<Result<Vec<_>>>()?,
    )?;
    let price = |value, field| finite(value, field).map(Price::per_mwh);
    Ok(RegulatorParams {
        storage_accreditation,
        satisfaction_target: finite(target, "regulator.satisfaction_target")?,
        return_per_satisfaction_point: finite(
            per_point,
            "regulator.return_per_satisfaction_point",
        )?,
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
        cost_per_cell: major_units(l.cost_per_cell_usd, "lines.cost_per_cell_usd")?,
        cells_per_month: l.cells_per_month,
        book_life_months: l.book_life_months,
    })
}

fn map_start(file: Geography<'_>, start: &StartDto) -> Result<StartDef> {
    let at = |lon: f64, lat: f64, field| -> Result<CellCoord> {
        within_lon(file, lon, field)?;
        within_lat(file, lat, field)?;
        Ok(to_cell_geometry(file, lon, lat))
    };
    Ok(StartDef {
        month: SimMonth::new(start.year, start.month).map_err(|_| invalid("start.month"))?,
        cash: major_units(start.cash_usd, "start.cash_usd")?,
        home_city: CityId(start.home_city),
        island_homes: Vec::new(),
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

pub(super) fn map_spec_v4(spec: &PlantSpecDtoV4) -> Result<PlantSpec> {
    Ok(PlantSpec {
        kind: map_kind(spec.kind),
        capacity: Megawatts::new(non_negative(spec.capacity_mw, "catalog.capacity_mw")?),
        min_stable: ratio(spec.min_stable, "catalog.min_stable")?,
        ramp: RampRate::mw_per_minute(non_negative(
            spec.ramp_mw_per_min,
            "catalog.ramp_mw_per_min",
        )?),
        startup: startup_duration(spec.startup_hours)?,
        startup_cost: major_units(spec.startup_cost, "catalog.startup_cost")?,
        marginal_cost: Price::per_mwh(finite(
            spec.marginal_cost_per_mwh,
            "catalog.marginal_cost_per_mwh",
        )?),
        fixed_om_per_month: major_units(spec.fixed_om_per_month, "catalog.fixed_om_per_month")?,
        capital_cost: major_units(spec.capital_cost, "catalog.capital_cost")?,
        build_months: spec.build_months,
        capacity_credit: ratio(spec.capacity_credit, "catalog.capacity_credit")?,
        book_life_months: spec.book_life_months,
        siting: map_siting(spec.siting)?,
        battery: spec.battery.as_ref().map(map_battery).transpose()?,
    })
}

fn map_city_v4(file: Geography<'_>, city: &CityDtoV4) -> Result<CityDef> {
    within_lon(file, city.lon, "cities.lon")?;
    within_lat(file, city.lat, "cities.lat")?;
    Ok(CityDef {
        id: CityId(city.id),
        name_key: city.name_key.clone(),
        cell: to_cell_geometry(file, city.lon, city.lat),
        base_load: Megawatts::new(non_negative(city.base_load_mw, "cities.base_load_mw")?),
    })
}

fn map_lines_v4(l: &LinesDtoV4) -> Result<LineParams> {
    Ok(LineParams {
        cost_per_cell: major_units(l.cost_per_cell, "lines.cost_per_cell")?,
        cells_per_month: l.cells_per_month,
        book_life_months: l.book_life_months,
    })
}

fn map_start_v4(file: Geography<'_>, start: &StartDtoV4) -> Result<StartDef> {
    let at = |lon: f64, lat: f64, field| -> Result<CellCoord> {
        within_lon(file, lon, field)?;
        within_lat(file, lat, field)?;
        Ok(to_cell_geometry(file, lon, lat))
    };
    Ok(StartDef {
        month: SimMonth::new(start.year, start.month).map_err(|_| invalid("start.month"))?,
        cash: major_units(start.cash, "start.cash")?,
        home_city: CityId(start.home_city),
        island_homes: start.island_homes.iter().copied().map(CityId).collect(),
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

fn map_regulator_v4(r: &RegulatorDtoV4) -> Result<RegulatorParams> {
    let ladder = &r.storage_accreditation;
    let target = r.satisfaction_target;
    let per_point = r.return_per_satisfaction_point;
    let storage_accreditation = StorageAccreditation::new(
        ladder
            .iter()
            .map(|&(h, c)| ratio(c, "regulator.storage_accreditation").map(|credit| (h, credit)))
            .collect::<Result<Vec<_>>>()?,
    )?;
    let price = |value, field| finite(value, field).map(Price::per_mwh);
    Ok(RegulatorParams {
        storage_accreditation,
        satisfaction_target: finite(target, "regulator.satisfaction_target")?,
        return_per_satisfaction_point: finite(
            per_point,
            "regulator.return_per_satisfaction_point",
        )?,
        starting_tariff: price(
            r.starting_tariff_per_mwh,
            "regulator.starting_tariff_per_mwh",
        )?,
        reference_tariff: price(
            r.reference_tariff_per_mwh,
            "regulator.reference_tariff_per_mwh",
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

/// The schema-4 checks that make `rasterize_geometry(file.geography())` safe to call.
pub(super) fn check_v4_geography(file: &RegionFileV4) -> Result<()> {
    if !matches!(file.region_schema, 4 | 5) {
        return Err(RegionError::UnsupportedRegionSchema(file.region_schema));
    }
    if file.grid.width > 256 || file.grid.height > 128 {
        return Err(invalid("grid"));
    }
    check_geometry(file.geography())?;
    if file.region_schema == 5 {
        check_field(
            file.geothermal_potential
                .as_ref()
                .ok_or(RegionError::MissingField {
                    field: "geothermal_potential",
                })?,
            "geothermal_potential",
        )?;
        if file.geothermal.is_none() {
            return Err(RegionError::MissingField {
                field: "geothermal",
            });
        }
    } else if file.geothermal.is_some()
        || file.geothermal_potential.is_some()
        || file
            .catalog
            .iter()
            .any(|s| s.kind == PlantKindDto::Geothermal)
    {
        return Err(invalid("region_schema"));
    }
    check_field(&file.wind_potential, "wind_potential")
}

/// Converts authored solar units after sampling; legacy terrain bypasses conversion.
fn calibrated_terrain(file: &RegionFileV4) -> Result<domain::region::TerrainGrid> {
    let raw = rasterize_geometry(file.geography());
    if file.solar.calibration == SolarCalibrationDto::Legacy {
        return Ok(raw);
    }
    let mut cells = Vec::new();
    for y in 0..raw.height() {
        for x in 0..raw.width() {
            let mut cell = raw
                .get(CellCoord { x, y })
                .expect("raster cell exists")
                .clone();
            let value = cell.irradiance.value();
            cell.irradiance = ratio(
                match file.solar.calibration {
                    SolarCalibrationDto::AngstromPrescott => (value / 0.75).min(1.0),
                    SolarCalibrationDto::ClearSkyFraction => value,
                    SolarCalibrationDto::Legacy => value,
                },
                "irradiance",
            )?;
            cells.push(cell);
        }
    }
    domain::region::TerrainGrid::new(raw.width(), raw.height(), cells)
}

/// Maps strictly authored schema 4; optional calibration preserves archived semantics.
pub fn map_region_v4(file: RegionFileV4) -> Result<RegionDefinition> {
    check_v4_geography(&file)?;
    let mut line_names = std::collections::BTreeMap::new();
    for (city, name) in &file.start.line_names {
        if line_names.insert(CityId(*city), name.clone()).is_some() {
            return Err(invalid("start.line_names"));
        }
    }
    let currency = match file.currency {
        CurrencyDto::Usd => domain::units::Currency::Usd,
        CurrencyDto::Eur => domain::units::Currency::Eur,
        CurrencyDto::Cny => domain::units::Currency::Cny,
        CurrencyDto::Inr => domain::units::Currency::Inr,
        CurrencyDto::Rub => domain::units::Currency::Rub,
        CurrencyDto::Brl => domain::units::Currency::Brl,
        CurrencyDto::Unknown => return Err(invalid("currency")),
    };
    let seasons = file
        .calendar
        .seasons
        .iter()
        .map(|s| match s {
            SeasonDto::Winter => domain::time::Season::Winter,
            SeasonDto::Spring => domain::time::Season::Spring,
            SeasonDto::Summer => domain::time::Season::Summer,
            SeasonDto::Autumn => domain::time::Season::Autumn,
            SeasonDto::Monsoon => domain::time::Season::Monsoon,
            SeasonDto::PostMonsoon => domain::time::Season::PostMonsoon,
        })
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| invalid("calendar.seasons"))?;
    let geothermal = file.geothermal.as_ref().map(map_geothermal).transpose()?;
    let mut parts = RegionDefinitionParts {
        id: RegionId::new(&file.id),
        name_key: file.name_key.clone(),
        currency,
        geo: GeoBounds {
            west: file.bounds.west,
            east: file.bounds.east,
            south: file.bounds.south,
            north: file.bounds.north,
        },
        clock: RegionClock {
            system_zone: ZoneId(file.system_zone),
            zones: file
                .time_zones
                .iter()
                .map(|z| {
                    if z.name_key.trim().is_empty() {
                        return Err(invalid("time_zones.name_key"));
                    }
                    Ok(TimeZone {
                        id: ZoneId(z.id),
                        name_key: Some(z.name_key.clone()),
                        standard_offset_minutes: z.standard_offset_minutes,
                        dst_months: z
                            .dst_months
                            .as_slice()
                            .try_into()
                            .map_err(|_| invalid("time_zones.dst_months"))?,
                    })
                })
                .collect::<Result<_>>()?,
        },
        city_locales: file
            .cities
            .iter()
            .map(|c| {
                (
                    CityId(c.id),
                    CityLocale {
                        zone: ZoneId(c.zone),
                        climate: ClimateId(c.climate),
                    },
                )
            })
            .collect(),
        start_line_names: line_names,
        areas: map_areas(&file)?,
        hvdc: file.hvdc.as_ref().map(map_hvdc).transpose()?,
        economic_zones: map_economic_zones(file.economic_zones.as_ref())?,
        terrain: calibrated_terrain(&file)?,
        catalog: file
            .catalog
            .iter()
            .map(map_spec_v4)
            .collect::<Result<_>>()?,
        cities: file
            .cities
            .iter()
            .map(|c| map_city_v4(file.geography(), c))
            .collect::<Result<_>>()?,
        climates: file
            .climates
            .iter()
            .map(map_climate)
            .collect::<Result<_>>()?,
        demand_noise: DemandNoiseParams {
            forecast_error: ratio(
                file.demand_noise.forecast_error,
                "demand_noise.forecast_error",
            )?,
            error_persistence: finite(
                file.demand_noise.error_persistence,
                "demand_noise.error_persistence",
            )?,
        },
        calendar: Calendar { seasons },
        solar: SolarParams {
            model: SolarModel::Astronomical,
            clearness_mean: array(&file.solar.clearness_mean, "solar.clearness_mean")?,
            clearness_sd: finite(file.solar.clearness_sd, "solar.clearness_sd")?,
            annual_site_clearness: file.solar.calibration != SolarCalibrationDto::Legacy,
        },
        wind: WindParams {
            monthly: array(&file.wind.monthly, "wind.monthly")?,
            persistence: file.wind.persistence,
            innovation_sd: file.wind.innovation_sd,
            initial_sd: file.wind.initial_sd,
            band: ratio(file.wind.band, "wind.band")?,
        },
        geothermal: None,
        hydro: HydroParams {
            monthly_flow: array(&file.hydro.monthly_flow, "hydro.monthly_flow")?,
        },
        frequency: map_frequency(&file.frequency)?,
        regulator: map_regulator_v4(&file.regulator)?,
        satisfaction: map_satisfaction(&file.satisfaction)?,
        lines: map_lines_v4(&file.lines)?,
        start: map_start_v4(file.geography(), &file.start)?,
    };
    parts.geothermal = geothermal;
    if file.region_schema <= 4 {
        parts.catalog.push(super::legacy::geothermal_stub()?);
    }
    RegionDefinition::try_from(parts)
}
fn map_climate(c: &ClimateDto) -> Result<Climate> {
    let mut names = std::collections::BTreeMap::new();
    let mut shapes = Vec::new();
    for (name, shape) in &c.shapes {
        if name.trim().is_empty() || names.insert(name.as_str(), shapes.len()).is_some() {
            return Err(invalid("climates.shapes"));
        }
        shapes.push(array(shape, "climates.shapes")?);
    }
    let indices = c
        .month_shapes
        .iter()
        .map(|name| {
            names
                .get(name.as_str())
                .copied()
                .ok_or_else(|| invalid("climates.month_shapes"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Climate {
        id: ClimateId(c.id),
        shapes,
        month_shapes: indices
            .try_into()
            .map_err(|_| invalid("climates.month_shapes"))?,
        monthly: array(&c.monthly, "climates.monthly")?,
    })
}

fn map_economic_zones(
    dto: Option<&EconomicZonesDto>,
) -> Result<Option<domain::region::EconomicZones>> {
    use domain::region::{
        EconomicZone, EconomicZoneId, EconomicZones, LineConstructionOverride,
        PlantConstructionOverride,
    };
    dto.map(|dto| {
        let zones = dto
            .zones
            .iter()
            .map(|z| {
                let money = |v: i64, field| {
                    major_units(v, field).map_err(|_| RegionError::InvalidEconomicZone {
                        zone: Some(EconomicZoneId(z.id)),
                        field,
                    })
                };
                Ok(EconomicZone {
                    id: EconomicZoneId(z.id),
                    name_key: z.name_key.clone(),
                    plants: z
                        .plants
                        .iter()
                        .map(|p| {
                            Ok(PlantConstructionOverride {
                                kind: map_kind(p.kind),
                                capital_cost: p
                                    .capital_cost
                                    .map(|v| money(v, "economic_zones.plants.capital_cost"))
                                    .transpose()?,
                                build_months: p.build_months,
                            })
                        })
                        .collect::<Result<_>>()?,
                    hvdc: domain::region::HvdcConstructionOverride {
                        converter_cost_per_end: z
                            .hvdc
                            .converter_cost_per_end
                            .map(|v| money(v, "economic_zones.hvdc.converter_cost_per_end"))
                            .transpose()?,
                        converter_build_months: z.hvdc.converter_build_months,
                        cost_per_cell: z
                            .hvdc
                            .cost_per_cell
                            .map(|v| money(v, "economic_zones.hvdc.cost_per_cell"))
                            .transpose()?,
                        cells_per_month: z.hvdc.cells_per_month,
                    },
                    ac_lines: LineConstructionOverride {
                        cost_per_cell: z
                            .ac_lines
                            .cost_per_cell
                            .map(|v| money(v, "economic_zones.ac_lines.cost_per_cell"))
                            .transpose()?,
                        cells_per_month: z.ac_lines.cells_per_month,
                    },
                })
            })
            .collect::<Result<_>>()?;
        Ok(EconomicZones {
            zones,
            cells: dto.cells.iter().map(|id| EconomicZoneId(*id)).collect(),
        })
    })
    .transpose()
}

fn map_hvdc(h: &HvdcDto) -> Result<domain::region::HvdcSpec> {
    Ok(domain::region::HvdcSpec {
        rating_mw: h.rating_mw,
        converter_cost: major_units(h.converter_cost, "hvdc.converter_cost")?,
        cost_per_cell: major_units(h.cost_per_cell, "hvdc.cost_per_cell")?,
        converter_build_months: h.converter_build_months,
        ramp_mw_per_min: h.ramp_mw_per_min,
        converter_loss: h.converter_loss,
        loss_per_cell: h.loss_per_cell,
        book_life_months: h.book_life_months,
        designed_hours_per_year: h.designed_hours_per_year,
    })
}
fn map_areas(file: &RegionFileV4) -> Result<Option<domain::region::ElectricalAreas>> {
    use domain::region::{AreaId, ElectricalArea, ElectricalAreas};
    if file.areas.is_empty() {
        return Ok(None);
    }
    for area in &file.areas {
        if area.polygons.is_empty()
            || area
                .polygons
                .iter()
                .any(|p| p.len() < 3 || p.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()))
        {
            return Err(invalid("areas.polygons"));
        }
    }
    let terrain = rasterize_geometry(file.geography());
    let indices: Vec<_> = file
        .areas
        .iter()
        .map(|a| super::raster::PolygonIndex::new(&a.polygons))
        .collect();
    let mut cells = Vec::new();
    for y in 0..file.grid.height {
        for x in 0..file.grid.width {
            let cell = CellCoord { x, y };
            if !terrain.get(cell).is_some_and(|c| c.land) {
                cells.push(AreaId(0));
                continue;
            }
            let lon = file.bounds.west
                + (f64::from(x) + 0.5) / f64::from(file.grid.width)
                    * (file.bounds.east - file.bounds.west);
            let lat = file.bounds.north
                - (f64::from(y) + 0.5) / f64::from(file.grid.height)
                    * (file.bounds.north - file.bounds.south);
            let matches: Vec<_> = file
                .areas
                .iter()
                .zip(&indices)
                .filter(|(_, index)| index.contains(lon, lat))
                .map(|(area, _)| area)
                .collect();
            if matches.len() != 1 {
                return Err(invalid("areas.partition"));
            }
            cells.push(AreaId(matches[0].id));
        }
    }
    Ok(Some(ElectricalAreas {
        areas: file
            .areas
            .iter()
            .map(|a| ElectricalArea {
                id: AreaId(a.id),
                name_key: a.name_key.clone(),
            })
            .collect(),
        cells,
    }))
}

fn map_geothermal(g: &GeothermalDto) -> Result<domain::region::GeothermalParams> {
    Ok(domain::region::GeothermalParams {
        success_min: ratio(g.success_min, "geothermal.success_min")?,
        success_max: ratio(g.success_max, "geothermal.success_max")?,
        quality_min: ratio(g.quality_min, "geothermal.quality_min")?,
        quality_max: ratio(g.quality_max, "geothermal.quality_max")?,
        exploration_months: g.exploration_months,
        exploration_share: ratio(g.exploration_share, "geothermal.exploration_share")?,
        monthly_decline: ratio(g.monthly_decline, "geothermal.monthly_decline")?,
        makeup_capital: major_units(g.makeup_capital, "geothermal.makeup_capital")?,
        makeup_months: g.makeup_months,
        ceiling_step: ratio(g.ceiling_step, "geothermal.ceiling_step")?,
        minimum_gain: ratio(g.minimum_gain, "geothermal.minimum_gain")?,
    })
}
