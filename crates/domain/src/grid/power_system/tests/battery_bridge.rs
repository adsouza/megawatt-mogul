//! Storage must support generators catching up without replacing available generation.
use super::*;

fn flat_system(demand: f64) -> (PowerSystem, RegionDefinition, SimRng) {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.demand.forecast_error = Ratio::ZERO;
    parts.demand.winter = [1.0; 24];
    parts.demand.summer = [1.0; 24];
    parts.demand.shoulder = [1.0; 24];
    parts.demand.monthly = [1.0; 12];
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
fn charged_battery_holds_after_six_pm_when_generators_already_meet_demand() {
    let (mut sys, region, mut rng) = flat_system(400.0);
    let opening = sys.plant(PlantId(2)).unwrap().battery().unwrap().stored();
    for tick in 1080..1200 {
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
    fn battery_discharge_begins_at_four_pm_and_covers_a_live_gap(gap in 30.0f64..80.0) {
        let (mut sys, region, mut rng) = flat_system(400.0);
        place(&mut sys, 1, 400.0 - gap, Some(400.0 - gap));
        sys.set_auto_follow(PlantId(1), true).unwrap();
        let demand = Megawatts::new(400.0);
        let before = sys.propose_battery_flows(&region, april(), 16 * 60 - 1, demand, demand);
        prop_assert_eq!(before[&PlantId(2)].discharge, Megawatts::ZERO);
        let at_start = sys.propose_battery_flows(&region, april(), 16 * 60, demand, demand);
        prop_assert!((at_start[&PlantId(2)].discharge.value() - gap).abs() < 1e-7);
        prop_assert_eq!(at_start[&PlantId(2)].charge, Megawatts::ZERO);
        let (summary, _) = sys.tick(&region, april(), 16 * 60, &mut rng);
        prop_assert!(summary.battery_discharge > Megawatts::ZERO);
        prop_assert_eq!(summary.battery_charge, Megawatts::ZERO);
        prop_assert_eq!(summary.unserved, Megawatts::ZERO);
    }

    #[test]
    fn battery_bridges_a_ramp_gap_then_stops(demand in 300.0f64..450.0, gap in 20.0f64..80.0) {
        let (mut sys, region, mut rng) = flat_system(demand);
        place(&mut sys, 1, demand - gap, Some(demand - gap));
        sys.set_auto_follow(PlantId(1), true).unwrap();
        let opening = sys.plant(PlantId(2)).unwrap().battery().unwrap().stored().value();
        let mut delivered = 0.0;
        for tick in 1080..1140 {
            let (summary, events) = sys.tick(&region, april(), tick, &mut rng);
            if tick == 1080 { prop_assert!(summary.battery_discharge.value() > 0.0); }
            let deficit = (summary.served.value() - sys.primary_supply().value()).max(0.0);
            prop_assert!((summary.battery_discharge.value() - deficit).abs() < 1e-6);
            prop_assert_eq!(summary.unserved, Megawatts::ZERO);
            prop_assert!(!events.iter().any(|e| matches!(e, DomainEvent::LoadShed { .. } | DomainEvent::Blackout)), "battery covers the ramp without shedding");
            delivered += summary.battery_discharge.value() / 60.0;
            if tick >= 1110 {
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
