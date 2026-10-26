//! The Milestones panel: every milestone, with the month it was reached or how far the player is
//! from it. The top bar opens and closes it in any in-game phase; it floats over the screen and
//! doesn't pause the game.

use application::Game;
use application::projections::{MILESTONES, MilestoneKind, MilestoneStatus};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::layout::HudArea;
use super::theme::{TextTone, heading_color};
use crate::l10n::{Localize, Localizer, format};
use crate::sim::Session;

/// The panel's content width.
pub const MILESTONES_WIDTH: f32 = 340.0;
/// The gap between the panel and the top bar and the right edge.
const MILESTONES_MARGIN: f32 = 12.0;

/// Whether the Milestones panel is open. Written by `dispatch`, closed with each game.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MilestonesOpen(pub bool);

pub(super) fn close_milestones(mut open: ResMut<MilestonesOpen>) {
    open.0 = false;
}

/// One milestone's line.
#[derive(Debug, Clone, PartialEq)]
pub struct MilestoneRow {
    pub name: String,
    /// *Reached* or *In progress*.
    pub status: String,
    /// The month it was reached, or the progress toward it.
    pub detail: String,
    pub reached: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MilestonesModel {
    pub title: String,
    /// Every milestone, in the order the game checks them.
    pub rows: Vec<MilestoneRow>,
    /// Closes the panel.
    pub close: ButtonModel,
}

pub fn milestones_model(game: &Game, l10n: &Localizer) -> MilestonesModel {
    let milestones = &game.projections().milestones;
    let now = game.session().clock().month();
    MilestonesModel {
        title: l10n.text("milestones-title"),
        rows: MILESTONES
            .iter()
            .map(|kind| {
                milestone_row(
                    *kind,
                    milestones.status(*kind, now, game.session().region().calendar()),
                    l10n,
                )
            })
            .collect(),
        close: ButtonModel::new(l10n.text("ui-close"), UiAction::ToggleMilestones),
    }
}

/// The line for `kind` in `status`. Split from [`milestones_model`] because a reached milestone
/// takes months of play.
pub fn milestone_row(
    kind: MilestoneKind,
    status: MilestoneStatus,
    l10n: &Localizer,
) -> MilestoneRow {
    let (status_key, detail, reached) = match status {
        MilestoneStatus::Reached { month } => {
            ("milestones-reached", format::month_year(month), true)
        }
        MilestoneStatus::InProgress(progress) => {
            ("milestones-in-progress", progress.localize(l10n), false)
        }
    };
    MilestoneRow {
        name: kind.localize(l10n),
        status: l10n.text(status_key),
        detail,
        reached,
    }
}

/// Each milestone's name, then its status and detail; Close at the bottom.
pub fn draw_milestones(ui: &mut egui::Ui, model: &MilestonesModel) -> Vec<UiAction> {
    let dismiss = super::action::cancelled(ui, &model.close);
    let mut actions = Vec::new();
    ui.set_width(MILESTONES_WIDTH);
    for row in &model.rows {
        let name_color = if row.reached {
            TextTone::Positive.color()
        } else {
            heading_color()
        };
        let status = if row.reached {
            TextTone::Positive
        } else {
            TextTone::Neutral
        };
        let detail = if row.reached {
            TextTone::Positive
        } else {
            TextTone::Information
        };
        ui.label(egui::RichText::new(&row.name).strong().color(name_color));
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(status.color(), &row.status);
            ui.colored_label(detail.color(), &row.detail);
        });
        ui.add_space(4.0);
    }
    ui.separator();
    ui.vertical_centered(|ui| {
        button(ui, &model.close, &mut actions);
    });
    if dismiss {
        vec![model.close.action.clone()]
    } else {
        actions
    }
}

/// A window under the top bar at the right, above the map and the Review modal alike.
pub(super) fn milestones_overlay(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    settings: Option<Res<crate::preferences::ClientSettings>>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let model = milestones_model(&session.0, &l10n);
    let ctx = contexts.ctx_mut()?;
    let top = area.free_rect(ctx).top();
    let default = egui::pos2(
        (ctx.content_rect().right() - MILESTONES_WIDTH - MILESTONES_MARGIN).max(0.0),
        top + MILESTONES_MARGIN,
    );
    let position = settings
        .as_ref()
        .and_then(|s| s.value.tools.get("milestones"))
        .map_or(default, |v| {
            egui::pos2(
                v[0].clamp(
                    0.0,
                    (ctx.content_rect().right() - MILESTONES_WIDTH - MILESTONES_MARGIN).max(0.0),
                ),
                v[1].clamp(0.0, (ctx.content_rect().bottom() - 80.0).max(0.0)),
            )
        });
    let actions = egui::Window::new(egui::RichText::new(&model.title).color(heading_color()))
        .id(egui::Id::new("milestones"))
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .default_pos(position)
        .show(ctx, |ui| draw_milestones(ui, &model))
        .and_then(|shown| shown.inner)
        .unwrap_or_default();
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use application::projections::MilestoneProgress;
    use domain::time::SimMonth;
    use domain::units::Megawatts;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn milestones_model_shows_progress() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let connected = game
            .session()
            .city_views()
            .iter()
            .filter(|city| city.connected)
            .count();
        assert!(
            connected < 10,
            "the eastern start has fewer than ten cities"
        );

        let model = milestones_model(&game, &l10n);

        assert_eq!(model.title, "Milestones");
        let rows: Vec<(&str, &str, &str)> = model
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.status.as_str(), row.detail.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "Serve a monthly peak of 50 GW",
                    "In progress",
                    "Best monthly peak so far: 0.0 GW of 50.0 GW"
                ),
                (
                    "Get through a full season without a strike",
                    "In progress",
                    "0 of 3 strike-free months this season"
                ),
                (
                    "Connect 10 cities",
                    "In progress",
                    format!("{connected} of 10 cities connected").as_str()
                ),
            ]
        );
        assert!(model.rows.iter().all(|row| !row.reached));
        assert_eq!(model.close.label, "Close");
        assert_eq!(model.close.action, UiAction::ToggleMilestones);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_reached_milestone_shows_its_month_and_progress_reads_as_copy() {
        let l10n = shipped_l10n();
        let kind = MilestoneKind::ServePeak { gigawatts: 50 };

        let reached = milestone_row(
            kind,
            MilestoneStatus::Reached {
                month: SimMonth::new(2028, 7).unwrap(),
            },
            &l10n,
        );
        let partway = milestone_row(
            kind,
            MilestoneStatus::InProgress(MilestoneProgress::Peak {
                best: Megawatts::new(31_234.0),
                goal: Megawatts::new(50_000.0),
            }),
            &l10n,
        );

        assert_eq!(reached.status, "Reached");
        assert_eq!(reached.detail, "July 2028");
        assert!(reached.reached);
        assert_eq!(
            partway.detail,
            "Best monthly peak so far: 31.2 GW of 50.0 GW"
        );
        // Below the gigawatt threshold the best peak is still in the goal's unit.
        let early = milestone_row(
            kind,
            MilestoneStatus::InProgress(MilestoneProgress::Peak {
                best: Megawatts::new(8_400.0),
                goal: Megawatts::new(50_000.0),
            }),
            &l10n,
        );
        assert_eq!(early.detail, "Best monthly peak so far: 8.4 GW of 50.0 GW");
        assert!(!partway.reached);
        assert_eq!(l10n.problems(), vec![]);
    }
}
