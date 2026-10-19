//! Pins the simulation's outcomes to `SIM_RULES_VERSION`.
//!
//! The digest holds what a player would notice (money, energy, reliability,
//! tariffs), formatted at fixed precision, and deliberately not
//! `state_hash`, which hashes `Debug` output and so changes on harmless
//! renames. If this snapshot changes, outcomes changed: either the change is
//! a bug, or `SIM_RULES_VERSION` must be bumped in the same commit. The
//! snapshot's first line makes a missing bump visible in review.
//!
//! Each month also pins its rotating outages and every regulatory event, so
//! a strike that expires before the final position is still visible.

#[allow(dead_code)]
mod support;

use std::fmt::Write as _;

use application::{Command, Game};
use domain::company::AssetRef;
use domain::events::DomainEvent;
use domain::ids::CellCoord;
use domain::region::PlantKind;
use domain::region::fixtures::tiny_region_parts;
use domain::session::MonthlyReport;
use support::{auto_follow_all, game_on, play_month};

const SEED: u64 = 2027;

/// The tiny region starts in April; nine months reach the first December
/// close, so the reserve review and the annual rate review are pinned too.
const MONTHS: usize = 9;

/// Plays the fixed script: AGC for the start fleet and one CCGT in month 1,
/// AGC alone afterwards. Returns the game and each month's operated-day
/// events, ending with its close.
fn scripted_game() -> (Game, Vec<Vec<DomainEvent>>) {
    let (mut game, _) = game_on(tiny_region_parts(), SEED);
    let mut months = vec![play_month(&mut game, |g| {
        auto_follow_all(g);
        g.execute(Command::BuildPlant {
            kind: PlantKind::Ccgt,
            cell: CellCoord { x: 4, y: 4 },
        })
        .expect("build plant");
    })];
    for _ in 1..MONTHS {
        months.push(play_month(&mut game, auto_follow_all));
    }
    (game, months)
}

/// The month's rotating outages and every regulatory event of its close
/// (notice, strike with its reason, strike expiry, licence revocation), so a
/// strike that later expires is still pinned in the month that drew it.
fn write_outages_and_regulation(out: &mut String, report: &MonthlyReport, events: &[DomainEvent]) {
    writeln!(
        out,
        "  outage rotating_outage={:.3} peak_outage_share={:.6}",
        report.rotating_outage.value(),
        report.peak_outage_share.value(),
    )
    .unwrap();
    let regulatory: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            DomainEvent::FrequencyNoticeIssued { month } => Some(format!(
                "FrequencyNoticeIssued month={}-{:02}",
                month.year(),
                month.month()
            )),
            DomainEvent::StrikeIssued { strikes, reason } => {
                Some(format!("StrikeIssued strikes={strikes} reason={reason:?}"))
            }
            DomainEvent::StrikeExpired { strikes } => {
                Some(format!("StrikeExpired strikes={strikes}"))
            }
            DomainEvent::LicenseRevoked => Some("LicenseRevoked".to_owned()),
            _ => None,
        })
        .collect();
    if regulatory.is_empty() {
        out.push_str("  regulatory none\n");
    }
    for line in regulatory {
        writeln!(out, "  regulatory {line}").unwrap();
    }
}

fn write_report(out: &mut String, report: &MonthlyReport) {
    let month = report.month;
    writeln!(
        out,
        "month {}-{:02} days={} revenue={} fuel={} start_up={} fixed_om={} construction={} net={}",
        month.year(),
        month.month(),
        report.days,
        report.revenue.cents(),
        report.fuel.cents(),
        report.start_up.cents(),
        report.fixed_om.cents(),
        report.construction.cents(),
        report.net.cents(),
    )
    .unwrap();
    writeln!(
        out,
        "  mwh demand={:.3} served={:.3} unserved={:.3} curtailed={:.3} imbalance={:.3} peak_mw={:.3} reliability={:.6}",
        report.demand.value(),
        report.served.value(),
        report.unserved.value(),
        report.curtailed.value(),
        report.imbalance.value(),
        report.peak_demand.value(),
        report.reliability.value(),
    )
    .unwrap();
    for (kind, energy) in &report.energy_by_kind {
        writeln!(out, "  energy {kind:?}={:.3}", energy.value()).unwrap();
    }
    if let Some(review) = &report.rate_review {
        writeln!(
            out,
            "  rate_review tariff={:.4} allowed_return={:.6} revenue_requirement={} cost_per_mwh={}",
            review.tariff.value(),
            review.allowed_return.value(),
            review.revenue_requirement.cents(),
            review
                .cost_per_mwh
                .map_or("none".to_owned(), |cost| format!("{cost:.4}")),
        )
        .unwrap();
    }
}

/// The scripted game's outcomes: each month's report, every ledger amount and
/// the company's closing position.
fn outcome_digest() -> String {
    let (game, months) = scripted_game();
    let mut out = String::new();
    let reports = game.projections().monthly_reports.history();
    assert_eq!(reports.len(), months.len(), "one report per month played");
    for (report, events) in reports.iter().zip(&months) {
        write_report(&mut out, report);
        write_outages_and_regulation(&mut out, report, events);
    }
    out.push_str("ledger\n");
    for entry in game.session().company().ledger() {
        let subject = match entry.subject {
            Some(AssetRef::Plant(plant)) => format!("plant {}", plant.0),
            Some(AssetRef::Line(line)) => format!("line {}", line.0),
            None => "-".to_owned(),
        };
        writeln!(
            out,
            "  {}-{:02} {:?} {} {}",
            entry.month.year(),
            entry.month.month(),
            entry.category,
            subject,
            entry.amount.cents()
        )
        .unwrap();
    }
    let company = game.session().company_view();
    writeln!(
        out,
        "company cash={} reliability={:.6} satisfaction={:.4} tariff={:.4} strikes={}",
        company.cash.cents(),
        company.reliability.value(),
        company.satisfaction,
        company.tariff.value(),
        company.strikes,
    )
    .unwrap();
    out
}

#[test]
fn outcome_digest_is_pinned_to_rules_version() {
    insta::assert_snapshot!(format!(
        "SIM_RULES_VERSION={}\n{}",
        domain::SIM_RULES_VERSION,
        outcome_digest()
    ));
}
