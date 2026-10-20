use super::*;
use crate::generation::PlantStatus;
use crate::grid::{GridMode, NetworkModel};
use crate::ids::CityId;
use crate::region::PlantKind;
use crate::region::fixtures::tiny_region;
use crate::regulation::Licence;
use crate::time::{SimMonth, TICKS_PER_DAY};
use crate::units::{Hertz, MegawattHours, Megawatts, Ratio};

fn session() -> GameSession {
    GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default())
}

/// A new game with the inherited schedule undone: nothing committed and no
/// plant on AGC, the start the crafted fleets below build on.
fn empty_session() -> GameSession {
    let mut s = session();
    s.empty_start();
    s
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
    // The tiny start fleet is all dispatchable, so all of it is inherited
    // committed all day (spec §16.3).
    assert!(s.plant_views().iter().all(|p| p.commitment == [true; 24]));
    let snapshot = s.grid_snapshot();
    assert_eq!(snapshot.phase, DayPhase::Planning);
    assert_eq!(snapshot.frequency, Hertz::new(60.0));
    assert_eq!(snapshot.last_tick, None);
}

/// The fixture's start fleet plus a solar farm, which is not dispatchable,
/// and a hydro plant, which is scheduled only under AGC.
fn session_with_solar_and_hydro() -> GameSession {
    session_with(|parts| {
        for (kind, cell) in [
            (PlantKind::Solar, CellCoord { x: 5, y: 5 }),
            (PlantKind::Hydro, CellCoord { x: 10, y: 1 }),
        ] {
            parts
                .start
                .fleet
                .push(crate::region::StartPlantDef { kind, cell });
        }
    })
}

#[test]
fn a_new_game_runs_the_previous_operators_schedule() {
    let s = session_with_solar_and_hydro();
    let views = s.plant_views();
    assert!(views.iter().any(|p| p.kind == PlantKind::Solar));
    assert!(views.iter().any(|p| p.kind == PlantKind::Hydro));
    assert!(views.iter().filter(|p| p.kind.is_dispatchable()).count() >= 3);
    for plant in &views {
        if plant.kind.is_dispatchable() {
            assert_eq!(plant.commitment, [true; 24], "{plant:?}");
            assert!(plant.auto_follow, "{plant:?}");
        } else {
            // Solar follows the sun: no pattern, no AGC.
            assert_eq!(plant.commitment, [false; 24], "{plant:?}");
            assert!(!plant.auto_follow, "{plant:?}");
        }
    }
}

#[test]
fn the_hands_off_plan_changes_nothing_on_a_new_game() {
    // Solar is not dispatchable, so the plan leaves it alone; hydro on AGC
    // is part of the inherited schedule too.
    let mut s = session_with_solar_and_hydro();
    assert!(s.plant_views().iter().any(|p| p.kind == PlantKind::Solar));
    assert!(s.plant_views().iter().any(|p| p.kind == PlantKind::Hydro));
    let before = s.state_hash();
    let dispatchable: Vec<PlantId> = s
        .plant_views()
        .iter()
        .filter(|p| p.kind.is_dispatchable())
        .map(|p| p.id)
        .collect();
    assert!(!dispatchable.is_empty());
    for plant in dispatchable {
        s.commit_plant(plant, [true; 24]).unwrap();
        s.set_auto_follow(plant, true).unwrap();
    }
    assert_eq!(s.state_hash(), before);
}

#[test]
fn the_inherited_schedule_charges_no_boundary_start() {
    let mut s = session();
    s.begin_operating().unwrap();
    // The day begins with the inherited fleet already running ...
    for plant in s.plant_views() {
        assert!(
            matches!(plant.status, PlantStatus::Online { .. }),
            "{plant:?}"
        );
    }
    // ... and an established plant's all-day pattern is no boundary start.
    assert_eq!(s.day_boundary_starts(), &BTreeMap::new());
    let events = run_ticks(&mut s, TICKS_PER_DAY);
    let report = events
        .iter()
        .find_map(|e| match e {
            DomainEvent::MonthClosed(report) => Some(report),
            _ => None,
        })
        .expect("month 1 closes");
    assert_eq!(report.start_up, Money::ZERO, "{report:?}");
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
    let mut s = empty_session();
    // Nothing is committed, so hour 0's forecast is wholly uncovered and the
    // day starts in a full rotating outage (spec §15.3).
    assert_eq!(
        s.begin_operating().unwrap(),
        vec![
            DomainEvent::PhaseChanged {
                from: DayPhase::Planning,
                to: DayPhase::Operating
            },
            DomainEvent::RotatingOutageOrdered {
                hour: 0,
                share: Ratio::ONE
            },
        ]
    );
    assert_eq!(s.phase(), DayPhase::Operating);
}

#[test]
fn begin_operating_orders_no_outage_on_a_covered_day() {
    // The start fleet on AGC covers hour 0, so the target is 0 and no
    // "outage lifted" order is reported: there was nothing to lift.
    let mut s = session();
    for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
        let plant = plant_of(&s, kind);
        s.start_plant(plant).unwrap();
        s.set_auto_follow(plant, true).unwrap();
    }
    assert_eq!(
        s.begin_operating().unwrap(),
        vec![DomainEvent::PhaseChanged {
            from: DayPhase::Planning,
            to: DayPhase::Operating
        }]
    );
    assert_eq!(s.power().outage(), Ratio::ZERO);
}

#[test]
fn begin_operating_orders_hour_zeros_rotating_outage() {
    // City 3 carries 3,000 MW of base load: hour 0's forecast (2,520 MW)
    // is about twice what the 1,300 MW start fleet can deliver.
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.start.connected_cities.push(CityId(3));
    let city = parts.cities.iter_mut().find(|c| c.id == CityId(3)).unwrap();
    city.base_load = Megawatts::new(3_000.0);
    let region = crate::region::RegionDefinition::try_from(parts).unwrap();
    let mut s = GameSession::new(Arc::new(region), 1, GameSettings::default());
    for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
        let plant = plant_of(&s, kind);
        s.start_plant(plant).unwrap();
        s.set_auto_follow(plant, true).unwrap();
    }
    let events = s.begin_operating().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0],
        DomainEvent::PhaseChanged {
            from: DayPhase::Planning,
            to: DayPhase::Operating
        }
    );
    // The exact share short: 1 − 1,300 / 2,520.
    let DomainEvent::RotatingOutageOrdered { hour: 0, share } = events[1] else {
        panic!("hour 0's outage is ordered: {events:?}");
    };
    assert!((share.value() - (1.0 - 1300.0 / 2520.0)).abs() < 1e-9);
}

#[test]
fn the_grid_snapshot_shows_hour_zeros_outage_before_the_first_tick() {
    // City 3 carries 3,000 MW: hour 0 starts in a rotating outage, which a
    // HUD must be able to show before any tick has run.
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.start.connected_cities.push(CityId(3));
    let city = parts.cities.iter_mut().find(|c| c.id == CityId(3)).unwrap();
    city.base_load = Megawatts::new(3_000.0);
    let region = crate::region::RegionDefinition::try_from(parts).unwrap();
    let mut s = GameSession::new(Arc::new(region), 1, GameSettings::default());
    let planning = s.grid_snapshot();
    assert_eq!(
        (planning.outage, planning.outage_target),
        (Ratio::ZERO, Ratio::ZERO)
    );

    s.begin_operating().unwrap();
    let snapshot = s.grid_snapshot();
    assert_eq!(snapshot.last_tick, None);
    assert!(snapshot.outage > Ratio::ZERO);
    assert_eq!(snapshot.outage, s.power().outage());
    assert_eq!(snapshot.outage_target, s.power().outage_target());
    assert_eq!(snapshot.outage, snapshot.outage_target);
}

#[test]
fn the_days_incidents_are_visible_while_operating_and_start_empty_each_day() {
    // City 3 carries 3,000 MW: hour 0 starts in a rotating outage, which is
    // the day's first incident, at tick 0.
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.start.connected_cities.push(CityId(3));
    let city = parts.cities.iter_mut().find(|c| c.id == CityId(3)).unwrap();
    city.base_load = Megawatts::new(3_000.0);
    let region = crate::region::RegionDefinition::try_from(parts).unwrap();
    let mut s = GameSession::new(Arc::new(region), 1, GameSettings::default());
    assert_eq!(s.incidents_today(), &[]);

    s.begin_operating().unwrap();

    let incidents = s.incidents_today();
    assert_eq!(incidents.len(), 1, "{incidents:?}");
    assert_eq!(incidents[0].tick, 0);
    assert!(matches!(
        incidents[0].kind,
        IncidentKind::RotatingOutage { share } if share == s.grid_snapshot().outage
    ));

    // The day's close publishes them in the report and the next day starts clean.
    run_ticks(&mut s, TICKS_PER_DAY);
    assert_eq!(s.phase(), DayPhase::Review);
    assert_eq!(s.incidents_today(), &[]);
}

#[test]
fn the_days_plant_energy_grows_while_operating_and_starts_empty_each_day() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.start_plant(coal).unwrap();
    s.set_auto_follow(coal, true).unwrap();
    assert!(s.plant_energy_today().is_empty(), "nothing yet in Planning");

    s.begin_operating().unwrap();
    run_ticks(&mut s, 60);

    // Unweighted: the played day's own energy, a minute at a time.
    let hour = s.plant_energy_today()[&coal];
    assert!(hour > MegawattHours::ZERO, "{hour:?}");
    assert_eq!(s.plant_energy_today(), s.month_to_date.plant_energy());
    run_ticks(&mut s, 60);
    assert!(s.plant_energy_today()[&coal] > hour);

    // The close weights it into the report, and the next day starts empty.
    run_ticks(&mut s, TICKS_PER_DAY - 120);
    assert_eq!(s.phase(), DayPhase::Review);
    assert!(s.plant_energy_today().is_empty());
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

/// Coal and a second CCGT committed all day at minimum with no AGC, in a
/// rotating outage sized for both. The extra CCGT trips as the day begins, a
/// loss no forecast saw: the grid blacks out and trips the coal.
fn session_with_tripped_coal() -> (GameSession, PlantId, Vec<DomainEvent>) {
    let mut s = empty_session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.start_plant(coal).unwrap();
    let extra = s.insert_commissioned_plant(PlantKind::Ccgt, CellCoord { x: 5, y: 5 });
    s.start_plant(extra).unwrap();
    s.begin_operating().unwrap();
    s.power.plant_mut(extra).unwrap().trip();
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
fn line_views_list_every_line_in_id_order_with_its_status() {
    let mut s = session();
    let views = s.line_views();
    // Two city lines and three plant lines, all in service at the start.
    assert_eq!(views.len(), 5);
    assert!(views.iter().all(|l| l.status == LineStatus::InService));
    assert!(views.windows(2).all(|pair| pair[0].id < pair[1].id));
    let city2 = s
        .city_views()
        .into_iter()
        .find(|c| c.id == CityId(2))
        .unwrap();
    let to_city2 = views.iter().find(|l| l.to == city2.node).unwrap();
    assert_eq!(to_city2.from, s.substation());

    // A line ordered now shows up under construction, after the existing ones.
    let (plant, _) = s
        .build_plant(PlantKind::Peaker, CellCoord { x: 5, y: 5 })
        .unwrap();
    let node = s.power().plant(plant).unwrap().node();
    let (line, _) = s.build_line(s.substation(), node).unwrap();
    let views = s.line_views();
    assert_eq!(views.len(), 6);
    assert_eq!(
        views.last(),
        Some(&LineView {
            id: line,
            from: s.substation(),
            to: node,
            status: LineStatus::UnderConstruction { months_left: 2 },
        })
    );
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

/// A session with scheduled hydro alone (the start fleet uncommitted),
/// operated for the whole first day. Hydro is taken off AGC in hour 23, so
/// the latest plan no longer allocates that hour. Returns the session, the
/// hydro plant and each hour's allocation as it stood when the hour began.
fn day_with_hydro(seed: u64) -> (GameSession, PlantId, [Megawatts; 24]) {
    let mut s = GameSession::new(Arc::new(tiny_region()), seed, GameSettings::default());
    s.empty_start();
    let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
    s.start_plant(hydro).unwrap();
    s.set_auto_follow(hydro, true).unwrap();
    s.begin_operating().unwrap();
    let mut at_start = [Megawatts::ZERO; 24];
    for (h, slot) in at_start.iter_mut().enumerate() {
        // The re-plan opening hour h runs in tick h × 60.
        run_ticks(&mut s, 1);
        *slot = s.hydro_schedule().plants[&hydro].hourly[h];
        if h == 23 {
            s.set_auto_follow(hydro, false).unwrap();
        }
        run_ticks(&mut s, 59);
    }
    assert_eq!(s.phase(), DayPhase::Review);
    (s, hydro, at_start)
}

#[test]
fn hydro_schedule_shows_the_plan_the_finished_day_then_nothing() {
    // Operating: the current plan.
    let mut s = session();
    let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
    s.start_plant(hydro).unwrap();
    s.set_auto_follow(hydro, true).unwrap();
    s.begin_operating().unwrap();
    run_ticks(&mut s, 61);
    let operating = s.hydro_schedule().plants[&hydro];
    assert!(operating.hourly[1..].iter().any(|mw| *mw > Megawatts::ZERO));

    // Review: the finished day, every hour as it stood when it began, even
    // hour 23, which the latest plan no longer allocates.
    let (mut s, hydro, at_start) = day_with_hydro(1);
    assert!(at_start[23] > Megawatts::ZERO);
    let review = s.hydro_schedule();
    assert_eq!(review.plants[&hydro].hourly, at_start);
    assert_eq!(review, s.power().hydro_schedule());

    // Planning: nothing until the next BeginOperating plans the new day.
    s.finish_review().unwrap();
    assert_eq!(s.phase(), DayPhase::Planning);
    assert!(s.hydro_schedule().plants.is_empty());
    // Clearing is part of the command, so replay reaches the same state.
    let (mut again, _, _) = day_with_hydro(1);
    again.finish_review().unwrap();
    assert_eq!(s.state_hash(), again.state_hash());
    // Commands in Planning never plan; BeginOperating does.
    s.set_auto_follow(hydro, true).unwrap();
    assert!(s.hydro_schedule().plants.is_empty());
    s.begin_operating().unwrap();
    assert!(s.hydro_schedule().plants.contains_key(&hydro));
}

/// The lower forecast net load at each hour's two ends, minute 0 and minute
/// 59, through the public per-tick forecast (an independent route from the
/// evaluator the minimum-generation check uses): the end that check reports.
fn net_load_at_hour_troughs(s: &GameSession) -> [Megawatts; 24] {
    std::array::from_fn(|h| {
        let start = h as u32 * 60;
        s.forecast_net_load(start)
            .min(s.forecast_net_load(start + 59))
    })
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
    let mut s = empty_session();
    let coal = plant_of(&s, PlantKind::Coal);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    let peaker = plant_of(&s, PlantKind::Peaker);
    // Coal and CCGT held at 600 + 500 MW through 03:00 only: 1,100 MW.
    commit_manual(&mut s, coal, 3..=3, 600.0);
    commit_manual(&mut s, ccgt, 3..=3, 500.0);
    // A peaker at 200 MW at 08:00-09:00 sits far under the morning load.
    commit_manual(&mut s, peaker, 8..=9, 200.0);

    // Hand-computed 03:00 net load, the lower end of the hour: cities 1 and 2
    // (1,000 MW) at the April shoulder shape's 0.63, times the 0.90 monthly
    // factor.
    let expected_net = 1_000.0 * 0.63 * 0.90;
    let warning = s.minimum_generation_check().expect("minimums exceed load");
    assert_eq!(warning.hour, 3);
    assert_eq!(warning.committed_minimum, Megawatts::new(1_100.0));
    assert!((warning.net_load.value() - expected_net).abs() < 1e-6);
    assert_eq!(warning.net_load, s.forecast_net_load(3 * 60));
    assert_eq!(warning.excess, Megawatts::new(1_100.0) - warning.net_load);
}

#[test]
fn minimum_generation_check_picks_the_largest_excess() {
    let mut s = empty_session();
    let coal = plant_of(&s, PlantKind::Coal);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    // Coal at 600 MW over 04:00-05:59; CCGT at 500 MW over 05:00-05:59 only.
    // Hour 5 carries 1,100 MW against a load barely above hour 4's, so it
    // beats hour 4 even though hour 4's load is lower.
    commit_manual(&mut s, coal, 4..=5, 600.0);
    commit_manual(&mut s, ccgt, 5..=5, 500.0);
    let net = net_load_at_hour_troughs(&s);
    assert!(net[4] < net[5]);
    let warning = s.minimum_generation_check().unwrap();
    assert_eq!(warning.hour, 5);
    assert_eq!(warning.committed_minimum, Megawatts::new(1_100.0));
    assert_eq!(warning.excess, Megawatts::new(1_100.0) - net[5]);
}

#[test]
fn minimum_generation_check_is_none_when_minimums_fit() {
    let mut s = empty_session();
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
    let mut s = empty_session();
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
    let mut s = empty_session();
    let coal = plant_of(&s, PlantKind::Coal);
    // Committed only 04:00-05:59: hour 3 has the lower net load, but coal
    // is not committed then, so it cannot be the worst hour.
    commit_manual(&mut s, coal, 4..=5, 600.0);
    let net = net_load_at_hour_troughs(&s);
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
    let mut s = empty_session();
    let coal = plant_of(&s, PlantKind::Coal);
    // Coal held at exactly the 03:30 net load through 03:00 only: the
    // minimum fits, with no excess to warn about.
    let net = net_load_at_hour_troughs(&s);
    commit_manual(&mut s, coal, 3..=3, net[3].value());
    assert_eq!(s.minimum_generation_check(), None);
}

#[test]
fn minimum_generation_ties_go_to_the_earliest_hour() {
    let mut s = flat_500_session();
    let coal = plant_of(&s, PlantKind::Coal);
    commit_manual(&mut s, coal, 0..=23, 600.0);
    let net = net_load_at_hour_troughs(&s);
    assert!(net.iter().all(|n| *n == net[0]), "demand should be flat");
    // A flat 500 MW net load under a 600 MW minimum: every hour has the same
    // 100 MW excess, so the earliest hour wins.
    let warning = s.minimum_generation_check().unwrap_or_else(|| {
        panic!("flat load {:?} should be under 600 MW", net[0]);
    });
    assert_eq!(warning.hour, 0);
}

/// A flat 0.5 demand shape with one knot, hour `knot`, dropped to 0.2: with
/// 1,000 MW of city load, a 200 MW trough on that hour's start, which the
/// linear interpolation lifts toward 500 MW within the hour. Nothing is
/// committed.
fn session_with_trough_knot(knot: usize) -> GameSession {
    let mut s = session_with(|parts| {
        let mut shape = [0.5; 24];
        shape[knot] = 0.2;
        parts.demand.winter = shape;
        parts.demand.summer = shape;
        parts.demand.shoulder = shape;
        parts.demand.monthly = [1.0; 12];
    });
    s.empty_start();
    s
}

#[test]
fn minimum_generation_check_sees_a_trough_at_an_hour_boundary() {
    // The trough sits on hour 3's first minute. A mid-hour sample reads
    // about 350 MW there, so a 300 MW minimum looked safe; the hour's start
    // is 200 MW, and the check must say so.
    let mut s = session_with_trough_knot(3);
    let coal = plant_of(&s, PlantKind::Coal);
    commit_manual(&mut s, coal, 3..=3, 300.0);
    assert!(s.forecast_net_load(3 * 60 + 30) > Megawatts::new(300.0));

    let warning = s.minimum_generation_check().expect("300 MW over a trough");
    assert_eq!(warning.hour, 3);
    assert_eq!(warning.committed_minimum, Megawatts::new(300.0));
    assert_eq!(warning.net_load, s.forecast_net_load(3 * 60));
    assert!((warning.net_load.value() - 200.0).abs() < 1e-9);
    assert_eq!(warning.excess, Megawatts::new(300.0) - warning.net_load);

    // The same trough approached from the hour before: hour 2's last minute
    // is within a minute of the knot, so its worse end is minute 59.
    let mut s = session_with_trough_knot(3);
    let coal = plant_of(&s, PlantKind::Coal);
    commit_manual(&mut s, coal, 2..=2, 300.0);
    let warning = s.minimum_generation_check().expect("300 MW over a trough");
    assert_eq!(warning.hour, 2);
    assert_eq!(warning.net_load, s.forecast_net_load(2 * 60 + 59));
    assert!(warning.net_load < Megawatts::new(300.0));
}

#[test]
fn minimum_generation_worse_end_ties_to_minute_zero() {
    // A flat load has the same net load at both ends of every hour; the
    // warning's figures are then minute 0's, which are the same as minute 59's.
    let mut s = flat_500_session();
    let coal = plant_of(&s, PlantKind::Coal);
    commit_manual(&mut s, coal, 7..=7, 600.0);
    let warning = s.minimum_generation_check().unwrap();
    assert_eq!(warning.hour, 7);
    assert_eq!(warning.net_load, s.forecast_net_load(7 * 60));
    assert_eq!(warning.net_load, s.forecast_net_load(7 * 60 + 59));
    assert_eq!(warning.excess, Megawatts::new(100.0));
}

/// A session whose cities demand a flat 500 MW all day (cities 1 and 2 total
/// 1,000 MW at a 0.5 shape and a 1.0 monthly factor), with nothing
/// committed.
fn flat_500_session() -> GameSession {
    let mut s = session_with(|parts| {
        parts.demand.winter = [0.5; 24];
        parts.demand.summer = [0.5; 24];
        parts.demand.shoulder = [0.5; 24];
        parts.demand.monthly = [1.0; 12];
    });
    s.empty_start();
    s
}

/// Commits `plant` on exactly the inclusive `(first, last)` hour blocks, on AGC or at `target`.
fn commit_blocks(
    s: &mut GameSession,
    plant: PlantId,
    blocks: &[(usize, usize)],
    target: Option<f64>,
) {
    let mut pattern = [false; 24];
    for &(first, last) in blocks {
        pattern[first..=last].fill(true);
    }
    s.commit_plant(plant, pattern).unwrap();
    match target {
        Some(mw) => s.set_dispatch_target(plant, Megawatts::new(mw)).unwrap(),
        None => s.set_auto_follow(plant, true).unwrap(),
    };
}

/// Coal (600 MW, AGC) covers 00:00-11:59, a CCGT held at 450 MW 12:00-15:59
/// and 19:00-23:59, and a peaker (200 MW, AGC) 16:00-18:59, against 500 MW.
fn shortfall_fleet() -> GameSession {
    let mut s = flat_500_session();
    let coal = plant_of(&s, PlantKind::Coal);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    let peaker = plant_of(&s, PlantKind::Peaker);
    commit_blocks(&mut s, coal, &[(0, 11)], None);
    commit_blocks(&mut s, ccgt, &[(12, 15), (19, 23)], Some(450.0));
    commit_blocks(&mut s, peaker, &[(16, 18)], None);
    s
}

#[test]
fn capacity_shortfall_check_reports_the_worst_hour() {
    let s = shortfall_fleet();
    // Hours 12-15 and 19-23 fall 50 MW short; hours 16-18 fall 300 MW short
    // and tie, so the earliest, 16:00, is reported.
    let warning = s
        .capacity_shortfall_check()
        .expect("the peaker hours fall short");
    assert_eq!(warning.hour, 16);
    assert_eq!(warning.demand, Megawatts::new(500.0));
    assert_eq!(warning.deliverable, Megawatts::new(200.0));
    assert_eq!(warning.shortfall, Megawatts::new(300.0));
}

#[test]
fn capacity_shortfall_check_counts_only_committed_hours() {
    let mut s = shortfall_fleet();
    let coal = plant_of(&s, PlantKind::Coal);
    // Dropping coal from hour 11 leaves that hour uncovered: 500 MW short is
    // now the worst, ahead of the peaker hours.
    commit_blocks(&mut s, coal, &[(0, 10)], None);
    let warning = s.capacity_shortfall_check().unwrap();
    assert_eq!(warning.hour, 11);
    assert_eq!(warning.deliverable, Megawatts::ZERO);
    assert_eq!(warning.shortfall, Megawatts::new(500.0));
}

#[test]
fn capacity_shortfall_check_is_none_when_covered() {
    let mut s = flat_500_session();
    // Nothing committed: the whole day is short, hour 0 first.
    assert_eq!(s.capacity_shortfall_check().unwrap().hour, 0);

    let coal = plant_of(&s, PlantKind::Coal);
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    // Coal's 600 MW covers the first half; the CCGT's 500 MW exactly meets
    // the 500 MW demand in the second, which is covered, not a shortfall.
    commit_blocks(&mut s, coal, &[(0, 11)], None);
    commit_blocks(&mut s, ccgt, &[(12, 23)], None);
    assert_eq!(s.capacity_shortfall_check(), None);
}

/// What the Planning check counts in each hour for a fleet of scheduled hydro
/// alone: its allocation from the preview, the same at both ends.
fn planned_hydro(s: &GameSession) -> [Megawatts; 24] {
    let ends = s.power().planned_hour_ends(s.region(), s.clock().month());
    std::array::from_fn(|h| {
        assert_eq!(ends[h][0].deliverable, ends[h][1].deliverable, "hour {h}");
        ends[h][0].deliverable
    })
}

#[test]
fn capacity_shortfall_check_counts_hydro_from_the_preview() {
    let mut s = empty_session();
    let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
    s.start_plant(hydro).unwrap();
    s.set_auto_follow(hydro, true).unwrap();
    assert!(
        s.hydro_schedule().plants.is_empty(),
        "Planning plans nothing"
    );

    let preview = planned_hydro(&s);
    let warning = s.capacity_shortfall_check().expect("hydro alone is short");
    let h = usize::from(warning.hour);
    assert_eq!(warning.deliverable, preview[h]);
    assert!(preview.iter().any(|mw| *mw > Megawatts::ZERO));

    // What BeginOperating then plans is the preview, hour by hour.
    s.begin_operating().unwrap();
    let planned = s.hydro_schedule().plants[&hydro].hourly;
    assert_eq!(preview, planned);
    assert_eq!(warning.deliverable, planned[h]);
}

#[test]
fn capacity_shortfall_check_previews_a_fresh_days_water_after_a_spent_day() {
    let (mut s, hydro, _) = day_with_hydro(1);
    s.finish_review().unwrap();
    s.set_auto_follow(hydro, true).unwrap();
    // The plant still holds yesterday's leftover water, but the preview
    // plans with the new day's budget, as BeginOperating will.
    let preview = planned_hydro(&s);
    s.begin_operating().unwrap();
    assert_eq!(preview, s.hydro_schedule().plants[&hydro].hourly);
}

/// The fixture in `month` with its city loads scaled by `scale` and no demand
/// noise; with `instant` every plant ramps its whole capacity in a minute.
/// The start fleet (coal, CCGT and peaker, 1,300 MW) is committed all day on
/// AGC.
fn shaped_day(month: SimMonth, scale: f64, instant: bool) -> GameSession {
    let mut parts = crate::region::fixtures::tiny_region_parts();
    parts.start.month = month;
    parts.demand.forecast_error = Ratio::ZERO;
    for city in &mut parts.cities {
        city.base_load = city.base_load * scale;
    }
    if instant {
        for spec in &mut parts.catalog {
            spec.ramp = crate::units::RampRate::mw_per_minute(spec.capacity.value());
        }
    }
    let region = crate::region::RegionDefinition::try_from(parts).unwrap();
    let mut s = GameSession::new(Arc::new(region), 1, GameSettings::default());
    for kind in [PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker] {
        let plant = plant_of(&s, kind);
        commit_blocks(&mut s, plant, &[(0, 23)], None);
    }
    s
}

/// Plays the whole day and returns the hours that ordered a non-zero outage.
fn hours_with_outages(s: &mut GameSession) -> Vec<u8> {
    let mut events = s.begin_operating().unwrap();
    events.extend(run_ticks(s, TICKS_PER_DAY));
    let mut hours: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::RotatingOutageOrdered { hour, share } if *share > Ratio::ZERO => {
                Some(*hour)
            }
            _ => None,
        })
        .collect();
    hours.dedup();
    hours
}

#[test]
fn capacity_shortfall_check_sees_a_peak_at_an_hour_boundary() {
    // The reviewer's probe: July, loads × 0.985, noise-free, the start fleet
    // on AGC. Demand peaks on the 17:00 knot at 985 × 1.15 × 1.15 =
    // 1,302.66 MW against 1,300 MW, but minute 30 of either neighbouring
    // hour sees under 1,300 MW, so a mid-hour check reported nothing.
    let july = SimMonth::new(2027, 7).unwrap();
    let mut s = shaped_day(july, 0.985, false);
    let warning = s
        .capacity_shortfall_check()
        .expect("the 17:00 peak is short");
    // Hour 17's minute 0 is the worst end of any hour: hour 16's minute 59
    // sits a minute below the knot.
    assert_eq!(warning.hour, 17);
    let peak = 985.0 * 1.15 * 1.15;
    assert!((warning.demand.value() - peak).abs() < 1e-9);
    assert_eq!(warning.deliverable, Megawatts::new(1_300.0));
    assert!((warning.shortfall.value() - (peak - 1_300.0)).abs() < 1e-9);
    // Operating sizing orders outages in the two hours either side of the
    // knot, the warned hour among them.
    assert_eq!(hours_with_outages(&mut s), vec![16, 17]);
}

#[test]
fn capacity_shortfall_check_reports_the_worse_end_of_the_hour() {
    // Hour 16 rises toward the 17:00 knot: minute 59 binds, not minute 0.
    let july = SimMonth::new(2027, 7).unwrap();
    let mut s = shaped_day(july, 0.985, false);
    let peaker = plant_of(&s, PlantKind::Peaker);
    commit_blocks(&mut s, peaker, &[(0, 15), (17, 23)], None);
    let warning = s.capacity_shortfall_check().unwrap();
    assert_eq!(warning.hour, 16);
    let factor = (0.98 + (1.00 - 0.98) * 59.0 / 60.0) * 1.15 * 1.15;
    assert!((warning.demand.value() - 985.0 * factor).abs() < 1e-9);
    assert_eq!(warning.deliverable, Megawatts::new(1_100.0));
}

#[test]
fn no_shortfall_warning_means_no_outage_on_a_noise_free_instant_ramp_day() {
    // A table of shaped days: four seasons' shapes, three fleets (the start
    // fleet alone, with solar, with scheduled hydro) and loads scaled around
    // the point where each fleet stops covering the month's peak. With an
    // instant-ramp fleet on AGC the operating sizing delivers at least what
    // the Planning check counts, so a day the check passes must order no
    // outage. Without hydro it delivers exactly that, so a warned hour must
    // be cut too; scheduled hydro can deliver more than the preview, because
    // each boundary re-plan pours water left unused earlier in the day.
    // Each fleet with the capacity its scales are centred on: solar is gone
    // by the evening peak, scheduled hydro's water runs out below its 800 MW.
    let fleets: [(Option<PlantKind>, f64); 3] = [
        (None, 1_300.0),
        (Some(PlantKind::Solar), 1_300.0),
        (Some(PlantKind::Hydro), 1_800.0),
    ];
    for month in [1, 4, 7, 10] {
        let month = SimMonth::new(2027, month).unwrap();
        let unscaled = shaped_day(month, 1.0, true);
        let peak = (0..TICKS_PER_DAY)
            .map(|t| unscaled.forecast_net_load(t).value())
            .fold(0.0, f64::max);
        for (extra, capacity) in fleets {
            let (mut quiet, mut warned) = (0, 0);
            // Dense just above the point of cover, where a peak on an hour
            // knot is short by less than any mid-hour sample shows.
            for fraction in [
                0.9, 0.95, 0.98, 0.99, 0.996, 1.0, 1.001, 1.002, 1.003, 1.004, 1.006, 1.01, 1.02,
                1.05, 1.1, 1.2, 1.3,
            ] {
                let scale = capacity / peak * fraction;
                let mut s = shaped_day(month, scale, true);
                match extra {
                    Some(PlantKind::Hydro) => {
                        let hydro = CellCoord { x: 10, y: 1 };
                        let plant = s.insert_commissioned_plant(PlantKind::Hydro, hydro);
                        commit_blocks(&mut s, plant, &[(0, 23)], None);
                    }
                    Some(kind) => {
                        s.insert_commissioned_plant(kind, CellCoord { x: 5, y: 5 });
                    }
                    None => {}
                }
                let case = format!("{month:?} {extra:?} × {scale:.4}");
                let warning = s.capacity_shortfall_check();
                let outages = hours_with_outages(&mut s);
                match warning {
                    None => {
                        quiet += 1;
                        assert_eq!(outages, vec![], "{case}: no warning");
                    }
                    Some(w) => {
                        warned += 1;
                        let exact = extra != Some(PlantKind::Hydro);
                        assert!(
                            !exact || outages.contains(&w.hour),
                            "{case}: {w:?}, {outages:?}"
                        );
                    }
                }
            }
            assert!(
                quiet > 0 && warned > 0,
                "{month:?} {extra:?}: {quiet} quiet, {warned} warned"
            );
        }
    }
}

#[test]
fn capacity_shortfall_check_is_pure() {
    let mut s = shortfall_fleet();
    let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
    s.start_plant(hydro).unwrap();
    s.set_auto_follow(hydro, true).unwrap();
    let before = s.state_hash();
    let first = s.capacity_shortfall_check();
    assert_eq!(s.state_hash(), before, "the check changed the session");
    assert_eq!(s.capacity_shortfall_check(), first);
    assert!(s.hydro_schedule().plants.is_empty());
    assert_eq!(s.state_hash(), before);
}

#[test]
fn plant_view_previews_the_days_hydro_budget_while_planning() {
    let mut s = session();
    let hydro = s.insert_commissioned_plant(PlantKind::Hydro, CellCoord { x: 10, y: 1 });
    let view = |s: &GameSession, id| s.plant_views().into_iter().find(|p| p.id == id).unwrap();

    // Planning: the remaining level is still the empty one a new plant has, but the view
    // already shows the water the coming day starts with.
    let planning = view(&s, hydro);
    assert_eq!(planning.hydro_budget, Some(MegawattHours::ZERO));
    let day_budget = planning
        .hydro_day_budget
        .expect("a commissioned hydro plant");
    assert!(day_budget > MegawattHours::ZERO, "{day_budget:?}");
    let plant = s.power().plant(hydro).unwrap();
    assert_eq!(
        day_budget,
        crate::grid::power_system::day_hydro_budget(s.region(), s.clock().month(), plant)
    );

    // Beginning the day sets the remaining level to exactly that.
    s.begin_operating().unwrap();
    assert_eq!(view(&s, hydro).hydro_budget, Some(day_budget));

    // Only a commissioned hydro plant has one.
    for other in s.plant_views().into_iter().filter(|p| p.id != hydro) {
        assert_eq!(other.hydro_day_budget, None, "{:?}", other.kind);
    }
}
