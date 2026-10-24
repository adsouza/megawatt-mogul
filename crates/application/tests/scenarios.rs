//! Canonical scenarios: scripted months on small fixture regions whose
//! `MonthlyReport`s are snapshotted, so tuning changes show up as diffs.

#[allow(dead_code)]
mod support;

use application::projections::Lesson;
use application::{Command, CommandError, Game};
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::generation::PlantStatus;
use domain::grid::TickSummary;
use domain::ids::{CellCoord, NodeId, PlantId};
use domain::region::{PlantKind, RegionDefinitionParts};
use domain::regulation::StrikeReason;
use domain::session::{
    DayPhase, GameOverReason, IncidentKind, MinimumGenerationWarning, MonthlyReport, SessionStatus,
};
use domain::time::{SimMonth, TICKS_PER_DAY, TICKS_PER_HOUR};
use domain::units::{MegawattHours, Megawatts, Money, Ratio};
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

#[test]
fn may_2033_evening_ccgt_shutdown_does_not_black_out_the_hydro_fleet() {
    let (mut game, _) = game_on(domain::region::fixtures::may_2033_parts(), 7);
    let tail = game
        .session()
        .plant_views()
        .into_iter()
        .rfind(|p| p.kind == PlantKind::Ccgt)
        .unwrap()
        .id;
    for plant in game.session().plant_views() {
        let mut hours = [true; 24];
        if plant.id == tail {
            hours[..17].fill(false);
        }
        game.execute(Command::CommitPlant {
            plant: plant.id,
            hours,
        })
        .unwrap();
        game.execute(Command::SetAutoFollow {
            plant: plant.id,
            on: true,
        })
        .unwrap();
    }
    assert_eq!(game.session().capacity_shortfall_check(), None);
    game.execute(Command::BeginOperating).unwrap();
    let start = game.checkpoint();
    let loaded = support::loaded_region(domain::region::fixtures::may_2033_parts());
    let mut restored = Game::restore(
        loaded,
        start,
        Box::new(application::InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    let events = game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    restored.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
    assert_eq!(game.checkpoint(), restored.checkpoint());
    assert!(!events.contains(&DomainEvent::Blackout));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::OverFrequencyTrip { .. }))
    );
    let report = latest_report(&game);
    assert_eq!(report.month, SimMonth::new(2033, 5).unwrap());
    assert_eq!(report.unserved, MegawattHours::ZERO);
    assert_eq!(report.reliability, Ratio::ONE);
    assert!(
        game.projections().timeline.completed_days()[0]
            .points
            .iter()
            .take(60)
            .all(|p| p.by_kind[&PlantKind::Hydro] > Megawatts::ZERO)
    );
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
        report.rotating_outage,
    ];
    assert!(
        energies.iter().all(|e| e.value().is_finite()),
        "NaN or infinite energy"
    );
    assert!(report.peak_demand.value().is_finite());
    assert!((0.0..=1.0).contains(&report.reliability.value()));
    let balance = report.served.value() + report.unserved.value() - report.demand.value();
    assert!(balance.abs() < 1e-6 * report.demand.value().max(1.0));
    // The planned outage is part of unserved energy, never added to it.
    assert!(
        report.rotating_outage.value() <= report.unserved.value() + 1e-6 * report.demand.value()
    );
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
    insta::assert_snapshot!(support::rounded_debug(
        g.projections().monthly_reports.latest().unwrap()
    ));
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
    insta::assert_snapshot!(support::rounded_debug(&report));
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
    let anchor = g.session().anchor_node();

    // Month 1: order the line (five cells at two a month: three months).
    play_month(&mut g, |g| {
        auto_follow_all(g);
        g.execute(Command::BuildLine {
            from: anchor,
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
    insta::assert_snapshot!(support::rounded_debug(&report));
}

/// The forecast shortfall of the current month's representative day, in
/// report units (weighted by the month's days): the sum over the day's
/// minutes of `max(0, forecast demand − fleet deliverable)` held for one
/// minute, times `days`.
///
/// "Fleet deliverable" is what the committed fleet can serve once ramped:
/// every connected, commissioned dispatchable plant committed in the minute's
/// hour, on AGC, at its availability, plus connected solar at its expected
/// output. For the non-hydro kinds the overreach fleet holds, availability is
/// nameplate capacity; hydro availability depends on the day's water, which
/// this test-side model does not reproduce, so a hydro plant is refused.
/// Read during Planning, after the commitment plan is set: it uses the public
/// forecast APIs (`connected_demand_forecast`, `expected_solar`) and the
/// plant views, not the outage sizing under test.
fn forecast_shortfall(game: &Game, days: u32) -> MegawattHours {
    let session = game.session();
    assert_eq!(session.phase(), DayPhase::Planning, "read the plan");
    let (region, month, power) = (session.region(), session.clock().month(), session.power());
    let fleet: Vec<_> = session
        .plant_views()
        .into_iter()
        .filter(|plant| {
            plant.connected
                && plant.auto_follow
                && plant.kind.is_dispatchable()
                && !matches!(plant.status, PlantStatus::UnderConstruction { .. })
        })
        .collect();
    assert!(
        fleet.iter().all(|plant| plant.kind != PlantKind::Hydro),
        "the test-side deliverable does not model hydro"
    );
    let per_day: MegawattHours = (0..TICKS_PER_DAY)
        .map(|tick| {
            let hour = (tick / TICKS_PER_HOUR) as usize;
            let plants: Megawatts = fleet
                .iter()
                .filter(|plant| plant.commitment[hour])
                .map(|plant| region.spec(plant.kind).capacity)
                .sum();
            let deliverable = plants + power.expected_solar(region, month, tick);
            let demand = power.connected_demand_forecast(region, month, tick);
            (demand - deliverable)
                .max(Megawatts::ZERO)
                .energy_over_ticks(1)
        })
        .sum();
    per_day * f64::from(days)
}

/// The overreach game's fourth month (July 2027): the shortfall the plan
/// leaves, every tick of the day, the report, the month's lessons and the
/// committed plants that were not online after the last tick.
struct OverreachMonth {
    shortfall: MegawattHours,
    ticks: Vec<TickSummary>,
    report: MonthlyReport,
    lessons: Vec<Lesson>,
    offline_committed: Vec<PlantId>,
}

/// Plays the overreach script on `parts` (see
/// `overreach_city_without_capacity`) to the close of month 4, the first
/// month the new city is connected, and returns that month.
fn overreach_month_four(parts: RegionDefinitionParts, seed: u64) -> OverreachMonth {
    let (mut g, _) = game_on(parts, seed);
    let city_node = g
        .session()
        .city_views()
        .iter()
        .find(|city| !city.connected)
        .expect("the start leaves one city unconnected")
        .node;
    let anchor = g.session().anchor_node();
    play_month(&mut g, |g| {
        auto_follow_all(g);
        g.execute(Command::BuildLine {
            from: anchor,
            to: city_node,
        })
        .expect("build line");
    });
    play_month(&mut g, auto_follow_all);
    play_month(&mut g, auto_follow_all);
    assert!(
        g.session().city_views().iter().all(|city| city.connected),
        "the line commissions at the third month close"
    );

    auto_follow_all(&mut g);
    let days = g.session().clock().month().days_in_month();
    let shortfall = forecast_shortfall(&g, days);
    assert!(
        shortfall.value() > 0.0,
        "the overreach month is forecast short: {shortfall:?}"
    );
    g.execute(Command::BeginOperating).expect("begin operating");
    let events = g
        .execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
        .expect("advance the day");
    let ticks = events
        .iter()
        .filter_map(|event| match event {
            DomainEvent::TickCompleted(summary) => Some(summary.clone()),
            _ => None,
        })
        .collect();
    let report = latest_report(&g).clone();
    assert_eq!(report.days, days);
    // In Review the plants hold the state the day's last tick left. Every
    // plant is committed all day, so each must be online after hour 23.
    let committed: Vec<_> = g
        .session()
        .plant_views()
        .into_iter()
        .filter(|plant| plant.commitment.iter().all(|&hour| hour))
        .collect();
    assert!(
        !committed.is_empty(),
        "the hands-off plan commits the fleet"
    );
    let offline_committed = committed
        .iter()
        .filter(|plant| !matches!(plant.status, PlantStatus::Online { .. }))
        .map(|plant| plant.id)
        .collect();
    OverreachMonth {
        shortfall,
        ticks,
        report,
        lessons: g.projections().lessons.latest().to_vec(),
        offline_committed,
    }
}

/// Spec §15.7/§15.8 "overreach no longer collapses", the checks shared by the
/// noisy and noise-free runs: no blackout, the committed fleet still online
/// at the close, some load served, frequency never at the blackout threshold
/// and the rotating-outage lesson taught.
fn assert_overreach_rotates(label: &str, month: &OverreachMonth) {
    let report = &month.report;
    assert_plausible(report);
    assert!(
        !report
            .incidents
            .iter()
            .any(|incident| incident.kind == IncidentKind::Blackout),
        "{label}: a blackout"
    );
    assert!(
        month.offline_committed.is_empty(),
        "{label}: committed plants offline at the close: {:?}",
        month.offline_committed
    );
    assert!(
        report.reliability > Ratio::ZERO,
        "{label}: reliability {:?}",
        report.reliability
    );
    let frequency = overreach().frequency;
    let (nominal, blackout_below) = (frequency.nominal.value(), frequency.blackout_below.value());
    // The fold below starts at +∞, so an empty day would pass vacuously.
    assert_eq!(
        month.ticks.len(),
        TICKS_PER_DAY as usize,
        "{label}: the month's ticks"
    );
    let min_dev = month
        .ticks
        .iter()
        .map(|summary| summary.frequency.value() - nominal)
        .fold(f64::INFINITY, f64::min);
    assert!(
        min_dev > blackout_below,
        "{label}: min deviation {min_dev} Hz"
    );
    assert!(
        month
            .lessons
            .iter()
            .any(|lesson| matches!(lesson, Lesson::RotatingOutages { .. })),
        "{label}: lessons {:?}",
        month.lessons
    );
}

/// The overreach month with demand noise (the scenario as scripted): the
/// planned outage covers the forecast shortfall to within a few percent.
/// Noise above the forecast still falls to protection, which this does not
/// bound (controller ruling for Task 7).
#[test]
fn overreach_rotates_outages_instead_of_collapsing() {
    for seed in [13, 1, 2, 3] {
        let label = format!("seed {seed}");
        let month = overreach_month_four(overreach(), seed);
        assert_overreach_rotates(&label, &month);
        let ratio = month.report.rotating_outage.value() / month.shortfall.value();
        assert!(
            (0.95..=1.10).contains(&ratio),
            "{label}: rotating outage {:?} is {ratio:.4} x the forecast shortfall {:?}",
            month.report.rotating_outage,
            month.shortfall
        );
    }
}

/// The overreach month without demand noise: the planned outage is at least
/// the forecast shortfall and exceeds it by no more than one `shed_step` of
/// the peak for each hour with an outage (the worse-end sizing and pacing
/// slack), and protection never sheds. One seed is enough: with no demand
/// noise and no solar on the fleet, nothing in the day is random.
#[test]
fn overreach_without_demand_noise_rotates_exactly_the_shortfall() {
    let mut parts = overreach();
    parts.demand.forecast_error = Ratio::ZERO;
    let shed_step = parts.frequency.shed_step.value();
    let label = "noise-free";
    let month = overreach_month_four(parts, 13);
    assert_overreach_rotates(label, &month);
    let report = &month.report;
    let hours_with_outage = (0..24)
        .filter(|&hour| {
            month.ticks.iter().any(|summary| {
                summary.tick / TICKS_PER_HOUR == hour && summary.outage > Ratio::ZERO
            })
        })
        .count();
    let days = f64::from(report.days);
    let slack = shed_step * report.peak_demand.value() * hours_with_outage as f64 * days;
    let (planned, shortfall) = (report.rotating_outage.value(), month.shortfall.value());
    assert!(
        planned >= shortfall - 1e-6 * report.demand.value(),
        "{label}: rotating outage {planned} below the shortfall {shortfall}"
    );
    assert!(
        planned <= shortfall + slack,
        "{label}: rotating outage {planned} above the shortfall {shortfall} + {slack}"
    );
    let shed_ticks = month
        .ticks
        .iter()
        .filter(|summary| summary.shed > Ratio::ZERO)
        .count();
    assert_eq!(shed_ticks, 0, "{label}: protection shed");
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
/// the anchor, each wired to it, while AGC runs the start fleet. The
/// CCGTs commission at the June 2029 close into a fleet that already covers
/// the tiny region's peak. Returns the game and the new plants.
fn overbuild_game() -> (Game, Vec<PlantId>) {
    let (mut g, _) = game_on(overbuild(), 14);
    let anchor = g.session().anchor_node();
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
                from: anchor,
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
/// notice in July 2029, then FrequencyPerformance strikes in August, October
/// and November revoke the licence at the November 2029 close (spec
/// §14.5: failure-path scripts may end in game over). The reserve review
/// never sees the new plants; `overbuild_idle_capacity_disallowance` covers it.
#[test]
fn overbuild_overcommitment_loses_licence() {
    let (mut g, _) = overbuild_game();
    let (timeline, status) = play_overbuild(&mut g, auto_follow_all);
    let at = |when| events_at(&timeline, when);

    // The game ends cleanly at the November 2029 close and rejects commands.
    assert_eq!(
        status,
        SessionStatus::GameOver(GameOverReason::LicenseRevoked)
    );
    let last_report = latest_report(&g).clone();
    assert_eq!(last_report.month, month(2029, 11));
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
    for strike_month in [month(2029, 8), month(2029, 10), month(2029, 11)] {
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
    // The rules-14 noise trace leaves September below the frequency failure
    // threshold: it cannot receive an invented strike to keep the old end date.
    assert!(
        !at(month(2029, 9))
            .iter()
            .any(|event| matches!(event, DomainEvent::StrikeIssued { .. }))
    );
    assert!(at(month(2029, 11)).contains(&&DomainEvent::LicenseRevoked));
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
    insta::assert_snapshot!(support::rounded_debug(&OverbuildOutcome {
        status,
        timeline,
        minimum_generation,
        last_report,
    }));
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
    insta::assert_snapshot!(support::rounded_debug(&OverbuildOutcome {
        status,
        timeline,
        minimum_generation,
        last_report,
    }));
}

/// Each plant's revenue in its `PlantHistory` for the month that just closed: what the history
/// gained since `before`, by plant, for every plant in the month's merit order.
fn plant_revenue_since(game: &Game, before: &[(PlantId, Money)]) -> Vec<(PlantId, Money)> {
    latest_report(game)
        .merit_order
        .iter()
        .map(|entry| {
            let now = game
                .projections()
                .plant_history
                .get(entry.plant)
                .map_or(Money::ZERO, |history| history.revenue);
            let was = before
                .iter()
                .find(|(plant, _)| *plant == entry.plant)
                .map_or(Money::ZERO, |(_, revenue)| *revenue);
            (entry.plant, now - was)
        })
        .collect()
}

/// The plant history's revenue across the first December review, on a whole game: December's
/// energy earns the tariff December was sold at, the one in force before the review, and
/// January's earns the tariff the review set. The domain emits `RateReviewCompleted` before
/// December's `MonthClosed`, and the projection relies on that order: the review's tariff is
/// adopted only after the month it closes with is priced.
#[test]
fn plant_revenue_takes_the_reviewed_tariff_from_the_january_after_the_review() {
    let mut parts = support::winter_peak();
    let year = parts.start.month.year();
    parts.start.month = SimMonth::new(year, 12).unwrap();
    let (mut g, _) = game_on(parts, 5);
    let before_review = g.session().company_view().tariff;

    let december = play_month(&mut g, auto_follow_all);

    let reviewed = december
        .iter()
        .find_map(|event| match event {
            DomainEvent::RateReviewCompleted { tariff, .. } => Some(*tariff),
            _ => None,
        })
        .expect("December closes with the rate review");
    assert_ne!(reviewed, before_review, "the review moves the tariff");
    assert_eq!(g.session().company_view().tariff, reviewed);
    let report = latest_report(&g).clone();
    assert_eq!(report.month.month(), 12);
    let december_revenue = plant_revenue_since(&g, &[]);
    assert!(
        report.merit_order.iter().any(
            |entry| before_review.cost_of(entry.energy, 1) != reviewed.cost_of(entry.energy, 1)
        ),
        "some plant's December revenue tells the two tariffs apart"
    );
    let at_old: Vec<(PlantId, Money)> = report
        .merit_order
        .iter()
        .map(|entry| (entry.plant, before_review.cost_of(entry.energy, 1)))
        .collect();
    assert_eq!(
        december_revenue, at_old,
        "December is sold at the old tariff"
    );

    let after_december: Vec<(PlantId, Money)> = g
        .session()
        .plant_views()
        .iter()
        .filter_map(|plant| {
            g.projections()
                .plant_history
                .get(plant.id)
                .map(|history| (plant.id, history.revenue))
        })
        .collect();
    play_month(&mut g, auto_follow_all);

    let report = latest_report(&g).clone();
    assert_eq!(report.month.month(), 1);
    let at_new: Vec<(PlantId, Money)> = report
        .merit_order
        .iter()
        .map(|entry| (entry.plant, reviewed.cost_of(entry.energy, 1)))
        .collect();
    assert!(at_new.iter().any(|(_, revenue)| *revenue > Money::ZERO));
    assert_eq!(
        plant_revenue_since(&g, &after_december),
        at_new,
        "January is the first month at the reviewed tariff"
    );
}

#[test]
fn rounded_debug_rounds_floats_and_keeps_integers() {
    #[derive(Debug)]
    #[allow(dead_code)]
    struct Sample {
        id_7: u32,
        cents: i64,
        mwh: f64,
        tiny: f64,
        negative: f64,
        whole: f64,
    }
    let text = support::rounded_debug(&Sample {
        id_7: 42,
        cents: 123_456_789_012_345,
        mwh: 45425.227257652965,
        tiny: 0.09468736188741605,
        negative: -6.176176867406136,
        whole: 0.0,
    });
    assert!(text.contains("id_7: 42,"), "{text}");
    assert!(text.contains("cents: 123456789012345,"), "{text}");
    assert!(text.contains("mwh: 45425.2272577,"), "{text}");
    assert!(text.contains("tiny: 0.0946873618874,"), "{text}");
    assert!(text.contains("negative: -6.17617686741,"), "{text}");
    assert!(text.contains("whole: 0.0,"), "{text}");
    // Last-bit platform differences round to the same text.
    assert_eq!(
        support::rounded_debug(&45425.22725765297_f64),
        support::rounded_debug(&45425.227257652965_f64)
    );
}
