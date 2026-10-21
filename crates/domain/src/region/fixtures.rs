//! A small, fully valid region for tests: 12 x 8 cells, three cities and a
//! three-plant starting fleet.

use super::*;
use crate::ids::{CellCoord, CityId, RegionId};
use crate::time::{SimDuration, SimMonth};
use crate::units::{Hertz, Megawatts, Money, Price, RampRate, Ratio};

fn r(value: f64) -> Ratio {
    Ratio::new(value).expect("fixture ratio is within 0..=1")
}

fn dollars(amount: i64) -> Money {
    Money::from_dollars(amount)
}

fn cell(x: u16, y: u16) -> Cell {
    Cell {
        land: x != 0,
        height: r(0.3),
        irradiance: r(0.8),
        cooling_water: r(if x >= 6 { 1.0 } else { 0.2 }),
        hydro_potential: r(if (x, y) == (10, 1) { 0.8 } else { 0.0 }),
    }
}

fn terrain() -> TerrainGrid {
    let (width, height) = (12u16, 8u16);
    let cells = (0..height)
        .flat_map(|y| (0..width).map(move |x| cell(x, y)))
        .collect();
    TerrainGrid::new(width, height, cells).expect("fixture grid is rectangular")
}

#[allow(clippy::too_many_arguments)]
fn spec(
    kind: PlantKind,
    capacity: f64,
    min_stable: f64,
    ramp: f64,
    startup: SimDuration,
    startup_cost: Money,
    marginal: f64,
    fixed_om: Money,
    capital: Money,
    build_months: u32,
    credit: f64,
    book_life_months: u32,
    siting: SitingRule,
) -> PlantSpec {
    PlantSpec {
        kind,
        capacity: Megawatts::new(capacity),
        min_stable: r(min_stable),
        ramp: RampRate::mw_per_minute(ramp),
        startup,
        startup_cost,
        marginal_cost: Price::per_mwh(marginal),
        fixed_om_per_month: fixed_om,
        capital_cost: capital,
        build_months,
        capacity_credit: r(credit),
        book_life_months,
        siting,
    }
}

/// One spec per kind, in `PlantKind::ALL` order.
fn catalog() -> Vec<PlantSpec> {
    let cooled = SitingRule::CoolingWater { min: r(0.5) };
    vec![
        spec(
            PlantKind::Nuclear,
            1100.0,
            0.70,
            5.0,
            SimDuration::from_hours(48),
            dollars(1_000_000),
            10.0,
            dollars(11_000_000),
            dollars(6_000_000_000),
            84,
            0.95,
            480,
            cooled,
        ),
        spec(
            PlantKind::Coal,
            600.0,
            0.40,
            6.0,
            SimDuration::from_hours(8),
            dollars(150_000),
            30.0,
            dollars(2_000_000),
            dollars(2_000_000_000),
            48,
            0.95,
            480,
            cooled,
        ),
        spec(
            PlantKind::Ccgt,
            500.0,
            0.40,
            25.0,
            SimDuration::from_hours(2),
            dollars(40_000),
            45.0,
            dollars(600_000),
            dollars(600_000_000),
            30,
            0.95,
            360,
            SitingRule::Anywhere,
        ),
        spec(
            PlantKind::Peaker,
            200.0,
            0.20,
            40.0,
            SimDuration::from_minutes(10),
            dollars(5_000),
            150.0,
            dollars(170_000),
            dollars(200_000_000),
            12,
            0.95,
            360,
            SitingRule::Anywhere,
        ),
        // Solar output is not ramp-limited, so the ramp is effectively unlimited.
        spec(
            PlantKind::Solar,
            300.0,
            0.0,
            300.0,
            SimDuration::from_ticks(0),
            Money::ZERO,
            0.0,
            dollars(500_000),
            dollars(400_000_000),
            12,
            0.3,
            300,
            SitingRule::Anywhere,
        ),
        spec(
            PlantKind::Hydro,
            800.0,
            0.0,
            100.0,
            SimDuration::from_minutes(5),
            dollars(1_000),
            0.0,
            dollars(2_700_000),
            dollars(3_000_000_000),
            72,
            0.8,
            600,
            SitingRule::HydroSite { min: r(0.5) },
        ),
    ]
}

fn cities() -> Vec<CityDef> {
    let city = |id, name_key: &str, x, y, mw| CityDef {
        id: CityId(id),
        name_key: name_key.to_owned(),
        cell: CellCoord { x, y },
        base_load: Megawatts::new(mw),
    };
    vec![
        city(1, "city-tiny-1", 6, 4, 600.0),
        city(2, "city-tiny-2", 8, 6, 400.0),
        city(3, "city-tiny-3", 3, 2, 300.0),
    ]
}

fn demand() -> DemandShape {
    let shoulder = [
        0.70, 0.66, 0.64, 0.63, 0.64, 0.68, 0.76, 0.85, 0.90, 0.92, 0.93, 0.94, 0.94, 0.94, 0.95,
        0.96, 0.98, 1.00, 0.99, 0.96, 0.91, 0.85, 0.78, 0.73,
    ];
    let scaled = |in_window: fn(usize) -> bool, factor: f64| {
        let mut shape = shoulder;
        for (hour, value) in shape.iter_mut().enumerate() {
            if in_window(hour) {
                *value *= factor;
            }
        }
        shape
    };
    DemandShape {
        winter: scaled(|h| matches!(h, 6..=9 | 17..=21), 1.1),
        summer: scaled(|h| matches!(h, 10..=19), 1.15),
        shoulder,
        monthly: [
            1.10, 1.05, 0.95, 0.90, 0.95, 1.05, 1.15, 1.15, 1.00, 0.92, 0.98, 1.08,
        ],
        forecast_error: r(0.03),
        error_persistence: 0.995,
    }
}

fn solar() -> SolarParams {
    SolarParams {
        daylight: [
            (7.5, 17.0),
            (7.1, 17.7),
            (6.7, 18.4),
            (6.3, 19.1),
            (5.9, 19.8),
            (5.5, 20.5),
            (5.5, 20.5),
            (5.9, 19.8),
            (6.3, 19.1),
            (6.7, 18.4),
            (7.1, 17.7),
            (7.5, 17.0),
        ],
        clearness_mean: [0.75; 12],
        clearness_sd: 0.15,
    }
}

fn frequency() -> FrequencyParams {
    FrequencyParams {
        nominal: Hertz::new(60.0),
        k_inertia: 5.0,
        k_damping: 0.2,
        normal_band: Hertz::new(0.05),
        shed_below: Hertz::new(-0.5),
        blackout_below: Hertz::new(-1.5),
        blackout_after_shed_ticks: 30,
        curtail_above: Hertz::new(0.5),
        trip_above: Hertz::new(1.5),
        trip_after_ticks: 5,
        shed_stage_spacing: Hertz::new(0.2),
        shed_step: r(0.05),
    }
}

fn regulator() -> RegulatorParams {
    RegulatorParams {
        starting_tariff: Price::per_mwh(95.0),
        reference_tariff: Price::per_mwh(100.0),
        base_return: r(0.09),
        min_return: r(0.07),
        max_return: r(0.11),
        reliability_target: r(0.999),
        return_per_reliability: 2.0,
        return_per_cost_improvement: 0.05,
        reserve_margin: r(0.20),
        strike_threshold: r(0.005),
        strikes_to_revoke: 3,
        clean_months_to_expire: 3,
        frequency_band: Hertz::new(0.2),
        frequency_violation_share: r(0.05),
    }
}

fn start() -> StartDef {
    let plant = |kind, x, y| StartPlantDef {
        kind,
        cell: CellCoord { x, y },
    };
    StartDef {
        month: SimMonth::new(2027, 4).expect("valid month"),
        cash: dollars(2_000_000_000),
        home_city: CityId(1),
        connected_cities: vec![CityId(1), CityId(2)],
        fleet: vec![
            plant(PlantKind::Coal, 7, 3),
            plant(PlantKind::Ccgt, 8, 4),
            plant(PlantKind::Peaker, 6, 5),
        ],
    }
}

pub fn tiny_region_parts() -> RegionDefinitionParts {
    RegionDefinitionParts {
        id: RegionId::new("tiny"),
        name_key: "region-tiny".to_owned(),
        terrain: terrain(),
        catalog: catalog(),
        cities: cities(),
        demand: demand(),
        solar: solar(),
        hydro: HydroParams {
            monthly_flow: [0.6, 0.6, 0.9, 1.2, 1.4, 1.3, 1.0, 0.8, 0.7, 0.6, 0.6, 0.6],
        },
        frequency: frequency(),
        regulator: regulator(),
        satisfaction: SatisfactionParams {
            starting: 70.0,
            reliability_floor: r(0.95),
            price_sensitivity: 2.0,
            smoothing: r(0.2),
        },
        lines: LineParams {
            cost_per_cell: dollars(40_000_000),
            cells_per_month: 2,
            book_life_months: 600,
        },
        start: start(),
    }
}

pub fn tiny_region() -> RegionDefinition {
    RegionDefinition::try_from(tiny_region_parts()).expect("tiny region fixture is valid")
}
