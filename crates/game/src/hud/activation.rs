//! Reviewable service activation, using the domain's prospective supply assessment.
use super::{ButtonModel, UiAction};
use crate::l10n::{Localizer, Names, format};
use domain::{
    ids::NodeId,
    session::{ActivationBlocker, GameSession},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ActivationCardModel {
    pub island: NodeId,
    pub revision: u64,
    pub has_shortfall: bool,
    pub title: String,
    pub stats: Vec<String>,
    pub review: ButtonModel,
}

pub(super) fn hours_text(hours: &[bool; 24], l10n: &Localizer) -> String {
    let mut ranges = Vec::new();
    let mut h = 0;
    while h < 24 {
        if !hours[h] {
            h += 1;
            continue;
        }
        let from = h;
        while h < 24 && hours[h] {
            h += 1;
        }
        ranges.push(l10n.args(
            "forecast-hour-range",
            &[
                ("start", format::clock(from as u32 * 60).into()),
                ("end", format::clock(h as u32 * 60).into()),
            ],
        ));
    }
    ranges.join(", ")
}

pub(super) fn activation_card(
    session: &GameSession,
    island: NodeId,
    l10n: &Localizer,
) -> Option<ActivationCardModel> {
    let a = session.activation_assessment(island).ok()?;
    let names = Names::new(l10n, session);
    let mut stats = vec![l10n.args(
        "activation-demand",
        &[
            ("cities", a.new_cities.into()),
            ("demand", format::megawatts(a.peak_demand).into()),
            ("supply", format::megawatts(a.supply_at_peak).into()),
        ],
    )];
    for blocker in &a.blockers {
        stats.push(match blocker {
            ActivationBlocker::MissingSynchronous { island, hours } => l10n.args(
                "activation-missing-source",
                &[
                    ("island", names.island(session, *island).into()),
                    ("hours", hours_text(hours, l10n).into()),
                ],
            ),
            ActivationBlocker::UnavailableImport { link, hours } => {
                let link = session.power().network().link(*link)?;
                l10n.args(
                    "activation-import-unavailable",
                    &[
                        ("from", names.converter(link.from).into()),
                        ("to", names.converter(link.to).into()),
                        ("hours", hours_text(hours, l10n).into()),
                    ],
                )
            }
        });
    }
    for (id, shortfall) in &a.shortfalls {
        stats.push(l10n.args(
            "shortfall-island-detail",
            &[
                ("island", names.island(session, *id).into()),
                ("amount", format::megawatts(shortfall.shortfall).into()),
                (
                    "time",
                    super::planning_warnings::time_of(shortfall.hour).into(),
                ),
            ],
        ));
    }
    stats.push(l10n.text(if !a.blockers.is_empty() {
        "activation-blocked"
    } else if !a.shortfalls.is_empty() {
        "activation-shortfall"
    } else {
        "activation-ready"
    }));
    Some(ActivationCardModel {
        island,
        revision: a.revision,
        has_shortfall: !a.shortfalls.is_empty(),
        title: names.island(session, island),
        stats,
        review: ButtonModel::new(
            l10n.text("activation-review"),
            UiAction::RequestActivation { island },
        )
        .enabled_if(a.blockers.is_empty()),
    })
}
