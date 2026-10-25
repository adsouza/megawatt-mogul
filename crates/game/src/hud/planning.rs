//! The Planning panel (spec §8.4): the bottom panel that holds the commitment grid, the forecast
//! chart and the tariff panel, and the button that begins the day.
//!
//! # Layout
//!
//! The window is at least 1024×640, and the commitment grid is the primary planning tool, so it
//! gets the full width in a bottom panel that opens at [`PLANNING_PANEL_HEIGHT`] and that the
//! player can drag taller (spec §16.7). The inspector and the build menu keep their side columns
//! above it, and the map takes what is left, so only a drag ever moves the map. The panel's
//! content is a tab:
//!
//! - **Commitments**: the planning warnings, then the grid, which scrolls when the fleet has more
//!   plants than fit. Warnings sit over the grid because *show hour* tints a column of it.
//! - **Forecast**: the chart. Side by side with the grid there would be no room at 1024 px.
//! - **Tariff**: the tariff and the projected rate review.
//! - **Construction**: the plants and lines under construction, with their progress and the month
//!   each enters service (spec §16.9). Its label counts them.
//!
//! *Start the day* is always in view in the tab row. It waits while a row has an edit that has not
//! been applied, since the day would begin without it. When the forecast is short (spec §16.4) it
//! asks first: it opens the shortfall prompt instead of beginning the day.

use crate::l10n::format::MoneyStyle;
use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::DayPhase;

use super::action::{
    ActionSink, ButtonModel, Prompt, ToggleModel, UiAction, button, dispatch, toggle_button,
};
use super::commitment::{CommitmentModel, commitment_model, draw_commitment, has_unsaved_draft};
use super::construction::{
    ConstructionModel, construction_model, construction_tab_label, draw_construction,
};
use super::forecast_chart::{ForecastModel, draw_forecast_chart};
use super::layout::{Edge, HudArea, OuterPanel};
use super::planning_cache::{PlanningForecast, PlanningForecastCache};
use super::planning_warnings::{WarningsModel, draw_warnings, warnings_for};
use super::tariff::{TariffModel, draw_tariff, tariff_model};
use super::theme::TextTone;
use super::ui_state::{CommitmentForm, PlanningTab, UiState};
use super::{SupplyHistoryModel, draw_supply_history, supply_history_model};
use crate::l10n::Localizer;
use crate::sim::Session;

/// The panel's height, including its margins, fixed so the map above it never moves.
pub const PLANNING_PANEL_HEIGHT: f32 = 240.0;
/// The Planning panel as an outer panel: along the bottom, resizable from
/// [`PLANNING_PANEL_HEIGHT`].
pub const PLANNING_PANEL: OuterPanel = OuterPanel {
    id: "planning",
    edge: Edge::Bottom,
    size: PLANNING_PANEL_HEIGHT,
};

#[derive(Debug, Clone, PartialEq)]
pub enum PlanningContent {
    Commitments {
        warnings: WarningsModel,
        grid: CommitmentModel,
    },
    Forecast(Box<ForecastModel>),
    Tariff(TariffModel),
    Construction(ConstructionModel),
    History(SupplyHistoryModel),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanningModel {
    /// One per tab, in order; the open one is on.
    pub tabs: Vec<ToggleModel>,
    /// Begins the day, or on a short forecast opens the prompt that asks first. Disabled while an
    /// edit is unapplied.
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
    style: &MoneyStyle,
) -> Option<PlanningModel> {
    if game.session().phase() != DayPhase::Planning {
        return None;
    }
    planning_model_for(
        game,
        l10n,
        ui,
        form,
        &PlanningForecast::of(game, l10n),
        style,
    )
}

fn planning_model_for(
    game: &Game,
    l10n: &Localizer,
    ui: &UiState,
    form: &CommitmentForm,
    forecast: &PlanningForecast,
    style: &MoneyStyle,
) -> Option<PlanningModel> {
    if game.session().phase() != DayPhase::Planning {
        return None;
    }
    let tab = |tab: PlanningTab, label: String| ToggleModel {
        button: ButtonModel::new(label, UiAction::SetPlanningTab(tab)),
        on: ui.tab == tab,
    };
    let unsaved = has_unsaved_draft(game, ui);
    Some(PlanningModel {
        tabs: vec![
            tab(PlanningTab::Commitments, l10n.text("commitment-title")),
            tab(PlanningTab::Forecast, l10n.text("forecast-title")),
            tab(PlanningTab::Tariff, l10n.text("tariff-title")),
            tab(
                PlanningTab::Construction,
                construction_tab_label(game, l10n),
            ),
            tab(PlanningTab::History, l10n.text("supply-history-title")),
        ],
        begin: ButtonModel::new(l10n.text("start-operating"), start_the_day(forecast))
            .enabled_if(!unsaved),
        begin_hint: unsaved.then(|| l10n.text("commitment-unsaved")),
        content: match ui.tab {
            PlanningTab::Commitments => PlanningContent::Commitments {
                warnings: warnings_for(
                    forecast.shortfall,
                    forecast.minimum,
                    &forecast.dry_hydro,
                    ui.warnings_open,
                    l10n,
                ),
                grid: commitment_model(game, l10n, ui, form),
            },
            PlanningTab::Forecast => {
                let mut chart = forecast.chart.clone();
                chart.highlight_hour = ui.highlight_hour;
                PlanningContent::Forecast(Box::new(chart))
            }
            PlanningTab::Tariff => PlanningContent::Tariff(tariff_model(game, l10n, style)),
            PlanningTab::Construction => {
                PlanningContent::Construction(construction_model(game, l10n))
            }
            PlanningTab::History => PlanningContent::History(
                supply_history_model(game, ui.supply_month, l10n, style)
                    .with_layout(ui.supply_chart_layout),
            ),
        },
    })
}

/// What *Start the day* sends: the day itself, or, when the committed fleet falls short of the
/// forecast (§15.5), the prompt that asks first, holding the figures the warnings strip shows.
/// Only a shortfall asks: a minimum-generation or dry-hydro warning doesn't.
fn start_the_day(forecast: &PlanningForecast) -> UiAction {
    match forecast.shortfall {
        None => UiAction::Command(Command::BeginOperating),
        Some(warning) => UiAction::Prompt(Some(Prompt::ShortfallStart {
            hour: warning.hour,
            shortfall: warning.shortfall,
        })),
    }
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
                ui.colored_label(TextTone::Caution.color(), hint);
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
        PlanningContent::Construction(construction) => draw_construction(ui, construction),
        PlanningContent::History(history) => actions.extend(draw_supply_history(ui, history)),
    }
    actions
}

pub(super) fn planning_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    forecast: Res<PlanningForecastCache>,
    text: (Res<Localizer>, Res<MoneyStyle>),
    mut form: ResMut<CommitmentForm>,
    mut sink: ActionSink,
) -> Result {
    let (l10n, style) = text;
    let Some(forecast) = forecast.get() else {
        return Ok(());
    };
    let Some(model) =
        planning_model_for(&session.0, &l10n, sink.ui_state(), &form, forecast, &style)
    else {
        return Ok(());
    };
    let actions = area.show_panel(contexts.ctx_mut()?, PLANNING_PANEL, 0.0, |ui| {
        draw_planning(ui, &model, &mut form)
    });
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use application::InMemoryCommandLog;

    use domain::region::PlantKind;

    use super::*;
    use crate::hud::action::Prompt;
    use crate::hud::planning_warnings::WarningKind;
    use crate::hud::test_support::{
        eastern_game, edited_western_game, empty_start, flat_load, order_plant, order_short_line,
        shipped_game, shipped_l10n,
    };

    fn build(game: &Game, ui: &UiState) -> PlanningModel {
        planning_model(
            game,
            &shipped_l10n(),
            ui,
            &CommitmentForm::default(),
            &usd_style(),
        )
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
                ("Tariff", false),
                ("Construction", false),
                ("History", false),
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
                UiAction::SetPlanningTab(PlanningTab::Construction),
                UiAction::SetPlanningTab(PlanningTab::History),
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

        ui.apply(&UiAction::SetPlanningTab(PlanningTab::Construction));
        let construction = build(&game, &ui);
        assert!(matches!(
            construction.content,
            PlanningContent::Construction(_)
        ));
        assert!(construction.tabs[3].on && !construction.tabs[2].on);
    }

    /// The Construction tab's label counts the projects; with none, the tab says so in one line
    /// (spec §16.9).
    #[test]
    fn the_construction_tab_label_counts_projects() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let ui = UiState {
            tab: PlanningTab::Construction,
            ..UiState::default()
        };
        let label = |model: &PlanningModel| model.tabs[3].button.label.clone();

        let empty = build(&game, &ui);
        assert_eq!(label(&empty), "Construction");
        assert_eq!(
            empty.content,
            PlanningContent::Construction(ConstructionModel {
                rows: Vec::new(),
                empty: Some("Nothing under construction.".to_owned()),
            })
        );

        order_plant(&mut game, PlantKind::Peaker, 0);
        order_short_line(&mut game);
        let two = build(&game, &ui);
        assert_eq!(label(&two), "Construction (2)");
        let PlanningContent::Construction(model) = &two.content else {
            panic!("the Construction tab is open");
        };
        assert_eq!(model, &construction_model(&game, &l10n));
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.empty, None);
        // The label counts on every tab, so the player sees it without opening this one.
        assert_eq!(
            label(&build(&game, &UiState::default())),
            "Construction (2)"
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_day_waits_for_an_unapplied_edit() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
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
    fn start_the_day_begins_directly_without_a_shortfall() {
        for region in ["eastern_interconnection", "western_interconnection"] {
            // The inherited schedule covers the forecast (spec §16.2).
            let game = shipped_game(region, Box::new(InMemoryCommandLog::default()));
            assert_eq!(game.session().capacity_shortfall_check(), None, "{region}");

            let begin = build(&game, &UiState::default()).begin;

            assert_eq!(begin.label, "Start the day", "{region}");
            assert_eq!(
                begin.action,
                UiAction::Command(Command::BeginOperating),
                "{region}"
            );
            assert!(begin.enabled, "{region}");
        }
    }

    #[test]
    fn start_the_day_asks_first_on_a_shortfall() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let worst = game
            .session()
            .capacity_shortfall_check()
            .expect("nothing is committed, so demand goes uncovered");

        // On every tab: the button is always in view.
        for tab in [
            PlanningTab::Commitments,
            PlanningTab::Forecast,
            PlanningTab::Tariff,
            PlanningTab::Construction,
        ] {
            let ui = UiState {
                tab,
                ..UiState::default()
            };
            let begin = planning_model(&game, &l10n, &ui, &CommitmentForm::default(), &usd_style())
                .expect("the game is planning")
                .begin;

            assert_eq!(begin.label, "Start the day", "{tab:?}");
            assert_eq!(
                begin.action,
                UiAction::Prompt(Some(Prompt::ShortfallStart {
                    hour: worst.hour,
                    shortfall: worst.shortfall,
                })),
                "{tab:?}"
            );
            assert!(begin.enabled, "{tab:?}");
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    /// Only a shortfall asks (spec §16.2's anti-goal). A minimum-generation warning on its own
    /// leaves *Start the day* beginning the day. A dry hydro plant can't happen on valid region
    /// data (every month's river flow is positive), and the button never reads that check.
    #[test]
    fn min_gen_or_dry_hydro_alone_never_asks() {
        let l10n = shipped_l10n();
        let mut game = edited_western_game(|text| flat_load(text, 5.0));
        let plants: Vec<_> = game
            .session()
            .plant_views()
            .into_iter()
            .filter(|plant| plant.kind == PlantKind::Ccgt)
            .map(|plant| plant.id)
            .collect();
        for plant in plants {
            game.execute(Command::StartPlant { plant }).unwrap();
        }
        let model = build(&game, &UiState::default());
        let PlanningContent::Commitments { warnings, .. } = &model.content else {
            panic!("the panel opens on the grid");
        };
        let kinds: Vec<WarningKind> = warnings.rows.iter().map(|row| row.kind).collect();
        assert_eq!(kinds, [WarningKind::MinimumGeneration]);

        assert_eq!(
            model.begin.action,
            UiAction::Command(Command::BeginOperating)
        );
        assert_eq!(l10n.problems(), vec![]);
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
                &CommitmentForm::default(),
                &usd_style()
            ),
            None
        );
    }
}
