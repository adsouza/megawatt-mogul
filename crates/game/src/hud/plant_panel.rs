//! The plant list and the selected-plant panel (spec §8.4), Operating only.
//!
//! The list shows every plant, its status, output and whether AGC is on; clicking a row selects
//! the plant, as clicking it on the map does. The panel above the list follows [`Selection`]: the
//! plant's output, target, ramp rate, marginal cost, its energy today, its energy and revenue over
//! the months closed so far, its water, and the controls (AGC, start, stop and a target slider).
//!
//! The slider edits [`PlantPanelForm`], not the game. Its value is sent as one
//! `SetDispatchTarget` when a drag ends or a keyboard step lands, never while it is being dragged,
//! and it is clamped to the plant's minimum stable output and capacity first.

use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::egui;
use domain::generation::PlantStatus;
use domain::ids::PlantId;
use domain::session::PlantView;
use domain::units::{MegawattHours, Megawatts, Money};

use super::action::{ButtonModel, ToggleModel, UiAction, button, toggle_button};
use super::plant_list::{is_commitable, kinds_in_merit_order};
use super::selection::Selection;
use super::top_bar::Stat;
use crate::l10n::{Localize, Localizer, Names, format};

/// One plant in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantListRow {
    pub plant: PlantId,
    /// The plant's name; selects it. On while it is the selected plant.
    pub select: ToggleModel,
    /// `Online`, `Starting: 40 min left`, ...
    pub status: String,
    /// `1,050 MW`.
    pub output: String,
    /// `AGC` while the plant follows the load by itself.
    pub agc: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlantListModel {
    pub title: String,
    /// Every plant, kinds in merit order, each kind's plants by id.
    pub rows: Vec<PlantListRow>,
}

/// The target slider of the selected plant.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetSlider {
    pub plant: PlantId,
    /// `Set target`.
    pub label: String,
    /// The plant's minimum stable output, in megawatts.
    pub min: f64,
    /// Its capacity, in megawatts.
    pub max: f64,
    /// Its target now, or its minimum when it has none.
    pub value: f64,
    /// Only an online plant under manual control takes a target.
    pub enabled: bool,
}

impl TargetSlider {
    /// The command for a slider value: the target, clamped to what the plant accepts.
    pub fn command(&self, value: f64) -> UiAction {
        UiAction::Command(Command::SetDispatchTarget {
            plant: self.plant,
            mw: Megawatts::new(value.clamp(self.min, self.max)),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectedPlant {
    pub plant: PlantId,
    /// `Coal plant 3`.
    pub name: String,
    pub status: String,
    pub agc: ToggleModel,
    pub agc_tip: String,
    pub start: ButtonModel,
    pub stop: ButtonModel,
    pub slider: TargetSlider,
    /// Output, target, ramp rate, marginal cost, energy today, energy and revenue over the closed
    /// months, and the water left for a hydro plant.
    pub stats: Vec<Stat>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlantPanelModel {
    Plant(Box<SelectedPlant>),
    /// No plant is selected. The panel keeps its place.
    Empty {
        hint: String,
    },
}

impl PlantPanelModel {
    /// What the block is called: the plant's name, or the hint when none is selected.
    pub fn name(&self) -> &str {
        match self {
            Self::Plant(plant) => &plant.name,
            Self::Empty { hint } => hint,
        }
    }
}

/// What the slider holds between frames. A `Resource`, like the menu's seed field.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct PlantPanelForm {
    /// The plant the value belongs to.
    pub plant: Option<PlantId>,
    /// The slider's value, in megawatts.
    pub value: f64,
    /// Whether the slider was being dragged last frame.
    pub held: bool,
}

impl PlantPanelForm {
    /// Follows the plant's current target, except while the player drags the slider of the same
    /// plant. Another plant's slider starts from that plant's target.
    pub fn sync(&mut self, slider: &TargetSlider) {
        if self.plant != Some(slider.plant) {
            *self = Self {
                plant: Some(slider.plant),
                value: slider.value,
                held: false,
            };
        } else if !self.held {
            self.value = slider.value;
        }
    }
}

pub(super) fn reset_plant_form(mut form: ResMut<PlantPanelForm>) {
    *form = PlantPanelForm::default();
}

/// Every plant, kinds in merit order, each kind's plants by id.
fn plants_in_order(game: &Game) -> Vec<PlantView> {
    let session = game.session();
    let mut views = session.plant_views();
    let order = kinds_in_merit_order(session.region());
    let rank = |plant: &PlantView| order.iter().position(|kind| *kind == plant.kind);
    views.sort_by_key(|plant| (rank(plant), plant.id));
    views
}

pub fn plant_list_model(game: &Game, l10n: &Localizer, selection: Selection) -> PlantListModel {
    let names = Names::new(l10n, game.session());
    let agc = l10n.text("plant-list-agc");
    PlantListModel {
        title: l10n.text("plant-list-title"),
        rows: plants_in_order(game)
            .into_iter()
            .map(|plant| PlantListRow {
                plant: plant.id,
                select: ToggleModel {
                    button: ButtonModel::new(
                        names.plant(plant.id),
                        UiAction::Select(Selection::Plant(plant.id)),
                    ),
                    on: selection == Selection::Plant(plant.id),
                },
                status: plant.status.localize(l10n),
                output: format::megawatts(plant.output),
                agc: plant.auto_follow.then(|| agc.clone()),
            })
            .collect(),
    }
}

pub fn plant_panel_model(game: &Game, l10n: &Localizer, selection: Selection) -> PlantPanelModel {
    let session = game.session();
    let plant = match selection {
        Selection::Plant(id) => session
            .plant_views()
            .into_iter()
            .find(|plant| plant.id == id),
        _ => None,
    };
    let Some(plant) = plant else {
        return PlantPanelModel::Empty {
            hint: l10n.text("plant-select-hint"),
        };
    };
    let spec = session.region().spec(plant.kind);
    let operable = is_commitable(&plant);
    let target = match plant.status {
        PlantStatus::Online { target, .. } => Some(target),
        _ => None,
    };
    let (min, capacity) = (spec.min_output(), spec.capacity);
    let history = game.projections().plant_history.get(plant.id);
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    let mut stats = vec![
        stat("plant-output", format::megawatts(plant.output)),
        stat(
            "plant-target",
            target.map_or_else(|| l10n.text("plant-target-none"), format::megawatts),
        ),
        stat("plant-ramp", format::ramp_rate(spec.ramp)),
        stat("plant-marginal-cost", format::price(spec.marginal_cost)),
        // The played day's energy, as it happens. It is not priced: revenue is booked once, at
        // the month close, so the money figure covers closed months only.
        stat(
            "plant-energy-today",
            format::megawatt_hours(
                session
                    .plant_energy_today()
                    .get(&plant.id)
                    .copied()
                    .unwrap_or(MegawattHours::ZERO),
            ),
        ),
        stat(
            "plant-closed-months",
            l10n.args(
                "plant-closed-months-value",
                &[
                    (
                        "energy",
                        format::megawatt_hours(history.map_or(MegawattHours::ZERO, |h| h.energy))
                            .into(),
                    ),
                    (
                        "revenue",
                        format::money(history.map_or(Money::ZERO, |h| h.revenue)).into(),
                    ),
                ],
            ),
        ),
    ];
    if let Some(water) = plant.hydro_budget {
        stats.push(stat("plant-hydro-budget", format::megawatt_hours(water)));
    }
    PlantPanelModel::Plant(Box::new(SelectedPlant {
        plant: plant.id,
        name: Names::new(l10n, session).plant(plant.id),
        status: plant.status.localize(l10n),
        agc: ToggleModel {
            button: ButtonModel::new(
                l10n.text("plant-list-agc"),
                UiAction::Command(Command::SetAutoFollow {
                    plant: plant.id,
                    on: !plant.auto_follow,
                }),
            )
            .enabled_if(operable),
            on: plant.auto_follow,
        },
        agc_tip: l10n.text("commitment-agc-tip"),
        start: ButtonModel::new(
            l10n.text("plant-start"),
            UiAction::Command(Command::StartPlant { plant: plant.id }),
        )
        .enabled_if(operable && plant.status == PlantStatus::Offline),
        stop: ButtonModel::new(
            l10n.text("plant-stop"),
            UiAction::Command(Command::StopPlant { plant: plant.id }),
        )
        .enabled_if(
            operable
                && matches!(
                    plant.status,
                    PlantStatus::Online { .. } | PlantStatus::Starting { .. }
                ),
        ),
        slider: TargetSlider {
            plant: plant.id,
            label: l10n.text("plant-target-slider"),
            min: min.value(),
            max: capacity.value(),
            value: target.unwrap_or(min).value(),
            enabled: operable && target.is_some() && !plant.auto_follow,
        },
        stats,
    }))
}

/// The most figures a selected plant shows: six, and a hydro plant's water left.
pub const MOST_PLANT_STATS: usize = 7;
/// The fewest rows the plant list keeps in view, however little height it is left.
pub const PLANT_LIST_MIN_ROWS: usize = 3;
/// The list's row spacing: compact, so more of the fleet is in view.
const PLANT_ROW_SPACING: egui::Vec2 = egui::vec2(6.0, 2.0);

/// A label's row: the body text's height, rounded up to whole points as egui lays it out.
fn text_row(ctx: &egui::Context) -> f32 {
    let font = egui::TextStyle::Body.resolve(&ctx.global_style());
    ctx.fonts_mut(|fonts| fonts.row_height(&font)).ceil()
}

/// The height of the selected plant's block: what its content needs for the plant with the most
/// figures ([`MOST_PLANT_STATS`]), with no room to spare. It is the same whatever is selected (or
/// nothing), so the list under it never moves when the selection changes.
///
/// A selected city's card shows in the same block (spec §16.8). Its tallest form, a connected
/// city in an outage with [`MOST_CITY_STATS`](super::city_card::MOST_CITY_STATS) figures under
/// its name and connection, needs well under this height; the real-pass test in
/// `tests/game/city_card.rs` checks that every row of it lies inside the block. So does a selected
/// plant's construction card (spec §16.9): its name, status, capacity, months left, progress bar
/// and in-service month, checked in `tests/game/construction.rs`.
pub fn plant_details_height(ctx: &egui::Context) -> f32 {
    let spacing = ctx.global_style().spacing.clone();
    let rows = MOST_PLANT_STATS as f32;
    // The name row, the controls and the slider (each a widget's height), then the figures'
    // grid, each a gap apart.
    3.0 * spacing.interact_size.y
        + 3.0 * spacing.item_spacing.y
        + rows * text_row(ctx)
        + (rows - 1.0) * STATS_ROW_SPACING
}

/// One row of the plant list: a row's button and the space under it.
fn list_row(ctx: &egui::Context) -> f32 {
    ctx.global_style().spacing.interact_size.y + PLANT_ROW_SPACING.y
}

/// The least height the plant list takes: its title, then [`PLANT_LIST_MIN_ROWS`] rows.
pub fn plant_list_min_height(ctx: &egui::Context) -> f32 {
    text_row(ctx)
        + ctx.global_style().spacing.item_spacing.y
        + PLANT_LIST_MIN_ROWS as f32 * list_row(ctx)
}

/// The space between two of the selected plant's figures.
const STATS_ROW_SPACING: f32 = 1.0;

/// The list: its title, then every plant in a list that fills the height left and scrolls, never
/// shorter than [`PLANT_LIST_MIN_ROWS`] rows.
pub fn draw_plant_list(ui: &mut egui::Ui, model: &PlantListModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let block = ui.scope(|ui| draw_plant_rows(ui, model, &mut actions));
    named_region(&block.response, &model.title);
    actions
}

/// Names the block `response` covers for AccessKit, so a screen reader (and a test) finds it.
pub(super) fn named_region(response: &egui::Response, name: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, name));
}

fn draw_plant_rows(ui: &mut egui::Ui, model: &PlantListModel, actions: &mut Vec<UiAction>) {
    ui.strong(model.title.as_str());
    let rows = PLANT_LIST_MIN_ROWS as f32 * list_row(ui.ctx());
    egui::ScrollArea::vertical()
        .id_salt("operating_plant_list")
        .auto_shrink([false, false])
        .min_scrolled_height(rows)
        .show(ui, |ui| {
            egui::Grid::new("operating_plant_rows")
                .num_columns(4)
                .spacing(PLANT_ROW_SPACING)
                .show(ui, |ui| {
                    for row in &model.rows {
                        ui.scope(|ui| {
                            ui.spacing_mut().button_padding = egui::vec2(4.0, 0.0);
                            toggle_button(ui, &row.select, actions);
                        });
                        ui.weak(row.status.as_str());
                        ui.label(row.output.as_str());
                        ui.label(row.agc.as_deref().unwrap_or(""));
                        ui.end_row();
                    }
                });
        });
}

/// The selected plant's controls first, so they are in view at the smallest window, then its
/// figures, which scroll.
pub fn draw_plant_panel(
    ui: &mut egui::Ui,
    model: &PlantPanelModel,
    form: &mut PlantPanelForm,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let plant = match model {
        PlantPanelModel::Empty { hint } => {
            ui.weak(hint.as_str());
            return actions;
        }
        PlantPanelModel::Plant(plant) => plant,
    };
    ui.horizontal(|ui| {
        ui.strong(plant.name.as_str());
        ui.weak(plant.status.as_str());
    });
    ui.horizontal(|ui| {
        toggle_button(ui, &plant.agc, &mut actions).on_hover_text(plant.agc_tip.as_str());
        button(ui, &plant.start, &mut actions);
        button(ui, &plant.stop, &mut actions);
    });
    let slider = &plant.slider;
    form.sync(slider);
    let response = ui.add_enabled(
        slider.enabled,
        egui::Slider::new(&mut form.value, slider.min..=slider.max)
            .text(slider.label.as_str())
            .show_value(false),
    );
    // A drag sends once, as it ends; a keyboard step changes the value without a drag.
    if slider.enabled && (response.drag_stopped() || (response.changed() && !response.dragged())) {
        actions.push(slider.command(form.value));
    }
    form.held = response.dragged();
    egui::ScrollArea::vertical()
        .id_salt("operating_plant_stats")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Compact rows, so every figure is in view at the smallest window.
            egui::Grid::new("operating_plant_stats_grid")
                .num_columns(2)
                .spacing(egui::vec2(8.0, STATS_ROW_SPACING))
                .min_row_height(0.0)
                .show(ui, |ui| {
                    for stat in &plant.stats {
                        ui.weak(stat.label.as_str());
                        ui.label(stat.value.as_str());
                        ui.end_row();
                    }
                });
        });
    actions
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::region::PlantKind;

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_game, shipped_l10n};

    /// A game a few minutes into the day, with the eastern coal fleet alone committed all day,
    /// under manual control.
    fn operating() -> Game {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        for plant in game.session().plant_views() {
            if plant.kind == PlantKind::Coal {
                game.execute(Command::StartPlant { plant: plant.id })
                    .unwrap();
            }
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 5 }).unwrap();
        game
    }

    fn view(game: &Game, kind: PlantKind) -> PlantView {
        game.session()
            .plant_views()
            .into_iter()
            .find(|plant| plant.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} plant"))
    }

    fn selected(game: &Game, plant: PlantId) -> SelectedPlant {
        match plant_panel_model(game, &shipped_l10n(), Selection::Plant(plant)) {
            PlantPanelModel::Plant(model) => *model,
            other => panic!("{other:?}"),
        }
    }

    /// The details block is sized for [`MOST_PLANT_STATS`] figures: no plant shows more.
    #[test]
    fn no_plant_shows_more_figures_than_the_details_block_holds() {
        let l10n = shipped_l10n();
        let mut most = 0;
        for region in ["eastern_interconnection", "western_interconnection"] {
            let game = shipped_game(region, Box::new(InMemoryCommandLog::default()));
            for plant in game.session().plant_views() {
                let count = selected(&game, plant.id).stats.len();
                assert!(count <= MOST_PLANT_STATS, "{region}: {:?}", plant.id);
                most = most.max(count);
            }
        }
        // And the most is reached, so the block has no room to spare.
        assert_eq!(most, MOST_PLANT_STATS);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_list_has_every_plant_by_kind_in_merit_order_and_selects_it() {
        let l10n = shipped_l10n();
        let game = operating();
        let session = game.session();
        let names = Names::new(&l10n, session);
        let coal = view(&game, PlantKind::Coal);

        let model = plant_list_model(&game, &l10n, Selection::Plant(coal.id));

        assert_eq!(model.title, "Plants");
        assert_eq!(model.rows.len(), session.plant_views().len(), "every plant");
        let order = kinds_in_merit_order(session.region());
        let kind_of = |id: PlantId| {
            session
                .plant_views()
                .into_iter()
                .find(|plant| plant.id == id)
                .unwrap()
                .kind
        };
        let ranks: Vec<(usize, u32)> = model
            .rows
            .iter()
            .map(|row| {
                let rank = order.iter().position(|k| *k == kind_of(row.plant)).unwrap();
                (rank, row.plant.0)
            })
            .collect();
        assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]), "{ranks:?}");

        let row = model.rows.iter().find(|row| row.plant == coal.id).unwrap();
        assert_eq!(row.select.button.label, names.plant(coal.id));
        assert_eq!(
            row.select.button.action,
            UiAction::Select(Selection::Plant(coal.id))
        );
        assert!(row.select.on, "the selected plant's row is on");
        assert_eq!(row.status, "Online");
        assert_eq!(row.output, format::megawatts(coal.output));
        assert_eq!(row.agc, None);
        let others_on = model
            .rows
            .iter()
            .filter(|row| row.plant != coal.id && row.select.on)
            .count();
        assert_eq!(others_on, 0);
        let nuclear = view(&game, PlantKind::Nuclear);
        let off = model
            .rows
            .iter()
            .find(|row| row.plant == nuclear.id)
            .unwrap();
        assert_eq!(off.status, "Off");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn agc_shows_in_the_list() {
        let l10n = shipped_l10n();
        let mut game = operating();
        let coal = view(&game, PlantKind::Coal);
        game.execute(Command::SetAutoFollow {
            plant: coal.id,
            on: true,
        })
        .unwrap();

        let model = plant_list_model(&game, &l10n, Selection::None);

        let row = model.rows.iter().find(|row| row.plant == coal.id).unwrap();
        assert_eq!(row.agc.as_deref(), Some("AGC"));
        assert!(model.rows.iter().all(|row| !row.select.on));
    }

    #[test]
    fn an_online_plant_under_manual_control_shows_its_figures_and_controls() {
        let l10n = shipped_l10n();
        let mut game = operating();
        // An hour into the day.
        game.execute(Command::AdvanceTicks { n: 55 }).unwrap();
        let session = game.session();
        let coal = view(&game, PlantKind::Coal);
        let spec = session.region().spec(PlantKind::Coal).clone();
        let PlantStatus::Online { target, .. } = coal.status else {
            panic!("committed all day: {:?}", coal.status);
        };

        let model = selected(&game, coal.id);

        assert_eq!(model.name, Names::new(&l10n, session).plant(coal.id));
        assert_eq!(model.status, "Online");
        assert_eq!(
            model.agc.button.action,
            UiAction::Command(Command::SetAutoFollow {
                plant: coal.id,
                on: true
            })
        );
        assert!(!model.agc.on && model.agc.button.enabled);
        assert_eq!(model.agc.button.label, "AGC");
        assert_eq!(
            model.agc_tip,
            "Automatic generation control: the plant follows the load by itself."
        );
        assert_eq!(
            model.start.action,
            UiAction::Command(Command::StartPlant { plant: coal.id })
        );
        assert!(!model.start.enabled, "it is running already");
        assert_eq!(
            model.stop.action,
            UiAction::Command(Command::StopPlant { plant: coal.id })
        );
        assert!(model.stop.enabled);
        assert_eq!(
            model.slider,
            TargetSlider {
                plant: coal.id,
                label: "Set target".to_owned(),
                min: spec.min_output().value(),
                max: spec.capacity.value(),
                value: target.value(),
                enabled: true,
            }
        );
        let stats: Vec<(&str, &str)> = model
            .stats
            .iter()
            .map(|stat| (stat.label.as_str(), stat.value.as_str()))
            .collect();
        // Today's energy is the played hour's, before any month has closed.
        let today = session.plant_energy_today()[&coal.id];
        assert!(today > MegawattHours::ZERO, "{today:?}");
        let history = game.projections().plant_history.get(coal.id);
        assert_eq!(history.map(|h| h.energy), None, "no month has closed yet");
        assert_eq!(
            stats,
            [
                ("Output", format::megawatts(coal.output).as_str()),
                ("Target", format::megawatts(target).as_str()),
                ("Ramp rate", format::ramp_rate(spec.ramp).as_str()),
                ("Marginal cost", format::price(spec.marginal_cost).as_str()),
                ("Energy today", format::megawatt_hours(today).as_str()),
                ("Closed months", "0.0 MWh, $0.00"),
            ]
        );
        assert_ne!(
            format::megawatt_hours(today),
            format::megawatt_hours(MegawattHours::ZERO)
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn closed_months_show_the_plants_history_and_today_starts_again() {
        let l10n = shipped_l10n();
        let mut game = operating();
        game.execute(Command::AdvanceTicks {
            n: domain::time::TICKS_PER_DAY - 5,
        })
        .unwrap();
        game.execute(Command::FinishReview).unwrap();
        let coal = view(&game, PlantKind::Coal);
        let history = *game
            .projections()
            .plant_history
            .get(coal.id)
            .expect("the month closed with the coal fleet running");
        assert!(history.energy > MegawattHours::ZERO);

        let model = selected(&game, coal.id);

        let figure = |label: &str| {
            model
                .stats
                .iter()
                .find(|stat| stat.label == label)
                .unwrap_or_else(|| panic!("no {label} in {:?}", model.stats))
                .value
                .clone()
        };
        assert_eq!(
            figure("Energy today"),
            format::megawatt_hours(MegawattHours::ZERO),
            "a new day has not begun"
        );
        assert_eq!(
            figure("Closed months"),
            l10n.args(
                "plant-closed-months-value",
                &[
                    ("energy", format::megawatt_hours(history.energy).into()),
                    ("revenue", format::money(history.revenue).into()),
                ],
            )
        );
        assert_eq!(
            figure("Closed months"),
            format!(
                "{}, {}",
                format::megawatt_hours(history.energy),
                format::money(history.revenue)
            )
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn controls_follow_the_plants_state() {
        let mut game = operating();
        let coal = view(&game, PlantKind::Coal);
        let nuclear = view(&game, PlantKind::Nuclear);

        // An offline plant can be started, not stopped, and takes no target.
        let off = selected(&game, nuclear.id);
        assert!(off.start.enabled && !off.stop.enabled);
        assert!(!off.slider.enabled);
        assert_eq!(off.slider.value, off.slider.min);
        assert!(off.stats.iter().any(|stat| stat.value == "none"));

        // Under AGC the target is the controller's, so the slider is off.
        game.execute(Command::SetAutoFollow {
            plant: coal.id,
            on: true,
        })
        .unwrap();
        let auto = selected(&game, coal.id);
        assert!(auto.agc.on);
        assert_eq!(
            auto.agc.button.action,
            UiAction::Command(Command::SetAutoFollow {
                plant: coal.id,
                on: false
            })
        );
        assert!(!auto.slider.enabled);

        // A stopping plant can't be stopped again.
        game.execute(Command::StopPlant { plant: coal.id }).unwrap();
        let stopping = selected(&game, coal.id);
        assert_eq!(stopping.status, "Stopping");
        assert!(!stopping.stop.enabled && !stopping.start.enabled);
    }

    #[test]
    fn a_hydro_plant_shows_its_water() {
        let l10n = shipped_l10n();
        let mut game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        game.execute(Command::BeginOperating).unwrap();
        let hydro = view(&game, PlantKind::Hydro);
        let water = hydro.hydro_budget.expect("a hydro plant has a budget");

        let model = selected(&game, hydro.id);

        let last = model.stats.last().unwrap();
        assert_eq!(last.label, "Water left today");
        assert_eq!(last.value, format::megawatt_hours(water));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn nothing_selected_keeps_the_panel_with_a_hint() {
        let game = operating();
        let session = game.session();
        for selection in [
            Selection::None,
            Selection::City(session.city_views()[0].id),
            Selection::Cell(domain::ids::CellCoord { x: 1, y: 1 }),
            Selection::Plant(PlantId(9_999)),
        ] {
            assert_eq!(
                plant_panel_model(&game, &shipped_l10n(), selection),
                PlantPanelModel::Empty {
                    hint: "Select a plant in the list or on the map.".to_owned()
                },
                "{selection:?}"
            );
        }
    }

    #[test]
    fn a_plant_nobody_dispatches_has_no_controls() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let region = session.region();
        let land = (0..region.terrain().height())
            .flat_map(|y| {
                (0..region.terrain().width()).map(move |x| domain::ids::CellCoord { x, y })
            })
            .find(|at| region.site_check(PlantKind::Peaker, *at).is_ok())
            .unwrap();
        game.execute(Command::BuildPlant {
            kind: PlantKind::Peaker,
            cell: land,
        })
        .unwrap();
        let building = game
            .session()
            .plant_views()
            .into_iter()
            .find(|plant| !is_commitable(plant))
            .unwrap();

        let model = selected(&game, building.id);

        assert!(
            model.status.starts_with("Under construction"),
            "{}",
            model.status
        );
        assert!(!model.agc.button.enabled);
        assert!(!model.start.enabled && !model.stop.enabled);
        assert!(!model.slider.enabled);
    }

    #[test]
    fn the_slider_command_is_clamped_to_the_plants_range() {
        let slider = TargetSlider {
            plant: PlantId(3),
            label: String::new(),
            min: 240.0,
            max: 600.0,
            value: 300.0,
            enabled: true,
        };
        let sent = |value| match slider.command(value) {
            UiAction::Command(Command::SetDispatchTarget { plant, mw }) => {
                assert_eq!(plant, PlantId(3));
                mw.value()
            }
            other => panic!("{other:?}"),
        };

        assert_eq!(sent(400.0), 400.0);
        assert_eq!(sent(100.0), 240.0);
        assert_eq!(sent(900.0), 600.0);
    }

    #[test]
    fn the_form_follows_the_target_unless_held_and_resyncs_on_a_new_plant() {
        let slider = |plant, value| TargetSlider {
            plant: PlantId(plant),
            label: String::new(),
            min: 0.0,
            max: 1_000.0,
            value,
            enabled: true,
        };
        let mut form = PlantPanelForm::default();

        form.sync(&slider(1, 300.0));
        assert_eq!((form.plant, form.value), (Some(PlantId(1)), 300.0));

        // Being dragged: the player's value stands.
        form.value = 420.0;
        form.held = true;
        form.sync(&slider(1, 300.0));
        assert_eq!(form.value, 420.0);

        // Let go: back to the target, which the command will have moved.
        form.held = false;
        form.sync(&slider(1, 420.0));
        assert_eq!(form.value, 420.0);

        // Another plant: its own target, even mid-drag.
        form.held = true;
        form.sync(&slider(2, 150.0));
        assert_eq!(
            form,
            PlantPanelForm {
                plant: Some(PlantId(2)),
                value: 150.0,
                held: false
            }
        );
    }
}
