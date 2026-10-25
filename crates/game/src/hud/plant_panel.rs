//! The Operating plant controls and the read-only Planning plant card (spec §8.4).
//!
//! The list shows every plant, its status, output and whether AGC is on; clicking a row selects
//! the plant, as clicking it on the map does. The panel above the list follows [`Selection`]: the
//! plant's output, target, ramp rate, marginal cost, its energy today, its energy and revenue over
//! the months closed so far, its water, and the controls (AGC, start, stop and a target slider).
//!
//! The slider edits [`PlantPanelForm`], not the game. Its value is sent as one
//! `SetDispatchTarget` when a drag ends or a keyboard step lands, never while it is being dragged,
//! and it is clamped to the plant's minimum stable output and capacity first.

use crate::l10n::format::MoneyStyle;
use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::egui;
use domain::generation::PlantStatus;
use domain::ids::PlantId;
use domain::region::{PlantSpec, RegionDefinition};
use domain::session::{DayPhase, PlantView};
use domain::time::{SimDuration, TICKS_PER_DAY};
use domain::units::{MegawattHours, Megawatts, Money};

use super::action::{ButtonModel, ToggleModel, UiAction, add_enabled, button, toggle_button};
use super::plant_list::{is_commitable, kinds_in_merit_order};
use super::selection::Selection;
use super::theme::{TextTone, ValueColor, heading_color, marginal_cost_fraction};
use super::top_bar::Stat;
use crate::l10n::{Localize, LocalizeWith, Localizer, Names, format};

/// A plant figure with its value-based color selected before formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantStat {
    pub label: String,
    pub value: String,
    pub color: ValueColor,
}

impl PlantStat {
    pub(super) fn new(l10n: &Localizer, key: &str, value: String) -> Self {
        Self {
            label: l10n.text(key),
            value,
            color: ValueColor::Tone(TextTone::Information),
        }
    }

    pub(super) fn with_color(mut self, color: ValueColor) -> Self {
        self.color = color;
        self
    }
}

impl From<Stat> for PlantStat {
    fn from(stat: Stat) -> Self {
        Self {
            label: stat.label,
            value: stat.value,
            color: ValueColor::Tone(TextTone::Information),
        }
    }
}

struct PlantValueColors {
    cost: ValueColor,
    ramp: ValueColor,
    startup: ValueColor,
    startup_cost: ValueColor,
}

fn plant_value_colors(region: &RegionDefinition, spec: &PlantSpec) -> PlantValueColors {
    let same = |a: f64, b: f64| {
        a == b
            || (a.is_finite()
                && b.is_finite()
                && (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0))
    };
    let rank = |value: f64, metric: fn(&PlantSpec) -> f64| {
        let mut values: Vec<_> = region
            .catalog()
            .iter()
            .filter(|candidate| candidate.kind.is_dispatchable())
            .map(metric)
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| same(*a, *b));
        let below = values
            .iter()
            .filter(|&&candidate| candidate < value && !same(candidate, value))
            .count();
        ValueColor::Scale(
            (below as f32 / values.len().saturating_sub(1).max(1) as f32).clamp(0.0, 1.0),
        )
    };
    // Capacity / ramp measures how long a plant needs to change its own full output.
    // It avoids treating a large plant as responsive merely because its MW/min is large.
    let response = |spec: &PlantSpec| {
        if spec.ramp.value() > 0.0 {
            spec.capacity.value() / spec.ramp.value()
        } else {
            f64::INFINITY
        }
    };
    let startup_cost_ceiling = region
        .catalog()
        .iter()
        .map(|candidate| candidate.startup_cost.cents())
        .max()
        .unwrap_or(0);
    PlantValueColors {
        cost: ValueColor::Scale(marginal_cost_fraction(region, spec.marginal_cost)),
        ramp: if spec.kind.is_dispatchable() {
            rank(response(spec), response)
        } else {
            ValueColor::Tone(TextTone::Neutral)
        },
        startup: if spec.kind.is_dispatchable() {
            rank(f64::from(spec.startup.ticks()), |spec| {
                f64::from(spec.startup.ticks())
            })
        } else {
            ValueColor::Tone(TextTone::Neutral)
        },
        startup_cost: ValueColor::Scale(if startup_cost_ceiling > 0 {
            (spec.startup_cost.cents() as f64 / startup_cost_ceiling as f64).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }),
    }
}

/// One plant in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantListRow {
    pub plant: PlantId,
    /// The plant's name; selects it. On while it is the selected plant.
    pub select: ToggleModel,
    /// `Online`, `Starting: 40 min left`, ...
    pub status: String,
    pub status_tone: TextTone,
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
    pub status_tone: TextTone,
    pub connected: bool,
    pub agc: ToggleModel,
    pub agc_tip: String,
    pub variable_generation: bool,
    pub storage_toggle: Option<ToggleModel>,
    pub storage_hint: Option<String>,
    pub start: ButtonModel,
    pub start_hint: Option<String>,
    pub stop: ButtonModel,
    pub slider: TargetSlider,
    /// Grid connection, output/capacity, target, ramp, cost, energy and history, plus hydro water.
    pub stats: Vec<PlantStat>,
    pub specifications_heading: String,
    pub specifications: Vec<PlantStat>,
}

/// Read-only Planning details for a commissioned plant.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantCardModel {
    pub storage_toggle: Option<ToggleModel>,
    pub storage_hint: Option<String>,
    pub plant: PlantId,
    pub name: String,
    pub status: String,
    pub status_tone: TextTone,
    pub connected: bool,
    pub stats: Vec<PlantStat>,
}

fn plant_status_tone(plant: &PlantView) -> TextTone {
    use domain::generation::BatteryMode;
    if let Some(battery) = &plant.battery {
        return match battery.mode {
            BatteryMode::GridSuspended => TextTone::Negative,
            BatteryMode::Disconnected => TextTone::Caution,
            BatteryMode::Charging | BatteryMode::Discharging | BatteryMode::Full => {
                TextTone::Positive
            }
            BatteryMode::Disabled | BatteryMode::Holding | BatteryMode::Empty => TextTone::Neutral,
        };
    }
    if !plant.connected {
        return TextTone::Caution;
    }
    match plant.status {
        PlantStatus::UnderConstruction { .. } => TextTone::Information,
        PlantStatus::Online { .. } => TextTone::Positive,
        PlantStatus::Starting { .. } | PlantStatus::Stopping { .. } => TextTone::Caution,
        PlantStatus::Offline => TextTone::Neutral,
    }
}

fn connection_tone(connected: bool) -> TextTone {
    if connected {
        TextTone::Positive
    } else {
        TextTone::Caution
    }
}

pub(super) fn battery_hint(
    l10n: &Localizer,
    key: &str,
    spec: domain::region::BatterySpec,
) -> String {
    l10n.args(
        key,
        &[
            (
                "charge_end",
                format::clock(u32::from(spec.charge_end_hour) * 60).into(),
            ),
            (
                "discharge_start",
                format::clock(u32::from(spec.discharge_start_hour) * 60).into(),
            ),
        ],
    )
}

pub(super) fn grid_connection_stat(l10n: &Localizer, connected: bool) -> Stat {
    Stat {
        label: l10n.text("plant-grid-connection"),
        value: l10n.text(if connected {
            "plant-grid-connected"
        } else {
            "plant-grid-disconnected"
        }),
    }
}

fn wind_stats(l10n: &Localizer, wind: &domain::session::WindView) -> Vec<PlantStat> {
    let stat = |key: &str, value| PlantStat::new(l10n, key, value);
    vec![
        stat("wind-potential", format::percent(wind.potential)),
        stat("wind-available", format::megawatts(wind.available)),
        stat("wind-delivered", format::megawatts(wind.delivered)),
        stat("wind-curtailed", format::megawatts(wind.curtailed)),
        stat(
            "wind-range",
            l10n.args(
                "wind-range-value",
                &[
                    ("central", format::megawatts(wind.forecast.0).into()),
                    ("low", format::megawatts(wind.forecast.1).into()),
                    ("high", format::megawatts(wind.forecast.2).into()),
                ],
            ),
        ),
    ]
}

pub fn plant_card_model(
    game: &Game,
    l10n: &Localizer,
    id: PlantId,
    style: &MoneyStyle,
) -> Option<PlantCardModel> {
    let session = game.session();
    let plant = session.plant_views().into_iter().find(|p| p.id == id)?;
    if matches!(plant.status, PlantStatus::UnderConstruction { .. }) {
        return None;
    }
    let spec = session.region().spec(plant.kind);
    let colors = plant_value_colors(session.region(), spec);
    let stat = |key: &str, value| PlantStat::new(l10n, key, value);
    let mut stats = vec![
        PlantStat::from(grid_connection_stat(l10n, plant.connected))
            .with_color(ValueColor::Tone(connection_tone(plant.connected))),
        stat("build-capacity", format::megawatts(spec.capacity)),
    ];
    if plant.kind.is_dispatchable() {
        stats.extend([
            stat("plant-min-output", format::megawatts(spec.min_output())),
            stat(
                "plant-initial-target",
                format::megawatts(plant.initial_target),
            ),
            stat("plant-ramp", format::ramp_rate(spec.ramp)).with_color(colors.ramp),
            stat("plant-startup-time", format::duration(spec.startup)).with_color(colors.startup),
            stat(
                "plant-startup-cost",
                format::money(spec.startup_cost, style),
            )
            .with_color(colors.startup_cost),
        ]);
    }
    stats.extend([
        stat(
            "plant-marginal-cost",
            format::price(spec.marginal_cost, style),
        )
        .with_color(colors.cost),
        stat(
            "plant-fixed-om",
            format::money(spec.fixed_om_per_month, style),
        ),
    ]);
    if let Some(water) = plant.hydro_day_budget {
        stats.push(stat("plant-hydro-available", format::megawatt_hours(water)));
    }
    if plant.kind == domain::region::PlantKind::Solar {
        stats.push(stat(
            "map-solar-scale",
            format::percent(session.region().solar_potential(plant.cell)),
        ));
    }
    if let Some(wind) = &plant.wind {
        stats.extend(wind_stats(l10n, wind));
    }
    if let Some(b) = &plant.battery {
        stats = battery_stats(l10n, b, plant.connected, spec.capacity);
        stats.push(stat(
            "plant-fixed-om",
            format::money(spec.fixed_om_per_month, style),
        ));
    }
    Some(PlantCardModel {
        storage_toggle: plant
            .battery
            .as_ref()
            .map(|b| battery_toggle(l10n, id, b.enabled)),
        storage_hint: plant
            .battery
            .as_ref()
            .map(|b| battery_hint(l10n, "battery-grid-charging-hint", b.spec)),
        plant: id,
        name: Names::new(l10n, session).plant(id),
        status: plant
            .battery
            .as_ref()
            .map_or_else(|| plant.status.localize(l10n), |b| b.mode.localize(l10n)),
        status_tone: plant_status_tone(&plant),
        connected: plant.connected,
        stats,
    })
}

pub fn draw_plant_card(ui: &mut egui::Ui, model: &PlantCardModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.heading(egui::RichText::new(&model.name).color(heading_color()));
    ui.colored_label(model.status_tone.color(), &model.status);
    if let Some(toggle) = &model.storage_toggle {
        toggle_button(ui, toggle, &mut actions);
    }
    if let Some(hint) = &model.storage_hint {
        ui.colored_label(TextTone::Neutral.color(), hint);
    }
    egui::Grid::new("plant_card_stats")
        .num_columns(2)
        .spacing(egui::vec2(8.0, 1.0))
        .min_row_height(0.0)
        .show(ui, |ui| {
            for stat in &model.stats {
                ui.colored_label(TextTone::Neutral.color(), &stat.label);
                ui.colored_label(stat.color.color(), &stat.value);
                ui.end_row();
            }
        });
    actions
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

/// What the slider holds between frames. A `Resource`, like the new-game setup's seed field.
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
                status: plant
                    .battery
                    .as_ref()
                    .map_or_else(|| plant.status.localize(l10n), |b| b.mode.localize(l10n)),
                status_tone: plant_status_tone(&plant),
                output: format::megawatts(plant.output),
                agc: plant.auto_follow.then(|| agc.clone()),
            })
            .collect(),
    }
}

pub fn plant_panel_model(
    game: &Game,
    l10n: &Localizer,
    selection: Selection,
    style: &MoneyStyle,
) -> PlantPanelModel {
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
    let too_late = session.phase() == DayPhase::Operating
        && spec.startup.ticks() > TICKS_PER_DAY.saturating_sub(session.clock().tick());
    let target = match plant.status {
        PlantStatus::Online { target, .. } => Some(target),
        _ => None,
    };
    let (min, capacity) = (spec.min_output(), spec.capacity);
    let history = game.projections().plant_history.get(plant.id);
    let colors = plant_value_colors(session.region(), spec);
    let stat = |key: &str, value: String| PlantStat::new(l10n, key, value);
    let mut stats = vec![
        PlantStat::from(grid_connection_stat(l10n, plant.connected))
            .with_color(ValueColor::Tone(connection_tone(plant.connected))),
        stat(
            "plant-output-capacity",
            l10n.args(
                "plant-output-capacity-value",
                &[
                    ("output", format::megawatts(plant.output).into()),
                    ("capacity", format::megawatts(spec.capacity).into()),
                ],
            ),
        ),
        stat(
            "plant-target",
            target.map_or_else(|| l10n.text("plant-target-none"), format::megawatts),
        ),
        stat("plant-ramp", format::ramp_rate(spec.ramp)).with_color(colors.ramp),
        stat(
            "plant-marginal-cost",
            format::price(spec.marginal_cost, style),
        )
        .with_color(colors.cost),
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
                        format::money(history.map_or(Money::ZERO, |h| h.revenue), style).into(),
                    ),
                ],
            ),
        ),
    ];
    if let Some(water) = plant.hydro_budget {
        stats.push(stat("plant-hydro-budget", format::megawatt_hours(water)));
    }
    if let Some(b) = &plant.battery {
        stats = battery_stats(l10n, b, plant.connected, spec.capacity);
    }
    if let Some(wind) = &plant.wind {
        stats
            .retain(|s| s.label != l10n.text("plant-target") && s.label != l10n.text("plant-ramp"));
        stats.extend(wind_stats(l10n, wind));
    }
    PlantPanelModel::Plant(Box::new(SelectedPlant {
        storage_toggle: plant
            .battery
            .as_ref()
            .map(|b| battery_toggle(l10n, plant.id, b.enabled)),
        storage_hint: plant
            .battery
            .as_ref()
            .map(|b| battery_hint(l10n, "battery-grid-charging-hint", b.spec)),
        plant: plant.id,
        name: Names::new(l10n, session).plant(plant.id),
        status: plant
            .battery
            .as_ref()
            .map_or_else(|| plant.status.localize(l10n), |b| b.mode.localize(l10n)),
        status_tone: plant_status_tone(&plant),
        connected: plant.connected,
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
        variable_generation: plant.kind.is_variable(),
        start: ButtonModel::new(
            l10n.text("plant-start"),
            UiAction::Command(Command::StartPlant { plant: plant.id }),
        )
        .enabled_if(operable && plant.status == PlantStatus::Offline && !too_late),
        start_hint: (operable && plant.status == PlantStatus::Offline && too_late).then(|| {
            domain::error::DomainError::StartUpBeyondDay {
                remaining: SimDuration::from_ticks(
                    TICKS_PER_DAY.saturating_sub(session.clock().tick()),
                ),
                startup: spec.startup,
            }
            .localize_with(l10n, &Names::new(l10n, session))
        }),
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
        specifications_heading: l10n.text("plant-specifications"),
        specifications: plant_card_model(game, l10n, plant.id, style)
            .map_or_else(Vec::new, |card| card.stats),
    }))
}

/// The most primary figures a selected plant shows: seven, and a hydro plant's water left.
/// Additional specifications expand inside the same scrolling block.
pub const MOST_PLANT_STATS: usize = 8;
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
const STATS_ROW_SPACING: f32 = 0.0;

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
    ui.label(
        egui::RichText::new(&model.title)
            .strong()
            .color(heading_color()),
    );
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
                        ui.colored_label(row.status_tone.color(), &row.status);
                        ui.colored_label(TextTone::Information.color(), &row.output);
                        ui.colored_label(
                            TextTone::Positive.color(),
                            row.agc.as_deref().unwrap_or(""),
                        );
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
        ui.label(
            egui::RichText::new(&plant.name)
                .strong()
                .color(heading_color()),
        );
        ui.colored_label(plant.status_tone.color(), &plant.status);
    });
    if let Some(toggle) = &plant.storage_toggle {
        toggle_button(ui, toggle, &mut actions);
        if let Some(hint) = &plant.storage_hint {
            ui.colored_label(TextTone::Neutral.color(), hint);
        }
    } else if !plant.variable_generation {
        ui.horizontal(|ui| {
            toggle_button(ui, &plant.agc, &mut actions).on_hover_text(plant.agc_tip.as_str());
            let start = button(ui, &plant.start, &mut actions);
            if let Some(hint) = &plant.start_hint {
                start.on_disabled_hover_text(hint);
            }
            button(ui, &plant.stop, &mut actions);
        });
        let slider = &plant.slider;
        form.sync(slider);
        let response = add_enabled(
            ui,
            slider.enabled,
            egui::Slider::new(&mut form.value, slider.min..=slider.max)
                .text(slider.label.as_str())
                .show_value(false),
        );
        // A drag sends once, as it ends; a keyboard step changes the value without a drag.
        if slider.enabled
            && (response.drag_stopped() || (response.changed() && !response.dragged()))
        {
            actions.push(slider.command(form.value));
        }
        form.held = response.dragged();
    }
    egui::ScrollArea::vertical()
        .id_salt(("operating_plant_stats", plant.plant.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Compact rows, so every figure is in view at the smallest window.
            egui::Grid::new("operating_plant_stats_grid")
                .num_columns(2)
                .spacing(egui::vec2(8.0, STATS_ROW_SPACING))
                .min_row_height(0.0)
                .show(ui, |ui| {
                    for stat in &plant.stats {
                        ui.colored_label(TextTone::Neutral.color(), &stat.label);
                        ui.colored_label(stat.color.color(), &stat.value);
                        ui.end_row();
                    }
                });
            ui.collapsing(
                egui::RichText::new(&plant.specifications_heading).color(heading_color()),
                |ui| {
                    egui::Grid::new(("operating_plant_specifications", plant.plant.0))
                        .num_columns(2)
                        .spacing(egui::vec2(8.0, 1.0))
                        .min_row_height(0.0)
                        .show(ui, |ui| {
                            for stat in &plant.specifications {
                                ui.colored_label(TextTone::Neutral.color(), &stat.label);
                                ui.colored_label(stat.color.color(), &stat.value);
                                ui.end_row();
                            }
                        });
                },
            );
        });
    actions
}

fn battery_toggle(l10n: &Localizer, id: PlantId, enabled: bool) -> ToggleModel {
    ToggleModel {
        button: ButtonModel::new(
            l10n.text(if enabled {
                "battery-disable"
            } else {
                "battery-enable"
            }),
            UiAction::Command(Command::SetBatteryEnabled {
                plant: id,
                on: !enabled,
            }),
        ),
        on: enabled,
    }
}
fn battery_stats(
    l10n: &Localizer,
    b: &domain::session::BatteryView,
    connected: bool,
    discharge: Megawatts,
) -> Vec<PlantStat> {
    let stat = |key: &str, value: String| PlantStat::new(l10n, key, value);
    vec![
        PlantStat::from(grid_connection_stat(l10n, connected))
            .with_color(ValueColor::Tone(connection_tone(connected))),
        stat(
            "battery-stored",
            format!(
                "{} / {}",
                format::megawatt_hours(b.stored),
                format::megawatt_hours(b.spec.energy_capacity)
            ),
        ),
        stat("battery-charging", format::megawatts(b.flow.charge)),
        stat("battery-discharging", format::megawatts(b.flow.discharge)),
        stat("battery-input", format::megawatt_hours(b.today.input)),
        stat("battery-delivered", format::megawatt_hours(b.today.output)),
        stat(
            "battery-losses",
            format::megawatt_hours(b.today.charge_loss + b.today.discharge_loss),
        ),
        stat(
            "battery-charge-rating",
            format::megawatts(b.spec.charge_power),
        ),
        stat("battery-discharge-rating", format::megawatts(discharge)),
        stat(
            "battery-energy-rating",
            format::megawatt_hours(b.spec.energy_capacity),
        ),
        stat(
            "battery-round-trip",
            format::percent(
                domain::units::Ratio::new(
                    b.spec.charge_efficiency.value() * b.spec.discharge_efficiency.value(),
                )
                .expect("validated efficiencies"),
            ),
        ),
        stat(
            "battery-charge-window",
            l10n.args(
                "battery-window-value",
                &[
                    (
                        "start",
                        format::clock((b.charge_window.0 * 60.0).ceil() as u32).into(),
                    ),
                    (
                        "end",
                        format::clock((b.charge_window.1 * 60.0).ceil() as u32).into(),
                    ),
                ],
            ),
        ),
        stat(
            "battery-evening-window",
            l10n.args(
                "battery-window-value",
                &[
                    (
                        "start",
                        format::clock(u32::from(b.spec.discharge_start_hour) * 60).into(),
                    ),
                    ("end", format::clock(1440).into()),
                ],
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use application::InMemoryCommandLog;
    use domain::region::PlantKind;

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_game, shipped_l10n};

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(32))]
        #[test]
        fn plant_responsiveness_colors_ignore_capacity_scale_and_penalize_slower_response(
            scale in 1.0f64..100.0,
            delay in 1u32..8,
        ) {
            let game = eastern_game(Box::new(InMemoryCommandLog::default()));
            let region = game.session().region();
            let mut spec = region.spec(PlantKind::Ccgt).clone();
            let original = plant_value_colors(region, &spec);
            spec.capacity = spec.capacity * scale;
            spec.ramp = domain::units::RampRate::mw_per_minute(spec.ramp.value() * scale);
            proptest::prop_assert_eq!(plant_value_colors(region, &spec).ramp, original.ramp);
            spec.ramp = domain::units::RampRate::mw_per_minute(spec.ramp.value() / f64::from(delay));
            spec.startup = SimDuration::from_ticks(spec.startup.ticks() * delay);
            spec.startup_cost = Money::from_cents(spec.startup_cost.cents() * i64::from(delay));
            let slower = plant_value_colors(region, &spec);
            for (before, after) in [(original.ramp, slower.ramp), (original.startup, slower.startup), (original.startup_cost, slower.startup_cost)] {
                let (ValueColor::Scale(before), ValueColor::Scale(after)) = (before, after) else {
                    panic!("dispatchable responsiveness needs a comparable scale");
                };
                proptest::prop_assert!(after >= before && after <= 1.0);
            }
            spec.ramp = domain::units::RampRate::mw_per_minute(0.0);
            proptest::prop_assert_eq!(plant_value_colors(region, &spec).ramp, ValueColor::Scale(1.0));
        }
    }

    #[test]
    fn battery_hints_show_current_and_archived_discharge_windows() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut spec = game
            .session()
            .region()
            .spec(PlantKind::Battery)
            .battery
            .unwrap();
        assert_eq!(spec.discharge_start_hour, 16);
        for key in ["battery-grid-charging-hint", "battery-cycle-hint"] {
            assert!(battery_hint(&l10n, key, spec).contains("16:00–24:00"));
            let archived_start = spec.discharge_start_hour;
            spec.discharge_start_hour = 17;
            assert!(battery_hint(&l10n, key, spec).contains("17:00–24:00"));
            spec.discharge_start_hour = archived_start;
        }
        assert!(l10n.problems().is_empty());
    }

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
        match plant_panel_model(game, &shipped_l10n(), Selection::Plant(plant), &usd_style()) {
            PlantPanelModel::Plant(model) => *model,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn operating_start_requires_enough_time_and_accepts_an_exact_midnight_finish() {
        use domain::time::TICKS_PER_DAY;
        for kind in [
            PlantKind::Nuclear,
            PlantKind::Coal,
            PlantKind::Ccgt,
            PlantKind::Peaker,
        ] {
            for one_tick_late in [false, true] {
                let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
                let plant = view(&game, kind).id;
                game.execute(Command::StopPlant { plant }).unwrap();
                let startup = game.session().region().spec(kind).startup.ticks();
                game.execute(Command::BeginOperating).unwrap();
                let tick = TICKS_PER_DAY.saturating_sub(startup) + u32::from(one_tick_late);
                game.execute(Command::AdvanceTicks { n: tick }).unwrap();
                assert_eq!(view(&game, kind).status, PlantStatus::Offline);
                let expected = startup <= TICKS_PER_DAY - tick;
                let model = selected(&game, plant);
                let before = game.checkpoint();
                let result = game.execute(Command::StartPlant { plant });
                assert_eq!(result.is_ok(), expected, "{kind:?} at {tick}");
                if !expected {
                    assert_eq!(game.checkpoint(), before);
                }
                assert_eq!(model.start.enabled, expected, "{kind:?} at {tick}");
                if expected {
                    let events = game.execute(Command::AdvanceTicks { n: startup }).unwrap();
                    assert!(events.iter().any(|e| matches!(e, domain::events::DomainEvent::PlantOnline { plant: id } if *id == plant)));
                }
            }
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(32))]
        #[test]
        fn start_availability_matches_accepted_commands_at_arbitrary_operating_times(
            kind_index in 0usize..4, tick in 0u32..TICKS_PER_DAY
        ) {
            let kind = [PlantKind::Nuclear, PlantKind::Coal, PlantKind::Ccgt, PlantKind::Peaker][kind_index];
            let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
            let plant = view(&game, kind).id;
            game.execute(Command::StopPlant { plant }).unwrap();
            game.execute(Command::BeginOperating).unwrap();
            game.execute(Command::AdvanceTicks { n: tick }).unwrap();
            proptest::prop_assert_eq!(view(&game, kind).status, PlantStatus::Offline);
            let model = selected(&game, plant);
            let before = game.checkpoint();
            let result = game.execute(Command::StartPlant { plant });
            proptest::prop_assert_eq!(model.start.enabled, result.is_ok());
            if result.is_err() { proptest::prop_assert_eq!(game.checkpoint(), before); }
        }
    }

    /// A disconnected plant stays disconnected after commissioning, until a line enters service.
    #[test]
    fn plant_cards_follow_commissioning_and_grid_connection_independently_of_running_status() {
        use domain::{ids::CellCoord, region::PlantKind, time::TICKS_PER_DAY};
        let l10n = shipped_l10n();
        for region in ["eastern_interconnection", "western_interconnection"] {
            let mut game = shipped_game(region, Box::new(InMemoryCommandLog::default()));
            let session = game.session();
            let anchor = session.anchor_node();
            let cities = session.city_views();
            let home = cities.iter().find(|c| c.node == anchor).unwrap().cell;
            let taken: Vec<_> = session
                .plant_views()
                .iter()
                .map(|p| p.cell)
                .chain(cities.iter().map(|c| c.cell))
                .collect();
            let terrain = session.region().terrain();
            let cell = (0..terrain.height())
                .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
                .filter(|at| {
                    !taken.contains(at)
                        && session.region().site_check(PlantKind::Peaker, *at).is_ok()
                })
                .min_by_key(|at| at.x.abs_diff(home.x).pow(2) + at.y.abs_diff(home.y).pow(2))
                .unwrap();
            game.execute(Command::BuildPlant {
                kind: PlantKind::Peaker,
                cell,
            })
            .unwrap();
            let plant = game
                .session()
                .plant_views()
                .into_iter()
                .find(|p| p.cell == cell)
                .unwrap();
            let build_months = game.session().region().spec(plant.kind).build_months;
            let check = |game: &Game, connected| {
                let view = game
                    .session()
                    .plant_views()
                    .into_iter()
                    .find(|p| p.id == plant.id)
                    .unwrap();
                assert_eq!(view.connected, connected);
                let expected = if connected {
                    "Connected"
                } else {
                    "Not connected"
                };
                let operating = selected(game, plant.id);
                assert_eq!(
                    operating
                        .stats
                        .iter()
                        .find(|s| s.label == "Grid connection")
                        .unwrap()
                        .value,
                    expected
                );
                let stats = super::super::construction::construction_card_model(
                    game,
                    &l10n,
                    plant.id,
                    &usd_style(),
                )
                .map(|card| card.stats.into_iter().map(PlantStat::from).collect())
                .unwrap_or_else(|| {
                    plant_card_model(game, &l10n, plant.id, &usd_style())
                        .unwrap()
                        .stats
                });
                assert_eq!(
                    stats
                        .iter()
                        .find(|s| s.label == "Grid connection")
                        .unwrap()
                        .value,
                    expected
                );
            };
            for _ in 0..build_months {
                check(&game, false);
                game.execute(Command::BeginOperating).unwrap();
                check(&game, false);
                game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
                    .unwrap();
                game.execute(Command::FinishReview).unwrap();
            }
            check(&game, false);
            assert_eq!(
                game.session()
                    .plant_views()
                    .into_iter()
                    .find(|p| p.id == plant.id)
                    .unwrap()
                    .status,
                PlantStatus::Offline
            );
            let quote = game.session().line_quote(anchor, plant.node).unwrap();
            game.execute(Command::BuildLine {
                from: anchor,
                to: plant.node,
            })
            .unwrap();
            // Ordering a line is not the same as entering service.
            check(&game, false);
            for _ in 0..quote.months {
                game.execute(Command::BeginOperating).unwrap();
                check(&game, false);
                game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
                    .unwrap();
                game.execute(Command::FinishReview).unwrap();
            }
            check(&game, true);
            // An idle plant can be connected; connection is not its operating status.
            assert_eq!(
                game.session()
                    .plant_views()
                    .into_iter()
                    .find(|p| p.id == plant.id)
                    .unwrap()
                    .status,
                PlantStatus::Offline
            );
            game.execute(Command::BeginOperating).unwrap();
            check(&game, true);
        }
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn planning_cards_show_specs_and_resources_from_the_loaded_region() {
        let l10n = shipped_l10n();
        let mut game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        let solar = crate::hud::test_support::order_plant(&mut game, PlantKind::Solar, 0);
        for _ in 0..game.session().region().spec(PlantKind::Solar).build_months {
            game.execute(Command::BeginOperating).unwrap();
            game.execute(Command::AdvanceTicks {
                n: domain::time::TICKS_PER_DAY,
            })
            .unwrap();
            game.execute(Command::FinishReview).unwrap();
        }
        for view in game.session().plant_views() {
            let spec = game.session().region().spec(view.kind);
            let card = plant_card_model(&game, &l10n, view.id, &usd_style()).unwrap();
            assert_eq!(selected(&game, view.id).specifications, card.stats);
            let stat = |label| &card.stats.iter().find(|s| s.label == label).unwrap().value;
            assert_eq!(stat("Capacity"), &format::megawatts(spec.capacity));
            assert_eq!(
                stat("Fixed O&M / month"),
                &format::money(spec.fixed_om_per_month, &usd_style())
            );
            assert_eq!(
                stat("Marginal cost"),
                &format::price(spec.marginal_cost, &usd_style())
            );
            if view.kind.is_dispatchable() {
                assert_eq!(stat("Start-up time"), &format::duration(spec.startup));
                assert_eq!(
                    stat("Start-up cost"),
                    &format::money(spec.startup_cost, &usd_style())
                );
                assert_eq!(
                    stat("Minimum output"),
                    &format::megawatts(spec.min_output())
                );
                assert_eq!(
                    stat("Initial target"),
                    &format::megawatts(view.initial_target)
                );
                assert_eq!(stat("Ramp rate"), &format::ramp_rate(spec.ramp));
            } else {
                assert_eq!(view.id, solar);
                assert!(!card.stats.iter().any(|s| s.label == "Start-up time"));
                let site = game.session().region().terrain().get(view.cell).unwrap();
                assert_eq!(stat("Solar potential"), &format::percent(site.irradiance));
            }
            if let Some(water) = view.hydro_day_budget {
                assert_eq!(
                    stat("Water available today"),
                    &format::megawatt_hours(water)
                );
            }
        }
        assert_eq!(
            plant_card_model(&game, &l10n, PlantId(u32::MAX), &usd_style()),
            None
        );
        assert!(l10n.problems().is_empty());
    }

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
        assert_eq!(model.agc_tip, l10n.text("commitment-agc-tip"));
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
                ("Grid connection", "Connected"),
                (
                    "Output / capacity",
                    format!(
                        "{} / {}",
                        format::megawatts(coal.output),
                        format::megawatts(spec.capacity)
                    )
                    .as_str()
                ),
                ("Target", format::megawatts(target).as_str()),
                ("Ramp rate", format::ramp_rate(spec.ramp).as_str()),
                (
                    "Marginal cost",
                    format::price(spec.marginal_cost, &usd_style()).as_str()
                ),
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
                    (
                        "revenue",
                        format::money(history.revenue, &usd_style()).into()
                    ),
                ],
            )
        );
        assert_eq!(
            figure("Closed months"),
            format!(
                "{}, {}",
                format::megawatt_hours(history.energy),
                format::money(history.revenue, &usd_style())
            )
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn controls_follow_the_plants_state() {
        let mut game = operating();
        let coal = view(&game, PlantKind::Coal);
        let nuclear = view(&game, PlantKind::Nuclear);

        // Offline does not make a 48-hour nuclear start fit in the remaining day.
        let off = selected(&game, nuclear.id);
        assert!(!off.start.enabled && !off.stop.enabled);
        assert!(off.start_hint.is_some());
        assert!(
            selected(&game, view(&game, PlantKind::Peaker).id)
                .start
                .enabled
        );
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
                plant_panel_model(&game, &shipped_l10n(), selection, &usd_style()),
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
