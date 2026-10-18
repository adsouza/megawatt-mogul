use super::*;
use crate::demand::shape_factor;
use crate::generation::{CommitmentSchedule, PlantStatus};
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
