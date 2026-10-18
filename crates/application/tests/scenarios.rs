//! Canonical scenarios: scripted months on small fixture regions whose
//! `MonthlyReport`s are snapshotted, so tuning changes show up as diffs.

#[allow(dead_code)]
mod support;

use application::{Command, Game};
use domain::events::DomainEvent;
use domain::ids::{CellCoord, NodeId, PlantId};
use domain::region::PlantKind;
use domain::session::{DayPhase, MonthlyReport};
use domain::time::SimMonth;
use domain::units::{MegawattHours, Money};
use support::{
    auto_follow_all, duck_curve, game_on, overbuild, overreach, play_month, winter_peak,
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

#[test]
fn overbuild_rate_review_disallowance() {
    let (mut g, _) = game_on(overbuild(), 14);
    let substation = g.session().substation();

    // Month 1: order four CCGTs near the substation, each wired to it.
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
        }
    });

    // Months 2..=48: the plants commission after 30 months; collect each
    // December's events (January 2027 + 11 is December 2027, and so on).
    let december = |year| SimMonth::new(year, 12).unwrap();
    let mut december_events = Vec::new();
    for _ in 2..=48 {
        let events = play_month(&mut g, auto_follow_all);
        let closed = latest_report(&g).month;
        if closed.month() == 12 {
            december_events.push((closed, events));
        }
    }
    assert_eq!(latest_report(&g).month, december(2030));
    let months: Vec<SimMonth> = december_events.iter().map(|(month, _)| *month).collect();
    assert_eq!(
        months[..],
        [
            december(2027),
            december(2028),
            december(2029),
            december(2030)
        ]
    );

    let capital = |year| {
        let (_, events) = december_events
            .iter()
            .find(|(month, _)| *month == december(year))
            .expect("December was played");
        let sum =
            |pick: fn(&DomainEvent) -> Option<Money>| events.iter().filter_map(pick).sum::<Money>();
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
    let zero = Money::ZERO;

    // Before the plants commission, the fleet is not in excess.
    assert_eq!(capital(2027), (zero, zero));
    assert_eq!(capital(2028), (zero, zero));
    // The first December after commissioning holds capital without a write-off.
    let (held, disallowed) = capital(2029);
    assert!(held > zero, "first excess review holds capital");
    assert_eq!(
        disallowed, zero,
        "nothing is written off at the first review"
    );
    // The next December is still in excess: the held capital is disallowed.
    let (_, disallowed) = capital(2030);
    assert!(disallowed > zero, "second consecutive review disallows");

    assert_plausible(latest_report(&g));
    insta::assert_debug_snapshot!(latest_report(&g));
}
