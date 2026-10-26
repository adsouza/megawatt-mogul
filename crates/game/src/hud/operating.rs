//! The Operating screen's two panels (spec §8.4), laid out like Planning's so the map moves only
//! when the player drags a panel's edge (both are resizable from their opening sizes, §16.7):
//!
//! - **The bottom panel**, full width, opening at [`OPERATING_PANEL_HEIGHT`]. Its body is
//!   a tab, *Supply* (the supply chart) or *Hydro* (the hydro schedule). Beside it is a column
//!   opening at [`OPERATING_LOG_WIDTH`], with a draggable divider, holding the ramp warnings
//!   and the incident ticker.
//! - **The side panel** on the right, opening at [`OPERATING_SIDE_WIDTH`]: the frequency
//!   gauge, the selected plant's details, city's card or construction card (or a placeholder of
//!   the same height), and the plant list, which fills the rest.
//!
//! The speed controls are a bar of their own under the top bar (`speed.rs`). The map takes what is
//! left.

use crate::l10n::format::MoneyStyle;
use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::DayPhase;

use super::action::{ActionSink, ButtonModel, ToggleModel, UiAction, dispatch, toggle_button};
use super::city_card::{CityCardModel, city_card_model, draw_city_card};
use super::construction::{ConstructionCardModel, construction_card_model, draw_construction_card};
use super::frequency::{FrequencyModel, draw_frequency, frequency_model};
use super::hydro_panel::{HydroModel, draw_hydro, hydro_model};
use super::incidents::{IncidentsModel, draw_incidents, incidents_model};
use super::layout::{Edge, HudArea, MAP_MIN_SIZE, OuterPanel};
use super::plant_panel::{
    PlantListModel, PlantPanelForm, PlantPanelModel, draw_plant_list, draw_plant_panel,
    named_region, plant_details_height, plant_list_min_height, plant_list_model, plant_panel_model,
};
use super::ramp_warnings::{RampWarningsModel, draw_ramp_warnings, ramp_warnings_model};
use super::selection::Selection;
use super::supply_chart::{SupplyModel, draw_supply_chart, supply_model};
use crate::l10n::Localizer;
use crate::sim::Session;

/// The bottom panel's height, including its margins.
pub const OPERATING_PANEL_HEIGHT: f32 = 200.0;
/// The opening width of the ramp warnings and incidents column inside the bottom panel.
pub const OPERATING_LOG_WIDTH: f32 = 480.0;
/// Resizing the log leaves room for the active chart or hydro schedule.
const OPERATING_CONTENT_MIN_WIDTH: f32 = 420.0;
const OPERATING_LOG_MIN_WIDTH: f32 = 300.0;
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
    History,
}

pub(super) fn reset_operating_tab(mut tab: ResMut<OperatingTab>) {
    *tab = OperatingTab::default();
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperatingContent {
    Supply(Box<SupplyModel>),
    Hydro(HydroModel),
    History(super::SupplyHistoryModel),
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
    style: &MoneyStyle,
) -> Option<OperatingPanelModel> {
    operating_model_with_history(
        game,
        l10n,
        tab,
        None,
        super::SupplyChartLayout::default(),
        style,
    )
}

fn operating_model_with_history(
    game: &Game,
    l10n: &Localizer,
    tab: OperatingTab,
    month: Option<domain::time::SimMonth>,
    layout: super::SupplyChartLayout,
    style: &MoneyStyle,
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
            toggle(OperatingTab::History, "supply-history-title"),
        ],
        content: match tab {
            OperatingTab::Supply => OperatingContent::Supply(Box::new(
                supply_model(game, l10n, style).with_layout(layout),
            )),
            OperatingTab::Hydro => OperatingContent::Hydro(hydro_model(game, l10n)),
            OperatingTab::History => OperatingContent::History(
                super::supply_history_model(game, month, l10n, style).with_layout(layout),
            ),
        },
        ramp: ramp_warnings_model(game, l10n),
        incidents: incidents_model(game, l10n),
    })
}

/// The column on the right, then the tab row and the open tab. Doesn't choose its container: the
/// system puts it in a bottom panel.
pub fn draw_operating_panel(ui: &mut egui::Ui, model: &OperatingPanelModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    if !matches!(model.content, OperatingContent::History(_)) {
        egui::Panel::right("operating_log")
            .resizable(true)
            .default_size(OPERATING_LOG_WIDTH)
            .size_range(
                OPERATING_LOG_MIN_WIDTH
                    ..=(ui.available_width() - OPERATING_CONTENT_MIN_WIDTH)
                        .max(OPERATING_LOG_MIN_WIDTH),
            )
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
    }
    ui.horizontal(|ui| {
        for tab in &model.tabs {
            toggle_button(ui, tab, &mut actions);
        }
    });
    ui.separator();
    match &model.content {
        OperatingContent::Supply(chart) => actions.extend(draw_supply_chart(ui, chart)),
        OperatingContent::Hydro(hydro) => actions.extend(draw_hydro(ui, hydro)),
        OperatingContent::History(history) => {
            actions.extend(super::draw_supply_history(ui, history))
        }
    }
    actions
}

/// What the side panel's selected-object block shows.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectedObject {
    /// The selected plant's details, or the placeholder when nothing (or a cell) is selected.
    Plant(PlantPanelModel),
    /// The selected city's card (spec §16.8).
    City(CityCardModel),
    /// The card of a selected plant under construction (spec §16.9).
    Construction(ConstructionCardModel),
}

impl SelectedObject {
    /// What the block is called: the plant's or the city's name, or the placeholder's hint.
    pub fn name(&self) -> &str {
        match self {
            Self::Plant(plant) => plant.name(),
            Self::City(city) => &city.name,
            Self::Construction(card) => &card.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperatingSideModel {
    pub frequency: FrequencyModel,
    pub plants: PlantListModel,
    pub selected: SelectedObject,
}

/// The side panel; `None` outside Operating.
pub fn operating_side_model(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
    style: &MoneyStyle,
) -> Option<OperatingSideModel> {
    if game.session().phase() != DayPhase::Operating {
        return None;
    }
    Some(OperatingSideModel {
        frequency: frequency_model(game, l10n),
        plants: plant_list_model(game, l10n, selection),
        selected: match selection {
            Selection::City(city) => city_card_model(game, l10n, city).map(SelectedObject::City),
            // `None` for a built plant, which shows its details.
            Selection::Plant(plant) => {
                construction_card_model(game, l10n, plant, style).map(SelectedObject::Construction)
            }
            _ => None,
        }
        .unwrap_or_else(|| SelectedObject::Plant(plant_panel_model(game, l10n, selection, style))),
    })
}

/// The space each of the side panel's separators takes.
const SIDE_SEPARATOR_SPACING: f32 = 6.0;

/// The least height the side panel needs, frame included: the gauge, the details block, the
/// list's title and its fewest rows, with the separators and gaps between them. The bottom panel
/// is never dragged so tall that the side panel has less (spec §16.7).
pub fn operating_side_min_height(ctx: &egui::Context) -> f32 {
    let style = ctx.global_style();
    let gap = style.spacing.item_spacing.y;
    let frame = egui::Frame::side_top_panel(&style).total_margin().sum().y;
    // Five blocks, top to bottom: the gauge, a separator, the details, a separator, the list.
    FREQUENCY_GAUGE_HEIGHT
        + plant_details_height(ctx)
        + plant_list_min_height(ctx)
        + 2.0 * SIDE_SEPARATOR_SPACING
        + 4.0 * gap
        + frame
}

/// From the top: the gauge, then the selected plant's details, city's card or construction card
/// in a block of fixed height (the same with nothing selected, so the list never moves), then
/// the plant list, which takes all the height left.
pub fn draw_operating_side(
    ui: &mut egui::Ui,
    model: &OperatingSideModel,
    form: &mut PlantPanelForm,
) -> Vec<UiAction> {
    let frame = egui::Frame::side_top_panel(ui.style())
        .total_margin()
        .sum()
        .y;
    let minimum = operating_side_min_height(ui.ctx()) - frame;
    if ui.available_height() + 0.5 < minimum {
        // Increased UI zoom can leave less room than the column's minimum, even with the
        // bottom bar at its smallest. Keep every block reachable instead of clipping the list.
        return egui::ScrollArea::vertical()
            .id_salt("operating_side_compact")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), minimum), |ui| {
                    ui.set_min_height(minimum);
                    draw_operating_side_contents(ui, model, form)
                })
                .inner
            })
            .inner;
    }
    draw_operating_side_contents(ui, model, form)
}

fn draw_operating_side_contents(
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
    ui.add(egui::Separator::default().spacing(SIDE_SEPARATOR_SPACING));
    let details = egui::vec2(width, plant_details_height(ui.ctx()));
    let block = ui.allocate_ui(details, |ui| {
        ui.set_min_size(details);
        match &model.selected {
            SelectedObject::Plant(plant) => actions.extend(draw_plant_panel(ui, plant, form)),
            // In a scroll area, like the plant's figures, so the block keeps its height whatever
            // the card holds.
            SelectedObject::City(city) => {
                egui::ScrollArea::vertical()
                    .id_salt("operating_city_card")
                    .auto_shrink([false, false])
                    .show(ui, |ui| draw_city_card(ui, city));
            }
            SelectedObject::Construction(card) => {
                egui::ScrollArea::vertical()
                    .id_salt("operating_construction_card")
                    .auto_shrink([false, false])
                    .show(ui, |ui| draw_construction_card(ui, card));
            }
        }
    });
    named_region(&block.response, model.selected.name());
    ui.add(egui::Separator::default().spacing(SIDE_SEPARATOR_SPACING));
    actions.extend(draw_plant_list(ui, &model.plants));
    actions
}

pub(super) fn operating_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
    style: Res<MoneyStyle>,
) -> Result {
    let Some(model) = operating_model_with_history(
        &session.0,
        &l10n,
        sink.operating_tab(),
        sink.ui_state().supply_month,
        sink.ui_state().supply_chart_layout,
        &style,
    ) else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    // The side panel shares the height above this panel with the map. The map's own minimum is
    // already left; reserve what the side panel's minimum needs beyond it.
    let reserved = (operating_side_min_height(ctx) - MAP_MIN_SIZE.y).max(0.0);
    let actions = area.show_panel(ctx, OPERATING_PANEL, reserved, |ui| {
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
    style: Res<MoneyStyle>,
) -> Result {
    let Some(model) = operating_side_model(&session.0, &l10n, sink.selection(), &style) else {
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
    use crate::hud::test_support::usd_style;
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{eastern_game, order_plant, shipped_l10n};

    #[test]
    fn the_panels_are_operating_only() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        assert_eq!(
            operating_panel_model(&game, &l10n, OperatingTab::Supply, &usd_style()),
            None
        );
        assert_eq!(
            operating_side_model(&game, &l10n, Selection::None, &usd_style()),
            None
        );

        game.execute(Command::BeginOperating).unwrap();

        assert!(operating_panel_model(&game, &l10n, OperatingTab::Supply, &usd_style()).is_some());
        assert!(operating_side_model(&game, &l10n, Selection::None, &usd_style()).is_some());
    }

    /// The block shows the selected city's card, and a plant's details (or the placeholder)
    /// otherwise (spec §16.8).
    #[test]
    fn the_selected_block_follows_a_city_or_a_plant() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 5 }).unwrap();
        let session = game.session();
        let city = session.city_views()[0].id;
        let plant = session.plant_views()[0].id;

        let side = |selection| operating_side_model(&game, &l10n, selection, &usd_style()).unwrap();
        let card = city_card_model(&game, &l10n, city).unwrap();
        assert_eq!(side(Selection::City(city)).selected.name(), card.name);
        assert_eq!(
            side(Selection::City(city)).selected,
            SelectedObject::City(card)
        );
        for selection in [Selection::Plant(plant), Selection::None] {
            assert_eq!(
                side(selection).selected,
                SelectedObject::Plant(plant_panel_model(&game, &l10n, selection, &usd_style())),
                "{selection:?}"
            );
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    /// A plant under construction shows its construction card in the block; a built plant keeps
    /// its details (spec §16.9).
    #[test]
    fn a_selected_plant_under_construction_shows_its_card() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let built = game.session().plant_views()[0].id;
        let plant = order_plant(&mut game, domain::region::PlantKind::Peaker, 0);
        game.execute(Command::BeginOperating).unwrap();

        let side = |selection| operating_side_model(&game, &l10n, selection, &usd_style()).unwrap();
        let card = construction_card_model(&game, &l10n, plant, &usd_style()).expect("a card");
        assert_eq!(side(Selection::Plant(plant)).selected.name(), card.name);
        assert_eq!(
            side(Selection::Plant(plant)).selected,
            SelectedObject::Construction(card)
        );
        assert_eq!(
            side(Selection::Plant(built)).selected,
            SelectedObject::Plant(plant_panel_model(
                &game,
                &l10n,
                Selection::Plant(built),
                &usd_style()
            ))
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_tabs_open_supply_or_hydro() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();

        let supply =
            operating_panel_model(&game, &l10n, OperatingTab::Supply, &usd_style()).unwrap();
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
                (
                    "History",
                    &UiAction::SetOperatingTab(OperatingTab::History),
                    false
                ),
            ]
        );
        assert!(matches!(supply.content, OperatingContent::Supply(_)));

        let hydro = operating_panel_model(&game, &l10n, OperatingTab::Hydro, &usd_style()).unwrap();
        assert!(matches!(hydro.content, OperatingContent::Hydro(_)));
        assert!(hydro.tabs[1].on && !hydro.tabs[0].on);
        assert_eq!(l10n.problems(), vec![]);
    }
}
