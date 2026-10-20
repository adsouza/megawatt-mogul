//! The Planning panel (spec §8.4): the bottom panel that holds the commitment grid, the forecast
//! chart and the tariff panel, and the button that begins the day.
//!
//! # Layout
//!
//! The window is at least 1024×640, and the commitment grid is the primary planning tool, so it
//! gets the full width in a bottom panel of fixed height ([`PLANNING_PANEL_HEIGHT`]). The
//! inspector and the build menu keep their fixed side columns above it, and the map takes what is
//! left, so nothing here ever moves the map between frames. The panel's content is a tab:
//!
//! - **Commitments**: the planning warnings, then the grid, which scrolls when the fleet has more
//!   plants than fit. Warnings sit over the grid because *show hour* tints a column of it.
//! - **Forecast**: the chart. Side by side with the grid there would be no room at 1024 px.
//! - **Tariff**: the tariff and the projected rate review.
//!
//! *Start the day* is always in view in the tab row. It waits while a row has an edit that has not
//! been applied, since the day would begin without it.

use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::DayPhase;

use super::action::{
    ActionSink, ButtonModel, ToggleModel, UiAction, button, dispatch, toggle_button,
};
use super::commitment::{CommitmentModel, commitment_model, draw_commitment, has_unsaved_draft};
use super::forecast_chart::{ForecastModel, draw_forecast_chart, forecast_model};
use super::layout::HudArea;
use super::planning_warnings::{WarningsModel, draw_warnings, warnings_model};
use super::tariff::{TariffModel, draw_tariff, tariff_model};
use super::ui_state::{CommitmentForm, PlanningTab, UiState};
use crate::l10n::Localizer;
use crate::sim::Session;

/// The panel's height, including its margins, fixed so the map above it never moves.
pub const PLANNING_PANEL_HEIGHT: f32 = 240.0;

#[derive(Debug, Clone, PartialEq)]
pub enum PlanningContent {
    Commitments {
        warnings: WarningsModel,
        grid: CommitmentModel,
    },
    Forecast(Box<ForecastModel>),
    Tariff(TariffModel),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanningModel {
    /// One per tab, in order; the open one is on.
    pub tabs: Vec<ToggleModel>,
    /// Begins the day. Disabled while an edit is unapplied.
    pub begin: ButtonModel,
    /// Why `begin` waits, when it does.
    pub begin_hint: Option<String>,
    pub content: PlanningContent,
}

/// The Planning panel; `None` outside Planning, when nothing can be committed.
pub fn planning_model(
    game: &Game,
    l10n: &Localizer,
    ui: &UiState,
    form: &CommitmentForm,
) -> Option<PlanningModel> {
    if game.session().phase() != DayPhase::Planning {
        return None;
    }
    let tab = |tab: PlanningTab, key: &str| ToggleModel {
        button: ButtonModel::new(l10n.text(key), UiAction::SetPlanningTab(tab)),
        on: ui.tab == tab,
    };
    let unsaved = has_unsaved_draft(game, ui);
    Some(PlanningModel {
        tabs: vec![
            tab(PlanningTab::Commitments, "commitment-title"),
            tab(PlanningTab::Forecast, "forecast-title"),
            tab(PlanningTab::Tariff, "tariff-title"),
        ],
        begin: ButtonModel::new(
            l10n.text("start-operating"),
            UiAction::Command(Command::BeginOperating),
        )
        .enabled_if(!unsaved),
        begin_hint: unsaved.then(|| l10n.text("commitment-unsaved")),
        content: match ui.tab {
            PlanningTab::Commitments => PlanningContent::Commitments {
                warnings: warnings_model(game, l10n),
                grid: commitment_model(game, l10n, ui, form),
            },
            PlanningTab::Forecast => {
                PlanningContent::Forecast(Box::new(forecast_model(game, l10n, ui)))
            }
            PlanningTab::Tariff => PlanningContent::Tariff(tariff_model(game, l10n)),
        },
    })
}

/// The tab row and the open tab. Doesn't choose its container: the system puts it in a bottom
/// panel.
pub fn draw_planning(
    ui: &mut egui::Ui,
    model: &PlanningModel,
    form: &mut CommitmentForm,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        for tab in &model.tabs {
            toggle_button(ui, tab, &mut actions);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            button(ui, &model.begin, &mut actions);
            if let Some(hint) = &model.begin_hint {
                ui.colored_label(ui.visuals().warn_fg_color, hint.as_str());
            }
        });
    });
    ui.separator();
    match &model.content {
        PlanningContent::Commitments { warnings, grid } => {
            actions.extend(draw_warnings(ui, warnings));
            ui.add_space(2.0);
            actions.extend(draw_commitment(ui, grid, form));
        }
        PlanningContent::Forecast(chart) => actions.extend(draw_forecast_chart(ui, chart)),
        PlanningContent::Tariff(tariff) => actions.extend(draw_tariff(ui, tariff)),
    }
    actions
}

pub(super) fn planning_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut form: ResMut<CommitmentForm>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = planning_model(&session.0, &l10n, sink.ui_state(), &form) else {
        return Ok(());
    };
    let mut ui = area.root_ui(contexts.ctx_mut()?, "planning");
    let actions = egui::Panel::bottom("planning")
        .resizable(false)
        .exact_size(PLANNING_PANEL_HEIGHT)
        .show(&mut ui, |ui| draw_planning(ui, &model, &mut form))
        .inner;
    area.claim(&ui);
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    fn build(game: &Game, ui: &UiState) -> PlanningModel {
        planning_model(game, &shipped_l10n(), ui, &CommitmentForm::default())
            .expect("the game is planning")
    }

    #[test]
    fn the_panel_opens_on_the_commitment_grid_with_its_warnings() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = build(&game, &UiState::default());

        let tabs: Vec<(&str, bool)> = model
            .tabs
            .iter()
            .map(|tab| (tab.button.label.as_str(), tab.on))
            .collect();
        assert_eq!(
            tabs,
            [
                ("Commitments", true),
                ("Forecast", false),
                ("Tariff", false)
            ]
        );
        assert!(matches!(model.content, PlanningContent::Commitments { .. }));
        assert_eq!(model.begin.label, "Start the day");
        assert_eq!(
            model.begin.action,
            UiAction::Command(Command::BeginOperating)
        );
        assert!(model.begin.enabled);
        assert_eq!(model.begin_hint, None);
    }

    #[test]
    fn each_tab_button_opens_its_tab() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut ui = UiState::default();

        let actions: Vec<UiAction> = build(&game, &ui)
            .tabs
            .iter()
            .map(|tab| tab.button.action.clone())
            .collect();
        assert_eq!(
            actions,
            [
                UiAction::SetPlanningTab(PlanningTab::Commitments),
                UiAction::SetPlanningTab(PlanningTab::Forecast),
                UiAction::SetPlanningTab(PlanningTab::Tariff),
            ]
        );

        ui.apply(&UiAction::SetPlanningTab(PlanningTab::Forecast));
        let forecast = build(&game, &ui);
        assert!(matches!(forecast.content, PlanningContent::Forecast(_)));
        assert!(forecast.tabs[1].on && !forecast.tabs[0].on);

        ui.apply(&UiAction::SetPlanningTab(PlanningTab::Tariff));
        assert!(matches!(
            build(&game, &ui).content,
            PlanningContent::Tariff(_)
        ));
    }

    #[test]
    fn the_day_waits_for_an_unapplied_edit() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut ui = UiState::default();
        let plant = game.session().plant_views()[0].id;

        // An edit that changes nothing is not unsaved.
        ui.apply(&UiAction::EditCommitment {
            plant,
            hours: [false; 24],
        });
        assert!(build(&game, &ui).begin.enabled);

        ui.apply(&UiAction::EditCommitment {
            plant,
            hours: [true; 24],
        });
        let waiting = build(&game, &ui);
        assert!(!waiting.begin.enabled);
        assert_eq!(
            waiting.begin_hint.as_deref(),
            Some("Apply or revert your changes before starting the day.")
        );

        ui.apply(&UiAction::DiscardCommitmentDraft);
        assert!(build(&game, &ui).begin.enabled);
    }

    #[test]
    fn there_is_no_planning_panel_once_the_day_has_begun() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();

        assert_eq!(
            planning_model(
                &game,
                &shipped_l10n(),
                &UiState::default(),
                &CommitmentForm::default()
            ),
            None
        );
    }
}
