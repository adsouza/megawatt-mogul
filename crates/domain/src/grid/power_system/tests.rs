use super::*;
use crate::demand::shape_factor;
use crate::generation::{CommitmentSchedule, PlantStatus, water_fill};
use crate::grid::network::{LineStatus, Node, NodeKind};
use crate::ids::{CellCoord, LineId};
use crate::region::fixtures::tiny_region;
use crate::time::{SimDuration, TICKS_PER_DAY};
use proptest::prelude::*;

const ROOT: NodeId = NodeId(0);

fn april() -> SimMonth {
    SimMonth::new(2027, 4).unwrap()
}

fn cell(x: u16, y: u16) -> CellCoord {
    CellCoord { x, y }
}

/// A plant to place in a test system, and whether a line joins it to the root.
struct PlantDef {
    kind: PlantKind,
    cell: CellCoord,
    connected: bool,
}

fn connected(kind: PlantKind, cell: CellCoord) -> PlantDef {
    PlantDef {
        kind,
        cell,
        connected: true,
    }
}

fn start_fleet(region: &RegionDefinition) -> Vec<PlantDef> {
    region
        .start()
        .fleet
        .iter()
        .map(|p| connected(p.kind, p.cell))
        .collect()
}

/// The tiny region's cities on their own nodes (the start's connected cities
/// joined to the root, the rest isolated), plus `defs` as plants 1, 2, ...
fn system_with(region: &RegionDefinition, defs: &[PlantDef]) -> PowerSystem {
    let mut network = Network::new(ROOT, region.start().substation);
    let mut cities = BTreeMap::new();
    for def in region.cities() {
        let node = NodeId(10 + def.id.0);
        network
            .add_node(
                node,
                Node {
                    kind: NodeKind::City(def.id),
                    cell: def.cell,
                },
            )
            .unwrap();
        if region.start().connected_cities.contains(&def.id) {
            network
                .add_line(LineId(node.0), ROOT, node, LineStatus::InService)
                .unwrap();
        }
        cities.insert(def.id, DemandCenter::new(def.clone(), node));
    }
    let mut plants = BTreeMap::new();
    for (i, def) in (1u32..).zip(defs) {
        let (id, node) = (PlantId(i), NodeId(100 + i));
        network
            .add_node(
                node,
                Node {
                    kind: NodeKind::Plant(id),
                    cell: def.cell,
                },
            )
            .unwrap();
        if def.connected {
            network
                .add_line(LineId(node.0), ROOT, node, LineStatus::InService)
                .unwrap();
        }
        let spec = region.spec(def.kind).clone();
        plants.insert(
            id,
            Plant::new_commissioned(id, node, def.cell, spec, april()),
        );
    }
    PowerSystem::new(network, plants, cities, NetworkModel::CopperPlate)
}

/// The start fleet committed all day on auto-follow, left at its default
/// initial targets: `begin_day` balances the boundary itself.
fn auto_follow_fleet(region: &RegionDefinition) -> PowerSystem {
    let mut sys = system_with(region, &start_fleet(region));
    let ids: Vec<PlantId> = sys.plants().keys().copied().collect();
    for id in ids {
        let p = sys.plant_mut(id).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_auto_follow(true).unwrap();
    }
    sys
}

fn run_ticks(
    sys: &mut PowerSystem,
    region: &RegionDefinition,
    ticks: std::ops::Range<u32>,
    rng: &mut SimRng,
) -> Vec<(TickSummary, Vec<DomainEvent>)> {
    ticks.map(|t| sys.tick(region, april(), t, rng)).collect()
}

#[test]
fn ample_auto_follow_capacity_serves_all_demand() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    let mut rng = SimRng::from_seed(7);
    sys.begin_day(&region, april(), &mut rng);
    let ticks = run_ticks(&mut sys, &region, 0..TICKS_PER_DAY, &mut rng);
    let unserved: f64 = ticks.iter().map(|(s, _)| s.unserved.value()).sum();
    assert_eq!(unserved, 0.0);
    assert!(ticks.iter().all(|(s, _)| s.mode == GridMode::Normal));
}

#[test]
fn disconnected_plant_contributes_nothing() {
    let region = tiny_region();
    let mut base = auto_follow_fleet(&region);
    let mut extra = auto_follow_fleet(&region);
    let (id, node, at) = (PlantId(99), NodeId(99), cell(6, 6));
    extra
        .network_mut()
        .add_node(
            node,
            Node {
                kind: NodeKind::Plant(id),
                cell: at,
            },
        )
        .unwrap();
    let spec = region.spec(PlantKind::Peaker).clone();
    let mut peaker = Plant::new_commissioned(id, node, at, spec, april());
    peaker.set_commitment(CommitmentSchedule::all_day());
    peaker.set_initial_target(peaker.spec().capacity);
    extra.insert_plant(peaker);

    let (mut rng_a, mut rng_b) = (SimRng::from_seed(3), SimRng::from_seed(3));
    base.begin_day(&region, april(), &mut rng_a);
    extra.begin_day(&region, april(), &mut rng_b);
    for t in 0..60 {
        let (a, _) = base.tick(&region, april(), t, &mut rng_a);
        let (b, _) = extra.tick(&region, april(), t, &mut rng_b);
        assert!(!b.by_plant.contains_key(&id));
        assert_eq!(a.generation, b.generation);
        assert_eq!(a, b);
    }
    // It still evolves physically: online at full output, just not on the grid.
    assert_eq!(extra.plant(id).unwrap().output(), Megawatts::new(200.0));
}

#[test]
fn no_supply_leads_to_blackout_and_trips_plants() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(&region, &defs);
    let solar = PlantId(4);
    let mut rng = SimRng::from_seed(11);
    sys.begin_day(&region, april(), &mut rng);
    let mut blackout_events = None;
    for t in 0..100 {
        let (_, events) = sys.tick(&region, april(), t, &mut rng);
        if events.contains(&DomainEvent::Blackout) {
            blackout_events = Some(events);
            break;
        }
    }
    let events = blackout_events.expect("a grid with no supply blacks out");
    assert!(events.contains(&DomainEvent::PlantTripped { plant: solar }));
    assert!(
        sys.plants()
            .values()
            .all(|p| p.status() == PlantStatus::Offline)
    );
    assert!(sys.plants().values().all(Plant::is_commitment_suppressed));
    assert_eq!(sys.protection().mode, GridMode::Blackout);
    assert_eq!(sys.deviation(), Hertz::ZERO);
}

#[test]
fn hydro_budget_uses_cell_potential_and_month_flow() {
    let region = tiny_region();
    let mut sys = system_with(&region, &[connected(PlantKind::Hydro, cell(10, 1))]);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    let budget = sys.plant(PlantId(1)).unwrap().hydro_budget().unwrap();
    assert!((budget.value() - 800.0 * 24.0 * 0.8 * 1.2).abs() < 1e-9);
    assert!((budget.value() - 18_432.0).abs() < 1e-9);
}

/// 200 ticks around solar noon, so solar and curtailment are exercised.
const MIDDAY: std::ops::Range<u32> = 660..860;

/// The most a tiny-region solar plant could produce at `tick` (clear sky).
fn clear_sky_solar(region: &RegionDefinition, tick: u32) -> f64 {
    300.0 * 0.8 * sun_factor(region.solar(), april(), tick)
}

/// The start fleet at random fixed (not auto-follow) targets, as fractions of
/// capacity, plus a connected solar plant (plant 4).
fn fixed_target_fleet(region: &RegionDefinition, targets: &[f64]) -> PowerSystem {
    let mut defs = start_fleet(region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(region, &defs);
    for (id, frac) in (1u32..).zip(targets) {
        let p = sys.plant_mut(PlantId(id)).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_initial_target(p.spec().capacity * *frac);
    }
    sys
}

proptest! {
    #[test]
    fn energy_is_conserved_every_tick(seed in any::<u64>(), targets in prop::collection::vec(0.0f64..1.0, 3)) {
        let region = tiny_region();
        let mut sys = fixed_target_fleet(&region, &targets);
        let mut rng = SimRng::from_seed(seed);
        sys.begin_day(&region, april(), &mut rng);
        for t in MIDDAY {
            let (s, _) = sys.tick(&region, april(), t, &mut rng);
            // Curtailment is physical (R13), so curtailed availability is
            // outside generation: the brief's `served + curtailed + imbalance`
            // becomes `served + imbalance`.
            let balance = s.served + s.imbalance;
            prop_assert!((s.generation - balance).abs().value() < 1e-6);
            prop_assert!((s.unserved - s.demand * s.shed.value()).abs().value() < 1e-9);
            prop_assert!(s.served.value() >= 0.0);
            prop_assert!(s.served.value() <= s.demand.value() + 1e-9);
            // Generation is what the connected plants actually produce.
            let by_plant: Megawatts = s.by_plant.values().copied().sum();
            let by_kind: Megawatts = s.by_kind.values().copied().sum();
            let produced: Megawatts = sys
                .plants()
                .values()
                .filter(|p| sys.network().is_connected(p.node()))
                .map(Plant::output)
                .sum();
            prop_assert!((by_plant - s.generation).abs().value() < 1e-6);
            prop_assert!((by_kind - s.generation).abs().value() < 1e-6);
            prop_assert!((produced - s.generation).abs().value() < 1e-6);
            // Curtailment withholds part of the solar availability, never more.
            let solar = s.by_kind.get(&PlantKind::Solar).copied().unwrap_or(Megawatts::ZERO);
            prop_assert!(s.curtailed >= Megawatts::ZERO);
            prop_assert!((s.curtailed + solar).value() <= clear_sky_solar(&region, t) + 1e-6);
        }
    }

    #[test]
    fn frequency_event_fires_only_on_band_change(seed in any::<u64>(), targets in prop::collection::vec(0.0f64..1.0, 3)) {
        let region = tiny_region();
        let p = region.frequency().clone();
        let mut sys = fixed_target_fleet(&region, &targets);
        let mut rng = SimRng::from_seed(seed);
        sys.begin_day(&region, april(), &mut rng);
        let mut last = FrequencyBand::Normal;
        for t in MIDDAY {
            let (_, events) = sys.tick(&region, april(), t, &mut rng);
            let now = band(sys.deviation(), &p);
            let fired: Vec<DomainEvent> = events
                .into_iter()
                .filter(|e| matches!(e, DomainEvent::FrequencyDeviated { .. }))
                .collect();
            if now == last {
                prop_assert!(fired.is_empty());
            } else {
                prop_assert_eq!(fired, vec![DomainEvent::FrequencyDeviated { band: now }]);
            }
            last = now;
        }
    }
}

#[test]
fn scheduled_start_is_online_at_committed_boundary_through_tick() {
    let region = tiny_region();
    let mut sys = system_with(&region, &start_fleet(&region));
    let forecast = sys.connected_demand_forecast(&region, april(), 0);
    // Coal and peaker carry the load all day; the CCGT is committed 06:00-21:00.
    let (coal, ccgt, peaker) = (PlantId(1), PlantId(2), PlantId(3));
    for (id, share) in [(coal, 0.8), (peaker, 0.2)] {
        let p = sys.plant_mut(id).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_auto_follow(true).unwrap();
        p.set_initial_target(forecast * share);
    }
    let mut hours = [false; 24];
    hours[6..21].fill(true);
    let p = sys.plant_mut(ccgt).unwrap();
    let schedule = CommitmentSchedule::try_new(hours, p.spec()).unwrap();
    p.set_commitment(schedule);

    let mut rng = SimRng::from_seed(5);
    sys.begin_day(&region, april(), &mut rng);
    let boundary = 6 * 60;
    let ticks = run_ticks(&mut sys, &region, 0..boundary - 1, &mut rng);
    assert!(ticks.iter().all(|(s, _)| s.mode == GridMode::Normal));
    let start_tick = boundary - 120;
    assert!(
        ticks[start_tick as usize]
            .1
            .contains(&DomainEvent::PlantStarting { plant: ccgt })
    );
    // State after tick b-2's step is instant b-1: one tick of start-up left.
    assert_eq!(
        sys.plant(ccgt).unwrap().status(),
        PlantStatus::Starting { ticks_left: 1 }
    );
    let (_, events) = sys.tick(&region, april(), boundary - 1, &mut rng);
    assert!(events.contains(&DomainEvent::PlantOnline { plant: ccgt }));
    assert!(sys.plant(ccgt).unwrap().is_online());
}

#[test]
fn blackout_restoration_reconnects_load_and_solar() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(&region, &defs);
    let (ccgt, peaker, solar) = (PlantId(2), PlantId(3), PlantId(4));
    let mut rng = SimRng::from_seed(2);
    sys.begin_day(&region, april(), &mut rng);
    let (_, events) = sys.tick(&region, april(), 0, &mut rng);
    assert!(events.contains(&DomainEvent::Blackout));

    // The operator black-starts the peaker and the CCGT on auto-follow.
    for id in [ccgt, peaker] {
        let p = sys.plant_mut(id).unwrap();
        p.set_auto_follow(true).unwrap();
        p.start().unwrap();
    }
    let mut saw_partial = false;
    let mut restored_at = None;
    for t in 1..400 {
        let (s, events) = sys.tick(&region, april(), t, &mut rng);
        if s.mode == GridMode::Blackout {
            assert_eq!(s.frequency, region.frequency().nominal);
            if s.shed < Ratio::ONE {
                saw_partial = true;
                // Solar reconnects as soon as restoration re-energises the grid.
                assert!(sys.plant(solar).unwrap().is_online());
            }
        }
        if events.contains(&DomainEvent::BlackoutRestored) {
            restored_at = Some(t);
            break;
        }
    }
    assert!(saw_partial);
    assert!(restored_at.is_some(), "restoration must not stall");
    assert_eq!(sys.protection().mode, GridMode::Normal);
    assert_eq!(sys.protection().shed, Ratio::ZERO);
    assert!(sys.plant(solar).unwrap().is_online());
    // Automatic commitment stays suppressed for the rest of the day.
    assert!(sys.plant(PlantId(1)).unwrap().is_commitment_suppressed());
}

#[test]
fn surplus_beyond_solar_forces_agc_to_minimum_then_curtails_all_solar() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(&region, &defs);
    let (coal, ccgt, peaker) = (PlantId(1), PlantId(2), PlantId(3));
    for id in [coal, ccgt, peaker] {
        let p = sys.plant_mut(id).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_initial_target(p.spec().capacity);
    }
    sys.plant_mut(peaker)
        .unwrap()
        .set_auto_follow(true)
        .unwrap();
    let mut rng = SimRng::from_seed(4);
    sys.begin_day(&region, april(), &mut rng);
    // Solar noon: 1300 MW of thermal against ~850 MW of demand, far more than
    // the solar output can cover.
    let (s, events) = sys.tick(&region, april(), 762, &mut rng);
    let solar_output = s.by_kind[&PlantKind::Solar];
    assert!(solar_output > Megawatts::ZERO);
    assert_eq!(
        s.curtailed,
        Megawatts::ZERO,
        "curtailment acts from the next tick"
    );
    assert!(events.contains(&DomainEvent::SolarCurtailed { mw: solar_output }));
    let min = region.spec(PlantKind::Peaker).min_output();
    assert_eq!(
        sys.plant(peaker).unwrap().status(),
        PlantStatus::Online {
            output: min,
            target: min
        }
    );
    // Plants not on auto-follow keep their schedule.
    assert_eq!(
        sys.plant(coal).unwrap().output(),
        region.spec(PlantKind::Coal).capacity
    );
    // Next tick the (slightly lower, past noon) solar availability is withheld entirely.
    let (s, _) = sys.tick(&region, april(), 763, &mut rng);
    assert!(s.curtailed > Megawatts::ZERO);
    assert_eq!(s.by_kind[&PlantKind::Solar], Megawatts::ZERO);
    assert_eq!(sys.plant(PlantId(4)).unwrap().output(), Megawatts::ZERO);
}

/// Non-AGC coal (500 MW) and CCGT (200 MW) plus a peaker at 100 MW (put on
/// AGC by [`begin_surplus_day`]): 800 MW of thermal under ~850 MW of midday demand, so the solar
/// plant (plant 4) alone pushes the grid into surplus.
fn solar_surplus_system(region: &RegionDefinition) -> PowerSystem {
    let mut defs = start_fleet(region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(region, &defs);
    for (id, mw) in [(1, 500.0), (2, 200.0), (3, 100.0)] {
        let p = sys.plant_mut(PlantId(id)).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_initial_target(Megawatts::new(mw));
    }
    sys
}

/// Begins the day with the peaker (plant 3) at its hand-set 100 MW, then puts
/// it on AGC: the boundary balances auto-follow plants, which would otherwise
/// erase the deliberate midday surplus this fixture builds.
fn begin_surplus_day(sys: &mut PowerSystem, region: &RegionDefinition, rng: &mut SimRng) {
    let peaker = PlantId(3);
    sys.plant_mut(peaker)
        .unwrap()
        .set_auto_follow(false)
        .unwrap();
    sys.begin_day(region, april(), rng);
    sys.plant_mut(peaker)
        .unwrap()
        .set_auto_follow(true)
        .unwrap();
}

#[test]
fn solar_driven_surplus_is_curtailed_until_frequency_recovers() {
    let region = tiny_region();
    let curtail_above = region.frequency().curtail_above;
    let mut sys = solar_surplus_system(&region);
    let peaker = PlantId(3);
    let mut rng = SimRng::from_seed(21);
    begin_surplus_day(&mut sys, &region, &mut rng);
    let (first, events) = sys.tick(&region, april(), 700, &mut rng);
    assert!(
        sys.deviation() > curtail_above,
        "solar surplus trips over-frequency"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DomainEvent::SolarCurtailed { .. }))
    );
    // Solar covers the surplus, so the AGC peaker only ramps; it is not forced to minimum.
    assert_eq!(
        sys.plant(peaker).unwrap().status(),
        PlantStatus::Online {
            output: Megawatts::new(100.0),
            target: Megawatts::new(60.0)
        }
    );
    let solar_before = first.by_kind[&PlantKind::Solar];
    for t in 701..820 {
        let (s, _) = sys.tick(&region, april(), t, &mut rng);
        assert!(s.curtailed > Megawatts::ZERO, "tick {t}: surplus persists");
        assert!(s.by_kind[&PlantKind::Solar] < solar_before);
        if t >= 710 {
            assert!(
                sys.deviation() < curtail_above,
                "tick {t}: {:?}",
                sys.deviation()
            );
        }
    }
    // Cheaper solar displaces the AGC peaker down to its minimum.
    assert_eq!(
        sys.plant(peaker).unwrap().output(),
        region.spec(PlantKind::Peaker).min_output()
    );
    assert_eq!(sys.protection().mode, GridMode::Normal);
}

#[test]
fn curtailment_is_released_when_the_surplus_disappears() {
    let region = tiny_region();
    let mut sys = solar_surplus_system(&region);
    let coal = PlantId(1);
    let mut rng = SimRng::from_seed(22);
    begin_surplus_day(&mut sys, &region, &mut rng);
    let mut last = None;
    for t in 700..760 {
        last = Some(sys.tick(&region, april(), t, &mut rng).0);
    }
    let engaged = last.unwrap();
    assert!(engaged.curtailed > Megawatts::ZERO);
    // The coal plant is turned down by more than the curtailed solar, so the
    // grid would be short without every megawatt of solar.
    let target = sys.plant(coal).unwrap().output() - engaged.curtailed - Megawatts::new(20.0);
    sys.plant_mut(coal).unwrap().set_target(target).unwrap();
    let mut released = None;
    for t in 760..860 {
        let (s, _) = sys.tick(&region, april(), t, &mut rng);
        if s.curtailed == Megawatts::ZERO {
            released = Some(s);
            break;
        }
    }
    let released = released.expect("curtailment is released");
    assert!(released.by_kind[&PlantKind::Solar] > engaged.by_kind[&PlantKind::Solar]);
    assert_eq!(released.mode, GridMode::Normal);
    assert_eq!(released.shed, Ratio::ZERO);
    // Solar now runs at its full availability, every following tick too.
    for t in 860..880 {
        let (s, _) = sys.tick(&region, april(), t, &mut rng);
        assert_eq!(s.curtailed, Megawatts::ZERO);
    }
}

#[test]
fn begin_day_draws_clearness_once_and_reports_boundary_starts() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    // The peaker gets a mixed pattern, so its restart is counted by its Start action instead.
    let mut hours = [true; 24];
    hours[2..5].fill(false);
    let p = sys.plant_mut(PlantId(3)).unwrap();
    let schedule = CommitmentSchedule::try_new(hours, p.spec()).unwrap();
    p.set_commitment(schedule);
    let (mut rng, mut reference) = (SimRng::from_seed(8), SimRng::from_seed(8));
    let init = sys.begin_day(&region, april(), &mut rng);
    reference.standard_normal();
    assert_eq!(rng, reference);
    assert_eq!(
        init.boundary_starts,
        BTreeMap::from([(PlantId(1), 1), (PlantId(2), 1)])
    );
    assert_eq!(sys.protection(), ProtectionState::default());
    assert_eq!(sys.deviation(), Hertz::ZERO);
}

#[test]
fn each_tick_draws_exactly_one_normal() {
    let region = tiny_region();
    for mut sys in [auto_follow_fleet(&region), system_with(&region, &[])] {
        let mut rng = SimRng::from_seed(6);
        sys.begin_day(&region, april(), &mut rng);
        for t in 0..5 {
            let mut reference = rng.clone();
            reference.standard_normal();
            sys.tick(&region, april(), t, &mut rng);
            assert_eq!(rng, reference);
        }
    }
}

#[test]
fn tick_completed_carries_the_summary_last() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    let mut rng = SimRng::from_seed(9);
    sys.begin_day(&region, april(), &mut rng);
    let (s, events) = sys.tick(&region, april(), 0, &mut rng);
    assert_eq!(s.tick, 0);
    assert_eq!(events.last(), Some(&DomainEvent::TickCompleted(s)));
}

#[test]
fn connected_demand_forecast_counts_connected_cities_only() {
    let region = tiny_region();
    let sys = system_with(&region, &[]);
    let factor = shape_factor(region.demand(), april(), 600);
    let forecast = sys.connected_demand_forecast(&region, april(), 600);
    assert!((forecast.value() - 1000.0 * factor).abs() < 1e-9);
}

#[test]
fn expected_solar_uses_mean_clearness_for_connected_plants() {
    let region = tiny_region();
    let isolated = PlantDef {
        kind: PlantKind::Solar,
        cell: cell(4, 4),
        connected: false,
    };
    let sys = system_with(
        &region,
        &[connected(PlantKind::Solar, cell(5, 5)), isolated],
    );
    let sun = sun_factor(region.solar(), april(), 762);
    let expected = 300.0 * 0.8 * sun * 0.75;
    let got = sys.expected_solar(&region, april(), 762);
    assert!((got.value() - expected).abs() < 1e-9);
    assert_eq!(sys.expected_solar(&region, april(), 0), Megawatts::ZERO);
}

#[test]
fn ramp_capability_counts_online_headroom_and_imminent_starts() {
    let region = tiny_region();
    let isolated_ccgt = PlantDef {
        kind: PlantKind::Ccgt,
        cell: cell(9, 5),
        connected: false,
    };
    let mut sys = system_with(
        &region,
        &[
            connected(PlantKind::Ccgt, cell(8, 4)),
            connected(PlantKind::Peaker, cell(6, 5)),
            connected(PlantKind::Coal, cell(7, 3)),
            isolated_ccgt,
        ],
    );
    let (ccgt, peaker, coal, isolated) = (PlantId(1), PlantId(2), PlantId(3), PlantId(4));
    for id in [ccgt, isolated] {
        let p = sys.plant_mut(id).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_initial_target(Megawatts::new(300.0));
    }
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    sys.plant_mut(peaker).unwrap().start().unwrap(); // 10 ticks of start-up
    sys.plant_mut(coal).unwrap().start().unwrap(); // 480 ticks of start-up
    let within = |ticks| {
        sys.ramp_capability_up(SimDuration::from_ticks(ticks))
            .value()
    };
    // CCGT: 25 MW/tick, 200 MW of room. Peaker: 40 MW at minimum, then 40 MW/tick.
    assert!((within(4) - 100.0).abs() < 1e-9);
    assert!((within(10) - (200.0 + 40.0)).abs() < 1e-9);
    assert!((within(12) - (200.0 + 120.0)).abs() < 1e-9);
    assert!((within(60) - (200.0 + 200.0)).abs() < 1e-9);
}

#[test]
fn firm_capacity_counts_commissioned_plants_by_credit() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(&region, &defs);
    let spec = region.spec(PlantKind::Nuclear).clone();
    sys.insert_plant(Plant::new_under_construction(
        PlantId(50),
        NodeId(50),
        cell(9, 2),
        spec,
    ));
    assert!((sys.firm_capacity().value() - (1300.0 * 0.95 + 300.0 * 0.3)).abs() < 1e-9);
}

#[test]
fn projected_peak_uses_summed_hourly_means_and_current_connectivity() {
    let region = tiny_region();
    let mut sys = system_with(&region, &[]);
    let peak_factor = |first: SimMonth| {
        let mut month = first;
        let mut best = 0.0f64;
        for _ in 0..12 {
            for h in 0..24 {
                best = best.max(shape_factor(region.demand(), month, h * 60 + 30));
            }
            month = month.next();
        }
        best
    };
    let got = sys.projected_connected_peak(&region, april());
    assert!((got.value() - 1000.0 * peak_factor(april())).abs() < 1e-9);
    // Connecting city 3 (300 MW) raises the projection at once.
    sys.network_mut()
        .add_line(LineId(999), ROOT, NodeId(13), LineStatus::InService)
        .unwrap();
    let got = sys.projected_connected_peak(&region, april());
    assert!((got.value() - 1300.0 * peak_factor(april())).abs() < 1e-9);
}

#[test]
fn begin_day_carries_nothing_over_from_the_previous_day() {
    let region = tiny_region();
    let mut used = solar_surplus_system(&region);
    let mut rng = SimRng::from_seed(23);
    begin_surplus_day(&mut used, &region, &mut rng);
    let (s, _) = run_ticks(&mut used, &region, 700..760, &mut rng)
        .pop()
        .unwrap();
    assert!(s.curtailed > Megawatts::ZERO);
    // Same RNG position, one system with a day behind it and one fresh: the
    // next day's first tick is identical (noise, curtailment, frequency reset).
    let mut fresh = solar_surplus_system(&region);
    let mut fresh_rng = rng.clone();
    begin_surplus_day(&mut used, &region, &mut rng);
    begin_surplus_day(&mut fresh, &region, &mut fresh_rng);
    assert_eq!(
        used.tick(&region, april(), 700, &mut rng),
        fresh.tick(&region, april(), 700, &mut fresh_rng)
    );
}

#[test]
fn begin_day_starts_a_fresh_hydro_plan() {
    let region = tiny_region();
    let mut used = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(29);
    used.begin_day(&region, january(), &mut rng);
    run_month(&mut used, &region, january(), 0..TICKS_PER_DAY, &mut rng);
    let mut fresh = west_like_fleet(&region);
    let mut fresh_rng = rng.clone();
    used.begin_day(&region, january(), &mut rng);
    fresh.begin_day(&region, january(), &mut fresh_rng);
    assert_eq!(used.hydro_plan, fresh.hydro_plan);
    assert_eq!(used.hydro_schedule(), fresh.hydro_schedule());
}

#[test]
fn auto_follow_fleet_starts_the_day_balanced() {
    let region = tiny_region();
    let params = region.frequency();
    let deviation = |s: &TickSummary| (s.frequency - params.nominal).value();
    for seed in 0..20 {
        let mut sys = auto_follow_fleet(&region);
        let mut rng = SimRng::from_seed(seed);
        sys.begin_day(&region, april(), &mut rng);
        let ticks = run_ticks(&mut sys, &region, 0..60, &mut rng);
        for (s, events) in &ticks {
            let at = format!("seed {seed} tick {}", s.tick);
            assert_eq!(s.mode, GridMode::Normal, "{at}");
            assert_eq!(s.shed, Ratio::ZERO, "{at}");
            assert_eq!(s.unserved, Megawatts::ZERO, "{at}");
            assert!(!events.contains(&DomainEvent::Blackout), "{at}");
            // Demand noise moves frequency, but never as far as load shedding.
            assert!(deviation(s) > params.shed_below.value(), "{at}");
        }
        // The day opens balanced: the first tick is inside the normal band.
        assert!(deviation(&ticks[0].0).abs() < params.normal_band.value());
    }
}

#[test]
fn begin_day_balances_the_boundary_against_load_net_of_manual_and_solar() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(&region, &defs);
    for id in 1..=3 {
        let p = sys.plant_mut(PlantId(id)).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_auto_follow(id != 2).unwrap();
    }
    sys.plant_mut(PlantId(2))
        .unwrap()
        .set_initial_target(Megawatts::new(450.0));
    sys.begin_day(&region, april(), &mut SimRng::from_seed(2));
    let load = sys.connected_demand_forecast(&region, april(), 0)
        - sys.expected_solar(&region, april(), 0);
    let output = |id: u32| sys.plant(PlantId(id)).unwrap().output();
    // The manual plant keeps its own initial target; the others carry the rest.
    assert_eq!(output(2), Megawatts::new(450.0));
    assert!(output(1) + output(3) + output(2) >= load - Megawatts::new(1e-9));
    let followers = output(1) + output(3);
    let expected = (load - Megawatts::new(450.0)).max(
        sys.plant(PlantId(1)).unwrap().spec().min_output()
            + sys.plant(PlantId(3)).unwrap().spec().min_output(),
    );
    assert!((followers - expected).abs().value() < 1e-9);
}

#[test]
fn begin_day_draw_count_is_unchanged() {
    let region = tiny_region();
    let (mut a, mut b) = (auto_follow_fleet(&region), auto_follow_fleet(&region));
    let (mut rng_a, mut rng_b) = (SimRng::from_seed(13), SimRng::from_seed(13));
    a.begin_day(&region, april(), &mut rng_a);
    b.begin_day(&region, april(), &mut rng_b);
    // Identical state draws identically, and the only draw is the day's clearness.
    assert_eq!(a, b);
    assert_eq!(rng_a, rng_b);
    let mut reference = SimRng::from_seed(13);
    reference.standard_normal();
    assert_eq!(rng_a, reference);
}

// ---- Mutation-killing tests: exact single-tick oracles ----

/// The tiny region with every plant able to ramp 1,000 MW per tick, so AGC is
/// limited by headroom to minimum output rather than by the ramp.
fn fast_ramp_region() -> RegionDefinition {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    for spec in &mut parts.catalog {
        spec.ramp = crate::units::RampRate::mw_per_minute(1_000.0);
    }
    RegionDefinition::try_from(parts).unwrap()
}

fn supply(sys: &PowerSystem) -> Megawatts {
    sys.connected_plants().map(Plant::output).sum()
}

fn target_of(sys: &PowerSystem, id: u32) -> Megawatts {
    match sys.plant(PlantId(id)).unwrap().status() {
        PlantStatus::Online { target, .. } => target,
        other => panic!("plant {id} is {other:?}"),
    }
}

fn close(a: Megawatts, b: Megawatts) -> bool {
    (a - b).abs().value() < 1e-6
}

#[test]
fn tick_demand_is_the_forecast_times_one_plus_the_drawn_error() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    let mut rng = SimRng::from_seed(5);
    sys.begin_day(&region, april(), &mut rng);
    for t in 0..40 {
        let mut shadow_rng = rng.clone();
        let mut shadow_noise = sys.noise;
        let error = shadow_noise.step(region.demand(), &mut shadow_rng);
        let forecast = sys.connected_demand_forecast(&region, april(), t);
        let (s, _) = sys.tick(&region, april(), t, &mut rng);
        assert!(close(s.demand, forecast * (1.0 + error)), "tick {t}");
    }
}

#[test]
fn shed_load_is_not_served_by_agc_and_does_not_count_in_the_imbalance() {
    let region = fast_ramp_region();
    let params = region.frequency();
    let mut sys = auto_follow_fleet(&region);
    let mut rng = SimRng::from_seed(3);
    sys.begin_day(&region, april(), &mut rng);
    let shed = 0.1;
    sys.protection.shed = Ratio::new(shed).unwrap();
    let before = supply(&sys);
    let coal_output = sys.plant(PlantId(1)).unwrap().output();
    let (s, _) = sys.tick(&region, april(), 0, &mut rng);

    // Only coal has headroom to lower; it absorbs the whole (negative) shortfall.
    let shortfall = s.demand * (1.0 - shed) - before;
    assert!(shortfall < Megawatts::ZERO);
    assert!(close(target_of(&sys, 1), coal_output + shortfall));

    let imbalance = before - s.demand * (1.0 - shed);
    let expected = next_deviation(Hertz::ZERO, imbalance, s.demand, params);
    assert!((sys.deviation() - expected).value().abs() < 1e-9);
    assert!(sys.deviation() > Hertz::ZERO);
    assert!(
        (s.frequency - (params.nominal + sys.deviation()))
            .value()
            .abs()
            < 1e-12
    );
}

#[test]
fn blackout_pre_positions_supply_for_the_next_restoration_step() {
    let region = fast_ramp_region();
    let params = region.frequency();
    let mut sys = auto_follow_fleet(&region);
    let mut rng = SimRng::from_seed(3);
    sys.begin_day(&region, april(), &mut rng);
    let shed = 0.2;
    sys.protection = ProtectionState {
        mode: GridMode::Blackout,
        shed: Ratio::new(shed).unwrap(),
        shed_ticks: 3,
        ..ProtectionState::default()
    };
    let before = supply(&sys);
    let coal_output = sys.plant(PlantId(1)).unwrap().output();
    let (s, _) = sys.tick(&region, april(), 0, &mut rng);

    let to_serve = 1.0 - shed + params.shed_step.value();
    let shortfall = s.demand * to_serve - before;
    assert!(shortfall < Megawatts::ZERO);
    assert!(close(target_of(&sys, 1), coal_output + shortfall));
    assert_eq!(sys.deviation(), Hertz::ZERO);
    assert_eq!(s.frequency, params.nominal);
}

#[test]
fn grid_following_solar_resynchronises_only_once_load_is_being_restored() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let solar = PlantId(4);

    // A tripped solar plant stays out in normal operation: it is not re-synchronised.
    let mut sys = system_with(&region, &defs);
    let mut rng = SimRng::from_seed(11);
    sys.begin_day(&region, april(), &mut rng);
    sys.plant_mut(solar).unwrap().trip();
    sys.tick(&region, april(), 0, &mut rng);
    assert_eq!(sys.plant(solar).unwrap().status(), PlantStatus::Offline);

    // During a blackout it stays out while nothing has been restored (shed = 1).
    let mut sys = system_with(&region, &defs);
    let mut rng = SimRng::from_seed(2);
    sys.begin_day(&region, april(), &mut rng);
    let (_, events) = sys.tick(&region, april(), 0, &mut rng);
    assert!(events.contains(&DomainEvent::Blackout));
    for t in 1..30 {
        let (s, _) = sys.tick(&region, april(), t, &mut rng);
        assert_eq!(
            (s.mode, s.shed),
            (GridMode::Blackout, Ratio::ONE),
            "tick {t}"
        );
        assert_eq!(
            sys.plant(solar).unwrap().status(),
            PlantStatus::Offline,
            "tick {t}"
        );
    }
}

#[test]
fn force_agc_to_minimum_returns_the_targets_it_removed() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    let (coal, ccgt, peaker) = (PlantId(1), PlantId(2), PlantId(3));
    sys.plant_mut(coal)
        .unwrap()
        .set_target(Megawatts::new(400.0))
        .unwrap();
    sys.plant_mut(ccgt)
        .unwrap()
        .set_target(Megawatts::new(300.0))
        .unwrap();
    sys.plant_mut(peaker)
        .unwrap()
        .set_target(Megawatts::new(40.0))
        .unwrap();
    // A plant on manual dispatch is left alone.
    sys.plant_mut(peaker)
        .unwrap()
        .set_auto_follow(false)
        .unwrap();
    sys.plant_mut(peaker)
        .unwrap()
        .set_target(Megawatts::new(100.0))
        .unwrap();

    let removed = sys.force_agc_to_minimum();

    assert_eq!(removed, Megawatts::new((400.0 - 240.0) + (300.0 - 200.0)));
    assert_eq!(target_of(&sys, 1), Megawatts::new(240.0));
    assert_eq!(target_of(&sys, 2), Megawatts::new(200.0));
    assert_eq!(target_of(&sys, 3), Megawatts::new(100.0));
}

#[test]
fn curtailment_is_what_forcing_agc_down_cannot_cover() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    sys.plant_mut(PlantId(1))
        .unwrap()
        .set_target(Megawatts::new(310.0))
        .unwrap();
    sys.plant_mut(PlantId(2))
        .unwrap()
        .set_target(Megawatts::new(200.0))
        .unwrap();
    sys.plant_mut(PlantId(3))
        .unwrap()
        .set_target(Megawatts::new(40.0))
        .unwrap();
    let mut events = Vec::new();

    // 100 MW of surplus, 60 MW of solar to curtail, and forcing AGC to minimum
    // removes 70 MW of targets: only 30 MW of solar then has to be curtailed.
    sys.update_curtailment(
        ProtectionOutcome::Curtail,
        Megawatts::new(100.0),
        Megawatts::new(60.0),
        &mut events,
    );

    assert_eq!(
        events,
        vec![DomainEvent::SolarCurtailed {
            mw: Megawatts::new(30.0)
        }]
    );
    assert_eq!(sys.curtailment, Megawatts::new(30.0));
    assert_eq!(target_of(&sys, 1), Megawatts::new(240.0));
}

#[test]
fn nothing_is_curtailed_or_forced_when_there_is_no_surplus_to_cover() {
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    let targets: Vec<Megawatts> = (1..=3).map(|id| target_of(&sys, id)).collect();
    let mut events = Vec::new();

    sys.update_curtailment(
        ProtectionOutcome::Curtail,
        Megawatts::ZERO,
        Megawatts::ZERO,
        &mut events,
    );

    assert!(events.is_empty());
    assert_eq!(sys.curtailment, Megawatts::ZERO);
    let after: Vec<Megawatts> = (1..=3).map(|id| target_of(&sys, id)).collect();
    assert_eq!(after, targets);
}

#[test]
fn surplus_without_over_frequency_is_not_curtailed() {
    // Thermal plants hold 50 MW more than load: below the curtail threshold,
    // so the sub-ramp surplus is left to frequency, not withheld from solar.
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(5, 5)));
    let mut sys = system_with(&region, &defs);
    for id in 1..=3 {
        let p = sys.plant_mut(PlantId(id)).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
    }
    let mut rng = SimRng::from_seed(21);
    let tick = 700;
    // Fixed targets for the thermal fleet, chosen from the day's own numbers.
    sys.plant_mut(PlantId(2))
        .unwrap()
        .set_initial_target(Megawatts::new(200.0));
    sys.plant_mut(PlantId(3))
        .unwrap()
        .set_initial_target(Megawatts::new(40.0));
    sys.begin_day(&region, april(), &mut rng);
    let (available, _, _) = sys.availability(&region, april(), tick);
    let solar_mw = available[&PlantId(4)];
    let forecast = sys.connected_demand_forecast(&region, april(), tick);
    let coal_mw = forecast + Megawatts::new(50.0) - solar_mw - Megawatts::new(240.0);
    sys.plant_mut(PlantId(1))
        .unwrap()
        .set_boundary_output(coal_mw);
    // Coal alone is on AGC, with only a 6 MW ramp to give back.
    sys.plant_mut(PlantId(1))
        .unwrap()
        .set_auto_follow(true)
        .unwrap();

    let (s, events) = sys.tick(&region, april(), tick, &mut rng);

    assert!(s.imbalance > Megawatts::new(30.0), "{:?}", s.imbalance);
    assert!(sys.deviation() < region.frequency().curtail_above);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::SolarCurtailed { .. }))
    );
    assert_eq!(sys.curtailment, Megawatts::ZERO);
}

#[test]
fn hydro_availability_is_its_remaining_budget_spread_over_an_hour() {
    let region = tiny_region();
    let mut sys = system_with(&region, &[connected(PlantKind::Hydro, cell(10, 1))]);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    sys.plant_mut(PlantId(1))
        .unwrap()
        .reset_hydro_budget(MegawattHours::new(2.0));
    let (available, _, _) = sys.availability(&region, april(), 700);
    assert_eq!(available[&PlantId(1)], Megawatts::new(120.0));
}

#[test]
fn begin_day_budgets_only_commissioned_hydro() {
    let region = tiny_region();
    let mut sys = system_with(&region, &[]);
    let spec = region.spec(PlantKind::Hydro).clone();
    sys.insert_plant(Plant::new_under_construction(
        PlantId(7),
        NodeId(107),
        cell(10, 1),
        spec,
    ));
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    assert_eq!(
        sys.plant(PlantId(7)).unwrap().hydro_budget(),
        Some(MegawattHours::ZERO)
    );
}

#[test]
fn begin_day_dispatches_only_connected_plants() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(PlantDef {
        kind: PlantKind::Ccgt,
        cell: cell(4, 4),
        connected: false,
    });
    let mut sys = system_with(&region, &defs);
    for id in 1..=4 {
        let p = sys.plant_mut(PlantId(id)).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_auto_follow(true).unwrap();
    }
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    // The isolated plant keeps its minimum; the connected coal plant takes the load.
    assert_eq!(
        sys.plant(PlantId(4)).unwrap().output(),
        Megawatts::new(200.0)
    );
    let load = sys.connected_demand_forecast(&region, april(), 0);
    let connected_total: Megawatts = supply(&sys);
    assert!(close(connected_total, load));
}

// ---- Hydro peak-shaving schedule (spec §12.3–§12.6) ----

const HYDRO: PlantId = PlantId(4);

fn january() -> SimMonth {
    SimMonth::new(2028, 1).unwrap()
}

/// The tiny region with every non-hydro plant able to ramp 1,000 MW per
/// tick, so thermal can always pick up what hydro gives back within a tick.
fn fast_thermal_region() -> RegionDefinition {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    for spec in &mut parts.catalog {
        if spec.kind != PlantKind::Hydro {
            spec.ramp = crate::units::RampRate::mw_per_minute(1_000.0);
        }
    }
    RegionDefinition::try_from(parts).unwrap()
}

/// A West-like fleet: the start fleet (coal 1, CCGT 2, peaker 3) plus hydro
/// 4 on the hydro site, all committed all day on auto-follow. In January the
/// hydro budget (9,216 MWh) is well below `capacity × 24 h` and the day's net
/// load, so the water must be rationed.
fn west_like_fleet(region: &RegionDefinition) -> PowerSystem {
    let mut defs = start_fleet(region);
    defs.push(connected(PlantKind::Hydro, cell(10, 1)));
    let mut sys = system_with(region, &defs);
    let ids: Vec<PlantId> = sys.plants().keys().copied().collect();
    for id in ids {
        let p = sys.plant_mut(id).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        p.set_auto_follow(true).unwrap();
    }
    sys
}

fn run_month(
    sys: &mut PowerSystem,
    region: &RegionDefinition,
    month: SimMonth,
    ticks: std::ops::Range<u32>,
    rng: &mut SimRng,
) -> Vec<(TickSummary, Vec<DomainEvent>)> {
    ticks.map(|t| sys.tick(region, month, t, rng)).collect()
}

/// Hydro's AGC ceiling for targets that first act on `effect_tick`.
fn ceiling(sys: &PowerSystem, effect_tick: u32) -> Megawatts {
    sys.hydro_ceilings(effect_tick)[&HYDRO]
}

/// Hydro's energy in hour `h` of `ticks` (each tick's output after its step).
fn hour_energy(ticks: &[(TickSummary, Vec<DomainEvent>)], h: usize) -> f64 {
    ticks[h * 60..(h + 1) * 60]
        .iter()
        .map(|(s, _)| s.by_plant[&HYDRO].value() / 60.0)
        .sum()
}

fn exhausted(ticks: &[(TickSummary, Vec<DomainEvent>)]) -> bool {
    ticks.iter().any(|(_, events)| {
        events
            .iter()
            .any(|e| matches!(e, DomainEvent::HydroBudgetExhausted { .. }))
    })
}

/// The hydro plant's one-tick ramp.
fn hydro_ramp(sys: &PowerSystem) -> Megawatts {
    sys.plant(HYDRO).unwrap().spec().ramp.per_tick()
}

fn hydro_output(sys: &PowerSystem) -> Megawatts {
    sys.plant(HYDRO).unwrap().output()
}

/// The latest plan's allocation for `hour`.
fn allocation(sys: &PowerSystem, hour: usize) -> Megawatts {
    sys.hydro_plan
        .get(&HYDRO)
        .expect("hydro is scheduled")
        .hourly[hour]
}

#[test]
fn hydro_is_spent_on_the_evening_peak() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(21);
    sys.begin_day(&region, january(), &mut rng);
    let hydro = sys.plant(HYDRO).unwrap();
    let budget = hydro.hydro_budget().unwrap();
    assert!(budget < hydro.spec().capacity.energy_over_ticks(TICKS_PER_DAY));
    // The day opens with hydro at no more than its hour-0 allocation.
    assert!(hydro_output(&sys) <= allocation(&sys, 0) + Megawatts::new(1e-9));
    assert!(hydro_output(&sys) > Megawatts::ZERO);

    let net = sys.hourly_net_load(&region, january());
    let peak_hour = (0..24u32)
        .max_by(|&a, &b| net[a as usize].value().total_cmp(&net[b as usize].value()))
        .unwrap();
    let ticks = run_month(&mut sys, &region, january(), 0..TICKS_PER_DAY, &mut rng);
    let output = |t: u32| ticks[t as usize].0.by_plant[&HYDRO];
    let energy = |hours: std::ops::Range<u32>| -> f64 {
        (hours.start * 60..hours.end * 60)
            .map(|t| output(t).value() / 60.0)
            .sum()
    };

    assert!((peak_hour * 60..(peak_hour + 1) * 60).all(|t| output(t) > Megawatts::ZERO));
    // Water lasts through the evening: no cliff at 20:00–21:00.
    assert!((17 * 60..22 * 60).all(|t| output(t) > Megawatts::ZERO));
    assert!(energy(0..6) < energy(17..22));
    assert!(ticks.iter().all(|(s, _)| s.unserved == Megawatts::ZERO));
    assert!(ticks.iter().all(|(_, events)| {
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::HydroBudgetExhausted { .. }))
    }));
}

/// The fast-thermal West-like fleet with hydro eligible 06:00–18:00 only, so
/// its allocation falls from the full 800 MW in hour 17 to 0 in hour 18. Run
/// to instant 1000, after which its commitment is suppressed so only the
/// schedule, not a Stop, lowers it past 18:00. Returns the ticks run.
fn eligible_until_six_pm(
    sys: &mut PowerSystem,
    region: &RegionDefinition,
    rng: &mut SimRng,
) -> Vec<(TickSummary, Vec<DomainEvent>)> {
    let mut hours = [false; 24];
    hours[6..18].fill(true);
    let p = sys.plant_mut(HYDRO).unwrap();
    let schedule = CommitmentSchedule::try_new(hours, p.spec()).unwrap();
    p.set_commitment(schedule);
    sys.begin_day(region, january(), rng);
    let ticks = run_month(sys, region, january(), 0..1000, rng);
    sys.plant_mut(HYDRO).unwrap().suppress_commitment_for_day();
    ticks
}

#[test]
fn falling_allocation_ramps_down_not_cliffs() {
    let region = fast_thermal_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(4);
    eligible_until_six_pm(&mut sys, &region, &mut rng);
    run_month(&mut sys, &region, january(), 1000..1040, &mut rng);
    assert!(close(allocation(&sys, 17), Megawatts::new(800.0)));
    assert_eq!(allocation(&sys, 18), Megawatts::ZERO);

    let ramp = hydro_ramp(&sys).value();
    let mut previous = hydro_output(&sys);
    assert!(previous > Megawatts::new(500.0), "{previous:?}");
    for t in 1040..1090 {
        // The ceiling that governs this tick's step was set by the last AGC.
        let governing = ceiling(&sys, t);
        let (s, _) = sys.tick(&region, january(), t, &mut rng);
        // The rest of the fleet picks up exactly what hydro gives back, so
        // only the demand noise is left unbalanced.
        assert!(
            s.imbalance.abs() < Megawatts::new(30.0),
            "tick {t}: {:?}",
            s.imbalance
        );
        assert_eq!(sys.protection().shed, Ratio::ZERO, "tick {t}");
        let now = hydro_output(&sys);
        let fall = (previous - now).value();
        assert!(fall <= ramp + 1e-9, "tick {t}: fell {fall}");
        // Once the fall starts it runs at the full ramp rate until empty.
        // The ramp envelope: never above what can still reach 0 by 18:00.
        assert!(
            now <= governing + Megawatts::new(1e-9),
            "tick {t}: {now:?} > {governing:?}"
        );
        // The fall is foreseen, so hydro is already dry when hour 18 begins.
        if t >= 1080 {
            assert!(now.value() <= 1e-9, "tick {t}: {now:?}");
        }
        previous = now;
    }
}

#[test]
fn unused_water_is_replanned_at_the_next_hour() {
    let region = fast_thermal_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(17);
    sys.begin_day(&region, january(), &mut rng);
    let first = sys.hydro_plan[&HYDRO];
    assert!(first.hourly[0] > Megawatts::ZERO);

    // Hydro is held at 0 MW for hour 0, below its allocation. Commands never
    // re-plan.
    sys.set_auto_follow(HYDRO, false).unwrap();
    sys.plant_mut(HYDRO)
        .unwrap()
        .set_target(Megawatts::ZERO)
        .unwrap();
    run_month(&mut sys, &region, january(), 0..60, &mut rng);
    sys.set_auto_follow(HYDRO, true).unwrap();
    let held = sys.hydro_plan[&HYDRO];
    assert!(!held.planned);
    assert_eq!(held.frozen, first.frozen);

    sys.tick(&region, january(), 60, &mut rng);
    let replanned = sys.hydro_plan[&HYDRO];
    let remaining = sys.plant(HYDRO).unwrap().hydro_budget().unwrap().value()
        + hydro_output(&sys).value() / 60.0; // the budget when tick 60 began
    let planned: f64 = replanned.hourly.iter().map(|mw| mw.value()).sum();
    assert!(
        (planned - remaining).abs() <= 1e-6 * remaining,
        "{planned} vs {remaining}"
    );
    assert_eq!(replanned.hourly[0], Megawatts::ZERO);
    let before: f64 = first.hourly[1..].iter().map(|mw| mw.value()).sum();
    // Hour 0's unused water is spread over the rest of the day.
    assert!(planned > before + 0.9 * first.hourly[0].value());
}

#[test]
fn emergency_lifts_the_cap_for_the_whole_restoration() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(2);
    sys.begin_day(&region, january(), &mut rng);
    run_month(&mut sys, &region, january(), 0..1030, &mut rng);
    assert!(hydro_output(&sys) <= ceiling(&sys, 1030) + Megawatts::new(1e-9));
    // Losing the CCGT sheds load without blacking out.
    sys.plant_mut(PlantId(2)).unwrap().trip();

    let (mut above_while_shedding, mut held_in_restoration, mut capped_again) = (0, 0, 0);
    for t in 1030..1080 {
        let shed_before = sys.protection().shed;
        let lifted = shed_before > Ratio::ZERO || sys.band == FrequencyBand::Shedding;
        let (s, _) = sys.tick(&region, january(), t, &mut rng);
        assert_eq!(s.mode, GridMode::Normal, "tick {t}");
        let alloc = ceiling(&sys, t + 1);
        let output = hydro_output(&sys);
        let target = target_of(&sys, HYDRO.0);
        let raising = s.demand * (1.0 - shed_before.value()) > s.generation;
        if lifted && output > alloc + Megawatts::new(1.0) {
            above_while_shedding += 1;
            // No cap while lifted: a short grid never pulls hydro back.
            if raising {
                assert!(target >= output - Megawatts::new(1e-9), "tick {t}");
                if sys.band != FrequencyBand::Shedding {
                    held_in_restoration += 1;
                }
            }
        }
        if !lifted && output > alloc + Megawatts::new(1e-9) {
            // The cap is back: hydro ramps down toward its allocation.
            capped_again += 1;
            let expected = (output - hydro_ramp(&sys)).max(alloc);
            assert!(
                close(target, expected),
                "tick {t}: {target:?} vs {expected:?}"
            );
        }
    }
    assert!(above_while_shedding > 0);
    assert!(held_in_restoration > 0);
    assert!(capped_again > 0);
    assert_eq!(sys.protection().shed, Ratio::ZERO);
    assert!(hydro_output(&sys) <= ceiling(&sys, 1079) + Megawatts::new(1e-9));
}

#[test]
fn hydro_plants_fill_in_id_order_against_the_residual() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let site = cell(10, 1);
    let spec = region.spec(PlantKind::Hydro).clone();
    // A second scheduled hydro plant, and a manual one that is never planned.
    for (id, auto) in [(PlantId(5), true), (PlantId(6), false)] {
        let mut plant =
            Plant::new_commissioned(id, NodeId(100 + id.0), site, spec.clone(), april());
        plant.set_commitment(CommitmentSchedule::all_day());
        plant.set_auto_follow(auto).unwrap();
        sys.network_mut()
            .add_node(
                plant.node(),
                Node {
                    kind: NodeKind::Plant(id),
                    cell: site,
                },
            )
            .unwrap();
        sys.network_mut()
            .add_line(
                LineId(plant.node().0),
                ROOT,
                plant.node(),
                LineStatus::InService,
            )
            .unwrap();
        sys.insert_plant(plant);
    }
    sys.begin_day(&region, january(), &mut SimRng::from_seed(3));
    assert!(!sys.hydro_plan.contains_key(&PlantId(6)));

    let budget = |id: u32| sys.plant(PlantId(id)).unwrap().hydro_budget().unwrap();
    let mut residual = sys.hourly_net_load(&region, january());
    for id in [4, 5] {
        let fill = water_fill(&residual, &[true; 24], spec.capacity, budget(id));
        let plan = sys.hydro_plan[&PlantId(id)];
        assert_eq!(
            (plan.level, plan.hourly),
            (fill.level, fill.hourly),
            "plant {id}"
        );
        for (left, used) in residual.iter_mut().zip(fill.hourly) {
            *left -= used;
        }
    }
}

#[test]
fn cap_is_lifted_while_shedding_or_any_load_is_shed() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    sys.begin_day(&region, january(), &mut SimRng::from_seed(1));
    assert!(!sys.hydro_cap_lifted());
    sys.band = FrequencyBand::Low;
    assert!(!sys.hydro_cap_lifted());
    sys.band = FrequencyBand::Shedding;
    assert!(sys.hydro_cap_lifted());
    sys.band = FrequencyBand::Normal;
    sys.protection.shed = Ratio::new(0.05).unwrap();
    assert!(sys.hydro_cap_lifted());
    sys.protection = ProtectionState {
        mode: GridMode::Blackout,
        shed: Ratio::ONE,
        shed_ticks: 0,
        ..ProtectionState::default()
    };
    assert!(sys.hydro_cap_lifted());
}

#[test]
fn manual_and_unplanned_hydro_are_uncapped() {
    let region = fast_thermal_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(9);
    sys.begin_day(&region, january(), &mut rng);
    run_month(&mut sys, &region, january(), 0..1020, &mut rng);

    // Manual from 17:00: unscheduled, so it follows the player's target.
    let manual = Megawatts::new(620.0);
    sys.set_auto_follow(HYDRO, false).unwrap();
    sys.plant_mut(HYDRO).unwrap().set_target(manual).unwrap();
    run_month(&mut sys, &region, january(), 1020..1030, &mut rng);
    assert!(!sys.hydro_plan.contains_key(&HYDRO));
    assert!(!sys.hydro_schedule().plants.contains_key(&HYDRO));
    assert_eq!(hydro_output(&sys), manual);

    // Back on AGC mid-hour: unplanned, so uncapped until the next re-plan.
    sys.set_auto_follow(HYDRO, true).unwrap();
    for t in 1030..1080 {
        sys.tick(&region, january(), t, &mut rng);
        assert!(!sys.hydro_plan.contains_key(&HYDRO));
        assert!(
            hydro_output(&sys) >= manual - Megawatts::new(1e-9),
            "tick {t}"
        );
    }

    // The 18:00 re-plan schedules it again, and the cap pulls it down.
    let uncapped = hydro_output(&sys);
    run_month(&mut sys, &region, january(), 1080..1090, &mut rng);
    let alloc = allocation(&sys, 18);
    assert!(
        uncapped > alloc + Megawatts::new(50.0),
        "{uncapped:?} vs {alloc:?}"
    );
    assert!(hydro_output(&sys) <= alloc + Megawatts::new(1e-9));
}

#[test]
fn tripped_hydro_is_unplanned_until_the_next_re_plan() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(14);
    sys.begin_day(&region, january(), &mut rng);
    run_month(&mut sys, &region, january(), 0..61, &mut rng);
    let elapsed = sys.hydro_schedule().plants[&HYDRO].hourly[0];
    let hour_1 = allocation(&sys, 1);
    assert!(sys.hydro_ceilings(62).contains_key(&HYDRO));

    // A deep frequency dip blacks the grid out and trips every plant.
    sys.deviation = Hertz::new(-2.0);
    let (_, events) = sys.tick(&region, january(), 61, &mut rng);
    assert!(events.contains(&DomainEvent::PlantTripped { plant: HYDRO }));
    assert!(!sys.hydro_ceilings(62).contains_key(&HYDRO));
    // Elapsed hours keep their record; the current and later hours are cleared.
    let view = sys.hydro_schedule().plants[&HYDRO];
    assert_eq!(view.hourly[0], elapsed);
    assert_eq!(view.level, None);
    assert!(view.hourly[1..].iter().all(|mw| *mw == Megawatts::ZERO));

    // Restarted, hydro alone restores the night load: far above its hour-1
    // allocation and never pulled back to it before the 02:00 re-plan.
    sys.plant_mut(HYDRO).unwrap().start().unwrap();
    let mut uncapped_after_restoration = 0;
    for t in 62..120 {
        let restored = sys.protection().mode == GridMode::Normal;
        sys.tick(&region, january(), t, &mut rng);
        assert!(!sys.hydro_ceilings(t + 1).contains_key(&HYDRO), "tick {t}");
        if restored {
            assert!(
                hydro_output(&sys) > hour_1 + Megawatts::new(300.0),
                "tick {t}"
            );
            uncapped_after_restoration += 1;
        }
    }
    assert!(
        uncapped_after_restoration >= 5,
        "{uncapped_after_restoration}"
    );
    assert_eq!(sys.protection().shed, Ratio::ZERO);

    // The 02:00 re-plan schedules it again by the normal criteria.
    sys.tick(&region, january(), 120, &mut rng);
    assert!(sys.hydro_ceilings(121).contains_key(&HYDRO));
    let view = sys.hydro_schedule().plants[&HYDRO];
    assert_eq!(view.hourly[0], elapsed);
    assert_eq!(view.hourly[1], hour_1);
    assert!(view.hourly[2] > Megawatts::ZERO);
}

#[test]
fn a_budget_tight_plan_ramps_out_of_water_without_a_cliff() {
    let region = fast_thermal_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(21);
    sys.begin_day(&region, january(), &mut rng);
    sys.plant_mut(HYDRO)
        .unwrap()
        .reset_hydro_budget(MegawattHours::new(1_500.0));
    sys.plan_hydro(&region, january(), 0);
    let last = (0..24)
        .rev()
        .find(|&h| allocation(&sys, h) > Megawatts::ZERO);
    assert_eq!(last, Some(20));

    let ramp = hydro_ramp(&sys).value();
    let mut previous = hydro_output(&sys);
    let mut ticks = Vec::new();
    for t in 0..TICKS_PER_DAY {
        ticks.push(sys.tick(&region, january(), t, &mut rng));
        let now = hydro_output(&sys);
        assert!(
            (previous - now).value() <= ramp + 1e-9,
            "tick {t}: {previous:?} -> {now:?}"
        );
        assert!(sys.plant(HYDRO).unwrap().hydro_budget().unwrap() >= MegawattHours::ZERO);
        assert_eq!(sys.protection().shed, Ratio::ZERO, "tick {t}");
        previous = now;
    }
    assert!(!exhausted(&ticks));
    assert!(hour_energy(&ticks, 20) > 0.0);
    // Only the water the early ramp saved is left for the re-plans after 21:00.
    let late: f64 = (21..24).map(|h| hour_energy(&ticks, h)).sum();
    assert!(late < 0.01 * 1_500.0, "{late}");
}

#[test]
fn an_unforeseen_drop_ramps_at_the_ramp_rate() {
    let region = fast_thermal_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(3);
    sys.begin_day(&region, january(), &mut rng);
    run_month(&mut sys, &region, january(), 0..610, &mut rng);
    let before = hydro_output(&sys);
    assert!(close(before, allocation(&sys, 10)), "{before:?}");
    // As if a re-plan had cut the current hour by three ramps.
    let ramp = hydro_ramp(&sys);
    let cut = before - ramp * 3.0;
    sys.hydro_plan.get_mut(&HYDRO).unwrap().hourly[10] = cut;
    for t in 610..616 {
        let output = hydro_output(&sys);
        let (s, _) = sys.tick(&region, january(), t, &mut rng);
        let now = hydro_output(&sys);
        assert!((output - now) <= ramp + Megawatts::new(1e-9), "tick {t}");
        let expected = (now - ramp).max(cut);
        assert!(close(target_of(&sys, HYDRO.0), expected), "tick {t}");
        assert!(
            s.imbalance.abs() < Megawatts::new(30.0),
            "tick {t}: {:?}",
            s.imbalance
        );
    }
    assert!(close(hydro_output(&sys), cut));
}

#[test]
fn no_hour_spends_more_than_its_allocation() {
    // An 800 MW → 0 fall at 18:00: a ramp tail after the boundary would spend
    // several MWh of hour 18, whose allocation is 0.
    let region = fast_thermal_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(4);
    let mut ticks = eligible_until_six_pm(&mut sys, &region, &mut rng);
    ticks.extend(run_month(
        &mut sys,
        &region,
        january(),
        1000..TICKS_PER_DAY,
        &mut rng,
    ));
    assert!(ticks.iter().all(|(s, _)| s.shed == Ratio::ZERO));
    let tolerance = hydro_ramp(&sys).value() / 60.0;
    let view = sys.hydro_schedule().plants[&HYDRO];
    assert_eq!(view.hourly[18], Megawatts::ZERO);
    for h in 0..24 {
        let energy = hour_energy(&ticks, h);
        assert!(
            energy <= view.hourly[h].value() + tolerance,
            "hour {h}: {energy}"
        );
    }

    // The tiny region's own evening, with several smaller foreseen falls.
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(21);
    sys.begin_day(&region, january(), &mut rng);
    let ticks = run_month(&mut sys, &region, january(), 0..TICKS_PER_DAY, &mut rng);
    assert!(ticks.iter().all(|(s, _)| s.shed == Ratio::ZERO));
    let view = sys.hydro_schedule().plants[&HYDRO];
    let mut falling = 0;
    for h in 0..24 {
        let energy = hour_energy(&ticks, h);
        assert!(
            energy <= view.hourly[h].value() + tolerance,
            "hour {h}: {energy} > {:?}",
            view.hourly[h]
        );
        if h < 23 && view.hourly[h + 1] < view.hourly[h] {
            falling += 1;
        }
    }
    // The evening has several foreseen falling boundaries.
    assert!(falling >= 4, "{falling}");
}

#[test]
fn the_last_hour_is_not_ramped_to_zero_before_midnight() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(21);
    sys.begin_day(&region, january(), &mut rng);
    let ticks = run_month(&mut sys, &region, january(), 0..TICKS_PER_DAY, &mut rng);
    let last_hour = sys.hydro_schedule().plants[&HYDRO].hourly[23];
    assert!(last_hour > Megawatts::new(100.0));
    for (s, _) in &ticks[23 * 60 - 2..] {
        eprintln!(
            "t={} out={:?} last={last_hour:?}",
            s.tick, s.by_plant[&HYDRO]
        );
    }
    // Hydro holds its last allocation to midnight (the first ticks of the hour
    // may still follow the previous plan's figure for it, a few kW apart).
    for (s, _) in &ticks[23 * 60..] {
        let gap = (s.by_plant[&HYDRO] - last_hour).abs();
        assert!(gap < Megawatts::new(0.1), "tick {}: {gap:?}", s.tick);
    }
    assert!(!exhausted(&ticks));
}

#[test]
fn turning_agc_off_unplans_hydro_until_the_next_re_plan() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(8);
    sys.begin_day(&region, january(), &mut rng);
    run_month(&mut sys, &region, january(), 0..605, &mut rng);
    let elapsed = sys.hydro_schedule().plants[&HYDRO].hourly[9];
    let hour_10 = allocation(&sys, 10);
    assert!(sys.hydro_ceilings(606).contains_key(&HYDRO));

    // Off and straight back on within the hour: no re-plan, but no cap either.
    sys.set_auto_follow(HYDRO, false).unwrap();
    sys.set_auto_follow(HYDRO, true).unwrap();
    let view = sys.hydro_schedule().plants[&HYDRO];
    assert_eq!(view.hourly[9], elapsed);
    assert!(view.hourly[10..].iter().all(|mw| *mw == Megawatts::ZERO));
    let mut above = false;
    for t in 605..660 {
        sys.tick(&region, january(), t, &mut rng);
        assert!(!sys.hydro_ceilings(t + 1).contains_key(&HYDRO), "tick {t}");
        above |= hydro_output(&sys) > hour_10 + Megawatts::new(10.0);
    }
    // Hydro is cheapest, so uncapped AGC raises it past its old allocation.
    assert!(above);

    sys.tick(&region, january(), 660, &mut rng);
    assert!(sys.hydro_ceilings(661).contains_key(&HYDRO));
    assert_eq!(sys.hydro_schedule().plants[&HYDRO].hourly[9], elapsed);
}

#[test]
fn uncommitted_hours_get_zero_allocation() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut hours = [false; 24];
    hours[6..=21].fill(true);
    let p = sys.plant_mut(HYDRO).unwrap();
    let schedule = CommitmentSchedule::try_new(hours, p.spec()).unwrap();
    p.set_commitment(schedule);
    let mut rng = SimRng::from_seed(5);
    sys.begin_day(&region, january(), &mut rng);
    let budget = sys.plant(HYDRO).unwrap().hydro_budget().unwrap().value();
    let plan = sys.hydro_schedule().plants[&HYDRO];
    for (h, mw) in plan.hourly.iter().enumerate() {
        if hours[h] {
            assert!(*mw > Megawatts::ZERO, "hour {h}");
        } else {
            assert_eq!(*mw, Megawatts::ZERO, "hour {h}");
        }
    }
    let planned: f64 = plan.hourly.iter().map(|mw| mw.value()).sum();
    assert!((planned - budget).abs() <= 1e-6 * budget);
    assert!(plan.level.is_some());

    run_month(&mut sys, &region, january(), 0..TICKS_PER_DAY, &mut rng);
    let plan = sys.hydro_schedule().plants[&HYDRO];
    assert!(
        (0..24)
            .filter(|&h| !hours[h])
            .all(|h| plan.hourly[h] == Megawatts::ZERO)
    );
}

#[test]
fn hydro_schedule_view_freezes_elapsed_hours() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(12);
    sys.begin_day(&region, january(), &mut rng);
    // Each hour's allocation just after the re-plan that opens it.
    let mut at_start = vec![allocation(&sys, 0)];
    let mut next = 0;
    for h in 1..=3u32 {
        run_month(&mut sys, &region, january(), next..h * 60 + 1, &mut rng);
        next = h * 60 + 1;
        at_start.push(allocation(&sys, h as usize));
    }
    run_month(&mut sys, &region, january(), next..200, &mut rng);

    let view = sys.hydro_schedule().plants[&HYDRO];
    let plan = sys.hydro_plan[&HYDRO];
    for (h, planned) in at_start.iter().enumerate() {
        assert_eq!(view.hourly[h], *planned, "hour {h}");
        assert!(view.hourly[h] > Megawatts::ZERO);
        if h < 3 {
            // The latest plan no longer allocates the elapsed hour.
            assert_eq!(plan.hourly[h], Megawatts::ZERO);
        }
    }
    assert_eq!(view.hourly[3..], plan.hourly[3..]);
    assert_eq!(view.level, plan.level);
    assert_eq!(
        view.budget_remaining,
        sys.plant(HYDRO).unwrap().hydro_budget().unwrap()
    );
}

#[test]
fn planning_draws_no_rng() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    let mut rng = SimRng::from_seed(31);
    sys.begin_day(&region, january(), &mut rng);
    run_month(&mut sys, &region, january(), 0..120, &mut rng);
    assert!(sys.hydro_schedule().plants.contains_key(&HYDRO));
    let mut reference = SimRng::from_seed(31);
    for _ in 0..1 + 120 {
        reference.standard_normal();
    }
    assert_eq!(rng, reference);
}

// ---- Over-frequency trips (spec §14.2, §14.6 "Trip order", "The surplus clears") ----

/// `defs` committed all day on auto-follow (solar stays as it is).
fn committed_auto_follow(region: &RegionDefinition, defs: &[PlantDef]) -> PowerSystem {
    let mut sys = system_with(region, defs);
    let ids: Vec<PlantId> = sys.plants().keys().copied().collect();
    for id in ids {
        let p = sys.plant_mut(id).unwrap();
        p.set_commitment(CommitmentSchedule::all_day());
        if p.kind().is_dispatchable() {
            p.set_auto_follow(true).unwrap();
        }
    }
    sys
}

/// Applies one `OverFrequencyTrip` outcome with `served` load and returns
/// its events.
fn over_frequency_trip_serving(sys: &mut PowerSystem, served: Megawatts) -> Vec<DomainEvent> {
    let region = tiny_region();
    let (available, curtailable, withheld) = sys.availability(&region, april(), 0);
    let redispatch = Redispatch {
        available: &available,
        to_serve: served,
        withheld,
        curtailable,
        effect_tick: 1,
    };
    let mut events = Vec::new();
    sys.apply_protection(
        ProtectionOutcome::OverFrequencyTrip,
        Megawatts::ZERO,
        served,
        region.frequency(),
        &redispatch,
        &mut events,
    );
    events
}

/// Applies one `OverFrequencyTrip` outcome with nothing served, so the
/// headroom gate never withholds it, and returns its events.
fn over_frequency_trip(sys: &mut PowerSystem) -> Vec<DomainEvent> {
    over_frequency_trip_serving(sys, Megawatts::ZERO)
}

fn tripped(plant: PlantId) -> Vec<DomainEvent> {
    vec![
        DomainEvent::OverFrequencyTrip { plant },
        DomainEvent::PlantTripped { plant },
    ]
}

#[test]
fn trips_most_expensive_producer_and_never_solar() {
    let region = tiny_region();
    // Plant 1 is a disconnected peaker: as expensive as plant 4 and lower in
    // id, so only the connectivity filter keeps it from tripping first.
    let defs = [
        PlantDef {
            kind: PlantKind::Peaker,
            cell: cell(6, 6),
            connected: false,
        },
        connected(PlantKind::Coal, cell(7, 3)),
        connected(PlantKind::Ccgt, cell(8, 4)),
        connected(PlantKind::Peaker, cell(6, 5)),
        connected(PlantKind::Solar, cell(5, 5)),
    ];
    let (disconnected, coal, ccgt, peaker, solar) =
        (PlantId(1), PlantId(2), PlantId(3), PlantId(4), PlantId(5));
    let mut sys = committed_auto_follow(&region, &defs);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    for id in [disconnected, coal, ccgt, peaker, solar] {
        assert!(sys.plant(id).unwrap().is_online(), "{id:?}");
    }

    assert_eq!(over_frequency_trip(&mut sys), tripped(peaker));
    assert_eq!(over_frequency_trip(&mut sys), tripped(ccgt));
    assert_eq!(over_frequency_trip(&mut sys), tripped(coal));
    // Only solar (never tripped) and the disconnected peaker are left: nothing happens.
    assert!(over_frequency_trip(&mut sys).is_empty());

    for id in [coal, ccgt, peaker] {
        let p = sys.plant(id).unwrap();
        assert_eq!(p.status(), PlantStatus::Offline, "{id:?}");
        assert!(p.is_commitment_suppressed(), "{id:?}");
    }
    for id in [disconnected, solar] {
        let p = sys.plant(id).unwrap();
        assert!(p.is_online(), "{id:?}");
        assert!(!p.is_commitment_suppressed(), "{id:?}");
    }
}

#[test]
fn equal_cost_trips_lowest_id_first() {
    let region = tiny_region();
    let defs = [
        connected(PlantKind::Ccgt, cell(8, 4)),
        connected(PlantKind::Ccgt, cell(6, 5)),
    ];
    let mut sys = committed_auto_follow(&region, &defs);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    assert_eq!(over_frequency_trip(&mut sys), tripped(PlantId(1)));
    assert_eq!(over_frequency_trip(&mut sys), tripped(PlantId(2)));
}

#[test]
fn only_online_producers_trip() {
    let region = tiny_region();
    let defs = [
        connected(PlantKind::Peaker, cell(6, 5)),
        connected(PlantKind::Hydro, cell(10, 1)),
        connected(PlantKind::Coal, cell(7, 3)),
    ];
    let (peaker, hydro, coal) = (PlantId(1), PlantId(2), PlantId(3));
    let mut sys = committed_auto_follow(&region, &defs);
    // Manual hydro opens the day online at its 0 MW initial target.
    sys.set_auto_follow(hydro, false).unwrap();
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    // A starting peaker produces nothing yet; stopping coal still produces
    // but is no longer online.
    sys.plant_mut(peaker).unwrap().trip();
    sys.plant_mut(peaker).unwrap().start().unwrap();
    sys.plant_mut(coal).unwrap().stop().unwrap();
    assert!(matches!(
        sys.plant(peaker).unwrap().status(),
        PlantStatus::Starting { .. }
    ));
    assert!(sys.plant(hydro).unwrap().is_online());
    assert_eq!(sys.plant(hydro).unwrap().output(), Megawatts::ZERO);
    assert!(sys.plant(coal).unwrap().output() > Megawatts::ZERO);

    assert!(over_frequency_trip(&mut sys).is_empty());
}

#[test]
fn a_trip_beyond_the_fleets_ramp_headroom_is_withheld_until_it_fits() {
    let region = tiny_region();
    let n = region.frequency().trip_after_ticks;
    let defs = [
        connected(PlantKind::Coal, cell(7, 3)),
        connected(PlantKind::Peaker, cell(6, 5)),
    ];
    let (coal, peaker) = (PlantId(1), PlantId(2));
    let mut sys = committed_auto_follow(&region, &defs);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    for id in [coal, peaker] {
        sys.plant_mut(id).unwrap().force_to_minimum();
    }
    // Coal at 240 MW can ramp up 6 MW in a tick; the peaker produces 40 MW.
    let ramp = sys.plant(coal).unwrap().spec().ramp.per_tick();
    assert_eq!(ramp, Megawatts::new(6.0));
    let output = supply(&sys);
    assert_eq!(output, Megawatts::new(280.0));
    sys.protection.over_armed = true;

    // A 30 MW surplus: tripping the peaker would open a 10 MW deficit > 6 MW.
    let events = over_frequency_trip_serving(&mut sys, output - Megawatts::new(30.0));
    assert!(events.is_empty());
    assert!(sys.plant(peaker).unwrap().is_online());
    assert_eq!(
        (sys.protection.over_ticks, sys.protection.over_armed),
        (n, true)
    );

    // A 34 MW surplus: the 6 MW deficit fits the headroom, so the trip fires.
    let events = over_frequency_trip_serving(&mut sys, output - Megawatts::new(34.0));
    assert_eq!(events, tripped(peaker));
}

#[test]
fn headroom_counts_only_what_agc_may_dispatch() {
    // A dry hydro plant has 800 MW of nameplate room but no water: AGC can't
    // raise it, so it must not make the peaker's trip look safe.
    let region = tiny_region();
    let n = region.frequency().trip_after_ticks;
    let defs = [
        connected(PlantKind::Peaker, cell(6, 5)),
        connected(PlantKind::Hydro, cell(10, 1)),
    ];
    let (peaker, hydro) = (PlantId(1), PlantId(2));
    let setup = |budget: f64| {
        let mut sys = committed_auto_follow(&region, &defs);
        sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
        for id in [peaker, hydro] {
            sys.plant_mut(id).unwrap().force_to_minimum();
        }
        sys.plant_mut(hydro)
            .unwrap()
            .reset_hydro_budget(MegawattHours::new(budget));
        sys.protection.over_armed = true;
        sys
    };
    // 40 MW of peaker against a 10 MW surplus: a 30 MW deficit.
    let served = Megawatts::new(30.0);

    let mut dry = setup(0.0);
    assert_eq!(dry.plant(hydro).unwrap().output(), Megawatts::ZERO);
    assert!(dry.plant(hydro).unwrap().headroom_up(Megawatts::new(800.0)) >= Megawatts::new(30.0));
    assert!(over_frequency_trip_serving(&mut dry, served).is_empty());
    assert!(dry.plant(peaker).unwrap().is_online());
    assert_eq!(dry.protection.over_ticks, n);

    // With water, the same trip fits the hydro's one-tick ramp.
    let mut wet = setup(10_000.0);
    assert_eq!(
        over_frequency_trip_serving(&mut wet, served),
        tripped(peaker)
    );
}

#[test]
fn an_all_solar_grid_has_nothing_to_trip() {
    let region = tiny_region();
    let mut sys = committed_auto_follow(&region, &[connected(PlantKind::Solar, cell(5, 5))]);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    assert!(over_frequency_trip(&mut sys).is_empty());
}

/// The reviewer's probe: after running ticks `warm_up` (no trips), plant
/// 2 is nudged into a small surplus at noon with protection one tick from a
/// trip at a modest deviation. Returns the summaries of ticks 720..740.
fn modest_deviation_trip(
    region: &RegionDefinition,
    defs: &[PlantDef],
    warm_up: std::ops::Range<u32>,
) -> Vec<TickSummary> {
    let n = region.frequency().trip_after_ticks;
    let mut sys = committed_auto_follow(region, defs);
    let mut rng = SimRng::from_seed(1);
    sys.begin_day(region, april(), &mut rng);
    for (_, events) in run_ticks(&mut sys, region, warm_up, &mut rng) {
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, DomainEvent::PlantTripped { .. }))
        );
    }
    sys.deviation = Hertz::new(0.6);
    sys.protection.over_armed = true;
    sys.protection.over_ticks = n - 1;
    let ccgt = sys.plant_mut(PlantId(2)).unwrap();
    let target = ccgt.output() + Megawatts::new(20.0);
    ccgt.set_target(target).unwrap();
    let mut out = Vec::new();
    for t in 720..740 {
        let (s, events) = sys.tick(region, april(), t, &mut rng);
        let trip = events
            .iter()
            .any(|e| matches!(e, DomainEvent::OverFrequencyTrip { .. }));
        assert_eq!(trip, t == 720, "tick {t}");
        out.push(s);
    }
    out
}

#[test]
fn a_trip_from_a_modest_deviation_does_not_shed() {
    // Fast ramps: the trip opens a ~200 MW deficit that the other CCGT and
    // coal can cover in one tick, but only if AGC re-runs after the trip.
    let fast = fast_ramp_region();
    let ticks = modest_deviation_trip(
        &fast,
        &[
            connected(PlantKind::Coal, cell(7, 3)),
            connected(PlantKind::Ccgt, cell(8, 4)),
            connected(PlantKind::Ccgt, cell(6, 5)),
        ],
        600..720,
    );
    assert!(ticks[0].served - ticks[0].generation > Megawatts::new(150.0));
    // The next tick's generation is AGC's post-trip dispatch: tick 720's load.
    assert!(close(ticks[1].generation, ticks[0].served));
    assert!(ticks.iter().all(|s| s.unserved == Megawatts::ZERO));

    // Real ramps: the peaker trips and the CCGT (coal is at capacity) ramps
    // up within its 25 MW tick to replace it on the very next tick.
    let tiny = tiny_region();
    let ticks = modest_deviation_trip(&tiny, &start_fleet(&tiny), 0..720);
    let rise = ticks[1].generation - ticks[0].generation;
    assert!(rise > Megawatts::ZERO);
    assert!(rise <= tiny.spec(PlantKind::Ccgt).ramp.per_tick());
    assert!(close(ticks[1].generation, ticks[0].served));
    assert!(ticks.iter().all(|s| s.unserved == Megawatts::ZERO));
}

/// Coal 1, CCGT 2, peaker 3 and hydro 4, all committed all day on AGC,
/// at the start of an April day with protection armed. The peaker is the
/// trip candidate.
fn falling_output_fleet(region: &RegionDefinition) -> PowerSystem {
    let defs = [
        connected(PlantKind::Coal, cell(7, 3)),
        connected(PlantKind::Ccgt, cell(8, 4)),
        connected(PlantKind::Peaker, cell(6, 5)),
        connected(PlantKind::Hydro, cell(10, 1)),
    ];
    let mut sys = committed_auto_follow(region, &defs);
    sys.begin_day(region, april(), &mut SimRng::from_seed(1));
    sys.protection.over_armed = true;
    sys
}

/// One `OverFrequencyTrip` outcome against a 100 MW surplus, well within the
/// headroom gate.
fn trip_with_ample_surplus(sys: &mut PowerSystem) -> Vec<DomainEvent> {
    let output = supply(sys);
    over_frequency_trip_serving(sys, output - Megawatts::new(100.0))
}

fn held_window(sys: &PowerSystem) -> bool {
    let n = tiny_region().frequency().trip_after_ticks;
    (sys.protection.over_ticks, sys.protection.over_armed) == (n, true)
}

#[test]
fn a_trip_waits_while_scheduled_hydro_ramps_down() {
    let region = tiny_region();
    let peaker = PlantId(3);
    let mut sys = falling_output_fleet(&region);
    let output = hydro_output(&sys);
    assert!(output > Megawatts::new(100.0));
    // The schedule now sits 50 MW below the hydro's output: AGC ramps it down.
    let plan = sys.hydro_plan.get_mut(&HYDRO).unwrap();
    plan.planned = true;
    plan.hourly = [output - Megawatts::new(50.0); 24];
    assert!(trip_with_ample_surplus(&mut sys).is_empty());
    assert!(held_window(&sys));
    // While the cap is lifted (load shed), the hydro is not held down.
    sys.protection.shed = Ratio::new(0.05).unwrap();
    assert_eq!(trip_with_ample_surplus(&mut sys), tripped(peaker));

    // Once the fall is over (the schedule is at or above output), it trips.
    let mut sys = falling_output_fleet(&region);
    let output = hydro_output(&sys);
    let plan = sys.hydro_plan.get_mut(&HYDRO).unwrap();
    plan.planned = true;
    plan.hourly = [output; 24];
    assert_eq!(trip_with_ample_surplus(&mut sys), tripped(peaker));
}

#[test]
fn a_trip_waits_while_a_plant_stops() {
    let region = tiny_region();
    let mut sys = falling_output_fleet(&region);
    sys.plant_mut(PlantId(1)).unwrap().stop().unwrap();
    assert!(sys.plant(PlantId(1)).unwrap().output() > Megawatts::ZERO);
    assert!(trip_with_ample_surplus(&mut sys).is_empty());
    assert!(held_window(&sys));
}

#[test]
fn a_trip_waits_while_a_manual_plant_ramps_down() {
    let region = tiny_region();
    let mut sys = falling_output_fleet(&region);
    // Hydro taken off AGC and told to come down to 0 MW.
    sys.set_auto_follow(HYDRO, false).unwrap();
    let output = hydro_output(&sys);
    assert!(output > Megawatts::ZERO);
    sys.plant_mut(HYDRO)
        .unwrap()
        .set_target(Megawatts::ZERO)
        .unwrap();
    assert!(trip_with_ample_surplus(&mut sys).is_empty());
    assert!(held_window(&sys));
    // A manual plant holding its output is not falling: the trip fires.
    sys.plant_mut(HYDRO).unwrap().set_target(output).unwrap();
    assert_eq!(trip_with_ample_surplus(&mut sys), tripped(PlantId(3)));
}

/// Brings an online plant to 0 MW on a manual target.
fn run_down_to_zero(sys: &mut PowerSystem, id: PlantId) {
    sys.set_auto_follow(id, false).unwrap();
    let plant = sys.plant_mut(id).unwrap();
    plant.set_target(Megawatts::ZERO).unwrap();
    while plant.output() > Megawatts::ZERO {
        plant.step(plant.spec().capacity);
    }
    assert!(plant.is_online());
}

#[test]
fn a_plant_stopping_from_zero_output_does_not_hold_a_trip() {
    let region = tiny_region();
    let mut sys = falling_output_fleet(&region);
    // Hydro at 0 MW, then stopped: stopping, but with no output left to lose.
    run_down_to_zero(&mut sys, HYDRO);
    sys.plant_mut(HYDRO).unwrap().stop().unwrap();
    assert_eq!(
        sys.plant(HYDRO).unwrap().status(),
        PlantStatus::Stopping {
            output: Megawatts::ZERO
        }
    );
    assert_eq!(trip_with_ample_surplus(&mut sys), tripped(PlantId(3)));
}

#[test]
fn an_online_plant_at_zero_output_is_never_tripped() {
    let region = tiny_region();
    let hydro = PlantId(1);
    let mut sys = committed_auto_follow(&region, &[connected(PlantKind::Hydro, cell(10, 1))]);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    run_down_to_zero(&mut sys, hydro);
    assert!(over_frequency_trip(&mut sys).is_empty());
    assert!(sys.plant(hydro).unwrap().is_online());
}

#[test]
fn a_hydro_fall_after_the_next_re_plan_does_not_hold_a_trip() {
    let region = tiny_region();
    let mut sys = falling_output_fleet(&region);
    let output = hydro_output(&sys);
    // The schedule holds the hydro's output through 01:59 and falls 100 MW
    // at 02:00. The trip acts on tick 1, and the plan is known only up to
    // the 01:00 re-plan, before the fall, so nothing holds the trip.
    let plan = sys.hydro_plan.get_mut(&HYDRO).unwrap();
    plan.planned = true;
    plan.hourly = [output - Megawatts::new(100.0); 24];
    plan.hourly[0] = output;
    plan.hourly[1] = output;
    assert_eq!(trip_with_ample_surplus(&mut sys), tripped(PlantId(3)));
}

#[test]
fn agc_lowers_scheduled_hydro_sitting_exactly_at_its_ceiling() {
    let region = tiny_region();
    let mut sys = falling_output_fleet(&region);
    let output = hydro_output(&sys);
    let plan = sys.hydro_plan.get_mut(&HYDRO).unwrap();
    plan.planned = true;
    plan.hourly = [output; 24];
    assert_eq!(sys.hydro_ceilings(1)[&HYDRO], output);
    // A surplus 50 MW beyond what the thermal plants free by falling to
    // their minimums: only the $0 hydro is left to lower, and it is at its
    // ceiling, not above it, so AGC dispatches it rather than ramping it.
    let thermal_room: Megawatts = [1, 2, 3]
        .map(|id| {
            let plant = sys.plant(PlantId(id)).unwrap();
            plant.output() - plant.spec().min_output()
        })
        .into_iter()
        .sum();
    let (available, _, _) = sys.availability(&region, april(), 0);
    sys.retarget_agc(&available, -(thermal_room + Megawatts::new(50.0)), 1);
    assert!(target_of(&sys, HYDRO.0) < output);
}

#[test]
fn hydro_without_water_is_not_scheduled() {
    let region = tiny_region();
    let mut sys = falling_output_fleet(&region);
    assert!(sys.hydro_schedule().plants.contains_key(&HYDRO));
    sys.plant_mut(HYDRO)
        .unwrap()
        .reset_hydro_budget(MegawattHours::ZERO);
    sys.plan_hydro(&region, april(), 0);
    assert!(!sys.hydro_schedule().plants.contains_key(&HYDRO));
    assert!(sys.hydro_ceilings(1).is_empty());
}

#[test]
fn hourly_net_load_is_demand_less_expected_solar() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(7, 1)));
    let sys = committed_auto_follow(&region, &defs);
    let net = sys.hourly_net_load(&region, april());
    let tick = 12 * TICKS_PER_HOUR + 30;
    let solar = sys.expected_solar(&region, april(), tick);
    assert!(solar > Megawatts::ZERO);
    assert_eq!(
        net[12],
        sys.connected_demand_forecast(&region, april(), tick) - solar
    );
}

#[test]
fn a_trip_leaves_solar_curtailed_by_what_the_redispatch_cannot_absorb() {
    let region = tiny_region();
    let mut defs = start_fleet(&region);
    defs.push(connected(PlantKind::Solar, cell(7, 1)));
    let mut sys = committed_auto_follow(&region, &defs);
    let mut rng = SimRng::from_seed(1);
    sys.begin_day(&region, april(), &mut rng);
    let noon = 12 * TICKS_PER_HOUR;
    run_ticks(&mut sys, &region, 0..noon, &mut rng);
    // Serving only 100 MW at noon: even with coal and CCGT at their
    // minimums after the peaker trips, solar must be held back.
    let (available, curtailable, withheld) = sys.availability(&region, april(), noon);
    assert!(curtailable > Megawatts::ZERO);
    let redispatch = Redispatch {
        available: &available,
        to_serve: Megawatts::new(100.0),
        withheld,
        curtailable,
        effect_tick: noon + 1,
    };
    let mut events = Vec::new();
    sys.apply_protection(
        ProtectionOutcome::OverFrequencyTrip,
        Megawatts::ZERO,
        Megawatts::new(100.0),
        region.frequency(),
        &redispatch,
        &mut events,
    );
    assert_eq!(events[..2], tripped(PlantId(3))[..]);
    let curtailment = sys.curtailment;
    assert!(curtailment > Megawatts::ZERO && curtailment <= curtailable);
    assert_eq!(
        events.last(),
        Some(&DomainEvent::SolarCurtailed { mw: curtailment })
    );
}

/// What one hydro-cut probe run did.
struct CutRun {
    trips: Vec<(u32, PlantId)>,
    /// Σ unserved MW over the run's ticks.
    shed: f64,
    blackout: bool,
}

/// The re-reviewers' probe: after warming up to tick `start`, the hydro's
/// schedule holds its output for the current hour and is cut by `cut` from
/// hour `from_hour` on, and CCGT 2 is nudged `bump` MW into a small surplus
/// at +0.6 Hz, with protection one tick from a due trip when `armed`. Runs 40
/// ticks.
fn hydro_cut_with_trip_due(
    start: u32,
    from_hour: usize,
    cut: f64,
    bump: f64,
    armed: bool,
) -> CutRun {
    let region = tiny_region();
    let n = region.frequency().trip_after_ticks;
    let mut sys = falling_output_fleet(&region);
    sys.protection.over_armed = false;
    let mut rng = SimRng::from_seed(1);
    sys.begin_day(&region, april(), &mut rng);
    run_ticks(&mut sys, &region, 0..start, &mut rng);
    let output = hydro_output(&sys);
    let plan = sys.hydro_plan.get_mut(&HYDRO).unwrap();
    plan.planned = true;
    plan.hourly[(start / TICKS_PER_HOUR) as usize] = output;
    for h in from_hour..24 {
        plan.hourly[h] = (output - Megawatts::new(cut)).max(Megawatts::ZERO);
    }
    sys.deviation = Hertz::new(0.6);
    sys.protection.over_armed = armed;
    sys.protection.over_ticks = if armed { n - 1 } else { 0 };
    let ccgt = sys.plant_mut(PlantId(2)).unwrap();
    let target = ccgt.output() + Megawatts::new(bump);
    ccgt.set_target(target).unwrap();
    let mut run = CutRun {
        trips: Vec::new(),
        shed: 0.0,
        blackout: false,
    };
    for t in start..start + 40 {
        let (s, events) = sys.tick(&region, april(), t, &mut rng);
        for e in &events {
            if let DomainEvent::OverFrequencyTrip { plant } = e {
                run.trips.push((t, *plant));
            }
        }
        run.shed += s.unserved.value();
        run.blackout |= s.mode == GridMode::Blackout;
    }
    run
}

#[test]
fn trips_wait_out_a_scheduled_hydro_fall() {
    // (start, from_hour, cut, bump): a mid-hour cut of the current hour at
    // 12:10, where the hydro falls 100 MW at once (an ungated trip there
    // sheds 447 and 357 MW·ticks); and cuts of hour 13 that make a foreseen
    // envelope fall of 100 MW a tick ending at 13:00, with the trip due a few
    // ticks before the fall starts (without the lookahead, 150 MW sheds 532
    // MW·ticks and 300 MW blacks out). The 300 MW fall sheds by itself;
    // protection must not add to it.
    let cases = [
        (730, 12, 100.0, 10.0),
        (730, 12, 100.0, 15.0),
        (772, 13, 150.0, 20.0),
        (776, 13, 200.0, 20.0),
        (776, 13, 250.0, 20.0),
        (770, 13, 300.0, 20.0),
        (776, 13, 300.0, 20.0),
    ];
    for (start, from_hour, cut, bump) in cases {
        let case = format!("start {start}, cut {cut} from hour {from_hour}, bump {bump}");
        let armed = hydro_cut_with_trip_due(start, from_hour, cut, bump, true);
        let unarmed = hydro_cut_with_trip_due(start, from_hour, cut, bump, false);
        // No trip before the fall ends at the next hour boundary.
        let fall_ends = (start / TICKS_PER_HOUR + 1) * TICKS_PER_HOUR;
        assert!(
            armed.trips.iter().all(|(t, _)| *t >= fall_ends),
            "{case}: {:?}",
            armed.trips
        );
        // Protection adds no shedding and no blackout to what the fall causes.
        assert_eq!(armed.shed, unarmed.shed, "{case}");
        assert!(!armed.blackout && !unarmed.blackout, "{case}");
    }
}

#[test]
fn a_withheld_trip_fires_on_the_next_tick_with_surplus_once_it_fits() {
    let region = tiny_region();
    let p = region.frequency();
    let mut s = ProtectionState {
        over_armed: true,
        ..ProtectionState::default()
    };
    s.withhold_trip(p);
    let (supply, demand) = (Megawatts::new(110.0), Megawatts::new(100.0));
    assert_eq!(
        evaluate(&mut s, Hertz::new(1.0), p, supply, demand),
        ProtectionOutcome::OverFrequencyTrip
    );
    // Without a surplus the held window resets instead.
    s.withhold_trip(p);
    assert_eq!(
        evaluate(&mut s, Hertz::new(1.0), p, demand, demand),
        ProtectionOutcome::Curtail
    );
    assert_eq!((s.over_ticks, s.over_armed), (0, true));
}

#[test]
fn an_over_frequency_trip_unplans_tripped_hydro() {
    let region = tiny_region();
    let mut sys = west_like_fleet(&region);
    sys.begin_day(&region, january(), &mut SimRng::from_seed(1));
    for id in 1..=3 {
        sys.plant_mut(PlantId(id)).unwrap().trip();
    }
    assert!(hydro_output(&sys) > Megawatts::ZERO);
    assert!(sys.hydro_ceilings(1).contains_key(&HYDRO));
    assert_eq!(over_frequency_trip(&mut sys), tripped(HYDRO));
    assert!(!sys.hydro_ceilings(1).contains_key(&HYDRO));
    assert_eq!(sys.hydro_schedule().plants[&HYDRO].level, None);
}

#[test]
fn an_over_frequency_trip_tick_still_curtails_first() {
    // As `curtailment_is_what_forcing_agc_down_cannot_cover`, on a trip tick.
    let region = tiny_region();
    let mut sys = auto_follow_fleet(&region);
    sys.begin_day(&region, april(), &mut SimRng::from_seed(1));
    for (id, mw) in [(1, 310.0), (2, 200.0), (3, 40.0)] {
        sys.plant_mut(PlantId(id))
            .unwrap()
            .set_target(Megawatts::new(mw))
            .unwrap();
    }
    let mut events = Vec::new();
    sys.update_curtailment(
        ProtectionOutcome::OverFrequencyTrip,
        Megawatts::new(100.0),
        Megawatts::new(60.0),
        &mut events,
    );
    assert_eq!(
        events,
        vec![DomainEvent::SolarCurtailed {
            mw: Megawatts::new(30.0)
        }]
    );
    assert_eq!(target_of(&sys, 1), Megawatts::new(240.0));
}

/// The overbuild fixture: the start fleet (coal 1, CCGT 2, peaker 3) plus
/// CCGTs 4 and 5, all committed all day on auto-follow. Their minimum stable
/// output (880 MW) is far above April's night load (about 570–630 MW), so
/// with no player action the fleet sits at minimum in a night surplus.
fn overbuild_fleet(region: &RegionDefinition) -> PowerSystem {
    let mut defs = start_fleet(region);
    defs.push(connected(PlantKind::Ccgt, cell(4, 6)));
    defs.push(connected(PlantKind::Ccgt, cell(5, 6)));
    committed_auto_follow(region, &defs)
}

/// Runs a whole day and checks the over-frequency properties on every tick
/// (spec §14.2, §14.6, and both builder's-call gates):
///
/// 1. no load is ever shed and the grid never blacks out;
/// 2. trips are at least one window apart, and each takes the highest-cost
///    (then lowest-id) connected online dispatchable producer;
/// 3. on every armed tick above `curtail_above` without a trip, either the
///    window is still filling, or the cost-first candidate could not be
///    tripped safely: its output less the surplus exceeds the one-tick ramp-up
///    headroom of the other auto-follow plants.
///
/// Returns the trips and each tick's deviation.
fn run_day_checking_over_frequency(
    sys: &mut PowerSystem,
    region: &RegionDefinition,
    rng: &mut SimRng,
) -> (Vec<(u32, PlantId)>, Vec<Hertz>) {
    let p = region.frequency();
    let n = p.trip_after_ticks;
    let cost = |plant: &Plant| plant.spec().marginal_cost.value();
    let mut trips: Vec<(u32, PlantId)> = Vec::new();
    let mut devs = Vec::new();
    for t in 0..TICKS_PER_DAY {
        let (s, events) = sys.tick(region, april(), t, rng);
        let dev = s.frequency - p.nominal;
        devs.push(dev);
        // (1)
        assert_eq!(s.mode, GridMode::Normal, "tick {t}");
        assert_eq!(s.unserved, Megawatts::ZERO, "tick {t}");

        let producers: Vec<&Plant> = sys
            .connected_plants()
            .filter(|q| q.is_online() && q.kind().is_dispatchable() && q.output() > Megawatts::ZERO)
            .collect();
        let tripped: Vec<PlantId> = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::OverFrequencyTrip { plant } => Some(*plant),
                _ => None,
            })
            .collect();
        // (2)
        assert!(tripped.len() <= 1, "tick {t}");
        if let Some(&id) = tripped.first() {
            if let Some(&(last, _)) = trips.last() {
                assert!(t - last >= n, "tick {t}: two trips in one window");
            }
            let gone = &sys.plants()[&id];
            for q in &producers {
                assert!(
                    (cost(q), std::cmp::Reverse(q.id())) < (cost(gone), std::cmp::Reverse(id)),
                    "tick {t}: tripped {id:?} ahead of {:?}",
                    q.id()
                );
            }
            trips.push((t, id));
            continue;
        }
        // (3) The fleet has neither solar nor hydro, so every plant's
        // dispatchable ceiling is its capacity.
        let protection = sys.protection();
        if protection.over_armed && dev > p.curtail_above && protection.over_ticks >= n {
            let surplus = sys.connected_plants().map(Plant::output).sum::<Megawatts>() - s.served;
            let candidate = producers
                .iter()
                .max_by(|a, b| {
                    cost(a)
                        .total_cmp(&cost(b))
                        .then_with(|| b.id().cmp(&a.id()))
                })
                .expect("a producer while over-frequency");
            let headroom: Megawatts = sys
                .connected_plants()
                .filter(|o| o.id() != candidate.id() && o.auto_follow())
                .map(|o| o.headroom_up(o.spec().capacity))
                .sum();
            assert!(
                candidate.output() - surplus > headroom,
                "tick {t}: {:?} could trip safely but did not",
                candidate.id()
            );
        }
    }
    (trips, devs)
}

/// What one whole-day run did, for comparing protection armed and unarmed.
struct DayRun {
    trips: usize,
    /// Σ unserved MW over the day's ticks.
    shed: f64,
    blackout: bool,
}

/// The tiny region with over-frequency trips out of reach (`trip_above` far
/// above any deviation), otherwise identical.
fn unarmed_region() -> RegionDefinition {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.frequency.trip_above = Hertz::new(1.0e6);
    RegionDefinition::try_from(parts).unwrap()
}

/// Runs one whole day of the start fleet plus `extra` CCGTs (and the hydro
/// site when `hydro`), all committed all day on auto-follow.
fn protection_day(
    region: &RegionDefinition,
    extra: usize,
    hydro: bool,
    month: u8,
    seed: u64,
) -> DayRun {
    let mut defs = start_fleet(region);
    for &(x, y) in [(4, 6), (5, 6), (6, 6)].iter().take(extra) {
        defs.push(connected(PlantKind::Ccgt, cell(x, y)));
    }
    if hydro {
        defs.push(connected(PlantKind::Hydro, cell(10, 1)));
    }
    let mut sys = committed_auto_follow(region, &defs);
    let month = SimMonth::new(2027, month).unwrap();
    let mut rng = SimRng::from_seed(seed);
    sys.begin_day(region, month, &mut rng);
    let mut run = DayRun {
        trips: 0,
        shed: 0.0,
        blackout: false,
    };
    for t in 0..TICKS_PER_DAY {
        let (s, events) = sys.tick(region, month, t, &mut rng);
        run.trips += events
            .iter()
            .filter(|e| matches!(e, DomainEvent::OverFrequencyTrip { .. }))
            .count();
        run.shed += s.unserved.value();
        run.blackout |= s.mode == GridMode::Blackout;
    }
    run
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Armed over-frequency protection never adds shed energy or a blackout
    /// to the same day run with it out of reach (spec §14.6), over fleets
    /// from the start fleet (which sheds on summer peaks) to three surplus
    /// CCGTs (which trip at night), with or without hydro, in any month.
    #[test]
    fn armed_protection_never_sheds_more_than_unarmed(
        extra in 0usize..=3,
        hydro in any::<bool>(),
        month in 1u8..=12,
        seed in any::<u64>(),
    ) {
        let armed = protection_day(&tiny_region(), extra, hydro, month, seed);
        let unarmed = protection_day(&unarmed_region(), extra, hydro, month, seed);
        prop_assert_eq!(unarmed.trips, 0);
        prop_assert!(
            armed.shed <= unarmed.shed + 1e-6,
            "armed sheds {} MW·ticks, unarmed {}",
            armed.shed,
            unarmed.shed
        );
        prop_assert!(!armed.blackout || unarmed.blackout);
    }
}

#[test]
fn overbuild_surplus_clears_as_far_as_safe_trips_allow() {
    let region = tiny_region();
    let p = region.frequency();
    let n = p.trip_after_ticks;
    let mut sys = overbuild_fleet(&region);
    let mut rng = SimRng::from_seed(1);
    sys.begin_day(&region, april(), &mut rng);
    let minimum: Megawatts = sys.plants().values().map(|p| p.spec().min_output()).sum();
    assert_eq!(minimum, Megawatts::new(880.0));

    let (trips, devs) = run_day_checking_over_frequency(&mut sys, &region, &mut rng);
    // The peaker (most expensive), then CCGT 2 (lowest id of the CCGTs), one
    // window apart from the arming window on. That leaves 640 MW of minimum
    // (coal plus CCGTs 4 and 5) against a night load falling to about 570 MW:
    // tripping another 200 MW CCGT would open a deficit far beyond the
    // fleet's ~31 MW of one-tick ramp, so the residual surplus stays as
    // over-frequency until the morning load rise absorbs it.
    assert_eq!(trips, vec![(n - 1, PlantId(3)), (2 * n - 1, PlantId(2))]);
    assert!(devs[180] > p.trip_above, "03:00 still over-frequency");
    // (4) The forecast load passes 640 MW between 05:00 and 06:00 (shape
    // 0.68 to 0.76 of 900 MW), so from 06:00 frequency is back for the day.
    for (t, dev) in devs.iter().enumerate().skip(6 * TICKS_PER_HOUR as usize) {
        assert!(*dev <= p.curtail_above, "tick {t}: {dev:?}");
    }
}

#[test]
fn overbuild_surplus_fully_clears_when_the_fleet_can_cover_each_trip() {
    // With every plant able to ramp 1,000 MW per tick, each trip fits the
    // rest of the fleet's headroom, so the surplus clears completely.
    let region = fast_ramp_region();
    let p = region.frequency();
    let n = p.trip_after_ticks;
    let mut sys = overbuild_fleet(&region);
    let mut rng = SimRng::from_seed(1);
    sys.begin_day(&region, april(), &mut rng);

    let (trips, devs) = run_day_checking_over_frequency(&mut sys, &region, &mut rng);
    assert_eq!(
        trips,
        vec![
            (n - 1, PlantId(3)),
            (2 * n - 1, PlantId(2)),
            (3 * n - 1, PlantId(4))
        ]
    );
    // Bound: one window per tripped plant, then two windows for frequency to
    // settle. The redispatch after each trip leaves no deficit to pull the
    // deviation down, so it decays by the damping factor alone: from at most
    // +3 Hz at the last trip, 3 × (1 − k_damping)^(2n) = 3 × 0.8^10 ≈ 0.32 Hz,
    // below `curtail_above`.
    let last = devs[(3 * n - 1) as usize];
    assert!(last <= Hertz::new(3.0), "{last:?}");
    assert_eq!(p.k_damping, 0.2);
    let surplus_plants = 3;
    let bound = n * (surplus_plants + 2);
    for (t, dev) in devs.iter().enumerate().skip(bound as usize) {
        assert!(*dev <= p.curtail_above, "tick {t}: {dev:?}");
    }
}
