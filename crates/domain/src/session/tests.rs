use super::*;
use crate::generation::PlantStatus;
use crate::grid::{GridMode, NetworkModel};
use crate::ids::CityId;
use crate::region::PlantKind;
use crate::region::fixtures::tiny_region;
use crate::regulation::Licence;
use crate::time::{SimMonth, TICKS_PER_DAY};
use crate::units::{Hertz, Megawatts, Ratio};

fn session() -> GameSession {
    GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default())
}

fn plant_of(s: &GameSession, kind: PlantKind) -> PlantId {
    s.plant_views()
        .iter()
        .find(|p| p.kind == kind)
        .expect("plant of kind")
        .id
}

fn run_ticks(s: &mut GameSession, n: u32) -> Vec<DomainEvent> {
    (0..n).flat_map(|_| s.tick().unwrap()).collect()
}

fn may() -> SimMonth {
    SimMonth::new(2027, 5).unwrap()
}

#[test]
fn new_session_matches_start_definition() {
    let s = GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default());
    assert_eq!(s.phase(), DayPhase::Planning);
    assert_eq!(s.company().cash(), Money::from_dollars(2_000_000_000));
    assert_eq!(
        s.plant_views()
            .iter()
            .filter(|p| p.connected && p.status == PlantStatus::Offline)
            .count(),
        3
    );
    assert_eq!(s.city_views().iter().filter(|c| c.connected).count(), 2);
}

#[test]
fn new_session_starts_active_in_the_start_month() {
    let s = session();
    assert_eq!(s.status(), SessionStatus::Active);
    assert_eq!(s.clock().month(), SimMonth::new(2027, 4).unwrap());
    assert_eq!(s.clock().tick(), 0);
    assert_eq!(s.settings(), &GameSettings::default());
    assert_eq!(s.settings().fidelity.network, NetworkModel::CopperPlate);
    assert_eq!(s.region().id().as_str(), "tiny");
    assert!(s.plant_views().iter().all(|p| p.commitment == [false; 24]));
    let snapshot = s.grid_snapshot();
    assert_eq!(snapshot.phase, DayPhase::Planning);
    assert_eq!(snapshot.frequency, Hertz::new(60.0));
    assert_eq!(snapshot.last_tick, None);
}

#[test]
fn start_fleet_and_lines_enter_the_rate_base_as_legacy_capital() {
    let s = session();
    let region = tiny_region();
    let rate_base = s.company().rate_base();
    let coal = plant_of(&s, PlantKind::Coal);
    let asset = rate_base.asset(AssetRef::Plant(coal)).unwrap();
    assert!(asset.legacy);
    assert_eq!(asset.original, region.spec(PlantKind::Coal).capital_cost);
    assert_eq!(asset.added_on, region.start().month);
    let coal_node = s.power().plant(coal).unwrap().node();
    assert_eq!(
        asset.profile,
        AssetProfile::Plant {
            firm: Megawatts::new(600.0 * 0.95),
            node: coal_node
        }
    );
    // Two city lines and three plant lines, all in service and legacy.
    let lines: Vec<_> = s.power().network().lines().collect();
    assert_eq!(lines.len(), 5);
    assert!(lines.iter().all(|l| l.status == LineStatus::InService));
    // City 2 at (8, 6) is sqrt(5) cells from the substation at (7, 4): 3 whole cells.
    let city2 = s
        .city_views()
        .into_iter()
        .find(|c| c.id == CityId(2))
        .unwrap();
    let line = lines.iter().find(|l| l.to == city2.node).unwrap();
    let line_asset = rate_base.asset(AssetRef::Line(line.id)).unwrap();
    assert!(line_asset.legacy);
    assert_eq!(line_asset.original, Money::from_dollars(120_000_000));
    assert_eq!(
        line_asset.profile,
        AssetProfile::Line {
            from: s.substation,
            to: city2.node
        }
    );
    assert_eq!(rate_base.assets().count(), 8);
    assert!(rate_base.assets().all(|a| a.legacy));
    // The start fleet is established, not returning from a month offline.
    assert!(
        s.power()
            .plants()
            .values()
            .all(|p| !p.previous_month_offline())
    );
    assert_eq!(s.regulator().tally().firm_added, Megawatts::ZERO);
}

#[test]
fn building_is_planning_only_and_ticking_is_operating_only() {
    let mut s = session();
    assert_eq!(
        s.tick().unwrap_err(),
        DomainError::WrongPhase {
            actual: DayPhase::Planning
        }
    );
    s.begin_operating().unwrap();
    assert_eq!(
        s.build_plant(PlantKind::Peaker, CellCoord { x: 5, y: 5 })
            .unwrap_err(),
        DomainError::WrongPhase {
            actual: DayPhase::Operating
        }
    );
    assert_eq!(
        s.begin_operating().unwrap_err(),
        DomainError::WrongPhase {
            actual: DayPhase::Operating
        }
    );
    assert_eq!(
        s.finish_review().unwrap_err(),
        DomainError::WrongPhase {
            actual: DayPhase::Operating
        }
    );
}

#[test]
fn begin_operating_emits_phase_change() {
    let mut s = session();
    assert_eq!(
        s.begin_operating().unwrap(),
        vec![DomainEvent::PhaseChanged {
            from: DayPhase::Planning,
            to: DayPhase::Operating
        }]
    );
    assert_eq!(s.phase(), DayPhase::Operating);
}

#[test]
fn full_day_moves_to_review() {
    let mut s = session();
    s.begin_operating().unwrap();
    let mut events = Vec::new();
    for _ in 0..TICKS_PER_DAY {
        events.extend(s.tick().unwrap());
        assert_eq!(s.check_invariants(), Ok(()));
    }
    assert_eq!(s.phase(), DayPhase::Review);
    assert_eq!(
        events.last(),
        Some(&DomainEvent::PhaseChanged {
            from: DayPhase::Operating,
            to: DayPhase::Review
        })
    );
    // Exactly one TickCompleted per tick: the session adds none of its own.
    let completed = events
        .iter()
        .filter(|e| matches!(e, DomainEvent::TickCompleted(_)))
        .count();
    assert_eq!(completed, TICKS_PER_DAY as usize);
    // The close publishes the day in its report and clears it.
    assert!(s.day_ticks.is_empty());
    assert_eq!(s.grid_snapshot().last_tick, None);
    assert_eq!(
        s.tick().unwrap_err(),
        DomainError::WrongPhase {
            actual: DayPhase::Review
        }
    );
    assert_eq!(
        s.finish_review().unwrap(),
        vec![DomainEvent::PhaseChanged {
            from: DayPhase::Review,
            to: DayPhase::Planning
        }]
    );
    assert_eq!(s.phase(), DayPhase::Planning);
    assert_eq!(s.clock().month(), may());
    assert_eq!(s.clock().tick(), 0);
}

#[test]
fn each_tick_advances_the_clock_once() {
    let mut s = session();
    s.begin_operating().unwrap();
    let events = s.tick().unwrap();
    assert_eq!(s.clock().tick(), 1);
    let Some(DomainEvent::TickCompleted(summary)) = events.last() else {
        panic!("last event is TickCompleted: {events:?}");
    };
    assert_eq!(summary.tick, 0);
    assert_eq!(s.grid_snapshot().last_tick.as_ref(), Some(summary));
}

/// Coal committed all day at minimum with no AGC cannot carry the load: the
/// grid sheds, blacks out and trips it.
fn session_with_tripped_coal() -> (GameSession, PlantId, Vec<DomainEvent>) {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.start_plant(coal).unwrap();
    s.begin_operating().unwrap();
    let events = run_ticks(&mut s, TICKS_PER_DAY);
    assert!(events.contains(&DomainEvent::PlantTripped { plant: coal }));
    assert_eq!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Offline
    );
    (s, coal, events)
}

#[test]
fn next_month_reinitialises_physical_state_from_pattern() {
    let (mut s, coal, _) = session_with_tripped_coal();
    s.finish_review().unwrap();
    s.begin_operating().unwrap();
    let min = tiny_region().spec(PlantKind::Coal).min_output();
    assert_eq!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Online {
            output: min,
            target: min
        }
    );
    assert!(!s.power().plant(coal).unwrap().is_commitment_suppressed());
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    assert_eq!(
        s.power().plant(ccgt).unwrap().status(),
        PlantStatus::Offline
    );
}

#[test]
fn next_month_resets_frequency_and_protection_without_erasing_history() {
    let (mut s, _, events) = session_with_tripped_coal();
    assert!(events.contains(&DomainEvent::Blackout));
    let end = s.grid_snapshot();
    assert!(end.mode == GridMode::Blackout || end.shed > Ratio::ZERO);
    let (company, regulator) = (s.company().clone(), s.regulator().clone());
    s.finish_review().unwrap();
    s.begin_operating().unwrap();
    let snapshot = s.grid_snapshot();
    assert_eq!(snapshot.frequency, tiny_region().frequency().nominal);
    assert_eq!(snapshot.mode, GridMode::Normal);
    assert_eq!(snapshot.shed, Ratio::ZERO);
    assert_eq!(snapshot.month, may());
    // Company and regulator history is untouched by the new day's reset.
    assert_eq!(s.company(), &company);
    assert_eq!(s.regulator(), &regulator);
}

#[test]
fn begin_operating_keeps_boundary_starts_for_accounting() {
    let mut s = session();
    let peaker = s.insert_commissioned_plant(PlantKind::Peaker, CellCoord { x: 5, y: 5 });
    s.start_plant(peaker).unwrap();
    s.begin_operating().unwrap();
    // A newly commissioned all-day plant returns from a month offline; the
    // established start fleet does not.
    assert_eq!(s.day_boundary_starts(), &BTreeMap::from([(peaker, 1)]));
}

fn play_day(seed: u64) -> GameSession {
    let mut s = GameSession::new(Arc::new(tiny_region()), seed, GameSettings::default());
    for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
        let plant = plant_of(&s, kind);
        s.start_plant(plant).unwrap();
        s.set_auto_follow(plant, true).unwrap();
    }
    s.build_plant(PlantKind::Ccgt, CellCoord { x: 4, y: 4 })
        .unwrap();
    s.begin_operating().unwrap();
    run_ticks(&mut s, TICKS_PER_DAY);
    s
}

#[test]
fn same_seed_and_commands_give_same_hash() {
    assert_eq!(play_day(5).state_hash(), play_day(5).state_hash());
    assert_ne!(play_day(5).state_hash(), play_day(6).state_hash());
}

#[test]
fn state_hash_covers_rng_position() {
    let a = session();
    let mut b = a.clone();
    assert_eq!(a.state_hash(), b.state_hash());
    b.rng.next_u64();
    assert_ne!(a.state_hash(), b.state_hash());
    assert!(format!("{a:?}").contains("word_pos"));
}

#[test]
fn state_hash_is_fnv1a_of_debug_output() {
    let s = session();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{s:?}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(s.state_hash(), hash);
}

#[test]
fn game_over_rejects_every_command_before_checking_phase() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.status = SessionStatus::GameOver(GameOverReason::Bankrupt);
    let over = DomainError::GameOver {
        reason: GameOverReason::Bankrupt,
    };
    let hash = s.state_hash();
    assert_eq!(
        s.build_plant(PlantKind::Peaker, CellCoord { x: 5, y: 5 })
            .unwrap_err(),
        over
    );
    assert_eq!(s.build_line(s.substation, NodeId(999)).unwrap_err(), over);
    assert_eq!(s.commit_plant(coal, [true; 24]).unwrap_err(), over);
    assert_eq!(
        s.set_dispatch_target(coal, Megawatts::new(300.0))
            .unwrap_err(),
        over
    );
    assert_eq!(s.set_auto_follow(coal, true).unwrap_err(), over);
    assert_eq!(s.start_plant(coal).unwrap_err(), over);
    assert_eq!(s.stop_plant(coal).unwrap_err(), over);
    assert_eq!(s.begin_operating().unwrap_err(), over);
    // Wrong phase too, but game over is reported first.
    assert_eq!(s.tick().unwrap_err(), over);
    assert_eq!(s.finish_review().unwrap_err(), over);
    assert_eq!(s.state_hash(), hash);
}

#[test]
fn hourly_forecast_covers_connected_cities_only() {
    let s = session();
    let region = tiny_region();
    // Cities 1 and 2 (600 + 400 MW base) are connected; city 3 is not.
    let forecast = s.hourly_forecast();
    let shape = region.demand().shoulder[0]
        + (region.demand().shoulder[1] - region.demand().shoulder[0]) * 0.5;
    let expected = 1000.0 * shape * region.demand().monthly[3];
    assert!((forecast[0].mean.value() - expected).abs() < 1e-9);
}

#[test]
fn company_view_reports_company_and_regulator() {
    let s = session();
    let region = tiny_region();
    let view = s.company_view();
    assert_eq!(view.cash, Money::from_dollars(2_000_000_000));
    assert_eq!(view.reliability, Ratio::ONE);
    assert_eq!(view.satisfaction, region.satisfaction().starting);
    assert_eq!(view.tariff, region.regulator().starting_tariff);
    assert_eq!(view.allowed_return, region.regulator().base_return);
    assert_eq!(view.strikes, 0);
    assert_eq!(view.licence, Licence::Active);
}

#[test]
fn city_views_follow_region_cities() {
    let s = session();
    let views = s.city_views();
    assert_eq!(views.len(), 3);
    let city3 = &views[2];
    assert_eq!(city3.id, CityId(3));
    assert_eq!(city3.name_key, "city-tiny-3");
    assert_eq!(city3.cell, CellCoord { x: 3, y: 2 });
    assert!(!city3.connected);
}

#[test]
fn invariants_detect_a_non_conserving_tick() {
    let mut s = session();
    s.begin_operating().unwrap();
    s.tick().unwrap();
    assert_eq!(s.check_invariants(), Ok(()));
    let last = s.day_ticks.last_mut().unwrap();
    last.served += Megawatts::new(1.0);
    assert_eq!(
        s.check_invariants(),
        Err(InvariantViolation::ConservationBroken { tick: 0 })
    );
}

#[test]
fn invariants_hold_through_a_healthy_day() {
    let mut s = play_day(3);
    assert_eq!(s.check_invariants(), Ok(()));
    s.finish_review().unwrap();
    assert_eq!(s.check_invariants(), Ok(()));
}

#[test]
fn forecast_net_load_is_demand_less_expected_solar() {
    let s = session();
    let region = s.region().clone();
    for tick in [0, 600, 1_020, 1_439] {
        let demand = s
            .power()
            .connected_demand_forecast(&region, s.clock().month(), tick);
        let solar = s.power().expected_solar(&region, s.clock().month(), tick);
        assert_eq!(s.forecast_net_load(tick), demand - solar);
    }
    // The tiny fleet has no solar, so net load is just demand.
    assert_eq!(
        s.forecast_net_load(600),
        s.power()
            .connected_demand_forecast(&region, s.clock().month(), 600)
    );
}

#[test]
fn rate_review_estimate_of_a_fresh_game_holds_nothing() {
    let s = session();
    let estimate = s.rate_review_estimate();
    assert_eq!(estimate.capital_to_hold, Money::ZERO);
    assert_eq!(estimate.capital_to_release, Money::ZERO);
    assert_eq!(estimate.capital_at_risk, Money::ZERO);
    let region = s.region().clone();
    let peak = s
        .power()
        .projected_connected_peak(&region, s.clock().month().next());
    let firm = s.power().firm_capacity();
    assert_eq!(estimate.reserve_margin, firm / peak - 1.0);
    assert!(estimate.projected_return.value() > 0.0);
}

fn session_with(edit: impl FnOnce(&mut crate::region::RegionDefinitionParts)) -> GameSession {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    edit(&mut parts);
    let region = crate::region::RegionDefinition::try_from(parts).unwrap();
    GameSession::new(Arc::new(region), 1, GameSettings::default())
}

#[test]
fn snapshot_frequency_is_nominal_plus_the_current_deviation() {
    let mut s = session();
    for id in s.power().plants().keys().copied().collect::<Vec<_>>() {
        s.start_plant(id).unwrap();
        s.set_auto_follow(id, true).unwrap();
    }
    s.begin_operating().unwrap();
    run_ticks(&mut s, 30);
    let deviation = s.power().deviation();
    assert_ne!(deviation, Hertz::ZERO);
    let nominal = s.region().frequency().nominal;
    assert_eq!(s.grid_snapshot().frequency, nominal + deviation);
}

#[test]
fn net_load_is_the_forecast_less_expected_solar() {
    let mut s = session();
    s.insert_commissioned_plant(PlantKind::Solar, CellCoord { x: 5, y: 5 });
    let region = s.region().clone();
    let month = s.clock().month();
    let forecast = s.power().connected_demand_forecast(&region, month, 700);
    let solar = s.power().expected_solar(&region, month, 700);
    assert!(solar > Megawatts::ZERO);
    assert_eq!(s.forecast_net_load(700), forecast - solar);
}

#[test]
fn rate_review_estimate_holds_new_capital_when_there_is_excess() {
    // No connected city: no demand at all, so every megawatt of firm capacity is excess.
    let mut s = session_with(|p| p.start.connected_cities.clear());
    let firm = s.power().firm_capacity();
    let asset = AssetRef::Plant(PlantId(900));
    let cost = Money::from_cents(1_000_000);
    s.company.rate_base_mut().add(
        asset,
        cost,
        100,
        s.clock.month(),
        AssetProfile::Plant {
            firm: Megawatts::new(100.0),
            node: NodeId(900),
        },
    );
    let estimate = s.rate_review_estimate();
    assert_eq!(estimate.capital_to_hold, cost);
    assert_eq!(estimate.capital_to_release, Money::ZERO);
    assert_eq!(estimate.capital_at_risk, Money::ZERO);
    // With no peak at all the reserve margin is reported as zero, not infinite.
    assert!(firm > Megawatts::ZERO);
    assert_eq!(estimate.reserve_margin, 0.0);
    // Previewing never changes the rate base.
    assert_eq!(s.company().rate_base().held_total(), Money::ZERO);
}

#[test]
fn rate_review_estimate_splits_held_capital_into_release_and_write_off() {
    let mut s = session_with(|p| p.start.connected_cities.clear());
    let firm = |mw: f64, node: u32| AssetProfile::Plant {
        firm: Megawatts::new(mw),
        node: NodeId(node),
    };
    let last_year = SimMonth::new(2026, 4).unwrap();
    let cost = Money::from_cents(1_000_000);
    for id in [900, 901] {
        let asset = AssetRef::Plant(PlantId(id));
        s.company
            .rate_base_mut()
            .add(asset, cost, 100, last_year, firm(800.0, id));
    }
    // Last year's review held both plants in full (1,600 MW of excess).
    s.company
        .rate_base_mut()
        .apply_reserve_review(&crate::company::ReserveReviewInputs {
            year: 2026,
            excess: Megawatts::new(1_600.0),
        });
    assert_eq!(s.company().rate_base().held_total(), cost * 2);

    // This year only the start fleet's 1,235 MW is excess: 617.5 MW of each
    // 800 MW slice stays excess (and is written off); the rest is released.
    let estimate = s.rate_review_estimate();
    assert_eq!(estimate.capital_at_risk, Money::from_cents(2 * 771_875));
    assert_eq!(estimate.capital_to_release, Money::from_cents(2 * 228_125));
    assert_eq!(estimate.capital_to_hold, Money::ZERO);
}

#[test]
fn require_operating_checks_status_then_phase() {
    let mut s = session();
    assert_eq!(
        s.require_operating(),
        Err(DomainError::WrongPhase {
            actual: DayPhase::Planning
        })
    );
    s.begin_operating().unwrap();
    assert_eq!(s.require_operating(), Ok(()));
    s.status = SessionStatus::GameOver(GameOverReason::LicenseRevoked);
    assert_eq!(
        s.require_operating(),
        Err(DomainError::GameOver {
            reason: GameOverReason::LicenseRevoked
        })
    );
}

#[test]
fn hydro_schedule_is_the_power_systems_schedule() {
    let mut s = session();
    let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
    s.start_plant(hydro).unwrap();
    s.set_auto_follow(hydro, true).unwrap();
    assert!(s.hydro_schedule().plants.is_empty());
    s.begin_operating().unwrap();
    run_ticks(&mut s, 61);
    let schedule = s.hydro_schedule();
    assert_eq!(schedule, s.power().hydro_schedule());
    let plan = schedule.plants[&hydro];
    assert_eq!(
        Some(plan.budget_remaining),
        s.power().plant(hydro).unwrap().hydro_budget()
    );
    assert!(plan.hourly[1..].iter().any(|mw| *mw > Megawatts::ZERO));
    // The command path unplans hydro taken off AGC; elapsed hours stay frozen.
    s.set_auto_follow(hydro, false).unwrap();
    let off = s.hydro_schedule().plants[&hydro];
    assert_eq!(off.hourly[0], plan.hourly[0]);
    assert!(off.hourly[1..].iter().all(|mw| *mw == Megawatts::ZERO));
}

/// The forecast net load of each hour at minute 30, through the public
/// per-tick forecast (an independent route from `hourly_net_load`).
fn net_load_at_half_hours(s: &GameSession) -> [Megawatts; 24] {
    std::array::from_fn(|h| s.forecast_net_load(h as u32 * 60 + 30))
}

/// Commits `plant` for `hours` (inclusive range) on a manual target.
fn commit_manual(
    s: &mut GameSession,
    plant: PlantId,
    hours: std::ops::RangeInclusive<usize>,
    mw: f64,
) {
    let mut pattern = [false; 24];
    for h in hours {
        pattern[h] = true;
    }
    s.commit_plant(plant, pattern).unwrap();
    s.set_dispatch_target(plant, Megawatts::new(mw)).unwrap();
}

#[test]
fn minimum_generation_check_reports_the_worst_hour() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    let peaker = plant_of(&s, PlantKind::Peaker);
    // Coal and CCGT held at 600 + 500 MW through 03:00 only: 1,100 MW.
    commit_manual(&mut s, coal, 3..=3, 600.0);
    commit_manual(&mut s, ccgt, 3..=3, 500.0);
    // A peaker at 200 MW at 08:00-09:00 sits far under the morning load.
    commit_manual(&mut s, peaker, 8..=9, 200.0);

    // Hand-computed 03:30 net load: cities 1 and 2 (1,000 MW) at the April
    // shoulder shape, halfway from 0.63 to 0.64, times the 0.90 monthly factor.
    let expected_net = 1_000.0 * (0.63 + 0.5 * 0.01) * 0.90;
    let warning = s.minimum_generation_check().expect("minimums exceed load");
    assert_eq!(warning.hour, 3);
    assert_eq!(warning.committed_minimum, Megawatts::new(1_100.0));
    assert!((warning.net_load.value() - expected_net).abs() < 1e-6);
    assert_eq!(warning.net_load, s.forecast_net_load(3 * 60 + 30));
    assert_eq!(warning.excess, Megawatts::new(1_100.0) - warning.net_load);
}

#[test]
fn minimum_generation_check_picks_the_largest_excess() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    // Coal at 600 MW over 04:00-05:59; CCGT at 500 MW over 05:00-05:59 only.
    // Hour 5 carries 1,100 MW against a load barely above hour 4's, so it
    // beats hour 4 even though hour 4's load is lower.
    commit_manual(&mut s, coal, 4..=5, 600.0);
    commit_manual(&mut s, ccgt, 5..=5, 500.0);
    let net = net_load_at_half_hours(&s);
    assert!(net[4] < net[5]);
    let warning = s.minimum_generation_check().unwrap();
    assert_eq!(warning.hour, 5);
    assert_eq!(warning.committed_minimum, Megawatts::new(1_100.0));
    assert_eq!(warning.excess, Megawatts::new(1_100.0) - net[5]);
}

#[test]
fn minimum_generation_check_is_none_when_minimums_fit() {
    let mut s = session();
    // Nothing committed: no minimum at all.
    assert_eq!(s.minimum_generation_check(), None);

    // The whole start fleet committed all day at its minimum (240 + 200 + 40
    // MW) sits under the 03:00 net load.
    for plant in s.power().plants().keys().copied().collect::<Vec<_>>() {
        s.start_plant(plant).unwrap();
    }
    assert_eq!(s.minimum_generation_check(), None);
}

#[test]
fn agc_plants_contribute_their_minimum_and_manual_plants_their_target() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    commit_manual(&mut s, coal, 0..=23, 600.0);
    let manual = s
        .minimum_generation_check()
        .expect("600 MW over the trough");
    assert_eq!(manual.committed_minimum, Megawatts::new(600.0));

    // On AGC the same plant can back down to its 240 MW minimum.
    s.set_auto_follow(coal, true).unwrap();
    assert_eq!(s.minimum_generation_check(), None);
}

#[test]
fn minimum_generation_check_counts_only_committed_hours_and_is_pure() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    // Committed only 04:00-05:59: hour 3 has the lower net load, but coal
    // is not committed then, so it cannot be the worst hour.
    commit_manual(&mut s, coal, 4..=5, 600.0);
    let net = net_load_at_half_hours(&s);
    assert!(net[3] < net[4]);

    let before = s.state_hash();
    let warning = s.minimum_generation_check().expect("600 MW over hour 4");
    assert_eq!(s.state_hash(), before, "the check changed the session");
    assert_eq!(warning.hour, 4);
    assert_eq!(warning.committed_minimum, Megawatts::new(600.0));
    assert_eq!(warning.excess, Megawatts::new(600.0) - net[4]);
    assert_eq!(s.minimum_generation_check(), Some(warning));
    assert_eq!(s.state_hash(), before);

    // A commitment outside every low-load hour has no excess at all.
    commit_manual(&mut s, coal, 12..=14, 600.0);
    assert_eq!(s.minimum_generation_check(), None);
}

#[test]
fn minimum_generation_exactly_at_net_load_is_no_warning() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    // Coal held at exactly the 03:30 net load through 03:00 only: the
    // minimum fits, with no excess to warn about.
    let net = net_load_at_half_hours(&s);
    commit_manual(&mut s, coal, 3..=3, net[3].value());
    assert_eq!(s.minimum_generation_check(), None);
}

#[test]
fn minimum_generation_ties_go_to_the_earliest_hour() {
    let mut s = session_with(|parts| {
        parts.demand.winter = [0.5; 24];
        parts.demand.summer = [0.5; 24];
        parts.demand.shoulder = [0.5; 24];
        parts.demand.monthly = [1.0; 12];
    });
    let coal = plant_of(&s, PlantKind::Coal);
    commit_manual(&mut s, coal, 0..=23, 600.0);
    let net = net_load_at_half_hours(&s);
    assert!(net.iter().all(|n| *n == net[0]), "demand should be flat");
    // A flat 500 MW net load under a 600 MW minimum: every hour has the same
    // 100 MW excess, so the earliest hour wins.
    let warning = s.minimum_generation_check().unwrap_or_else(|| {
        panic!("flat load {:?} should be under 600 MW", net[0]);
    });
    assert_eq!(warning.hour, 0);
}
