//! A small, fully valid region for tests: 12 x 8 cells, three cities and a
//! three-plant starting fleet.

use super::*;
use crate::ids::{CellCoord, CityId, RegionId};
use crate::time::{SimDuration, SimMonth};
use crate::units::{Hertz, MegawattHours, Megawatts, Money, Price, RampRate, Ratio};

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
        wind_potential: r(0.8),
        cooling_water: r(if x >= 6 { 1.0 } else { 0.2 }),
        hydro_potential: r(if (x, y) == (10, 1) { 0.8 } else { 0.0 }),
        geothermal_potential: r(0.8),
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
        battery: (kind == PlantKind::Battery).then_some(BatterySpec {
            charge_power: Megawatts::new(200.0),
            energy_capacity: MegawattHours::new(800.0),
            charge_efficiency: r(0.9),
            discharge_efficiency: Ratio::ONE,
            discharge_start_hour: 6,
            charge_end_hour: 22,
        }),
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
            PlantKind::Wind,
            300.0,
            0.0,
            0.0,
            SimDuration::from_ticks(0),
            Money::ZERO,
            0.0,
            dollars(500_000),
            dollars(300_000_000),
            3,
            0.2,
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
        spec(
            PlantKind::Battery,
            200.0,
            0.0,
            0.0,
            SimDuration::from_ticks(0),
            Money::ZERO,
            0.0,
            dollars(100_000),
            dollars(100_000_000),
            2,
            0.0,
            180,
            SitingRule::Anywhere,
        ),
        spec(
            PlantKind::Geothermal,
            150.0,
            0.5,
            2.0,
            SimDuration::from_hours(12),
            dollars(100_000),
            5.0,
            dollars(300_000),
            dollars(150_000_000),
            12,
            0.95,
            360,
            SitingRule::GeothermalSite { min: r(0.3) },
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

fn climate() -> Climate {
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
    Climate {
        id: ClimateId(0),
        shapes: vec![
            scaled(|h| matches!(h, 6..=9 | 17..=21), 1.1),
            scaled(|h| matches!(h, 10..=19), 1.15),
            shoulder,
        ],
        month_shapes: [0, 0, 2, 2, 2, 1, 1, 1, 2, 2, 2, 0],
        monthly: [
            1.10, 1.05, 0.95, 0.90, 0.95, 1.05, 1.15, 1.15, 1.00, 0.92, 0.98, 1.08,
        ],
    }
}

fn solar() -> SolarParams {
    SolarParams {
        model: SolarModel::DaylightTable([
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
        ]),
        clearness_mean: [0.75; 12],
        clearness_sd: 0.15,
        annual_site_clearness: false,
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
        storage_accreditation: StorageAccreditation::new(vec![
            (2.0, r(0.65)),
            (4.0, r(0.85)),
            (6.0, r(0.93)),
            (8.0, r(0.96)),
        ])
        .expect("fixture accreditation is ordered"),
        satisfaction_target: 90.0,
        return_per_satisfaction_point: 0.0005,
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
        island_homes: Vec::new(),
        hvdc_links: Vec::new(),
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
        start_line_names: Default::default(),
        areas: None,
        hvdc: None,
        economic_zones: None,
        id: RegionId::new("tiny"),
        name_key: "region-tiny".to_owned(),
        currency: crate::units::Currency::Usd,
        geo: GeoBounds {
            west: -6.0,
            east: 6.0,
            south: 0.0,
            north: 8.0,
        },
        clock: RegionClock {
            system_zone: ZoneId(0),
            zones: vec![TimeZone {
                id: ZoneId(0),
                name_key: None,
                standard_offset_minutes: 0,
                dst_months: [false; 12],
            }],
        },
        city_locales: cities()
            .iter()
            .map(|city| {
                (
                    city.id,
                    CityLocale {
                        zone: ZoneId(0),
                        climate: ClimateId(0),
                    },
                )
            })
            .collect(),
        terrain: terrain(),
        catalog: catalog(),
        cities: cities(),
        climates: vec![climate()],
        demand_noise: DemandNoiseParams {
            forecast_error: r(0.03),
            error_persistence: 0.995,
        },
        calendar: Calendar::northern_meteorological(),
        solar: solar(),
        wind: WindParams {
            monthly: [0.6; 12],
            persistence: 0.995,
            innovation_sd: 0.015,
            initial_sd: 0.15,
            band: r(0.25),
        },
        geothermal: Some(geothermal_params()),
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

/// A wide, northern region on the Moscow system clock, sharing tiny-region tuning.
pub fn astronomical_region() -> RegionDefinition {
    RegionDefinition::try_from(astronomical_region_parts()).expect("astronomical fixture is valid")
}

/// The largest grid a region may ship (256 x 128) with varied relief, a lake, 40 cities and the
/// astronomical sun: the size Plan 7's frame-budget tests draw. Relief is a smooth deterministic
/// function of the cell so neighbours differ (many owned walls) without any randomness.
pub fn max_grid_region() -> RegionDefinition {
    const WIDTH: u16 = 256;
    const HEIGHT: u16 = 128;
    const LAKE: (f64, f64, f64) = (150.0, 70.0, 18.0);
    let mut parts = astronomical_region_parts();
    let city = |id: u32| {
        // Distinct columns 8, 14, .. 242 and rows spread over the grid, all clear of the lake.
        let x = 8 + (id as u16 - 1) * 6;
        let y = 6 + ((id as u16 - 1) * 29) % 116;
        CityDef {
            id: CityId(id),
            name_key: format!("city-max-{id}"),
            cell: CellCoord { x, y },
            base_load: Megawatts::new(150.0 + f64::from(id % 7) * 60.0),
        }
    };
    parts.cities = (1..=40).map(city).collect();
    parts.city_locales = parts
        .cities
        .iter()
        .map(|c| {
            (
                c.id,
                CityLocale {
                    zone: ZoneId(0),
                    climate: ClimateId(0),
                },
            )
        })
        .collect();
    let occupied: Vec<CellCoord> = parts
        .cities
        .iter()
        .map(|c| c.cell)
        .chain(parts.start.fleet.iter().map(|p| p.cell))
        .collect();
    let mut cells: Vec<Cell> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).map(move |x| {
                let (fx, fy) = (f64::from(x), f64::from(y));
                let relief = 0.5
                    + 0.3 * (fx * 0.21).sin() * (fy * 0.17).cos()
                    + 0.2 * (fx * 0.05 + fy * 0.09).sin();
                Cell {
                    land: x != 0 && (fx - LAKE.0).hypot(fy - LAKE.1) >= LAKE.2,
                    height: r(relief.clamp(0.0, 1.0)),
                    hydro_potential: Ratio::ZERO,
                    geothermal_potential: Ratio::ZERO,
                    ..cell(x, y)
                }
            })
        })
        .collect();
    // Cities and the starting fleet stand on land with cooling water, wherever the relief puts them.
    for at in occupied {
        let c = &mut cells[usize::from(at.y) * usize::from(WIDTH) + usize::from(at.x)];
        c.land = true;
        c.cooling_water = Ratio::ONE;
    }
    parts.terrain = TerrainGrid::new(WIDTH, HEIGHT, cells).expect("fixture grid is rectangular");
    RegionDefinition::try_from(parts).expect("max-grid fixture is valid")
}

fn astronomical_region_parts() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    parts.solar.model = SolarModel::Astronomical;
    parts.geo = GeoBounds {
        west: 30.0,
        east: 90.0,
        south: 45.0,
        north: 65.0,
    };
    parts.clock.zones[0].standard_offset_minutes = 180;
    parts
}

/// Reduced May 2033 incident: five 800 MW hydro units, three all-day 500 MW CCGTs,
/// and one CCGT whose evening run ends at midnight. No personal save/history is embedded.
/// Flat, noise-free 3,552.12 MW demand isolates the shutdown tail and protection response.
pub fn may_2033_parts() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    parts.start.month = SimMonth::new(2033, 5).expect("valid month");
    parts.start.fleet = (6..=10)
        .map(|x| StartPlantDef {
            kind: PlantKind::Hydro,
            cell: CellCoord { x, y: 1 },
        })
        .chain((2..=5).map(|x| StartPlantDef {
            kind: PlantKind::Ccgt,
            cell: CellCoord { x, y: 1 },
        }))
        .collect();
    let cells = (0..8)
        .flat_map(|y| {
            (0..12).map(move |x| {
                let mut c = cell(x, y);
                if y == 1 && (6..=10).contains(&x) {
                    c.hydro_potential = r(0.8);
                }
                c
            })
        })
        .collect();
    parts.terrain = TerrainGrid::new(12, 8, cells).expect("rectangular fixture");
    parts
        .catalog
        .iter_mut()
        .find(|s| s.kind == PlantKind::Ccgt)
        .expect("CCGT spec")
        .min_stable = r(0.3);
    parts.cities[0].base_load = Megawatts::new(2_131.272);
    parts.cities[1].base_load = Megawatts::new(1_420.848);
    parts.climates[0].shapes[2] = [1.0; 24];
    parts.climates[0].monthly = [1.0; 12];
    parts.demand_noise.forecast_error = Ratio::ZERO;
    parts
}

/// Two fictional construction markets; cents and durations are test inputs only.
pub fn economic_region_parts() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    parts.economic_zones = Some(EconomicZones {
        zones: [
            (1, "economic-zone-west", 101, 2, 3, 2),
            (2, "economic-zone-east", 202, 3, 7, 3),
        ]
        .into_iter()
        .map(|(id, name, cost, months, line, speed)| EconomicZone {
            id: EconomicZoneId(id),
            name_key: name.into(),
            plants: vec![PlantConstructionOverride {
                kind: PlantKind::Wind,
                capital_cost: Some(Money::from_cents(cost)),
                build_months: Some(months),
            }],
            hvdc: Default::default(),
            ac_lines: LineConstructionOverride {
                cost_per_cell: Some(Money::from_cents(line)),
                cells_per_month: Some(speed),
            },
        })
        .collect(),
        cells: (0..parts.terrain.height())
            .flat_map(|_| {
                (0..parts.terrain.width()).map(|x| {
                    EconomicZoneId(if x == 0 {
                        0
                    } else if x < 6 {
                        1
                    } else {
                        2
                    })
                })
            })
            .collect(),
    });
    parts
}

/// Two independently balanced areas with a small DC catalog (all tuning belongs here).
pub fn two_area_parts() -> RegionDefinitionParts {
    let mut p = tiny_region_parts();
    p.areas = Some(super::ElectricalAreas {
        areas: vec![
            super::ElectricalArea {
                id: super::AreaId(1),
                name_key: "area-east".into(),
            },
            super::ElectricalArea {
                id: super::AreaId(2),
                name_key: "area-west".into(),
            },
        ],
        cells: (0..p.terrain.height())
            .flat_map(|_| {
                (0..p.terrain.width()).map(|x| {
                    super::AreaId(if x == 0 {
                        0
                    } else if x < 6 {
                        2
                    } else {
                        1
                    })
                })
            })
            .collect(),
    });
    p.hvdc = Some(super::HvdcSpec {
        rating_mw: 200.0,
        converter_cost: Money::from_cents(101),
        cost_per_cell: Money::from_cents(7),
        converter_build_months: 2,
        ramp_mw_per_min: 20.0,
        converter_loss: 0.01,
        loss_per_cell: 0.001,
        book_life_months: 240,
        designed_hours_per_year: 4000.0,
    });
    p
}

/// A19 tuning in the test fixture; production tuning is authored in region RON.
pub fn geothermal_params() -> GeothermalParams {
    GeothermalParams {
        success_min: r(0.45),
        success_max: r(0.80),
        quality_min: r(0.15),
        quality_max: Ratio::ONE,
        exploration_months: 3,
        exploration_share: r(0.12),
        monthly_decline: r(0.002535048613836126),
        makeup_capital: dollars(15_000_000),
        makeup_months: 3,
        ceiling_step: r(0.05),
        minimum_gain: r(0.03),
    }
}

/// Fast A19 acceptance fixture: a proven starting field with accelerated reservoir decline.
pub fn geothermal_acceptance_parts() -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    let g = parts.geothermal.as_mut().expect("fixture geothermal");
    g.success_min = Ratio::ONE;
    g.success_max = Ratio::ONE;
    g.monthly_decline = r(0.04);
    parts.start.fleet.push(StartPlantDef {
        kind: PlantKind::Geothermal,
        cell: CellCoord { x: 4, y: 4 },
    });
    parts
}
