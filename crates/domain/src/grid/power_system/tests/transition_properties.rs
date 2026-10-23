//! Whole-controller contracts, independent of the dispatch implementation's targets.
use super::*;

/// Construct a run on the 24-hour circle with enough time for BOTH shutdown
/// and startup. No rejection sampling: every generated schedule is exercised.
fn mixed_hours(spec: &crate::region::PlantSpec, start: usize, extra_gap: usize) -> [bool; 24] {
    let shutdown = (spec.capacity.value() / spec.ramp.per_tick().value()).ceil() as u32;
    let minimum_gap = (shutdown + spec.startup.ticks()).div_ceil(TICKS_PER_HOUR) as usize;
    assert!(minimum_gap < 24, "this fixture supports a recurring run");
    if extra_gap % 2 == 1 && minimum_gap < 12 {
        // Two independent off-gaps, including one across the circle's origin.
        let gap = minimum_gap + extra_gap % (12 - minimum_gap);
        let mut hours = [false; 24];
        for offset in 0..24 {
            hours[(start + offset) % 24] = offset % 12 < 12 - gap;
        }
        CommitmentSchedule::try_new(hours, spec).unwrap();
        return hours;
    }
    let gap = minimum_gap + extra_gap % (24 - minimum_gap);
    let mut hours = [false; 24];
    for offset in 0..24 - gap {
        hours[(start + offset) % 24] = true;
    }
    CommitmentSchedule::try_new(hours, spec).unwrap();
    hours
}

fn state_index(status: PlantStatus) -> usize {
    match status {
        PlantStatus::Online { .. } => 0,
        PlantStatus::Offline => 1,
        PlantStatus::Starting { .. } => 2,
        PlantStatus::Stopping { .. } => 3,
        PlantStatus::UnderConstruction { .. } => panic!("commissioned test fleet"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn mixed_schedule_controller_obeys_physical_transitions_through_a_whole_day(
        seed in any::<u64>(), rotation in 0usize..24, extra_gap in 0usize..24,
        manual_fraction in 0u16..=100, dry in any::<bool>(),
        isolated in any::<bool>(), evening_agc in any::<bool>(),
    ) {
        let mut parts = crate::region::fixtures::tiny_region_parts();
        if dry { parts.hydro.monthly_flow = [0.00001; 12]; }
        let region = RegionDefinition::try_from(parts).unwrap();
        let defs = [
            connected(PlantKind::Ccgt, cell(5, 1)),
            connected(PlantKind::Ccgt, cell(5, 2)),
            connected(PlantKind::Ccgt, cell(5, 3)),
            connected(PlantKind::Ccgt, cell(5, 4)),
            PlantDef { kind: PlantKind::Hydro, cell: cell(10, 1), connected: !isolated },
            connected(PlantKind::Peaker, cell(5, 5)),
        ];
        let mut sys = system_with(&region, &defs);
        for (id, plant) in &mut sys.plants {
            let hours = match id.0 {
                1 => [true; 24],
                2 => [false; 24],
                3 => { let mut h = [false; 24]; h[17..].fill(true); h },
                4 => { let mut h = [false; 24]; h[1..8].fill(true); h },
                _ => mixed_hours(plant.spec(), rotation, extra_gap),
            };
            plant.set_commitment(CommitmentSchedule::try_new(hours, plant.spec()).unwrap());
            plant.set_auto_follow(if id.0 == 3 { evening_agc } else { id.0 != 1 }).unwrap();
            let spec = plant.spec();
            plant.set_initial_target(spec.min_output() + (spec.capacity - spec.min_output())
                * (f64::from(manual_fraction) / 100.0));
        }
        let mut rng = SimRng::from_seed(seed);
        sys.begin_day(&region, april(), &mut rng);
        // Require each boundary state in EVERY case, rather than hoping seeds hit it.
        let mut seen = [false; 4];
        for plant in sys.plants.values() { seen[state_index(plant.status())] = true; }
        prop_assert_eq!(seen, [true; 4]);

        let mut transitions = [false; 2];
        for tick in 0..TICKS_PER_DAY {
            let before = sys.plants.clone();
            let (summary, events) = sys.tick(&region, april(), tick, &mut rng);
            let reported: Megawatts = sys.connected_plants().map(Plant::output).sum();
            prop_assert!((summary.generation - reported).abs().value() < 1e-6);
            for (id, after) in &sys.plants {
                let previous = &before[id];
                transitions[0] |= matches!(previous.status(), PlantStatus::Starting { .. }) && after.is_online()
                    && events.contains(&DomainEvent::PlantOnline { plant: *id });
                transitions[1] |= matches!(previous.status(), PlantStatus::Stopping { .. })
                    && after.status() == PlantStatus::Offline
                    && events.contains(&DomainEvent::PlantStopped { plant: *id });
                // Trips may instantly remove output. Startup may enter at minimum
                // stable output. Neither excuses a ramp violation on ordinary ticks.
                if events.contains(&DomainEvent::PlantTripped { plant: *id }) { continue; }
                let old = previous.output().value();
                let now = after.output().value();
                let ramp = previous.spec().ramp.per_tick().value();
                let resource = previous.hydro_available().value();
                match previous.status() {
                    PlantStatus::Online { .. } | PlantStatus::Stopping { .. } => {
                        let lower = (old - ramp).max(0.0).min(resource);
                        prop_assert!(now + 1e-6 >= lower,
                            "tick {} plant {:?}: {} -> {}, ramp {}, available {}", tick, id, old, now, ramp, resource);
                        prop_assert!(now <= old + ramp + 1e-6);
                        prop_assert!(now <= resource + 1e-6);
                    }
                    PlantStatus::Starting { .. } | PlantStatus::Offline => {
                        if now > 0.0 {
                            prop_assert!(events.contains(&DomainEvent::PlantOnline { plant: *id }), "positive output requires a completed start");
                            prop_assert!((now - after.spec().min_output().value()).abs() < 1e-6);
                        }
                    }
                    PlantStatus::UnderConstruction { .. } => unreachable!(),
                }
                if let Some(budget) = previous.hydro_budget() {
                    let expected = (budget.value() - now / 60.0).max(0.0);
                    prop_assert!((after.hydro_budget().unwrap().value() - expected).abs() < 1e-6);
                }
                if !sys.network.is_connected(after.node()) {
                    prop_assert!(!summary.by_plant.contains_key(id));
                }
            }
        }
        // Physical start/stop completion must actually be covered, not only setup.
        prop_assert_eq!(transitions, [true; 2]);
    }

    #[test]
    fn extra_fixed_generation_displaces_the_same_amount_of_midnight_agc(
        delta in 1u16..200, tail_agc in any::<bool>(), seed in any::<u64>(),
    ) {
        let region = RegionDefinition::try_from(crate::region::fixtures::may_2033_parts()).unwrap();
        let mut base = auto_follow_fleet(&region);
        // An evening unit gives this comparison a shutdown tail as well.
        let mut evening = [false; 24]; evening[17..].fill(true);
        let tail = base.plant_mut(PlantId(9)).unwrap();
        tail.set_commitment(CommitmentSchedule::try_new(evening, tail.spec()).unwrap());
        tail.set_auto_follow(tail_agc).unwrap();
        let manual = base.plant_mut(PlantId(6)).unwrap();
        manual.set_auto_follow(false).unwrap();
        let initial = manual.spec().min_output();
        manual.set_initial_target(initial);
        let mut extra = base.clone();
        extra.plant_mut(PlantId(6)).unwrap().set_initial_target(initial + Megawatts::new(f64::from(delta)));
        let month = region.start().month;
        base.begin_day(&region, month, &mut SimRng::from_seed(seed));
        extra.begin_day(&region, month, &mut SimRng::from_seed(seed));
        let flexible = |s: &PowerSystem| s.connected_plants()
            .filter(|p| p.auto_follow() && p.is_online()).map(|p| p.output().value()).sum::<f64>();
        prop_assert!((flexible(&base) - flexible(&extra) - f64::from(delta)).abs() < 1e-6);
        let demand = base.connected_demand_forecast(&region, month, 0).value();
        for s in [&base, &extra] {
            let generation: f64 = s.connected_plants().map(|p| p.output().value()).sum();
            prop_assert!((generation - demand).abs() < 1e-6);
        }
    }

    #[test]
    fn controllable_surplus_recovers_without_a_protection_induced_shortage(
        fraction in 0u16..=100, seed in any::<u64>(),
    ) {
        let region = RegionDefinition::try_from(crate::region::fixtures::may_2033_parts()).unwrap();
        let month = region.start().month;
        let mut sys = auto_follow_fleet(&region);
        let mut rng = SimRng::from_seed(seed);
        sys.begin_day(&region, month, &mut rng);
        // An independent disturbance after boundary dispatch: the first four
        // hydro units have ample downward ramp room, and the fifth was idle.
        let excess = region.spec(PlantKind::Hydro).ramp.per_tick() * (5.0 + f64::from(fraction) / 100.0);
        sys.plant_mut(PlantId(5)).unwrap().set_output_unchecked(excess);
        let mut curtail_band_seen = false;
        for tick in 0..60 {
            let before = sys.plants.clone();
            let (summary, events) = sys.tick(&region, month, tick, &mut rng);
            curtail_band_seen |= summary.frequency - region.frequency().nominal > region.frequency().curtail_above;
            prop_assert_eq!(summary.mode, GridMode::Normal);
            prop_assert_eq!(summary.unserved, Megawatts::ZERO);
            prop_assert_eq!(summary.shed, Ratio::ZERO);
            prop_assert!(!events.iter().any(|e| matches!(e, DomainEvent::PlantTripped { .. })), "ample ramp room needs no trip");
            for p in sys.connected_plants().filter(|p| p.kind() == PlantKind::Hydro) {
                prop_assert!((p.output() - before[&p.id()].output()).abs() <= p.spec().ramp.per_tick(),
                    "tick {}: {:?} bypassed its physical ramp", tick, p.id());
            }
        }
        prop_assert!(curtail_band_seen, "the property must actually exercise over-frequency protection");
        prop_assert!(sys.deviation().value().abs() < region.frequency().normal_band.value());
    }
}
