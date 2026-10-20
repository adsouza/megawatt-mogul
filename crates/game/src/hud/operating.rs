//! The Operating screen's two panels (spec §8.4), laid out like Planning's so the map moves only
//! when the player drags a panel's edge (both are resizable from their opening sizes, §16.7):
//!
//! - **The bottom panel**, full width, opening at [`OPERATING_PANEL_HEIGHT`]. Its body is
//!   a tab, *Supply* (the supply chart) or *Hydro* (the hydro schedule). Beside it is a column of
//!   fixed width ([`OPERATING_LOG_WIDTH`]) holding the ramp warnings and the incident ticker.
//! - **The side panel** on the right, opening at [`OPERATING_SIDE_WIDTH`]: the frequency
//!   gauge, the selected plant's details (or a placeholder of the same height), and the plant
//!   list, which fills the rest.
//!
//! The speed controls are a bar of their own under the top bar (`speed.rs`). The map takes what is
//! left.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::DayPhase;

use super::action::{ActionSink, ButtonModel, ToggleModel, UiAction, dispatch, toggle_button};
use super::frequency::{FrequencyModel, draw_frequency, frequency_model};
use super::hydro_panel::{HydroModel, draw_hydro, hydro_model};
use super::incidents::{IncidentsModel, draw_incidents, incidents_model};
use super::layout::{Edge, HudArea, OuterPanel};
use super::plant_panel::{
    PlantListModel, PlantPanelForm, PlantPanelModel, draw_plant_list, draw_plant_panel,
    named_region, plant_details_height, plant_list_model, plant_panel_model,
};
use super::ramp_warnings::{RampWarningsModel, draw_ramp_warnings, ramp_warnings_model};
use super::selection::Selection;
use super::supply_chart::{SupplyModel, draw_supply_chart, supply_model};
use crate::l10n::Localizer;
use crate::sim::Session;

/// The bottom panel's height, including its margins.
pub const OPERATING_PANEL_HEIGHT: f32 = 200.0;
/// The width of the ramp warnings and incidents column inside the bottom panel.
pub const OPERATING_LOG_WIDTH: f32 = 300.0;
/// The right side panel's width.
pub const OPERATING_SIDE_WIDTH: f32 = 300.0;
/// The bottom panel as an outer panel, resizable from [`OPERATING_PANEL_HEIGHT`].
pub const OPERATING_PANEL: OuterPanel = OuterPanel {
    id: "operating",
    edge: Edge::Bottom,
    size: OPERATING_PANEL_HEIGHT,
};
/// The side panel as an outer panel, resizable from [`OPERATING_SIDE_WIDTH`].
pub const OPERATING_SIDE_PANEL: OuterPanel = OuterPanel {
    id: "operating_side",
    edge: Edge::Right,
    size: OPERATING_SIDE_WIDTH,
};
/// The frequency gauge's height in the side panel, room for every line it can show.
pub const FREQUENCY_GAUGE_HEIGHT: f32 = 104.0;
/// The ramp warnings' share of the column; the incidents take the rest.
const RAMP_WARNINGS_HEIGHT: f32 = 72.0;

/// The tabs of the bottom Operating panel. Written by [`dispatch`], reset with each game.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperatingTab {
    #[default]
    Supply,
    Hydro,
}

pub(super) fn reset_operating_tab(mut tab: ResMut<OperatingTab>) {
    *tab = OperatingTab::default();
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperatingContent {
    Supply(Box<SupplyModel>),
    Hydro(HydroModel),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperatingPanelModel {
    /// One per tab, in order; the open one is on.
    pub tabs: Vec<ToggleModel>,
    pub content: OperatingContent,
    pub ramp: RampWarningsModel,
    pub incidents: IncidentsModel,
}

/// The bottom panel; `None` outside Operating.
pub fn operating_panel_model(
    game: &Game,
    l10n: &Localizer,
    tab: OperatingTab,
) -> Option<OperatingPanelModel> {
    if game.session().phase() != DayPhase::Operating {
        return None;
    }
    let toggle = |choice: OperatingTab, key: &str| ToggleModel {
        button: ButtonModel::new(l10n.text(key), UiAction::SetOperatingTab(choice)),
        on: tab == choice,
    };
    Some(OperatingPanelModel {
        tabs: vec![
            toggle(OperatingTab::Supply, "operating-tab-supply"),
            toggle(OperatingTab::Hydro, "hydro-title"),
        ],
        content: match tab {
            OperatingTab::Supply => OperatingContent::Supply(Box::new(supply_model(game, l10n))),
            OperatingTab::Hydro => OperatingContent::Hydro(hydro_model(game, l10n)),
        },
        ramp: ramp_warnings_model(game, l10n),
        incidents: incidents_model(game, l10n),
    })
}

/// The column on the right, then the tab row and the open tab. Doesn't choose its container: the
/// system puts it in a bottom panel.
pub fn draw_operating_panel(ui: &mut egui::Ui, model: &OperatingPanelModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    egui::Panel::right("operating_log")
        .resizable(false)
        .exact_size(OPERATING_LOG_WIDTH)
        .show(ui, |ui| {
            ui.allocate_ui(
                egui::vec2(ui.available_width(), RAMP_WARNINGS_HEIGHT),
                |ui| {
                    ui.set_min_height(RAMP_WARNINGS_HEIGHT);
                    egui::ScrollArea::vertical()
                        .id_salt("operating_ramp_warnings")
                        .auto_shrink([false, false])
                        .max_height(RAMP_WARNINGS_HEIGHT)
                        .show(ui, |ui| actions.extend(draw_ramp_warnings(ui, &model.ramp)));
                },
            );
            ui.separator();
            actions.extend(draw_incidents(ui, &model.incidents));
        });
    ui.horizontal(|ui| {
        for tab in &model.tabs {
            toggle_button(ui, tab, &mut actions);
        }
    });
    ui.separator();
    match &model.content {
        OperatingContent::Supply(chart) => actions.extend(draw_supply_chart(ui, chart)),
        OperatingContent::Hydro(hydro) => actions.extend(draw_hydro(ui, hydro)),
    }
    actions
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperatingSideModel {
    pub frequency: FrequencyModel,
    pub plants: PlantListModel,
    pub selected: PlantPanelModel,
}

/// The side panel; `None` outside Operating.
pub fn operating_side_model(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
) -> Option<OperatingSideModel> {
    if game.session().phase() != DayPhase::Operating {
        return None;
    }
    Some(OperatingSideModel {
        frequency: frequency_model(game, l10n),
        plants: plant_list_model(game, l10n, selection),
        selected: plant_panel_model(game, l10n, selection),
    })
}

/// From the top: the gauge, then the selected plant's details in a block of fixed height (the
/// same with nothing selected, so the list never moves), then the plant list, which takes all
/// the height left.
pub fn draw_operating_side(
    ui: &mut egui::Ui,
    model: &OperatingSideModel,
    form: &mut PlantPanelForm,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let width = ui.available_width();
    ui.allocate_ui(egui::vec2(width, FREQUENCY_GAUGE_HEIGHT), |ui| {
        ui.set_min_size(egui::vec2(width, FREQUENCY_GAUGE_HEIGHT));
        actions.extend(draw_frequency(ui, &model.frequency));
    });
    ui.separator();
    let details = egui::vec2(width, plant_details_height(ui));
    let block = ui.allocate_ui(details, |ui| {
        ui.set_min_size(details);
        actions.extend(draw_plant_panel(ui, &model.selected, form));
    });
    named_region(&block.response, model.selected.name());
    ui.separator();
    actions.extend(draw_plant_list(ui, &model.plants));
    actions
}

pub(super) fn operating_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = operating_panel_model(&session.0, &l10n, sink.operating_tab()) else {
        return Ok(());
    };
    let actions = area.show_panel(contexts.ctx_mut()?, OPERATING_PANEL, 0.0, |ui| {
        draw_operating_panel(ui, &model)
    });
    dispatch(actions, &mut sink);
    Ok(())
}

pub(super) fn operating_side(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut form: ResMut<PlantPanelForm>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = operating_side_model(&session.0, &l10n, sink.selection()) else {
        return Ok(());
    };
    let actions = area.show_panel(contexts.ctx_mut()?, OPERATING_SIDE_PANEL, 0.0, |ui| {
        draw_operating_side(ui, &model, &mut form)
    });
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn the_panels_are_operating_only() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        assert_eq!(
            operating_panel_model(&game, &l10n, OperatingTab::Supply),
            None
        );
        assert_eq!(operating_side_model(&game, &l10n, Selection::None), None);

        game.execute(Command::BeginOperating).unwrap();

        assert!(operating_panel_model(&game, &l10n, OperatingTab::Supply).is_some());
        assert!(operating_side_model(&game, &l10n, Selection::None).is_some());
    }

    #[test]
    fn the_tabs_open_supply_or_hydro() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();

        let supply = operating_panel_model(&game, &l10n, OperatingTab::Supply).unwrap();
        let tabs: Vec<(&str, &UiAction, bool)> = supply
            .tabs
            .iter()
            .map(|tab| (tab.button.label.as_str(), &tab.button.action, tab.on))
            .collect();
        assert_eq!(
            tabs,
            [
                (
                    "Supply",
                    &UiAction::SetOperatingTab(OperatingTab::Supply),
                    true
                ),
                (
                    "Hydro",
                    &UiAction::SetOperatingTab(OperatingTab::Hydro),
                    false
                ),
            ]
        );
        assert!(matches!(supply.content, OperatingContent::Supply(_)));

        let hydro = operating_panel_model(&game, &l10n, OperatingTab::Hydro).unwrap();
        assert!(matches!(hydro.content, OperatingContent::Hydro(_)));
        assert!(hydro.tabs[1].on && !hydro.tabs[0].on);
        assert_eq!(l10n.problems(), vec![]);
    }
}
