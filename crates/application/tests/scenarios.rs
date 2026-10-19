//! Canonical scenarios: scripted months on small fixture regions whose
//! `MonthlyReport`s are snapshotted, so tuning changes show up as diffs.

#[allow(dead_code)]
mod support;

use application::{Command, CommandError, Game};
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::ids::{CellCoord, NodeId, PlantId};
use domain::region::PlantKind;
use domain::regulation::StrikeReason;
use domain::session::{
    DayPhase, GameOverReason, MinimumGenerationWarning, MonthlyReport, SessionStatus,
};
use domain::time::SimMonth;
use domain::units::{MegawattHours, Money};
use support::{
    auto_follow_all, duck_curve, game_on, overbuild, overreach, play_month, play_month_to_close,
    winter_peak,
};

fn latest_report(game: &Game) -> &MonthlyReport {
    game.projections()
        .monthly_reports
        .latest()
        .expect("a month has closed")
}

/// Sanity checks every snapshotted report must pass before it is worth
/// reviewing: finite energies, reliability in 0..=1, served + unserved =
/// demand, `net` the sum of the money lines, and revenue whenever energy was
/// served.
fn assert_plausible(report: &MonthlyReport) {
    let energies = [
        report.demand,
        report.served,
        report.unserved,
        report.curtailed,
        report.imbalance,
    ];
    assert!(
        energies.iter().all(|e| e.value().is_finite()),
        "NaN or infinite energy"
    );
    assert!(report.peak_demand.value().is_finite());
    assert!((0.0..=1.0).contains(&report.reliability.value()));
    let balance = report.served.value() + report.unserved.value() - report.demand.value();
    assert!(balance.abs() < 1e-6 * report.demand.value().max(1.0));
    assert_eq!(
        report.net,
        report.revenue + report.fuel + report.start_up + report.fixed_om + report.construction
    );
    assert_eq!(
        report.served > MegawattHours::ZERO,
        report.revenue > Money::ZERO
    );
}

fn node_of(game: &Game, plant: PlantId) -> NodeId {
    game.session()
        .plant_views()
        .iter()
        .find(|view| view.id == plant)
        .expect("plant exists")
        .node
}

/// Orders a plant and returns its id.
fn build_plant(game: &mut Game, kind: PlantKind, cell: CellCoord) -> PlantId {
    let events = game
        .execute(Command::BuildPlant { kind, cell })
        .expect("build plant");
    events
        .iter()
        .find_map(|event| match event {
            DomainEvent::PlantConstructionStarted { plant, .. } => Some(*plant),
            _ => None,
        })
        .expect("construction started")
}

#[test]
fn winter_peak_month() {
    let (mut g, _) = game_on(winter_peak(), 11);
    play_month(&mut g, auto_follow_all);
    assert_plausible(latest_report(&g));
    insta::assert_debug_snapshot!(g.projections().monthly_reports.latest().unwrap());
}

#[test]
fn duck_curve_month() {
    let (mut g, _) = game_on(duck_curve(), 12);
    auto_follow_all(&mut g);
    g.execute(Command::BeginOperating).unwrap();
    // Advance in five-minute steps so the ramp-warning projection, which keeps
    // only its latest evaluation, is observed through the whole day.
    let mut saw_ramp_warning = false;
    while g.session().phase() == DayPhase::Operating {
        g.execute(Command::AdvanceTicks { n: 5 }).unwrap();
        saw_ramp_warning |= g.projections().ramp_warnings.current().is_some();
    }
    g.execute(Command::FinishReview).unwrap();

    let report = latest_report(&g);
    assert!(
        report.curtailed > MegawattHours::ZERO || saw_ramp_warning,
        "a duck-curve day curtails solar or warns of the evening ramp"
    );
    assert_plausible(report);
    insta::assert_debug_snapshot!(report);
}

#[test]
fn overreach_city_without_capacity() {
    let (mut g, _) = game_on(overreach(), 13);
    let city_node = g
        .session()
        .city_views()
        .iter()
        .find(|city| !city.connected)
        .expect("the start leaves one city unconnected")
        .node;
    let substation = g.session().substation();

    // Month 1: order the line (five cells at two a month: three months).
    play_month(&mut g, |g| {
        auto_follow_all(g);
        g.execute(Command::BuildLine {
            from: substation,
            to: city_node,
        })
        .expect("build line");
    });
    play_month(&mut g, auto_follow_all);
    let third = play_month(&mut g, auto_follow_all);
    assert!(
        third
            .iter()
            .any(|event| matches!(event, DomainEvent::CityConnected { .. })),
        "the line commissions at the third month close"
    );
    assert_eq!(latest_report(&g).unserved, MegawattHours::ZERO);

    // Month 4: the new city's load far exceeds the fleet.
    let fourth = play_month(&mut g, auto_follow_all);
    let report = latest_report(&g);
    assert!(report.unserved > MegawattHours::ZERO);
    assert!(
        fourth
            .iter()
            .any(|event| matches!(event, DomainEvent::StrikeIssued { .. })),
        "the regulator strikes the unreliable month"
    );
    assert_plausible(report);
    insta::assert_debug_snapshot!(report);
}

/// What the overbuild scenarios snapshot: how the game stands at the end,
/// the regulatory and reserve-review events in the order they happened (each
/// tagged with the month whose close or day produced it), the final month's
/// minimum-generation warning and its report.
#[derive(Debug)]
#[allow(dead_code, reason = "read through `Debug` by the snapshot")]
struct OverbuildOutcome {
    status: SessionStatus,
    timeline: Vec<(SimMonth, DomainEvent)>,
    minimum_generation: Option<MinimumGenerationWarning>,
    last_report: MonthlyReport,
}

/// The events the overbuild timeline keeps: commissioning, the regulator's
/// verdicts and the December reviews.
fn regulatory(event: &DomainEvent) -> bool {
    matches!(
        event,
        DomainEvent::PlantCommissioned { .. }
            | DomainEvent::FrequencyNoticeIssued { .. }
            | DomainEvent::StrikeIssued { .. }
            | DomainEvent::StrikeExpired { .. }
            | DomainEvent::LicenseRevoked
            | DomainEvent::CapitalHeld { .. }
            | DomainEvent::CapitalReleased { .. }
            | DomainEvent::CapitalDisallowed { .. }
            | DomainEvent::RateReviewCompleted { .. }
    )
}

/// Starts the overbuild game and plays month 1, which orders four CCGTs near
/// the substation, each wired to it, while AGC runs the start fleet. The
/// CCGTs commission at the June 2029 close into a fleet that already covers
/// the tiny region's peak. Returns the game and the new plants.
fn overbuild_game() -> (Game, Vec<PlantId>) {
    let (mut g, _) = game_on(overbuild(), 14);
    let substation = g.session().substation();
    let mut ccgts = Vec::new();
    play_month(&mut g, |g| {
        auto_follow_all(g);
        for cell in [(9, 4), (9, 5), (8, 5), (7, 5)] {
            let plant = build_plant(
                g,
                PlantKind::Ccgt,
                CellCoord {
                    x: cell.0,
                    y: cell.1,
                },
            );
            let node = node_of(g, plant);
            g.execute(Command::BuildLine {
                from: substation,
                to: node,
            })
            .expect("build line");
            ccgts.push(plant);
        }
    });
    (g, ccgts)
}

/// Plays months 2..=48 with `plan`, stopping at a month close that ends the
/// game. Returns the regulatory timeline and the status at the end.
fn play_overbuild(
    g: &mut Game,
    mut plan: impl FnMut(&mut Game),
) -> (Vec<(SimMonth, DomainEvent)>, SessionStatus) {
    let mut timeline = Vec::new();
    let mut status = SessionStatus::Active;
    for _ in 2..=48 {
        let (events, after) = play_month_to_close(g, &mut plan);
        let closed = latest_report(g).month;
        timeline.extend(
            events
                .into_iter()
                .filter(regulatory)
                .map(|event| (closed, event)),
        );
        status = after;
        if status != SessionStatus::Active {
            break;
        }
    }
    (timeline, status)
}

/// The timeline's events at the close (or day) of `when`.
fn events_at(timeline: &[(SimMonth, DomainEvent)], when: SimMonth) -> Vec<&DomainEvent> {
    timeline
        .iter()
        .filter(|(closed, _)| *closed == when)
        .map(|(_, event)| event)
        .collect()
}

fn month(year: i32, month: u8) -> SimMonth {
    SimMonth::new(year, month).unwrap()
}

/// The December reviews before commissioning complete with nothing held or
/// disallowed: until the CCGTs commission, the fleet is not in excess.
fn assert_no_excess_before_commissioning(timeline: &[(SimMonth, DomainEvent)]) {
    for december in [month(2027, 12), month(2028, 12)] {
        let events = events_at(timeline, december);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DomainEvent::RateReviewCompleted { .. })),
            "{december:?} completes a rate review"
        );
        assert!(!events.iter().any(|event| matches!(
            event,
            DomainEvent::CapitalHeld { .. } | DomainEvent::CapitalDisallowed { .. }
        )));
    }
}

/// Over-commitment (amendment A3 end to end): the four CCGTs are committed
/// all day on AGC as soon as they commission. Their minimum stable output
/// pushes the committed minimum far above the overnight load, so frequency
/// runs above the performance band despite the over-frequency trips: a
/// notice in July 2029, then a FrequencyPerformance strike at each of the
/// next three closes revokes the licence at the October 2029 close (spec
/// §14.5: failure-path scripts may end in game over). The reserve review
/// never sees the new plants; `overbuild_idle_capacity_disallowance` covers it.
#[test]
fn overbuild_overcommitment_loses_licence() {
    let (mut g, _) = overbuild_game();
    let (timeline, status) = play_overbuild(&mut g, auto_follow_all);
    let at = |when| events_at(&timeline, when);

    // The game ends cleanly at the October 2029 close and rejects commands.
    assert_eq!(
        status,
        SessionStatus::GameOver(GameOverReason::LicenseRevoked)
    );
    let last_report = latest_report(&g).clone();
    assert_eq!(last_report.month, month(2029, 10));
    assert_eq!(
        g.execute(Command::FinishReview),
        Err(CommandError::Domain(DomainError::GameOver {
            reason: GameOverReason::LicenseRevoked
        }))
    );

    // The new CCGTs are online before the revocation.
    let commissioned = timeline
        .iter()
        .filter(|(_, event)| {
            matches!(
                event,
                DomainEvent::PlantCommissioned {
                    kind: PlantKind::Ccgt,
                    ..
                }
            )
        })
        .count();
    assert_eq!(commissioned, 4, "the four CCGTs commission");

    // A notice, then three frequency-performance strikes, then revocation.
    assert!(
        at(month(2029, 7))
            .iter()
            .any(|event| matches!(event, DomainEvent::FrequencyNoticeIssued { .. })),
        "July 2029 draws a notice first"
    );
    for strike_month in [month(2029, 8), month(2029, 9), month(2029, 10)] {
        assert!(
            at(strike_month).iter().any(|event| matches!(
                event,
                DomainEvent::StrikeIssued {
                    reason: StrikeReason::FrequencyPerformance,
                    ..
                }
            )),
            "{strike_month:?} draws a frequency-performance strike"
        );
    }
    assert!(at(month(2029, 10)).contains(&&DomainEvent::LicenseRevoked));
    assert!(last_report.frequency_failed);
    assert!(last_report.over_frequency_trips > 0);
    assert_eq!(last_report.unserved, MegawattHours::ZERO);
    // The planning-time check would have warned: the committed minimum
    // exceeds the overnight net load.
    let minimum_generation = g.session().minimum_generation_check();
    assert!(
        minimum_generation.is_some(),
        "the overbuilt fleet over-generates"
    );
    assert_no_excess_before_commissioning(&timeline);

    assert_plausible(&last_report);
    insta::assert_debug_snapshot!(OverbuildOutcome {
        status,
        timeline,
        minimum_generation,
        last_report,
    });
}

/// Idle capacity (the used-and-useful reserve review end to end): the same
/// four CCGTs are built but never committed, so they sit offline as excess
/// capacity in the rate base. Leaving them uncommitted is what keeps this
/// case about the review: committed, their minimums over-generate and the
/// licence is lost first (`overbuild_overcommitment_loses_licence`). The first
/// December after commissioning (2029) holds the excess capital without a
/// write-off; the next (2030) finds the same capacity still in excess and
/// disallows it. All 48 months are played.
#[test]
fn overbuild_idle_capacity_disallowance() {
    let (mut g, ccgts) = overbuild_game();
    // The start fleet runs on AGC all day as before; each new CCGT, once
    // commissioned, is committed for no hours at all.
    let (timeline, status) = play_overbuild(&mut g, |g| {
        let commissioned: Vec<PlantId> = g
            .session()
            .plant_views()
            .iter()
            .filter(|plant| {
                plant.kind.is_dispatchable()
                    && !matches!(plant.status, PlantStatus::UnderConstruction { .. })
            })
            .map(|plant| plant.id)
            .collect();
        for plant in commissioned {
            let idle = ccgts.contains(&plant);
            g.execute(Command::CommitPlant {
                plant,
                hours: [!idle; 24],
            })
            .expect("commit plant");
            if !idle {
                g.execute(Command::SetAutoFollow { plant, on: true })
                    .expect("turn AGC on");
            }
        }
    });
    let at = |when| events_at(&timeline, when);
    let capital = |when| {
        let events = at(when);
        let sum = |pick: fn(&DomainEvent) -> Option<Money>| {
            events.iter().copied().filter_map(pick).sum::<Money>()
        };
        (
            sum(|e| match e {
                DomainEvent::CapitalHeld { amount, .. } => Some(*amount),
                _ => None,
            }),
            sum(|e| match e {
                DomainEvent::CapitalDisallowed { amount, .. } => Some(*amount),
                _ => None,
            }),
        )
    };

    assert_eq!(status, SessionStatus::Active);
    let last_report = latest_report(&g).clone();
    assert_eq!(last_report.month, month(2030, 12));
    assert_no_excess_before_commissioning(&timeline);
    // The new CCGTs never run.
    for plant in &ccgts {
        let energy = last_report
            .merit_order
            .iter()
            .find(|entry| entry.plant == *plant)
            .map_or(MegawattHours::ZERO, |entry| entry.energy);
        assert_eq!(energy, MegawattHours::ZERO, "{plant:?} stays offline");
    }
    // The first December after commissioning holds capital without a
    // write-off; the next is still in excess and disallows it.
    let zero = Money::ZERO;
    let (held, disallowed) = capital(month(2029, 12));
    assert!(held > zero, "first excess review holds capital");
    assert_eq!(
        disallowed, zero,
        "nothing is written off at the first review"
    );
    let (_, disallowed) = capital(month(2030, 12));
    assert!(disallowed > zero, "second consecutive review disallows");
    assert!(last_report.rate_review.is_some());

    assert_plausible(&last_report);
    let minimum_generation = g.session().minimum_generation_check();
    insta::assert_debug_snapshot!(OverbuildOutcome {
        status,
        timeline,
        minimum_generation,
        last_report,
    });
}
