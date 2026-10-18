use std::sync::Arc;

use super::super::GameSettings;
use super::*;
use crate::company::AssetRef;
use crate::region::SiteError;
use crate::region::fixtures::tiny_region;
use crate::units::Money;

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

fn cell(x: u16, y: u16) -> CellCoord {
    CellCoord { x, y }
}

/// Asserts `result` is `Err(expected)` and the session hash did not move.
fn rejected<T: std::fmt::Debug>(
    s: &mut GameSession,
    expected: DomainError,
    command: impl FnOnce(&mut GameSession) -> Result<T, DomainError>,
) {
    let before = s.state_hash();
    assert_eq!(command(s).unwrap_err(), expected);
    assert_eq!(s.state_hash(), before, "rejected command changed state");
}

#[test]
fn invalid_sites_are_rejected_with_reason() {
    let mut s = session();
    for (kind, cell, reason) in [
        (
            PlantKind::Peaker,
            CellCoord { x: 0, y: 1 },
            SiteError::NotLand,
        ),
        (
            PlantKind::Peaker,
            CellCoord { x: 99, y: 1 },
            SiteError::OutOfBounds,
        ),
        (
            PlantKind::Hydro,
            CellCoord { x: 9, y: 1 },
            SiteError::NoHydroSite,
        ),
    ] {
        assert_eq!(
            s.build_plant(kind, cell).unwrap_err(),
            DomainError::InvalidSite { kind, cell, reason }
        );
    }
    assert!(matches!(
        s.build_plant(PlantKind::Nuclear, CellCoord { x: 2, y: 1 }),
        Err(DomainError::InvalidSite {
            reason: SiteError::InsufficientCoolingWater { .. },
            ..
        })
    ));
}

#[test]
fn failed_command_leaves_state_unchanged() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    let root = s.substation;
    let city1 = s.city_views()[0].node;
    rejected(
        &mut s,
        DomainError::InvalidSite {
            kind: PlantKind::Peaker,
            cell: cell(0, 1),
            reason: SiteError::NotLand,
        },
        |s| s.build_plant(PlantKind::Peaker, cell(0, 1)),
    );
    rejected(
        &mut s,
        DomainError::UnknownNode { node: NodeId(999) },
        |s| s.build_line(root, NodeId(999)),
    );
    rejected(
        &mut s,
        DomainError::DuplicateLine {
            from: city1,
            to: root,
        },
        |s| s.build_line(city1, root),
    );
    rejected(&mut s, DomainError::LineToSelf { node: root }, |s| {
        s.build_line(root, root)
    });
    rejected(
        &mut s,
        DomainError::UnknownPlant {
            plant: PlantId(999),
        },
        |s| s.start_plant(PlantId(999)),
    );
    let mut hours = [true; 24];
    hours[12] = false;
    rejected(
        &mut s,
        DomainError::CommitmentInfeasible {
            gap: SimDuration::from_hours(1),
            shutdown: SimDuration::from_ticks(100),
            startup: SimDuration::from_hours(8),
        },
        |s| s.commit_plant(coal, hours),
    );
    rejected(
        &mut s,
        DomainError::AboveCapacity {
            plant: coal,
            max: Megawatts::new(600.0),
        },
        |s| s.set_dispatch_target(coal, Megawatts::new(700.0)),
    );
    s.begin_operating().unwrap();
    rejected(&mut s, DomainError::PlantNotRunning { plant: coal }, |s| {
        s.stop_plant(coal)
    });
}

#[test]
fn build_plant_starts_construction_on_an_unconnected_node() {
    let mut s = session();
    let at = cell(5, 5);
    let (plant, events) = s.build_plant(PlantKind::Peaker, at).unwrap();
    assert_eq!(
        events,
        vec![DomainEvent::PlantConstructionStarted {
            plant,
            kind: PlantKind::Peaker,
            cell: at
        }]
    );
    let view = s.plant_views().into_iter().find(|p| p.id == plant).unwrap();
    assert_eq!(
        view.status,
        PlantStatus::UnderConstruction { months_left: 12 }
    );
    assert!(!view.connected);
    assert_eq!(view.cell, at);
    let node = s.power().network().node(view.node).unwrap();
    assert_eq!(node.kind, NodeKind::Plant(plant));
    let instalments: Vec<Money> = s
        .pending_instalments(AssetRef::Plant(plant))
        .unwrap()
        .iter()
        .copied()
        .collect();
    assert_eq!(instalments, Money::from_dollars(200_000_000).split_even(12));
    // Construction is not yet capital in the rate base.
    assert!(
        s.company()
            .rate_base()
            .asset(AssetRef::Plant(plant))
            .is_none()
    );
}

#[test]
fn build_line_prices_whole_cells_and_schedules_months() {
    let mut s = session();
    let (plant, _) = s.build_plant(PlantKind::Peaker, cell(5, 5)).unwrap();
    let node = s.power().plant(plant).unwrap().node();
    let root = s.substation;
    // (7, 4) to (5, 5) is sqrt(5) cells: 3 whole cells, 2 months at 2 cells a month.
    let (line, events) = s.build_line(root, node).unwrap();
    assert_eq!(events, vec![DomainEvent::LineConstructionStarted { line }]);
    let built = s.power().network().lines().find(|l| l.id == line).unwrap();
    assert_eq!(
        built.status,
        LineStatus::UnderConstruction { months_left: 2 }
    );
    let instalments: Vec<Money> = s
        .pending_instalments(AssetRef::Line(line))
        .unwrap()
        .iter()
        .copied()
        .collect();
    assert_eq!(instalments, vec![Money::from_dollars(60_000_000); 2]);
}

#[test]
fn short_line_takes_at_least_one_month() {
    let mut s = session();
    let (plant, _) = s.build_plant(PlantKind::Peaker, cell(7, 5)).unwrap();
    let node = s.power().plant(plant).unwrap().node();
    let (line, _) = s.build_line(node, s.substation).unwrap();
    let built = s.power().network().lines().find(|l| l.id == line).unwrap();
    assert_eq!(
        built.status,
        LineStatus::UnderConstruction { months_left: 1 }
    );
    assert_eq!(
        s.pending_instalments(AssetRef::Line(line))
            .unwrap()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![Money::from_dollars(40_000_000)]
    );
}

#[test]
fn whole_cells_snaps_float_noise() {
    assert_eq!(whole_cells(2.0), 2);
    assert_eq!(whole_cells(2.000_000_000_1), 2);
    assert_eq!(whole_cells(2.1), 3);
    assert_eq!(whole_cells(0.0), 0);
}

#[test]
fn commit_plant_validates_with_the_plants_own_spec() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    let peaker = plant_of(&s, PlantKind::Peaker);
    // A one-hour gap fits the peaker (5 + 10 ticks) but not coal (100 + 480).
    let mut hours = [true; 24];
    hours[12] = false;
    assert_eq!(s.commit_plant(peaker, hours).unwrap(), Vec::new());
    assert_eq!(s.power().plant(peaker).unwrap().commitment().hours(), hours);
    assert!(matches!(
        s.commit_plant(coal, hours),
        Err(DomainError::CommitmentInfeasible { .. })
    ));
    assert_eq!(
        s.power().plant(coal).unwrap().commitment().hours(),
        [false; 24]
    );
}

#[test]
fn commit_plant_rejects_unbuilt_unknown_and_non_dispatchable_plants() {
    let mut s = session();
    let (built, _) = s.build_plant(PlantKind::Peaker, cell(5, 5)).unwrap();
    rejected(
        &mut s,
        DomainError::PlantNotCommissioned { plant: built },
        |s| s.commit_plant(built, [true; 24]),
    );
    rejected(
        &mut s,
        DomainError::UnknownPlant {
            plant: PlantId(999),
        },
        |s| s.commit_plant(PlantId(999), [true; 24]),
    );
    let solar = s.insert_commissioned_plant(PlantKind::Solar, cell(4, 4));
    rejected(
        &mut s,
        DomainError::PlantNotDispatchable { plant: solar },
        |s| s.commit_plant(solar, [true; 24]),
    );
    let coal = plant_of(&s, PlantKind::Coal);
    s.begin_operating().unwrap();
    rejected(
        &mut s,
        DomainError::WrongPhase {
            actual: DayPhase::Operating,
        },
        |s| s.commit_plant(coal, [true; 24]),
    );
}

#[test]
fn planning_target_sets_the_initial_target() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    rejected(
        &mut s,
        DomainError::BelowMinimumStableOutput {
            plant: coal,
            min: Megawatts::new(240.0),
        },
        |s| s.set_dispatch_target(coal, Megawatts::new(100.0)),
    );
    s.set_dispatch_target(coal, Megawatts::new(300.0)).unwrap();
    assert_eq!(
        s.power().plant(coal).unwrap().initial_target(),
        Megawatts::new(300.0)
    );
    s.start_plant(coal).unwrap();
    s.begin_operating().unwrap();
    let target = Megawatts::new(300.0);
    assert_eq!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Online {
            output: target,
            target
        }
    );
    s.set_dispatch_target(coal, Megawatts::new(450.0)).unwrap();
    assert_eq!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Online {
            output: target,
            target: Megawatts::new(450.0)
        }
    );
    let ccgt = plant_of(&s, PlantKind::Ccgt);
    rejected(&mut s, DomainError::PlantNotOnline { plant: ccgt }, |s| {
        s.set_dispatch_target(ccgt, Megawatts::new(300.0))
    });
}

#[test]
fn auto_follow_is_set_in_planning_and_operating() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.set_auto_follow(coal, true).unwrap();
    assert!(s.power().plant(coal).unwrap().auto_follow());
    s.begin_operating().unwrap();
    s.set_auto_follow(coal, false).unwrap();
    assert!(!s.power().plant(coal).unwrap().auto_follow());
    let solar = s.insert_commissioned_plant(PlantKind::Solar, cell(4, 4));
    rejected(
        &mut s,
        DomainError::PlantNotDispatchable { plant: solar },
        |s| s.set_auto_follow(solar, true),
    );
}

#[test]
fn plant_commands_are_rejected_in_review() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.begin_operating().unwrap();
    run_ticks(&mut s, TICKS_PER_DAY);
    let review = DomainError::WrongPhase {
        actual: DayPhase::Review,
    };
    rejected(&mut s, review.clone(), |s| s.start_plant(coal));
    rejected(&mut s, review.clone(), |s| s.stop_plant(coal));
    rejected(&mut s, review.clone(), |s| s.set_auto_follow(coal, true));
    rejected(&mut s, review, |s| {
        s.set_dispatch_target(coal, Megawatts::new(300.0))
    });
}

/// The start fleet committed all day on auto-follow, plus two extra
/// peakers, so the grid stays healthy without coal.
fn healthy_session() -> (GameSession, PlantId) {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    let mut fleet = vec![
        plant_of(&s, PlantKind::Ccgt),
        plant_of(&s, PlantKind::Peaker),
    ];
    fleet.push(s.insert_commissioned_plant(PlantKind::Peaker, cell(5, 5)));
    fleet.push(s.insert_commissioned_plant(PlantKind::Peaker, cell(4, 4)));
    for plant in fleet {
        s.start_plant(plant).unwrap();
        s.set_auto_follow(plant, true).unwrap();
    }
    s.set_auto_follow(coal, true).unwrap();
    (s, coal)
}

#[test]
fn stopping_committed_plant_suppresses_actions_without_changing_pattern() {
    let (mut s, coal) = healthy_session();
    s.start_plant(coal).unwrap();
    s.begin_operating().unwrap();
    run_ticks(&mut s, 600);
    assert!(s.power().plant(coal).unwrap().is_online());
    s.stop_plant(coal).unwrap();
    assert!(matches!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Stopping { .. }
    ));
    assert!(s.power().plant(coal).unwrap().is_commitment_suppressed());
    let events = run_ticks(&mut s, TICKS_PER_DAY - 600);
    assert!(!events.contains(&DomainEvent::PlantStarting { plant: coal }));
    assert!(events.contains(&DomainEvent::PlantStopped { plant: coal }));
    assert_eq!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Offline
    );
    assert_eq!(
        s.power().plant(coal).unwrap().commitment().hours(),
        [true; 24]
    );
}

#[test]
fn stopping_a_mixed_pattern_plant_suppresses_its_next_scheduled_start() {
    // On 00–09 and 20–23: the 10-hour gap fits coal's 100 + 480 ticks.
    let hours: [bool; 24] = std::array::from_fn(|h| !(10..20).contains(&h));
    let start_tick = 20 * 60 - 480;
    let (mut control, coal) = healthy_session();
    control.commit_plant(coal, hours).unwrap();
    control.begin_operating().unwrap();
    let mut stopped = control.clone();

    let events = run_ticks(&mut control, start_tick + 1);
    assert!(events.contains(&DomainEvent::PlantStarting { plant: coal }));

    run_ticks(&mut stopped, 300);
    stopped.stop_plant(coal).unwrap();
    let events = run_ticks(&mut stopped, TICKS_PER_DAY - 300);
    assert!(!events.contains(&DomainEvent::Blackout));
    assert!(!events.contains(&DomainEvent::PlantStarting { plant: coal }));
    assert_eq!(
        stopped.power().plant(coal).unwrap().commitment().hours(),
        hours
    );
}

#[test]
fn planning_start_and_stop_select_patterns_without_advancing_time() {
    let mut s = session();
    let nuclear = s.insert_commissioned_plant(PlantKind::Nuclear, cell(9, 2));
    let (clock, rng) = (*s.clock(), s.rng.clone());
    assert_eq!(s.start_plant(nuclear).unwrap(), Vec::new());
    assert_eq!(
        s.power().plant(nuclear).unwrap().commitment().hours(),
        [true; 24]
    );
    assert_eq!(
        s.power().plant(nuclear).unwrap().status(),
        PlantStatus::Offline
    );
    assert_eq!(s.stop_plant(nuclear).unwrap(), Vec::new());
    assert_eq!(
        s.power().plant(nuclear).unwrap().commitment().hours(),
        [false; 24]
    );
    assert_eq!((*s.clock(), &s.rng), (clock, &rng));
    assert_eq!(s.phase(), DayPhase::Planning);
}

#[test]
fn planning_start_rejects_unbuilt_plants() {
    let mut s = session();
    let (built, _) = s.build_plant(PlantKind::Peaker, cell(5, 5)).unwrap();
    rejected(
        &mut s,
        DomainError::PlantNotCommissioned { plant: built },
        |s| s.start_plant(built),
    );
    rejected(
        &mut s,
        DomainError::PlantNotCommissioned { plant: built },
        |s| s.stop_plant(built),
    );
}

#[test]
fn manual_start_beyond_midnight_is_rejected_unchanged() {
    let mut s = session();
    let nuclear = s.insert_commissioned_plant(PlantKind::Nuclear, cell(9, 2));
    s.begin_operating().unwrap();
    rejected(
        &mut s,
        DomainError::StartUpBeyondDay {
            remaining: SimDuration::from_ticks(TICKS_PER_DAY),
            startup: SimDuration::from_hours(48),
        },
        |s| s.start_plant(nuclear),
    );
    let coal = plant_of(&s, PlantKind::Coal);
    run_ticks(&mut s, 961);
    rejected(
        &mut s,
        DomainError::StartUpBeyondDay {
            remaining: SimDuration::from_ticks(479),
            startup: SimDuration::from_hours(8),
        },
        |s| s.start_plant(coal),
    );
}

#[test]
fn manual_start_finishing_exactly_at_midnight_is_allowed() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.begin_operating().unwrap();
    run_ticks(&mut s, 960);
    assert_eq!(
        s.start_plant(coal).unwrap(),
        vec![DomainEvent::PlantStarting { plant: coal }]
    );
    assert_eq!(
        s.power().plant(coal).unwrap().status(),
        PlantStatus::Starting { ticks_left: 480 }
    );
    let events = run_ticks(&mut s, TICKS_PER_DAY - 960);
    assert!(events.contains(&DomainEvent::PlantOnline { plant: coal }));
    assert_eq!(s.phase(), DayPhase::Review);
    assert!(s.power().plant(coal).unwrap().is_online());
}

#[test]
fn manual_start_of_a_running_plant_reports_its_state() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    s.start_plant(coal).unwrap();
    s.begin_operating().unwrap();
    rejected(&mut s, DomainError::PlantNotOffline { plant: coal }, |s| {
        s.start_plant(coal)
    });
}

#[test]
fn planning_target_accepts_exactly_minimum_and_capacity() {
    let mut s = session();
    let coal = plant_of(&s, PlantKind::Coal);
    for mw in [240.0, 600.0] {
        s.set_dispatch_target(coal, Megawatts::new(mw)).unwrap();
        assert_eq!(
            s.power().plant(coal).unwrap().initial_target(),
            Megawatts::new(mw)
        );
    }
    rejected(
        &mut s,
        DomainError::AboveCapacity {
            plant: coal,
            max: Megawatts::new(600.0),
        },
        |s| s.set_dispatch_target(coal, Megawatts::new(600.1)),
    );
}
