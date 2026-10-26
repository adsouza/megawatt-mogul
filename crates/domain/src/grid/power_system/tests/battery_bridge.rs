//! Storage must support generators catching up without replacing available generation.
use super::*;

fn flat_system(demand: f64) -> (PowerSystem, RegionDefinition, SimRng) {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.demand_noise.forecast_error = Ratio::ZERO;
    parts.climates[0].shapes[0] = [1.0; 24];
    parts.climates[0].shapes[1] = [1.0; 24];
    parts.climates[0].shapes[2] = [1.0; 24];
    parts.climates[0].monthly = [1.0; 12];
    let count = parts.start.connected_cities.len() as f64;
    for city in &mut parts.cities {
        city.base_load = Megawatts::new(demand / count);
    }
    let region = RegionDefinition::try_from(parts).unwrap();
    let mut sys = committed_auto_follow(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Battery, cell(5, 2)),
        ],
    );
    let mut rng = SimRng::from_seed(7);
    sys.begin_day(&region, april(), &mut rng);
    charge_inventory(&mut sys, PlantId(2), 60);
    (sys, region, rng)
}

#[test]
fn charged_battery_holds_after_ten_pm_when_generators_already_meet_demand() {
    let (mut sys, region, mut rng) = flat_system(400.0);
    let opening = sys.plant(PlantId(2)).unwrap().battery().unwrap().stored();
    for tick in 1320..1440 {
        let (summary, _) = sys.tick(&region, april(), tick, &mut rng);
        assert_eq!(summary.battery_discharge, Megawatts::ZERO, "minute {tick}");
        assert!((sys.primary_supply().value() - 400.0).abs() < 1e-7);
    }
    assert_eq!(
        sys.plant(PlantId(2)).unwrap().battery().unwrap().stored(),
        opening
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn battery_discharge_begins_at_six_am_and_covers_a_live_gap(gap in 30.0f64..80.0) {
        let (mut sys, region, mut rng) = flat_system(400.0);
        place(&mut sys, 1, 400.0 - gap, None);
        sys.set_auto_follow(PlantId(1), true).unwrap();
        let demand = Megawatts::new(400.0);
        let before = sys.propose_battery_flows(&region, april(), 6 * 60 - 1, demand, demand);
        prop_assert_eq!(before[&PlantId(2)].discharge, Megawatts::ZERO);
        let at_start = sys.propose_battery_flows(&region, april(), 6 * 60, demand, demand);
        prop_assert!((at_start[&PlantId(2)].discharge.value() - gap).abs() < 1e-7);
        prop_assert_eq!(at_start[&PlantId(2)].charge, Megawatts::ZERO);
        let (summary, _) = sys.tick(&region, april(), 6 * 60, &mut rng);
        prop_assert!(summary.battery_discharge > Megawatts::ZERO);
        prop_assert_eq!(summary.battery_charge, Megawatts::ZERO);
        prop_assert_eq!(summary.unserved, Megawatts::ZERO);
    }

    #[test]
    fn battery_overlap_selects_only_the_direction_of_the_actual_balance(
        tick in 360u32..1320,
        gap in 30.0f64..80.0,
    ) {
        let (mut sys, region, _) = flat_system(400.0);
        let demand = Megawatts::new(400.0);
        let opening = sys.plant(PlantId(2)).unwrap().battery().unwrap().stored();
        place(&mut sys, 1, 400.0 - gap, None);
        let deficit = sys.propose_battery_flows(&region, april(), tick, demand, demand);
        prop_assert!((deficit[&PlantId(2)].discharge.value() - gap).abs() < 1e-7);
        prop_assert_eq!(deficit[&PlantId(2)].charge, Megawatts::ZERO);
        place(&mut sys, 1, 400.0 + gap, None);
        let surplus = sys.propose_battery_flows(&region, april(), tick, demand, demand);
        prop_assert!(surplus[&PlantId(2)].charge > Megawatts::ZERO);
        prop_assert!(surplus[&PlantId(2)].charge.value() <= gap + 1e-7);
        prop_assert_eq!(surplus[&PlantId(2)].discharge, Megawatts::ZERO);
        prop_assert_eq!(sys.plant(PlantId(2)).unwrap().battery().unwrap().stored(), opening);
    }

    #[test]
    fn battery_bridges_a_ramp_gap_then_stops(demand in 300.0f64..450.0, gap in 20.0f64..80.0) {
        let (mut sys, region, mut rng) = flat_system(demand);
        place(&mut sys, 1, demand - gap, Some(demand - gap));
        sys.set_auto_follow(PlantId(1), true).unwrap();
        let opening = sys.plant(PlantId(2)).unwrap().battery().unwrap().stored().value();
        let mut delivered = 0.0;
        for tick in 1320..1380 {
            let (summary, events) = sys.tick(&region, april(), tick, &mut rng);
            if tick == 1320 { prop_assert!(summary.battery_discharge.value() > 0.0); }
            let deficit = (summary.served.value() - sys.primary_supply().value()).max(0.0);
            prop_assert!((summary.battery_discharge.value() - deficit).abs() < 1e-6);
            prop_assert_eq!(summary.unserved, Megawatts::ZERO);
            prop_assert!(!events.iter().any(|e| matches!(e, DomainEvent::LoadShed { .. } | DomainEvent::Blackout)), "battery covers the ramp without shedding");
            delivered += summary.battery_discharge.value() / 60.0;
            if tick >= 1350 {
                prop_assert!(summary.battery_discharge.value() < 1e-6);
                prop_assert!((sys.primary_supply().value() - demand).abs() < 1e-6);
            }
        }
        prop_assert!(delivered < gap / 60.0 * 30.0, "support ends once generation catches up");
        let left = sys.plant(PlantId(2)).unwrap().battery().unwrap().stored().value();
        prop_assert!((opening - left - delivered).abs() < 1e-6);
    }
}

#[test]
fn battery_charging_stops_at_ten_pm_even_with_actual_surplus() {
    let (mut sys, region, _) = flat_system(400.0);
    place(&mut sys, 1, 450.0, None);
    let demand = Megawatts::new(400.0);
    let before = sys.propose_battery_flows(&region, april(), 22 * 60 - 1, demand, demand);
    assert!(before[&PlantId(2)].charge > Megawatts::ZERO);
    for tick in [22 * 60, 22 * 60 + 1, 1439] {
        let after = sys.propose_battery_flows(&region, april(), tick, demand, demand);
        assert_eq!(after[&PlantId(2)].charge, Megawatts::ZERO);
        assert_eq!(after[&PlantId(2)].discharge, Megawatts::ZERO);
    }
}

#[test]
fn late_grid_charging_tapers_into_the_ten_pm_cutoff_without_shedding() {
    let (mut sys, region, mut rng) = flat_system(400.0);
    sys.plant_mut(PlantId(2))
        .unwrap()
        .battery_mut()
        .unwrap()
        .close_day();
    sys.plan_batteries(&region, april(), 21 * 60);
    let mut charged_late = false;
    for tick in 21 * 60..23 * 60 {
        let (summary, events) = sys.tick(&region, april(), tick, &mut rng);
        if (21 * 60 + 30..22 * 60).contains(&tick) {
            charged_late |= summary.battery_charge > Megawatts::ZERO;
        }
        if tick >= 22 * 60 {
            assert_eq!(summary.battery_charge, Megawatts::ZERO, "minute {tick}");
        }
        assert_eq!(summary.battery_discharge, Megawatts::ZERO);
        assert_eq!(summary.unserved, Megawatts::ZERO);
        assert_eq!(summary.mode, GridMode::Normal);
        assert!(
            !events.iter().any(|event| matches!(
                event,
                DomainEvent::LoadShed { .. }
                    | DomainEvent::Blackout
                    | DomainEvent::OverFrequencyTrip { .. }
            )),
            "minute {tick}: {events:?}"
        );
        assert!(
            (summary.generation - summary.served - summary.battery_charge - summary.imbalance)
                .value()
                .abs()
                < 1e-7
        );
    }
    assert!(charged_late, "exercise actual grid charging after 21:30");
}

#[test]
fn projected_schedule_holds_charge_when_generation_can_cover_the_evening() {
    let (sys, region, _) = flat_system(400.0);
    let plan = sys.preview_batteries(&region, april());
    assert!(
        plan.plants[&PlantId(2)]
            .charge
            .iter()
            .any(|mw| *mw > Megawatts::ZERO)
    );
    assert!(
        plan.plants[&PlantId(2)]
            .discharge
            .iter()
            .all(|mw| *mw == Megawatts::ZERO)
    );
}
