use super::*;
use crate::ids::{CellCoord, CityId, PlantId};
use crate::region::fixtures::{tiny_region_parts, two_area_parts};
use crate::region::{PlantKind, RegionDefinition, RegionDefinitionParts, StartPlantDef};
use crate::session::GameSettings;
use crate::units::{Megawatts, Money};
use std::sync::Arc;

/// The tiny fixture: Coal 600 MW @ $30, CCGT 500 @ $45, Peaker 200 @ $150. At its own demand the
/// coal and CCGT (1,100 MW) clear the evening peak's top band (949 MW); `scale` multiplies every
/// city's load.
fn parts_at(scale: f64) -> RegionDefinitionParts {
    let mut parts = tiny_region_parts();
    for city in &mut parts.cities {
        city.base_load = Megawatts::new(city.base_load.value() * scale);
    }
    parts
}

fn session_of(parts: RegionDefinitionParts) -> GameSession {
    GameSession::new(
        Arc::new(RegionDefinition::try_from(parts).unwrap()),
        1,
        GameSettings::default(),
    )
}

fn plant_of(s: &GameSession, kind: PlantKind) -> PlantId {
    s.plant_views()
        .iter()
        .find(|p| p.kind == kind)
        .expect("plant of kind")
        .id
}

fn first_on_after_gap(pattern: CommitmentSchedule) -> Option<u8> {
    let hours = pattern.hours();
    (0..24u8).find(|&h| hours[usize::from(h)] && !hours[usize::from((h + 23) % 24)])
}

#[test]
fn peaker_comes_off_when_the_fleet_covers_the_band_without_it() {
    let s = session_of(parts_at(1.0));
    let plan = s.automatic_commitment_plan();
    assert_eq!(plan[&plant_of(&s, PlantKind::Peaker)].hours(), [false; 24]);
    assert_eq!(
        plan[&plant_of(&s, PlantKind::Coal)],
        CommitmentSchedule::all_day()
    );
}

#[test]
fn peaker_stays_when_needed_for_the_band() {
    let s = session_of(parts_at(1.2));
    let plan = s.automatic_commitment_plan();
    let peaker = plan[&plant_of(&s, PlantKind::Peaker)];
    assert!(peaker.is_committed(17), "{:?}", peaker.hours());
}

#[test]
fn battery_spare_discharge_lets_an_evening_peaker_go() {
    let mut parts = parts_at(1.2);
    parts.start.fleet.push(StartPlantDef {
        kind: PlantKind::Battery,
        cell: CellCoord { x: 5, y: 1 },
    });
    let s = session_of(parts);
    let plan = s.automatic_commitment_plan();
    let peaker = plan[&plant_of(&s, PlantKind::Peaker)];
    assert!(!peaker.is_committed(17), "{:?}", peaker.hours());
}

#[test]
fn startup_cost_outweighing_fuel_keeps_the_plant_all_day() {
    let s = session_of(parts_at(1.2));
    let gapped = s.automatic_commitment_plan()[&plant_of(&s, PlantKind::Peaker)];
    assert_ne!(
        gapped,
        CommitmentSchedule::all_day(),
        "a cheap restart is worth a gap"
    );
    let mut parts = parts_at(1.2);
    for spec in &mut parts.catalog {
        if spec.kind == PlantKind::Peaker {
            spec.startup_cost = Money::from_dollars(10_000_000);
        }
    }
    let s = session_of(parts);
    let plan = s.automatic_commitment_plan();
    assert_eq!(
        plan[&plant_of(&s, PlantKind::Peaker)],
        CommitmentSchedule::all_day()
    );
}

#[test]
fn restart_credit_follows_the_ramp_from_minimum_output() {
    let mut hours = [true; 24];
    hours[0..6].fill(false);
    // Online at 06:00 at its 40 MW minimum, rising 2 MW a minute to its 200 MW capacity.
    let credit = |tick| restart_credit(hours, 40.0, 200.0, 2.0, tick);
    assert_eq!(credit(360), 40.0);
    assert_eq!(credit(370), 60.0);
    assert_eq!(credit(360 + 80), 200.0);
    assert_eq!(credit(1000), 200.0);
    assert_eq!(restart_credit([true; 24], 40.0, 200.0, 2.0, 360), 200.0);
}

#[test]
fn restart_ramp_keeps_a_plant_that_cannot_make_the_morning_ramp() {
    // A 10 MW minimum: a peaker restarting just as the band first needs it must then climb to
    // the afternoon's need. At 0.2 MW a minute it cannot, so it has to come back earlier.
    let restart = |ramp: f64| {
        let mut parts = parts_at(1.2);
        for spec in &mut parts.catalog {
            if spec.kind == PlantKind::Peaker {
                spec.ramp = crate::units::RampRate::mw_per_minute(ramp);
                spec.min_stable = crate::units::Ratio::new(0.05).unwrap();
            }
        }
        let s = session_of(parts);
        first_on_after_gap(s.automatic_commitment_plan()[&plant_of(&s, PlantKind::Peaker)])
    };
    let (fast, slow) = (restart(40.0), restart(0.2));
    assert!(
        matches!((fast, slow), (Some(fast), Some(slow)) if slow < fast),
        "fast {fast:?}, slow {slow:?}"
    );
}

#[test]
fn leaves_manual_and_fixed_kinds_alone() {
    let mut parts = parts_at(1.0);
    parts.start.fleet.push(StartPlantDef {
        kind: PlantKind::Nuclear,
        cell: CellCoord { x: 9, y: 3 },
    });
    let mut s = session_of(parts);
    let (nuclear, coal) = (
        plant_of(&s, PlantKind::Nuclear),
        plant_of(&s, PlantKind::Coal),
    );
    s.commit_plant(nuclear, [false; 24]).unwrap();
    s.set_auto_follow(coal, false).unwrap();
    let plan = s.automatic_commitment_plan();
    assert!(!plan.contains_key(&nuclear));
    assert!(!plan.contains_key(&coal));
    assert!(plan.contains_key(&plant_of(&s, PlantKind::Ccgt)));
    let kinds: Vec<_> = plan
        .keys()
        .map(|id| s.power.plant(*id).unwrap().kind())
        .collect();
    assert!(
        kinds
            .iter()
            .all(|kind| matches!(kind, PlantKind::Coal | PlantKind::Ccgt | PlantKind::Peaker))
    );
}

#[test]
fn one_tight_hour_still_allows_overnight_trimming() {
    // At 1.4x the whole fleet (1,300 MW) misses the evening band, but nights need far less.
    let s = session_of(parts_at(1.4));
    let plan = s.automatic_commitment_plan();
    let peaker = plan[&plant_of(&s, PlantKind::Peaker)];
    assert!(peaker.is_committed(17));
    assert!(!peaker.is_committed(3), "{:?}", peaker.hours());
}

#[test]
fn sunny_forecast_lets_a_ccgt_off_at_midday_that_a_dull_one_keeps() {
    let midday_ccgt = |clearness: f64| {
        let mut parts = parts_at(0.75);
        for spec in &mut parts.catalog {
            if spec.kind == PlantKind::Ccgt {
                spec.startup_cost = Money::ZERO;
            }
        }
        parts.start.fleet.push(StartPlantDef {
            kind: PlantKind::Solar,
            cell: CellCoord { x: 1, y: 1 },
        });
        let mut s = session_of(parts);
        let mut forecast = s.power.weather_forecast().clone();
        forecast.clearness = clearness;
        forecast.expected_clearness = clearness;
        s.power.set_weather_forecast(forecast);
        let plan = s.automatic_commitment_plan();
        plan[&plant_of(&s, PlantKind::Ccgt)].is_committed(12)
    };
    assert!(midday_ccgt(0.2), "a dull day keeps the CCGT at noon");
    assert!(!midday_ccgt(1.0), "a bright day lets it go");
}

fn linked_session() -> GameSession {
    let mut parts = two_area_parts();
    parts.start.island_homes = vec![CityId(1), CityId(3)];
    parts.start.connected_cities.push(CityId(3));
    parts.start.hvdc_links = vec![(CityId(1), CityId(3))];
    parts.start.fleet.push(StartPlantDef {
        kind: PlantKind::Peaker,
        cell: CellCoord { x: 4, y: 2 },
    });
    session_of(parts)
}

#[test]
fn linked_islands_stay_covered() {
    let mut s = linked_session();
    assert!(s.power.island_nodes().len() > 1);
    let floor = s.evaluate(&s.all_day_plan()).floor();
    let plan = s.automatic_commitment_plan();
    for (id, pattern) in &plan {
        s.commit_plant(*id, pattern.hours()).unwrap();
    }
    assert!(
        plan.values()
            .any(|pattern| *pattern != CommitmentSchedule::all_day()),
        "the planner trimmed something: {plan:?}"
    );
    // Power crosses the link under the plan: an island imports in some hour while another
    // exports, so trimming one island really changes what the other can count on.
    let flows: Vec<Vec<f64>> = s
        .power
        .island_nodes()
        .keys()
        .map(|id| {
            let island = s.island_session(*id).unwrap();
            (0..24)
                .map(|hour| island.power.scheduled_injection(hour * 60 + 30).value())
                .collect()
        })
        .collect();
    assert!(
        flows.iter().any(|hours| hours.iter().any(|mw| *mw > 1.0)),
        "an island imports: {flows:?}"
    );
    assert!(
        flows.iter().any(|hours| hours.iter().any(|mw| *mw < -1.0)),
        "an island exports: {flows:?}"
    );
    // Re-planned transfers included: every island still clears its floor.
    let after = s.evaluate(&BTreeMap::new());
    assert!(after.covers(&floor));
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    #[test]
    fn plan_is_valid_covered_and_never_worse(scale in 0.5..1.5f64) {
        let s = session_of(parts_at(scale));
        let all_day = s.all_day_plan();
        let base = s.evaluate(&all_day);
        let plan = s.automatic_commitment_plan();
        for (id, pattern) in &plan {
            let plant = s.power.plant(*id).unwrap();
            proptest::prop_assert!(
                CommitmentSchedule::try_new(pattern.hours(), &plant.commitment_spec()).is_ok()
            );
        }
        let planned = s.evaluate(&plan);
        proptest::prop_assert!(planned.covers(&base.floor()));
        proptest::prop_assert!(!base.better_than(&planned));
    }

    #[test]
    fn plan_is_pure_and_deterministic(scale in 0.5..1.5f64) {
        let s = session_of(parts_at(scale));
        let before = s.state_hash();
        let first = s.automatic_commitment_plan();
        proptest::prop_assert_eq!(s.state_hash(), before);
        proptest::prop_assert_eq!(first, s.automatic_commitment_plan());
    }
}
