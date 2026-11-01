//! Turns the domain's automatic commitment plan into the commands a player would send.

use domain::session::{
    AutomaticCommitmentSearch, CommitmentComparison, CommitmentPlan, CommitmentReport, DayPhase,
    GameSession,
};
use std::fmt::Write;

use crate::{Command, Game};

/// One `CommitPlant` per plant whose planned pattern differs from its current one, in plant
/// order. Empty outside Planning, where commitments can't change.
pub fn automatic_commitment_commands(game: &Game) -> Vec<Command> {
    let session = game.session();
    if session.phase() != DayPhase::Planning {
        return Vec::new();
    }
    commands_for(game, session.automatic_commitment_plan())
}

fn commands_for(game: &Game, plan: CommitmentPlan) -> Vec<Command> {
    let current: std::collections::BTreeMap<_, _> = game
        .session()
        .plant_views()
        .into_iter()
        .map(|view| (view.id, view.commitment))
        .collect();
    plan.into_iter()
        .filter(|(plant, schedule)| current.get(plant) != Some(&schedule.hours()))
        .map(|(plant, schedule)| Command::CommitPlant {
            plant,
            hours: schedule.hours(),
        })
        .collect()
}

impl Game {
    /// Records a diagnostic for every Planning run, even if no commitment changes. Pure
    /// queries can still use `automatic_commitment_commands`; replay uses only commands.
    pub fn plan_automatic_commitments(&mut self) -> Vec<Command> {
        let Some(run) = automatic_commitment_run(self) else {
            return Vec::new();
        };
        self.record_planner_diagnostic(&run.diagnostic);
        run.commands
    }
}

/// One computation, ready to queue in the client without borrowing its session mutably.
pub struct AutomaticCommitmentRun {
    pub commands: Vec<Command>,
    pub diagnostic: String,
}

pub fn automatic_commitment_run(game: &Game) -> Option<AutomaticCommitmentRun> {
    if game.session().phase() != DayPhase::Planning {
        return None;
    }
    Some(run_from_report(
        game,
        game.session().automatic_commitment_report(),
    ))
}

fn run_from_report(game: &Game, report: CommitmentReport) -> AutomaticCommitmentRun {
    let commands = commands_for(game, report.plan.clone());
    let diagnostic =
        game.automatic_commitment_diagnostic_context(&format_report(&report), commands.len());
    AutomaticCommitmentRun {
        commands,
        diagnostic,
    }
}

/// Speculative next-month work. Advancing an owned copy draws exactly the weather that the
/// real FinishReview will draw; neither the live RNG nor the command journal is consumed.
pub struct ReviewCommitmentWork {
    search: AutomaticCommitmentSearch,
    planning_state_hash: u64,
}

impl ReviewCommitmentWork {
    pub fn new(mut review: GameSession) -> Option<Self> {
        review.finish_review().ok()?;
        let planning_state_hash = review.state_hash();
        Some(Self {
            search: AutomaticCommitmentSearch::new(review),
            planning_state_hash,
        })
    }

    pub fn advance(&mut self) -> bool {
        self.search.advance()
    }

    pub fn finish(self) -> PreparedReviewCommitments {
        PreparedReviewCommitments {
            report: self.search.finish(),
            planning_state_hash: self.planning_state_hash,
        }
    }
}

/// Reusable only at the exact next-month Planning boundary it was computed for.
pub struct PreparedReviewCommitments {
    report: CommitmentReport,
    planning_state_hash: u64,
}

impl PreparedReviewCommitments {
    pub fn into_run(self, game: &Game) -> Option<AutomaticCommitmentRun> {
        (game.session().phase() == DayPhase::Planning
            && game.session().state_hash() == self.planning_state_hash)
            .then(|| run_from_report(game, self.report))
    }
}

fn format_report(report: &CommitmentReport) -> String {
    let mut text = format!(
        "policy: demand_cushion_factor={:.6}; preserve min(cushioned demand, all-day supply) at every sample; minimize minimum-generation surplus before estimated fuel + recurring startup cost.\nStart with all-day commitments for connected, commissioned Coal/CCGT/Peaker plants on AGC; other plants are unchanged. Evaluate at most {} capacity-screened candidates per plant, ranked by estimated savings; this is a bounded search.\nEqual surplus and equal estimated cost (within {}) keep the incumbent commitment. Fixed maintenance is charged regardless of commitment; there is no separate online fee. Scores are whole-fleet estimates per representative day in the region's currency units; margins are MW. Patterns run from hour 00 through 23 (1=on, 0=off).\nreference_score={:?}\nselected_score={:?}\n",
        report.demand_cushion_factor,
        report.evaluation_limit_per_plant,
        report.score_tolerance,
        report.reference_score,
        report.selected_score,
    );
    for decision in &report.decisions {
        let _ = writeln!(
            text,
            "plant={:?} kind={:?} grid={:?} selected={} before={:?} selected_score={:?} valid_candidates={} capacity_screened={} evaluation_limited={} all_off_screen_margin_mw={:.6}",
            decision.plant,
            decision.kind,
            decision.grid,
            pattern(decision.selected.hours()),
            decision.before,
            decision.selected_score,
            decision.valid_candidates,
            decision.capacity_screened,
            decision.evaluation_limited,
            decision.all_off_screen_margin_mw,
        );
        if decision.all_off_screen_margin_mw < -report.score_tolerance {
            let _ = writeln!(
                text,
                "  all-off screened out: insufficient hourly capacity slack to remove the plant's full capacity credit"
            );
        }
        if decision.selected.hours() == [true; 24]
            && decision.candidates.iter().any(|candidate| {
                candidate.hours == [false; 24]
                    && candidate.covers_floor
                    && candidate.comparison == CommitmentComparison::EqualScore
            })
        {
            let _ = writeln!(
                text,
                "  kept online: shutting down all day has equal estimated cost and surplus; keep available reserve because shutting down earns no estimated saving"
            );
        }
        for candidate in &decision.candidates {
            let reason = if !candidate.covers_floor {
                "rejected: coverage floor would be breached (including restart ramps and forecast weather)"
            } else {
                match candidate.comparison {
                    CommitmentComparison::LowerExcess => {
                        "accepted as incumbent: less minimum-generation surplus"
                    }
                    CommitmentComparison::HigherExcess => {
                        "rejected: more minimum-generation surplus"
                    }
                    CommitmentComparison::LowerCost => {
                        "accepted as incumbent: lower estimated fuel + startup cost"
                    }
                    CommitmentComparison::HigherCost => {
                        "rejected: higher estimated fuel + startup cost"
                    }
                    CommitmentComparison::EqualScore => {
                        "kept incumbent: equal estimated cost and surplus; no saving from changing commitment"
                    }
                }
            };
            let _ = writeln!(
                text,
                "  candidate={} score={:?} compared_to={:?} estimated_cost_delta={:.9} covers_floor={} reason={reason}",
                pattern(candidate.hours),
                candidate.score,
                candidate.compared_to,
                candidate.score.estimated_cost - candidate.compared_to.estimated_cost,
                candidate.covers_floor
            );
        }
    }
    text
}

fn pattern(hours: [bool; 24]) -> String {
    hours
        .into_iter()
        .map(|on| if on { '1' } else { '0' })
        .collect()
}
