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
    assert_eq!(s.day_ticks.len(), TICKS_PER_DAY as usize);
    assert_eq!(s.grid_snapshot().last_tick.unwrap().tick, TICKS_PER_DAY - 1);
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
