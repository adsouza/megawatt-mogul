//! The supply chart (spec §8.4): the day so far, minute by minute, as supply stacked by kind in
//! merit order (cheapest at the bottom) against actual demand and the load served, with the day's
//! forecast and the system marginal cost (the dearest plant producing, Task 2's timeline field)
//! on a secondary axis.

use super::SupplyChartLayout;
use crate::l10n::format::MoneyStyle;
use application::Game;
use application::projections::{CompletedDay, TimelinePoint};
use bevy_egui::egui;
use domain::demand::ForecastPoint;
use domain::region::{PlantKind, RegionDefinition};
use domain::time::{TICKS_PER_DAY, TICKS_PER_HOUR};
use egui_plot::{FilledArea, HPlacement, HoverPosition, Line, LineStyle, Plot, PlotMemory, VLine};

use super::action::UiAction;
use super::frequency::NORMAL_COLOR;
use super::map::{Rgb, color32, kind_color};
use super::plant_list::kinds_in_merit_order;
use crate::l10n::{Localize, Localizer};

/// The served line: the load actually supplied. It takes the gauge's in-band green, which stays
/// clear of every stack colour (the shedding red was nearly the peaker's red). Where it falls
/// under demand, load is being cut.
pub const SERVED_COLOR: Rgb = NORMAL_COLOR;
/// The system marginal cost line, on the secondary axis.
pub const MARGINAL_COST_COLOR: Rgb = [240, 190, 60];
/// Inventory is distinct from battery power, which is teal.
pub const STORED_ENERGY_COLOR: Rgb = [200, 180, 255];

/// One kind's band of the stack: from `low` (the kinds below it) to `high`, at each point.
#[derive(Debug, Clone, PartialEq)]
pub struct SupplyStack {
    pub kind: PlantKind,
    pub name: String,
    pub color: Rgb,
    pub low: Vec<f64>,
    pub high: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SupplyModel {
    pub transfers: Vec<(String, Vec<f64>)>,
    pub money_style: MoneyStyle,
    pub layout: SupplyChartLayout,
    pub layout_label: String,
    pub hour_axis: String,
    pub power_axis: String,
    pub cost_axis: String,
    pub power_unit: String,
    pub cost_unit: String,
    pub energy_unit: String,
    pub demand_label: String,
    pub served_label: String,
    pub forecast_label: String,
    pub cost_label: String,
    pub storage_load_label: String,
    pub storage_energy_label: String,
    pub storage_energy_legend: String,
    pub storage_unused_label: String,
    pub storage_load: Vec<f64>,
    pub stored: Vec<f64>,
    pub has_storage: bool,
    /// The day's ticks so far, in hours: `0.0, 0.0167, ...`. The supply series are sampled here.
    pub x: Vec<f64>,
    /// The kinds that produced today, in merit order.
    pub stacks: Vec<SupplyStack>,
    /// Actual demand, in megawatts.
    pub demand: Vec<f64>,
    /// The load served: demand less the rotating outage and protection's shed.
    pub served: Vec<f64>,
    /// The forecast at the middle of each hour, `(hour + 0.5, MW)`.
    pub forecast: Vec<[f64; 2]>,
    /// `(hour, $/MWh)` for each tick some plant produced in.
    pub marginal_cost: Vec<[f64; 2]>,
    /// The time of day now, in hours.
    pub now: f64,
}

impl SupplyModel {
    pub fn with_layout(mut self, layout: SupplyChartLayout) -> Self {
        self.layout = layout;
        self
    }
}

/// The timeline points of the day being played: the projection keeps the last 1,440 ticks, so
/// before the day ends it still holds the end of the last one. Today's are the run of ticks
/// before `now` that ends the timeline, oldest first.
pub fn todays_points<'a>(
    points: impl Iterator<Item = &'a TimelinePoint>,
    now: u32,
) -> Vec<&'a TimelinePoint> {
    let all: Vec<&TimelinePoint> = points.collect();
    let mut today = Vec::new();
    let mut later = now;
    for point in all.into_iter().rev() {
        if point.tick >= later {
            break;
        }
        later = point.tick;
        today.push(point);
    }
    today.reverse();
    today
}

pub fn supply_model(game: &Game, l10n: &Localizer, style: &MoneyStyle) -> SupplyModel {
    let session = game.session();
    let now = session.clock().tick();
    let points = todays_points(game.projections().timeline.points(), now);
    supply_model_for(
        points,
        &session.hourly_forecast(),
        now,
        session.region(),
        l10n,
        style,
    )
}

pub fn completed_supply_model(
    game: &Game,
    day: &CompletedDay,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> SupplyModel {
    supply_model_for(
        day.points.iter().collect(),
        &day.forecast,
        TICKS_PER_DAY,
        game.session().region(),
        l10n,
        style,
    )
}

fn supply_model_for(
    points: Vec<&TimelinePoint>,
    forecast: &[ForecastPoint; 24],
    now: u32,
    region: &RegionDefinition,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> SupplyModel {
    let hours = |tick: u32| f64::from(tick) / f64::from(TICKS_PER_HOUR);
    let output = |point: &TimelinePoint, kind: PlantKind| {
        point.by_kind.get(&kind).map_or(0.0, |mw| mw.value())
    };
    let mut below = vec![0.0; points.len()];
    let stacks = kinds_in_merit_order(region)
        .into_iter()
        .filter(|kind| points.iter().any(|point| output(point, *kind) > 0.0))
        .map(|kind| {
            let low = below.clone();
            for (total, point) in below.iter_mut().zip(&points) {
                *total += output(point, kind);
            }
            SupplyStack {
                kind,
                name: kind.localize(l10n),
                color: kind_color(kind),
                low,
                high: below.clone(),
            }
        })
        .collect();
    SupplyModel {
        transfers: Vec::new(),
        money_style: style.clone(),
        layout: SupplyChartLayout::default(),
        layout_label: l10n.text("supply-chart-combine"),
        hour_axis: l10n.text("forecast-axis-hour"),
        power_axis: l10n.text("forecast-axis-power"),
        cost_axis: l10n.args(
            "supply-chart-axis-cost",
            &[("symbol", style.symbol().into())],
        ),
        power_unit: l10n.text("supply-chart-unit-power"),
        cost_unit: l10n.args(
            "supply-chart-unit-cost",
            &[("symbol", style.symbol().into())],
        ),
        energy_unit: l10n.text("supply-chart-unit-energy"),
        demand_label: l10n.text("supply-chart-demand"),
        served_label: l10n.text("supply-chart-served"),
        forecast_label: l10n.text("supply-chart-forecast"),
        cost_label: l10n.text("supply-chart-marginal-cost"),
        storage_load_label: l10n.text("battery-total-load"),
        storage_energy_label: l10n.text("battery-stored-axis"),
        storage_energy_legend: l10n.text("battery-stored"),
        storage_unused_label: l10n.text("battery-midnight-marker"),
        storage_load: points
            .iter()
            .map(|p| p.demand.value() + p.batteries.values().map(|b| b.charge.value()).sum::<f64>())
            .collect(),
        stored: points
            .iter()
            .map(|p| p.batteries.values().map(|b| b.stored.value()).sum())
            .collect(),
        has_storage: points.iter().any(|p| !p.batteries.is_empty()),
        x: points.iter().map(|point| hours(point.tick)).collect(),
        stacks,
        demand: points.iter().map(|point| point.demand.value()).collect(),
        served: points.iter().map(|point| point.served.value()).collect(),
        forecast: forecast
            .iter()
            .enumerate()
            .map(|(hour, point)| [hour as f64 + 0.5, point.mean.value()])
            .collect(),
        marginal_cost: points
            .iter()
            .filter_map(|point| Some([hours(point.tick), point.marginal_cost?.value()]))
            .collect(),
        now: hours(now),
    }
}

/// The chart and its wrapping key fill the space given. Visibility is local plot UI state;
/// layout changes return a UI action and never alter the simulation.
pub fn draw_supply_chart(ui: &mut egui::Ui, model: &SupplyModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let combined = model.has_storage && model.layout == SupplyChartLayout::Combined;
    let midnight = model.now >= 24.0;
    let line_color = ui.visuals().strong_text_color();
    let now_color = ui.visuals().weak_text_color();
    // Keep the existing plot ID/visibility choices while giving its key real layout space.
    let id = ui.make_persistent_id(egui::Id::new("operating_supply"));
    use super::chart_style::KeyMark;
    let colour = ui.visuals().strong_text_color();
    let entries = model
        .stacks
        .iter()
        .map(|stack| (stack.name.as_str(), color32(stack.color), KeyMark::Band))
        .chain([
            (model.demand_label.as_str(), colour, KeyMark::Line),
            (
                model.served_label.as_str(),
                color32(SERVED_COLOR),
                KeyMark::Line,
            ),
            (model.forecast_label.as_str(), colour, KeyMark::Dashed),
            (
                model.cost_label.as_str(),
                color32(MARGINAL_COST_COLOR),
                KeyMark::Line,
            ),
        ]);
    let mut entries: Vec<_> = entries.collect();
    if model.has_storage {
        entries.push((
            model.storage_load_label.as_str(),
            color32(kind_color(PlantKind::Battery)),
            KeyMark::Dashed,
        ));
        entries.push((
            model.storage_energy_legend.as_str(),
            color32(STORED_ENERGY_COLOR),
            KeyMark::Line,
        ));
    }
    let hovered = super::chart_style::draw_key_with_control(ui, id, entries, |ui| {
        if model.has_storage {
            let mut on = combined;
            if ui
                .checkbox(&mut on, egui::RichText::new(&model.layout_label).size(12.0))
                .changed()
            {
                actions.push(UiAction::SetSupplyChartLayout(if on {
                    SupplyChartLayout::Combined
                } else {
                    SupplyChartLayout::Separate
                }));
            }
        }
    });
    if model.has_storage && midnight {
        let unused = model.stored.last().copied().unwrap_or(0.0);
        let tone = if unused > 0.0 {
            super::theme::TextTone::Caution
        } else {
            super::theme::TextTone::Neutral
        };
        ui.colored_label(
            tone.color(),
            format!(
                "{}: {}",
                model.storage_unused_label,
                crate::l10n::format::megawatt_hours(domain::units::MegawattHours::new(unused))
            ),
        );
    }
    // The marginal cost shares the power axis's space: the highest cost reaches the top of the
    // power data, and the right-hand axis reads it back in $/MWh.
    let top = model
        .demand
        .iter()
        .chain(model.storage_load.iter())
        .chain(model.stacks.iter().flat_map(|stack| stack.high.iter()))
        .chain(model.forecast.iter().map(|[_, mw]| mw))
        .fold(1.0_f64, |top, mw| top.max(*mw));
    let dearest = model
        .marginal_cost
        .iter()
        .fold(0.0_f64, |top, [_, cost]| top.max(*cost));
    let scale = if dearest > 0.0 { top / dearest } else { 1.0 };
    let points =
        |ys: &[f64]| -> Vec<[f64; 2]> { model.x.iter().zip(ys).map(|(x, y)| [*x, *y]).collect() };
    let energy_peak = model.stored.iter().copied().fold(1.0_f64, f64::max);
    let energy_scale = top / energy_peak;
    let stored_points = |scale: f64| -> Vec<[f64; 2]> {
        model
            .x
            .iter()
            .zip(&model.stored)
            .map(|(x, y)| [*x + 1.0 / 60.0, y * scale])
            .collect()
    };
    // Both panes share the remaining height after the external key and midnight annotation.
    let plot_height = ui.available_height();
    // Long rotated titles can extend into the legend in a short bottom bar. The key already
    // names each quantity; compact axes retain the units without overlapping other UI.
    let compact = plot_height < 180.0;
    let power_label = if compact {
        &model.power_unit
    } else {
        &model.power_axis
    };
    let cost_label = if compact {
        &model.cost_unit
    } else {
        &model.cost_axis
    };
    let energy_label = if compact {
        &model.energy_unit
    } else {
        &model.storage_energy_label
    };
    let power = |ui: &mut egui::Ui| {
        let mut axes = vec![
            super::chart_style::y_axis(power_label),
            super::chart_style::y_axis(cost_label)
                .placement(HPlacement::Right)
                .formatter(move |mark, _| format!("{:.0}", mark.value / scale)),
        ];
        if combined {
            axes.push(
                super::chart_style::y_axis(energy_label)
                    .placement(HPlacement::Right)
                    .formatter(move |mark, _| format!("{:.0}", mark.value / energy_scale)),
            );
        }
        Plot::new("operating_supply")
            .id(id)
            .height(plot_height)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .show_crosshair(false)
            .include_x(0.0)
            .include_x(24.0)
            .include_y(0.0)
            .x_grid_spacer(super::chart_style::hour_grid)
            .custom_x_axes(vec![super::chart_style::hour_axis(ui, &model.hour_axis)])
            .custom_y_axes(axes)
            .label_formatter(|position| {
                let HoverPosition::NearDataPoint {
                    plot_name,
                    position,
                    ..
                } = position
                else {
                    return None;
                };
                use crate::l10n::format;
                use domain::units::{MegawattHours, Megawatts, Price};
                let value = if *plot_name == model.storage_energy_legend {
                    format::megawatt_hours(MegawattHours::new(position.y / energy_scale))
                } else if *plot_name == model.cost_label {
                    format::price(Price::per_mwh(position.y / scale), &model.money_style)
                } else {
                    format::megawatts(Megawatts::new(position.y))
                };
                Some(format!(
                    "{plot_name}\n{}: {:.2}\n{value}",
                    model.hour_axis, position.x
                ))
            })
            .show(ui, |plot| {
                for stack in &model.stacks {
                    plot.add(
                        FilledArea::new(stack.name.as_str(), &model.x, &stack.low, &stack.high)
                            .highlight(hovered == Some(egui::Id::new(&stack.name)))
                            .fill_color(color32(stack.color).gamma_multiply(0.8)),
                    );
                }
                plot.line(
                    Line::new(model.demand_label.as_str(), points(&model.demand))
                        .highlight(hovered == Some(egui::Id::new(&model.demand_label)))
                        .color(line_color)
                        .width(2.0),
                );
                plot.line(
                    Line::new(model.served_label.as_str(), points(&model.served))
                        .highlight(hovered == Some(egui::Id::new(&model.served_label)))
                        .color(color32(SERVED_COLOR))
                        .width(1.5),
                );
                plot.line(
                    Line::new(model.forecast_label.as_str(), model.forecast.clone())
                        .highlight(hovered == Some(egui::Id::new(&model.forecast_label)))
                        .color(line_color)
                        .style(LineStyle::dashed_loose())
                        .width(1.0),
                );
                for (label, series) in &model.transfers {
                    plot.line(
                        Line::new(label, points(series))
                            .color(egui::Color32::from_rgb(110, 220, 240))
                            .style(LineStyle::dashed_loose())
                            .width(1.5),
                    );
                }
                let cost: Vec<[f64; 2]> = model
                    .marginal_cost
                    .iter()
                    .map(|[x, cost]| [*x, cost * scale])
                    .collect();
                plot.line(
                    Line::new(model.cost_label.as_str(), cost)
                        .highlight(hovered == Some(egui::Id::new(&model.cost_label)))
                        .color(color32(MARGINAL_COST_COLOR))
                        .width(1.5),
                );
                if model.has_storage {
                    plot.line(
                        Line::new(
                            model.storage_load_label.as_str(),
                            points(&model.storage_load),
                        )
                        .color(color32(kind_color(PlantKind::Battery)))
                        .style(LineStyle::dashed_loose())
                        .highlight(hovered == Some(egui::Id::new(&model.storage_load_label))),
                    );
                }
                if combined {
                    plot.line(
                        Line::new(&model.storage_energy_legend, stored_points(energy_scale))
                            .color(color32(STORED_ENERGY_COLOR))
                            .highlight(hovered == Some(egui::Id::new(&model.storage_energy_legend)))
                            .width(2.5),
                    );
                    if midnight {
                        plot.vline(VLine::new(&model.storage_unused_label, 24.0).color(line_color));
                    }
                }
                plot.vline(VLine::new("", model.now).color(now_color).width(1.0));
            });
    };
    if model.has_storage && !combined {
        let gap = ui.spacing().item_spacing.x;
        let unit = (ui.available_width() - gap).max(0.0) / 3.0;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(unit * 2.0, plot_height),
                egui::Layout::top_down(egui::Align::Min),
                power,
            );
            ui.allocate_ui_with_layout(
                egui::vec2(unit, plot_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let hidden = PlotMemory::load(ui.ctx(), id).is_some_and(|memory| {
                        memory
                            .hidden_items
                            .contains(&egui::Id::new(&model.storage_energy_legend))
                    });
                    Plot::new("battery_inventory")
                        .id(id.with("inventory"))
                        .height(plot_height)
                        .allow_zoom(false)
                        .allow_drag(false)
                        .allow_scroll(false)
                        .allow_boxed_zoom(false)
                        .allow_double_click_reset(false)
                        .show_crosshair(false)
                        .include_x(0.0)
                        .include_x(24.0)
                        .include_y(0.0)
                        .include_y(energy_peak)
                        .x_grid_spacer(super::chart_style::hour_grid)
                        .custom_x_axes(vec![super::chart_style::hour_axis(ui, &model.hour_axis)])
                        .custom_y_axes(vec![super::chart_style::y_axis(energy_label)])
                        .show(ui, |plot| {
                            if !hidden {
                                plot.line(
                                    Line::new(&model.storage_energy_legend, stored_points(1.0))
                                        .color(color32(STORED_ENERGY_COLOR))
                                        .highlight(
                                            hovered
                                                == Some(egui::Id::new(
                                                    &model.storage_energy_legend,
                                                )),
                                        )
                                        .width(2.5),
                                );
                            }
                            if midnight {
                                plot.vline(
                                    VLine::new(&model.storage_unused_label, 24.0).color(line_color),
                                );
                            }
                        });
                },
            );
        });
    } else {
        power(ui);
    }
    actions
}

pub fn island_supply_model(
    game: &Game,
    id: domain::ids::NodeId,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> SupplyModel {
    let session = game.session();
    if super::ui_state::island_scope(session, Some(id)).is_none() {
        return supply_model(game, l10n, style);
    }
    let now = session.clock().tick();
    let scoped = session.island_session(id).expect("selected island");
    scoped_supply_model(
        game,
        id,
        todays_points(game.projections().timeline.points(), now),
        &scoped.hourly_forecast(),
        now,
        l10n,
        style,
    )
}

pub(super) fn island_completed_supply_model(
    game: &Game,
    id: domain::ids::NodeId,
    day: &CompletedDay,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> Option<SupplyModel> {
    if super::ui_state::island_scope(game.session(), Some(id)).is_none() {
        return Some(completed_supply_model(game, day, l10n, style));
    }
    Some(scoped_supply_model(
        game,
        id,
        day.points.iter().collect(),
        day.island_forecasts.get(&id)?,
        TICKS_PER_DAY,
        l10n,
        style,
    ))
}

fn scoped_supply_model(
    game: &Game,
    id: domain::ids::NodeId,
    points: Vec<&TimelinePoint>,
    forecast: &[ForecastPoint; 24],
    now: u32,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> SupplyModel {
    let session = game.session();
    let scoped = session.island_session(id).expect("selected island");
    let points: Vec<_> = points
        .into_iter()
        .filter_map(|point| {
            let row = point.islands.get(&id)?;
            let mut p = point.clone();
            p.demand = row.demand;
            p.served = row.served;
            p.frequency = row.frequency;
            p.by_kind = row.by_kind.clone();
            p.marginal_cost = row
                .by_kind
                .iter()
                .filter(|(kind, mw)| **kind != PlantKind::Battery && mw.value() > 0.0)
                .map(|(kind, _)| session.region().spec(*kind).marginal_cost)
                .max_by(|a, b| a.value().total_cmp(&b.value()));
            p.batteries
                .retain(|plant, _| scoped.power().plant(*plant).is_some());
            Some(p)
        })
        .collect();
    let mut chart = supply_model_for(
        points.iter().collect(),
        forecast,
        now,
        session.region(),
        l10n,
        style,
    );
    chart.transfers = vec![
        (
            l10n.text("hvdc-chart-imports"),
            points
                .iter()
                .map(|p| p.islands[&id].imports.value())
                .collect(),
        ),
        (
            l10n.text("hvdc-chart-exports"),
            points
                .iter()
                .map(|p| -p.islands[&id].exports.value())
                .collect(),
        ),
    ];
    chart
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use std::collections::BTreeMap;

    use application::{Command, InMemoryCommandLog};
    use domain::units::{Hertz, Megawatts};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    /// The Euclidean distance between two colours in RGB space.
    fn rgb_distance(a: Rgb, b: Rgb) -> f64 {
        a.iter()
            .zip(b)
            .map(|(&x, y)| (f64::from(x) - f64::from(y)).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    #[test]
    fn the_served_line_stands_apart_from_every_stack_colour() {
        // Playtest: the served line was the shedding red, nearly the peaker's red.
        for kind in PlantKind::ALL {
            let distance = rgb_distance(SERVED_COLOR, kind_color(kind));
            assert!(
                distance > 80.0,
                "{kind:?} is {distance:.0} from the served line"
            );
        }
        assert!(rgb_distance(SERVED_COLOR, MARGINAL_COST_COLOR) > 80.0);
    }

    /// A game one hour into the day, with every commitable plant committed all day.
    fn an_hour_in() -> Game {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        for plant in game.session().plant_views() {
            if crate::hud::is_commitable(&plant) {
                game.execute(Command::StartPlant { plant: plant.id })
                    .unwrap();
            }
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_HOUR })
            .unwrap();
        game
    }

    #[test]
    fn supply_series_stack_in_merit_order() {
        let l10n = shipped_l10n();
        let game = an_hour_in();
        let session = game.session();
        let region = session.region();
        let points: Vec<&TimelinePoint> = game.projections().timeline.points().collect();
        assert_eq!(points.len(), 60, "an hour of ticks");

        let model = supply_model(&game, &l10n, &usd_style());

        // One sample per tick, in hours.
        assert_eq!(model.x.len(), 60);
        assert_eq!(model.x[0], 0.0);
        assert_eq!(model.x[30], 0.5);
        assert_eq!(model.now, 1.0);
        // The kinds that produced, cheapest at the bottom.
        let kinds: Vec<PlantKind> = model.stacks.iter().map(|stack| stack.kind).collect();
        let order = kinds_in_merit_order(region);
        let expected: Vec<PlantKind> = order
            .iter()
            .copied()
            .filter(|kind| {
                points
                    .iter()
                    .any(|p| p.by_kind.get(kind).is_some_and(|mw| *mw > Megawatts::ZERO))
            })
            .collect();
        assert!(expected.len() >= 2, "{expected:?}");
        assert_eq!(kinds, expected);
        // Each band starts where the one below ends, and is as thick as its kind's output.
        for (i, point) in points.iter().enumerate() {
            let mut below = 0.0;
            for stack in &model.stacks {
                let mw = point.by_kind.get(&stack.kind).map_or(0.0, |mw| mw.value());
                assert_eq!(stack.low[i], below, "{:?} at {i}", stack.kind);
                assert_eq!(stack.high[i], below + mw, "{:?} at {i}", stack.kind);
                below += mw;
            }
            assert_eq!(model.demand[i], point.demand.value());
            assert_eq!(model.served[i], point.served.value());
        }
        assert_eq!(model.stacks[0].name, model.stacks[0].kind.localize(&l10n));
        assert_eq!(model.stacks[0].color, kind_color(model.stacks[0].kind));
        // The forecast for the whole day, and the cost whenever something produced.
        let forecast = session.hourly_forecast();
        assert_eq!(model.forecast.len(), 24);
        for (hour, [x, mw]) in model.forecast.iter().enumerate() {
            assert_eq!(*x, hour as f64 + 0.5);
            assert_eq!(*mw, forecast[hour].mean.value());
        }
        let costs: Vec<[f64; 2]> = points
            .iter()
            .filter_map(|p| Some([f64::from(p.tick) / 60.0, p.marginal_cost?.value()]))
            .collect();
        assert_eq!(model.marginal_cost, costs);
        assert!(!costs.is_empty());
        assert_eq!(model.cost_label, "System marginal cost");
        assert_eq!(model.cost_axis, "Marginal cost ($/MWh)");
        assert_eq!(l10n.problems(), vec![]);
    }

    fn point(tick: u32) -> TimelinePoint {
        TimelinePoint {
            islands: Default::default(),
            batteries: Default::default(),
            tick,
            demand: Megawatts::new(100.0),
            served: Megawatts::new(100.0),
            frequency: Hertz::new(60.0),
            by_kind: BTreeMap::new(),
            marginal_cost: None,
        }
    }

    #[test]
    fn only_todays_ticks_are_drawn() {
        // The end of yesterday, then today's first three ticks.
        let timeline: Vec<TimelinePoint> = [1_437, 1_438, 1_439, 0, 1, 2]
            .into_iter()
            .map(point)
            .collect();
        let ticks = |now| -> Vec<u32> {
            todays_points(timeline.iter(), now)
                .iter()
                .map(|p| p.tick)
                .collect()
        };

        assert_eq!(ticks(3), [0, 1, 2]);
        // Before the first tick of the day, nothing of yesterday shows.
        assert_eq!(ticks(0), Vec::<u32>::new());
        let yesterday: Vec<TimelinePoint> = [1_438, 1_439].into_iter().map(point).collect();
        assert_eq!(todays_points(yesterday.iter(), 0).len(), 0);
    }

    #[test]
    fn before_the_first_tick_the_chart_holds_only_the_forecast() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();

        let model = supply_model(&game, &l10n, &usd_style());

        assert!(model.x.is_empty() && model.stacks.is_empty() && model.demand.is_empty());
        assert_eq!(model.forecast.len(), 24);
        assert_eq!(model.now, 0.0);
    }
}
