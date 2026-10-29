//! The forecast chart (spec §2.2, §8.4): the coming day's demand forecast with its uncertainty
//! band, the expected solar output, and the capacity committed in each hour, stacked by kind.
//! Hydro counts its water-aware allocation on AGC, or its initial target on manual dispatch.
//! Expected solar contributes to the stack as well as its reference curve.
//! Every fleet uses the shortfall evaluator's supply source. Enabled storage uses feasible minute flows. Its signed contribution
//! is stacked first, so charging lowers the baseline and discharge raises total supply.
//! Read it against the commitment grid: where the stack dips under the forecast, the plan is
//! short.

use application::Game;
use bevy_egui::egui;
use domain::demand::ForecastPoint;
use domain::grid::SupplyForecast;
use domain::region::PlantKind;
use domain::time::TICKS_PER_HOUR;
use domain::units::Megawatts;
use egui_plot::{Bar, BarChart, FilledArea, Line, Plot, VLine};

use super::action::{ButtonModel, UiAction, button};
use super::map::{Rgb, color32, kind_color};
use super::plant_list::{kinds_in_merit_order, plants_by_kind_session};
use super::ui_state::UiState;
use crate::l10n::{Localize, Localizer, Names, format};

#[derive(Debug, Clone, PartialEq)]
pub struct IslandBalanceModel {
    pub island: domain::ids::NodeId,
    pub active: bool,
    pub shortfall_label: String,
    pub excess_label: String,
    pub shortfall: Vec<[f64; 2]>,
    pub excess: Vec<[f64; 2]>,
    pub inspect: ButtonModel,
}

/// The forecast supply in each hour for one kind of plant.
#[derive(Debug, Clone, PartialEq)]
pub struct StackModel {
    pub kind: PlantKind,
    /// `Coal plant`.
    pub name: String,
    pub color: Rgb,
    /// MW: generation or signed feasible battery output (discharge minus charging).
    pub mw: [f64; 24],
    /// The sum of contributions below this kind: signed storage first, then generation in merit order.
    pub base: [f64; 24],
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindForecastModel {
    pub label: String,
    pub range_label: String,
    pub central: [f64; 24],
    pub low: [f64; 24],
    pub high: [f64; 24],
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForecastModel {
    /// "All" must show local risks, never let another island's surplus cancel them.
    pub islands: Vec<IslandBalanceModel>,
    pub island_note: String,
    pub island_picker_label: String,
    pub risk_axis: String,
    pub scope_label: Option<String>,
    pub service_note: Option<String>,
    pub activation: Option<ButtonModel>,
    pub minimum_label: String,
    pub minimum: Vec<[f64; 2]>,
    pub minimum_demand: Vec<[f64; 2]>,
    pub constraint_label: String,
    pub link_issues: Vec<String>,
    pub links: ButtonModel,
    pub wind: Option<WindForecastModel>,
    pub hour_axis: String,
    pub power_axis: String,
    pub forecast_label: String,
    pub band_label: String,
    pub solar_label: String,
    pub storage_charge_label: String,
    pub storage_discharge_label: String,
    pub storage_charge: Vec<[f64; 2]>,
    pub storage_discharge: Vec<[f64; 2]>,
    pub net_supply_label: String,
    /// Net supply and demand at the warning's assessed points: all minutes with
    /// enabled storage, hour endpoints otherwise. Bars remain midpoint samples.
    pub net_supply: Vec<[f64; 2]>,
    pub demand_minutes: Vec<[f64; 2]>,
    /// The middle of each hour, in hours: `0.5, 1.5, ...`. Bars are sampled here.
    pub x: [f64; 24],
    /// Forecast demand at minute 30 of each hour, in megawatts.
    pub mean: [f64; 24],
    /// The forecast less two standard deviations of its error.
    pub low: [f64; 24],
    /// The forecast plus two standard deviations of its error.
    pub high: [f64; 24],
    /// Expected solar output at each hour's midpoint, also included in the supply stack.
    pub solar: [f64; 24],
    /// Net supply: signed storage first, then generation in merit order.
    pub stacks: Vec<StackModel>,
    /// The hour a warning pointed at, if any.
    pub highlight_hour: Option<u8>,
}

/// Capacity committed per hour for each kind of commissioned dispatchable plant, stacked in merit
/// order. Hydro uses the shortfall check's water-aware preview on AGC and initial targets
/// on manual dispatch; disconnected hydro contributes nothing. Expected solar from connected,
/// commissioned plants is included independently of commitment, as in the shortfall check.
/// With enabled storage, all contributions use the shared sequential battery evaluator and
/// its net battery output is the first stack, keeping the top at generation + discharge − charge.
pub fn stacked_committed_capacity(game: &Game, l10n: &Localizer) -> Vec<StackModel> {
    let forecast = game.session().supply_forecast();
    stacked_supply(game.session(), l10n, &forecast)
}

fn stacked_supply(
    session: &domain::session::GameSession,
    l10n: &Localizer,
    forecast: &SupplyForecast,
) -> Vec<StackModel> {
    let storage = &forecast.minutes;
    let groups = plants_by_kind_session(session);
    let order = kinds_in_merit_order(session.region());
    // Put the signed battery contribution below generation: charging lowers
    // the baseline, so the top of the stack remains net supply in both directions.
    let kinds = std::iter::once(PlantKind::Battery)
        .filter(|_| forecast.has_storage)
        .chain(order.into_iter().filter(|kind| {
            groups.iter().any(|group| group.kind == *kind)
                || (kind.is_variable()
                    && storage.iter().any(|p| p.generation[kind] > Megawatts::ZERO))
        }));
    let mut below = [0.0; 24];
    kinds
        .map(|kind| {
            let mw = std::array::from_fn(|hour| {
                let point = &storage[hour * TICKS_PER_HOUR as usize + TICKS_PER_HOUR as usize / 2];
                if kind == PlantKind::Battery {
                    (point.battery_discharge - point.battery_charge).value()
                } else {
                    point
                        .generation
                        .get(&kind)
                        .copied()
                        .unwrap_or(Megawatts::ZERO)
                        .value()
                }
            });
            let stack = StackModel {
                kind,
                name: kind.localize(l10n),
                color: kind_color(kind),
                mw,
                base: below,
            };
            for (total, add) in below.iter_mut().zip(mw) {
                *total += add;
            }
            stack
        })
        .collect()
}

pub fn forecast_model(game: &Game, l10n: &Localizer, ui: &UiState) -> ForecastModel {
    if let Some(local) = super::ui_state::island_scope(game.session(), ui.island)
        .and_then(|id| game.session().island_session(id))
    {
        let mut chart = forecast_model_for_session(&local, l10n, ui, &local.supply_forecast());
        if ui
            .island
            .is_some_and(|id| !game.session().power().island_active(id))
        {
            chart.service_note = Some(l10n.text("island-inactive-status"));
            chart.activation = ui.island.and_then(|id| {
                super::activation::activation_card(game.session(), id, l10n).map(|card| card.review)
            });
        }
        chart.link_issues = super::links::planning_issues(game.session(), l10n, ui.island);
        chart.constraint_label = l10n.args(
            "forecast-hvdc-constraints",
            &[("count", (chart.link_issues.len() as u32).into())],
        );
        return chart;
    }
    forecast_model_for(game, l10n, ui, &game.session().supply_forecast())
}

pub(super) fn forecast_model_for(
    game: &Game,
    l10n: &Localizer,
    ui: &UiState,
    forecast_supply: &SupplyForecast,
) -> ForecastModel {
    forecast_model_for_session(game.session(), l10n, ui, forecast_supply)
}
pub(super) fn forecast_model_for_session(
    session: &domain::session::GameSession,
    l10n: &Localizer,
    ui: &UiState,
    forecast_supply: &SupplyForecast,
) -> ForecastModel {
    let forecast: [ForecastPoint; 24] = session.hourly_forecast();
    let at = |read: fn(&ForecastPoint) -> Megawatts| -> [f64; 24] {
        std::array::from_fn(|hour| read(&forecast[hour]).value())
    };
    let mean = at(|point| point.mean);
    let storage = &forecast_supply.minutes;
    let stacks = stacked_supply(session, l10n, forecast_supply);
    let solar = stacks
        .iter()
        .find(|stack| stack.kind == PlantKind::Solar)
        .map_or([0.0; 24], |stack| stack.mw);
    let storage_series = |charge: bool| {
        storage
            .iter()
            .enumerate()
            .filter(|_| forecast_supply.has_storage)
            .map(|(tick, point)| {
                [
                    tick as f64 / 60.0,
                    if charge {
                        point.battery_charge.value()
                    } else {
                        point.battery_discharge.value()
                    },
                ]
            })
            .collect()
    };
    let minimum_samples = session.minimum_generation_forecast();
    let names = Names::new(l10n, session);
    let link_issues =
        if session.region().areas().is_none() || session.power().island_nodes().is_empty() {
            Vec::new()
        } else {
            super::links::planning_issues(session, l10n, ui.island)
        };
    let islands = session
        .power()
        .island_nodes()
        .keys()
        .filter(|_| session.region().areas().is_some())
        .filter_map(|id| {
            let local = session.island_session(*id)?;
            let supply = local.supply_forecast();
            let name = names.island(session, *id);
            let shortage = domain::session::GameSession::capacity_shortfall_for(&supply);
            let min = local.minimum_generation_check();
            let hour = shortage
                .map(|w| w.hour)
                .or_else(|| min.map(|w| w.hour))
                .unwrap_or(0);
            Some(IslandBalanceModel {
                island: *id,
                active: session.power().island_active(*id),
                shortfall_label: l10n.args(
                    "forecast-island-shortfall",
                    &[
                        ("island", name.clone().into()),
                        (
                            "amount",
                            format::megawatts(shortage.map_or(Megawatts::ZERO, |w| w.shortfall))
                                .into(),
                        ),
                        (
                            "time",
                            super::planning_warnings::time_of(shortage.map_or(0, |w| w.hour))
                                .into(),
                        ),
                    ],
                ),
                excess_label: l10n.args(
                    "forecast-island-excess",
                    &[
                        ("island", name.clone().into()),
                        (
                            "amount",
                            format::megawatts(min.map_or(Megawatts::ZERO, |w| w.excess)).into(),
                        ),
                        (
                            "time",
                            super::planning_warnings::time_of(min.map_or(0, |w| w.hour)).into(),
                        ),
                    ],
                ),
                shortfall: supply
                    .assessed_minutes()
                    .map(|(tick, p)| {
                        [
                            tick as f64 / 60.0,
                            (p.demand - p.gross_supply).value().max(0.0),
                        ]
                    })
                    .collect(),
                excess: local
                    .minimum_generation_forecast()
                    .into_iter()
                    .map(|p| {
                        [
                            p.tick as f64 / 60.0,
                            (p.committed_minimum - p.net_load).value().max(0.0),
                        ]
                    })
                    .collect(),
                inspect: ButtonModel::new(
                    l10n.args(
                        if session.power().island_active(*id) {
                            "forecast-island-inspect"
                        } else {
                            "island-inactive-name"
                        },
                        &[("island", name.into())],
                    ),
                    UiAction::InspectForecast { island: *id, hour },
                ),
            })
        })
        .collect();
    ForecastModel {
        islands,
        island_note: l10n.text("forecast-island-note"),
        island_picker_label: l10n.text("forecast-inspect-island"),
        risk_axis: l10n.text("forecast-risk-axis"),
        scope_label: super::ui_state::island_scope(session, ui.island)
            .map(|id| names.island(session, id)),
        service_note: None,
        activation: None,
        minimum_label: l10n.text("forecast-minimum"),
        minimum_demand: minimum_samples
            .iter()
            .map(|p| {
                [
                    p.tick as f64 / 60.0,
                    storage[p.tick as usize].demand.value(),
                ]
            })
            .collect(),
        minimum: minimum_samples
            .into_iter()
            .map(|p| {
                let demand = storage[p.tick as usize].demand;
                [
                    p.tick as f64 / 60.0,
                    (p.committed_minimum + demand - p.net_load).value(),
                ]
            })
            .collect(),
        constraint_label: l10n.args(
            "forecast-hvdc-constraints",
            &[("count", (link_issues.len() as u32).into())],
        ),
        link_issues,
        links: ButtonModel::new(l10n.text("hvdc-title"), UiAction::ShowLinks(true)),
        wind: session
            .plant_views()
            .iter()
            .any(|p| p.kind == PlantKind::Wind && p.connected)
            .then(|| {
                let forecasts: [(Megawatts, Megawatts, Megawatts); 24] = std::array::from_fn(|h| {
                    session.power().wind_forecast(
                        session.region(),
                        session.clock().month(),
                        (h as u32 * 60) + 30,
                    )
                });
                WindForecastModel {
                    label: l10n.text("wind-forecast"),
                    range_label: l10n.text("wind-range"),
                    central: forecasts.map(|f| f.0.value()),
                    low: forecasts.map(|f| f.1.value()),
                    high: forecasts.map(|f| f.2.value()),
                }
            }),
        net_supply_label: l10n.text("forecast-net-supply"),
        net_supply: forecast_supply
            .assessed_minutes()
            .map(|(tick, p)| [tick as f64 / 60.0, p.net_supply().value()])
            .collect(),
        demand_minutes: forecast_supply
            .assessed_minutes()
            .map(|(tick, p)| [tick as f64 / 60.0, p.demand.value()])
            .collect(),
        storage_charge_label: l10n.text("battery-preview-charge"),
        storage_discharge_label: l10n.text("battery-preview-discharge"),
        storage_charge: storage_series(true),
        storage_discharge: storage_series(false),
        hour_axis: l10n.text("forecast-axis-hour"),
        power_axis: l10n.text("forecast-axis-power"),
        forecast_label: l10n.text("commitment-forecast"),
        band_label: l10n.text("forecast-band"),
        solar_label: l10n.text("forecast-solar"),
        x: std::array::from_fn(|hour| hour as f64 + 0.5),
        mean,
        low: at(|point| point.low),
        high: at(|point| point.high),
        solar,
        stacks,
        highlight_hour: ui.highlight_hour,
    }
}

/// Plot gestures do not consume scrolling. Island buttons and link details navigate the plan.
pub fn draw_forecast_chart(ui: &mut egui::Ui, model: &ForecastModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    if let Some(scope) = &model.scope_label {
        ui.horizontal_wrapped(|ui| {
            ui.strong(scope);
            if let Some(review) = &model.activation {
                button(ui, review, &mut actions);
            }
        });
    }
    if let Some(note) = &model.service_note {
        ui.label(note);
    }
    if !model.link_issues.is_empty() {
        ui.horizontal(|ui| {
            ui.menu_button(&model.constraint_label, |ui| {
                ui.set_max_width((ui.ctx().content_rect().width() - 32.0).min(560.0));
                egui::ScrollArea::vertical()
                    .id_salt("forecast_link_constraints")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for issue in &model.link_issues {
                            ui.label(issue);
                        }
                    });
            });
            button(ui, &model.links, &mut actions);
        });
    }
    if !model.islands.is_empty() {
        actions.extend(draw_island_balances(ui, model));
        return actions;
    }
    let line_color = ui.visuals().strong_text_color();
    let band_color = egui::Color32::from_rgba_unmultiplied(160, 160, 170, 60);
    let solar_color = color32(kind_color(PlantKind::Solar));
    let net_supply_color = egui::Color32::from_rgb(80, 210, 160);
    let points = |ys: &[f64; 24]| -> Vec<[f64; 2]> {
        model.x.iter().zip(ys).map(|(x, y)| [*x, *y]).collect()
    };
    let (x, high, low) = if model.demand_minutes.is_empty() {
        (model.x.to_vec(), model.high.to_vec(), model.low.to_vec())
    } else {
        // The error band is a constant share of demand; use the same demand
        // samples as the warning curve so hourly peaks stay inside the band.
        let (low_scale, high_scale) = model
            .mean
            .iter()
            .enumerate()
            .find(|(_, mean)| **mean > 0.0)
            .map_or((0.0, 0.0), |(h, mean)| {
                (model.low[h] / mean, model.high[h] / mean)
            });
        (
            model.demand_minutes.iter().map(|p| p[0]).collect(),
            model
                .demand_minutes
                .iter()
                .map(|p| p[1] * high_scale)
                .collect(),
            model
                .demand_minutes
                .iter()
                .map(|p| p[1] * low_scale)
                .collect(),
        )
    };
    use super::chart_style::KeyMark;
    let id = ui.make_persistent_id(egui::Id::new("planning_forecast"));
    let entries = model
        .stacks
        .iter()
        .map(|stack| (stack.name.as_str(), color32(stack.color), KeyMark::Band))
        .chain([
            (model.forecast_label.as_str(), line_color, KeyMark::Line),
            (model.band_label.as_str(), band_color, KeyMark::Band),
            (model.solar_label.as_str(), solar_color, KeyMark::Line),
        ]);
    let mut entries: Vec<_> = entries.collect();
    entries.push((
        model.net_supply_label.as_str(),
        net_supply_color,
        KeyMark::Line,
    ));
    entries.push((
        &model.minimum_label,
        egui::Color32::from_rgb(245, 185, 65),
        KeyMark::Dashed,
    ));
    if !model.storage_charge.is_empty() {
        entries.extend([
            (
                model.storage_charge_label.as_str(),
                color32(kind_color(PlantKind::Battery)),
                KeyMark::Dashed,
            ),
            (
                model.storage_discharge_label.as_str(),
                color32(kind_color(PlantKind::Battery)),
                KeyMark::Line,
            ),
        ]);
    }
    if let Some(wind) = &model.wind {
        let color = color32(kind_color(PlantKind::Wind));
        entries.extend([
            (wind.label.as_str(), color, KeyMark::Line),
            (wind.range_label.as_str(), color, KeyMark::Band),
        ]);
    }
    let hovered = super::chart_style::draw_key(ui, id, entries);
    Plot::new("planning_forecast")
        .id(id)
        .height(ui.available_height())
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_crosshair(false)
        .include_x(0.0)
        .include_x(24.0)
        .include_y(0.0)
        // Every commitment hour needs its own label, at full contrast, even in a narrow HUD.
        .x_grid_spacer(|_| {
            (0..=24)
                .map(|hour| egui_plot::GridMark {
                    value: f64::from(hour),
                    step_size: 1.0,
                })
                .collect()
        })
        .custom_x_axes(vec![
            super::chart_style::x_axis(&model.hour_axis)
                .label_spacing(egui::emath::Rangef::new(0.0, 0.0)),
        ])
        .custom_y_axes(vec![super::chart_style::y_axis(&model.power_axis)])
        .show(ui, |plot| {
            // Paint storage last so its negative charging band remains visible
            // where generation starts below zero on the adjusted baseline.
            for stack in model
                .stacks
                .iter()
                .filter(|s| s.kind != PlantKind::Battery)
                .chain(model.stacks.iter().filter(|s| s.kind == PlantKind::Battery))
            {
                let bars = (0..24)
                    .map(|hour| {
                        Bar::new(model.x[hour], stack.mw[hour])
                            .base_offset(stack.base[hour])
                            .width(0.85)
                            .fill(color32(stack.color).gamma_multiply(0.8))
                    })
                    .collect();
                plot.add(super::chart_style::ReadableBars(
                    BarChart::new(stack.name.as_str(), bars)
                        .color(color32(stack.color))
                        .highlight(hovered == Some(egui::Id::new(&stack.name))),
                ));
            }
            plot.add(
                FilledArea::new(model.band_label.as_str(), &x, &low, &high)
                    .fill_color(band_color)
                    .highlight(hovered == Some(egui::Id::new(&model.band_label))),
            );
            plot.line(
                Line::new(
                    model.forecast_label.as_str(),
                    if model.demand_minutes.is_empty() {
                        points(&model.mean)
                    } else {
                        model.demand_minutes.clone()
                    },
                )
                .highlight(hovered == Some(egui::Id::new(&model.forecast_label)))
                .color(line_color)
                .width(2.0),
            );
            plot.line(
                Line::new(model.solar_label.as_str(), points(&model.solar))
                    .highlight(hovered == Some(egui::Id::new(&model.solar_label)))
                    .color(solar_color)
                    .width(2.0),
            );
            plot.line(
                Line::new(&model.net_supply_label, model.net_supply.clone())
                    .color(net_supply_color)
                    .width(2.0)
                    .highlight(hovered == Some(egui::Id::new(&model.net_supply_label))),
            );
            let minimum_x: Vec<_> = model.minimum.iter().map(|p| p[0]).collect();
            let demand: Vec<_> = model.minimum_demand.iter().map(|p| p[1]).collect();
            let upper: Vec<_> = model
                .minimum
                .iter()
                .zip(&demand)
                .map(|(p, d)| p[1].max(*d))
                .collect();
            plot.add(
                FilledArea::new(&model.minimum_label, &minimum_x, &demand, &upper)
                    .fill_color(egui::Color32::from_rgba_unmultiplied(245, 185, 65, 45)),
            );
            plot.line(
                Line::new(&model.minimum_label, model.minimum.clone())
                    .color(egui::Color32::from_rgb(245, 185, 65))
                    .style(egui_plot::LineStyle::dashed_dense())
                    .width(2.0)
                    .highlight(hovered == Some(egui::Id::new(&model.minimum_label))),
            );
            if let Some(wind) = &model.wind {
                let color = color32(kind_color(PlantKind::Wind));
                plot.add(
                    FilledArea::new(&wind.range_label, &model.x, &wind.low, &wind.high)
                        .fill_color(color.gamma_multiply(0.2))
                        .highlight(hovered == Some(egui::Id::new(&wind.range_label))),
                );
                plot.line(
                    Line::new(&wind.label, points(&wind.central))
                        .color(color)
                        .highlight(hovered == Some(egui::Id::new(&wind.label))),
                );
            }
            if !model.storage_charge.is_empty() {
                plot.line(
                    Line::new(&model.storage_charge_label, model.storage_charge.clone())
                        .color(color32(kind_color(PlantKind::Battery)))
                        .style(egui_plot::LineStyle::dashed_loose())
                        .highlight(hovered == Some(egui::Id::new(&model.storage_charge_label))),
                );
                plot.line(
                    Line::new(
                        &model.storage_discharge_label,
                        model.storage_discharge.clone(),
                    )
                    .color(color32(kind_color(PlantKind::Battery)))
                    .highlight(hovered == Some(egui::Id::new(&model.storage_discharge_label))),
                );
            }
            if let Some(hour) = model.highlight_hour {
                plot.vline(
                    VLine::new("", f64::from(hour) + 0.5)
                        .color(egui::Color32::from_rgb(240, 190, 60))
                        .width(2.0),
                );
            }
        });
    actions
}

fn draw_island_balances(ui: &mut egui::Ui, model: &ForecastModel) -> Vec<UiAction> {
    use super::chart_style::KeyMark;
    let mut actions = Vec::new();
    ui.label(&model.island_note);
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("forecast_island_picker")
            .selected_text(&model.island_picker_label)
            .show_ui(ui, |ui| {
                for island in &model.islands {
                    if ui.selectable_label(false, &island.inspect.label).clicked() {
                        actions.push(island.inspect.action.clone());
                        ui.close();
                    }
                }
            });
        // Keep urgent grids one click away; balanced inherited grids stay in the picker.
        for island in model
            .islands
            .iter()
            .filter(|island| {
                !island.active
                    || island
                        .shortfall
                        .iter()
                        .chain(&island.excess)
                        .any(|point| point[1] > 0.0)
            })
            .take(2)
        {
            button(ui, &island.inspect, &mut actions);
        }
    });
    let id = ui.make_persistent_id("island_forecast_risks");
    // A stable colour per island; solid means shortage, dashed means surplus floor.
    let palette = [
        egui::Color32::from_rgb(245, 110, 110),
        egui::Color32::from_rgb(245, 190, 70),
        egui::Color32::from_rgb(110, 195, 255),
        egui::Color32::from_rgb(195, 150, 255),
        egui::Color32::from_rgb(100, 225, 175),
    ];
    let entries = model.islands.iter().enumerate().flat_map(|(i, island)| {
        [
            (
                island.shortfall_label.as_str(),
                palette[i % palette.len()],
                KeyMark::Line,
            ),
            (
                island.excess_label.as_str(),
                palette[i % palette.len()],
                KeyMark::Dashed,
            ),
        ]
        .into_iter()
        .zip([&island.shortfall, &island.excess])
        .filter(|(_, points)| points.iter().any(|p| p[1] > 0.0))
        .map(|(entry, _)| entry)
    });
    let hovered = egui::ScrollArea::vertical()
        .id_salt("island_risk_legend")
        .max_height(48.0)
        .show(ui, |ui| super::chart_style::draw_key(ui, id, entries))
        .inner;
    Plot::new("island_forecast_risks")
        .id(id)
        .height(ui.available_height().max(60.0))
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .include_x(0.0)
        .include_x(24.0)
        .include_y(0.0)
        .include_y(1.0)
        .x_grid_spacer(super::chart_style::hour_grid)
        .custom_x_axes(vec![super::chart_style::hour_axis(ui, &model.hour_axis)])
        .custom_y_axes(vec![super::chart_style::y_axis(&model.risk_axis)])
        .show(ui, |plot| {
            for (i, island) in model.islands.iter().enumerate() {
                let color = palette[i % palette.len()];
                if island.shortfall.iter().any(|p| p[1] > 0.0) {
                    plot.line(
                        Line::new(&island.shortfall_label, island.shortfall.clone())
                            .color(color)
                            .width(2.5)
                            .highlight(hovered == Some(egui::Id::new(&island.shortfall_label))),
                    );
                }
                if island.excess.iter().any(|p| p[1] > 0.0) {
                    plot.line(
                        Line::new(&island.excess_label, island.excess.clone())
                            .color(color)
                            .width(2.5)
                            .style(egui_plot::LineStyle::dashed_dense())
                            .highlight(hovered == Some(egui::Id::new(&island.excess_label))),
                    );
                }
            }
        });
    actions
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{
        eastern_game, edited_western_game, empty_start, flat_load, order_plant, shipped_game,
        shipped_l10n,
    };

    #[test]
    fn island_risk_curves_match_local_warnings_and_scope_without_cross_island_cancellation() {
        let mut game = crate::hud::test_support::sgcc_planning_with_small_remote_source();
        for plant in game
            .session()
            .plant_views()
            .into_iter()
            .filter(|p| p.kind.is_dispatchable())
        {
            game.execute(Command::CommitPlant {
                plant: plant.id,
                hours: [true; 24],
            })
            .unwrap();
            game.execute(Command::SetAutoFollow {
                plant: plant.id,
                on: false,
            })
            .unwrap();
            let mw = game.session().region().spec(plant.kind).capacity;
            game.execute(Command::SetDispatchTarget {
                plant: plant.id,
                mw,
            })
            .unwrap();
        }
        let l10n = shipped_l10n();
        let links: Vec<_> = game
            .session()
            .power()
            .network()
            .links()
            .map(|link| link.id)
            .collect();
        for automatic in [true, false] {
            for &link in &links {
                game.execute(Command::SetLinkAutoFollow {
                    link,
                    on: automatic,
                })
                .unwrap();
            }
            let model = forecast_model(&game, &l10n, &UiState::default());
            assert_eq!(
                model.islands.len(),
                game.session().power().island_nodes().len()
            );
            for island in &model.islands {
                let local = game.session().island_session(island.island).unwrap();
                let shortage = local.capacity_shortfall_check();
                let excess = local.minimum_generation_check();
                let max = |points: &[[f64; 2]]| points.iter().map(|p| p[1]).fold(0.0, f64::max);
                assert!(
                    (max(&island.shortfall) - shortage.map_or(0.0, |w| w.shortfall.value())).abs()
                        < 1e-8
                );
                assert!(
                    (max(&island.excess) - excess.map_or(0.0, |w| w.excess.value())).abs() < 1e-8
                );
                if !island.shortfall_label.contains("Shanghai") {
                    assert!(max(&island.excess) > 0.0);
                    assert_eq!(max(&island.shortfall), 0.0);
                } else {
                    assert!(max(&island.shortfall) > 0.0);
                    assert_eq!(max(&island.excess), 0.0);
                    assert!(island.shortfall_label.contains("Shanghai"));
                }
                let mut ui = UiState::default();
                ui.apply(&island.inspect.action);
                assert_eq!(ui.island, Some(island.island));
                assert_eq!(ui.tab, super::super::PlanningTab::Forecast);
                let chart = forecast_model(&game, &l10n, &ui);
                assert!(chart.islands.is_empty());
                assert!(chart.scope_label.is_some());
                let worst = chart
                    .minimum
                    .iter()
                    .zip(&chart.minimum_demand)
                    .map(|(floor, demand)| (floor[1] - demand[1]).max(0.0))
                    .fold(0.0, f64::max);
                assert!((worst - excess.map_or(0.0, |w| w.excess.value())).abs() < 1e-8);
            }
            assert_eq!(
                model
                    .link_issues
                    .iter()
                    .any(|s| s.contains("Shanghai") && s.contains("hourly transfers are zero")),
                !automatic,
                "a zero stored schedule warns only in Manual mode"
            );
        }
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn forecast_key_has_its_own_space_and_preserves_series_toggles() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut model = forecast_model(&game, &shipped_l10n(), &Default::default());
        let l10n = shipped_l10n();
        model.stacks = PlantKind::ALL
            .into_iter()
            .map(|kind| StackModel {
                kind,
                name: kind.localize(&l10n),
                color: kind_color(kind),
                mw: [100.0; 24],
                base: [0.0; 24],
            })
            .collect();
        for width in [1024.0 / 1.25, 1024.0, 1920.0] {
            let ctx = egui::Context::default();
            let mut id = egui::Id::NULL;
            let mut key_at = None;
            for _ in 0..3 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 240.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        id = ui.make_persistent_id(egui::Id::new("planning_forecast"));
                        draw_forecast_chart(ui, &model);
                    },
                );
                if ctx.cumulative_frame_nr() == 3 {
                    let memory = egui_plot::PlotMemory::load(&ctx, id).unwrap();
                    let frame = *memory.transform().frame();
                    assert!(frame.height() >= 80.0);
                    fn find(shape: &egui::Shape, name: &str) -> Option<egui::Rect> {
                        match shape {
                            egui::Shape::Vec(children) => {
                                children.iter().find_map(|s| find(s, name))
                            }
                            egui::Shape::Text(text) if text.galley.text() == name => {
                                Some(text.visual_bounding_rect())
                            }
                            _ => None,
                        }
                    }
                    for name in model.stacks.iter().map(|s| s.name.as_str()).chain([
                        model.forecast_label.as_str(),
                        model.band_label.as_str(),
                        model.solar_label.as_str(),
                    ]) {
                        let rect = output
                            .shapes
                            .iter()
                            .find_map(|s| find(&s.shape, name))
                            .expect(name);
                        assert!(
                            rect.bottom() <= frame.top(),
                            "{name} covers the plot at width {width}: {rect:?}, plot {frame:?}"
                        );
                        if name == model.forecast_label {
                            key_at = Some(rect.center());
                        }
                    }
                }
                output.drop_without_applying_deltas();
            }
            let at = key_at.unwrap();
            for hidden in [true, false] {
                for pressed in [true, false] {
                    let output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 240.0),
                            )),
                            events: vec![
                                egui::Event::PointerMoved(at),
                                egui::Event::PointerButton {
                                    pos: at,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            ..Default::default()
                        },
                        |ui| {
                            draw_forecast_chart(ui, &model);
                        },
                    );
                    output.drop_without_applying_deltas();
                }
                assert_eq!(
                    egui_plot::PlotMemory::load(&ctx, id)
                        .unwrap()
                        .hidden_items
                        .contains(&egui::Id::new(&model.forecast_label)),
                    hidden
                );
            }
        }
    }

    #[test]
    fn forecast_renders_every_hour_at_minimum_window_and_enlarged_ui_zoom() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let model = forecast_model(&game, &shipped_l10n(), &Default::default());
        for zoom in [1.0, 1.25] {
            let ctx = egui::Context::default();
            ctx.set_visuals(super::super::theme::visuals());
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1024.0 / zoom, 240.0),
                    )),
                    ..Default::default()
                };
                let output = ctx.run_ui(input, |ui| {
                    draw_forecast_chart(ui, &model);
                });
                fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect, egui::Color32)>) {
                    match shape {
                        egui::Shape::Vec(children) => {
                            for shape in children {
                                texts(shape, out);
                            }
                        }
                        egui::Shape::Text(text) => out.push((
                            text.galley.text().to_owned(),
                            text.visual_bounding_rect(),
                            text.fallback_color,
                        )),
                        _ => {}
                    }
                }
                let mut labels = Vec::new();
                for shape in &output.shapes {
                    texts(&shape.shape, &mut labels);
                }
                output.drop_without_applying_deltas();
                if ctx.cumulative_frame_nr() < 3 {
                    continue;
                }
                let bottom = labels
                    .iter()
                    .filter(|(text, _, _)| text.parse::<u8>().is_ok())
                    .map(|(_, rect, _)| rect.center().y)
                    .fold(0.0, f32::max);
                let hours: Vec<_> = labels
                    .iter()
                    .filter(|(text, rect, _)| {
                        text.parse::<u8>().is_ok() && (rect.center().y - bottom).abs() < 1.0
                    })
                    .collect();
                for hour in 0..=24 {
                    assert!(
                        hours.iter().any(|(text, _, _)| *text == hour.to_string()),
                        "missing hour {hour} at zoom {zoom}: {hours:?}"
                    );
                }
                let screen =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0 / zoom, 240.0));
                for (_, rect, color) in &hours {
                    assert_eq!(*color, super::super::theme::TEXT);
                    assert!(
                        screen.contains_rect(*rect),
                        "hour label is clipped at zoom {zoom}"
                    );
                }
                for pair in hours.windows(2) {
                    assert!(
                        !pair[0].1.intersects(pair[1].1),
                        "hour labels overlap at zoom {zoom}"
                    );
                }
            }
        }
    }

    fn stack_of(stacks: &[StackModel], kind: PlantKind) -> &StackModel {
        stacks
            .iter()
            .find(|stack| stack.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} stack"))
    }

    #[test]
    fn manual_thermal_forecast_uses_targets_and_matches_warning_without_storage() {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let plant = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Ccgt)
            .unwrap();
        let target = game.session().region().spec(plant.kind).capacity * 0.5;
        game.execute(Command::StartPlant { plant: plant.id })
            .unwrap();
        game.execute(Command::SetDispatchTarget {
            plant: plant.id,
            mw: target,
        })
        .unwrap();
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert_eq!(
            stack_of(&model.stacks, PlantKind::Ccgt).mw,
            [target.value(); 24]
        );
        assert_eq!(
            model.net_supply.len(),
            48,
            "hour endpoints expose peaks missed by midpoint bars"
        );
        let worst = model
            .demand_minutes
            .iter()
            .zip(&model.net_supply)
            .enumerate()
            .map(|(i, (d, s))| (i, d[1] - s[1]))
            .fold((0, f64::NEG_INFINITY), |worst, next| {
                if next.1 > worst.1 { next } else { worst }
            });
        let warning = game.session().capacity_shortfall_check().unwrap();
        assert_eq!(model.net_supply[worst.0][0].floor() as u8, warning.hour);
        assert!((worst.1 - warning.shortfall.value()).abs() < 1e-6);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn storage_free_forecast_exposes_an_hour_boundary_shortfall_hidden_by_midpoints() {
        let original = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        let session = original.session();
        let capacity = session.region().spec(PlantKind::Ccgt).capacity.value();
        let peak = (0..24)
            .map(|hour| {
                session.region().load_factor(
                    session.region().cities()[0].id,
                    session.clock().month(),
                    hour * 60,
                )
            })
            .fold(0.0, f64::max);
        let cities = session
            .city_views()
            .iter()
            .filter(|city| city.connected)
            .count();
        let base_load = capacity * 1.001 / peak / cities as f64;
        let mut game = edited_western_game(|text| flat_load(text, base_load));
        empty_start(&mut game);
        let plant = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Ccgt)
            .unwrap()
            .id;
        game.execute(Command::StartPlant { plant }).unwrap();
        game.execute(Command::SetAutoFollow { plant, on: true })
            .unwrap();
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert!(
            model.mean.iter().all(|mean| *mean < capacity),
            "every midpoint looks covered"
        );
        let warning = game.session().capacity_shortfall_check().unwrap();
        let (demand, supply) = model
            .demand_minutes
            .iter()
            .zip(&model.net_supply)
            .find(|(d, _)| d[0] == f64::from(warning.hour))
            .expect("the hour boundary is drawn");
        assert!((demand[1] - supply[1] - warning.shortfall.value()).abs() < 1e-6);
        assert!(demand[1] > supply[1]);
        assert_eq!(l10n.problems(), vec![]);
    }

    fn battery_forecast_game(energy_mwh: Option<f64>) -> Game {
        let mut game = edited_western_game(|text| {
            let mut text = text.replacen("fleet: [", "fleet: [(Battery, -117.0, 34.5),", 1);
            if let Some(energy) = energy_mwh {
                let start = text.find("energy_mwh: ").unwrap() + "energy_mwh: ".len();
                let end = start + text[start..].find(',').unwrap();
                text.replace_range(start..end, &format!("{energy:?}"));
            }
            text
        });
        let mut hours = [false; 24];
        hours[..17].fill(true);
        for plant in game.session().plant_views() {
            if plant.kind.is_dispatchable() && plant.kind != PlantKind::Hydro {
                game.execute(Command::CommitPlant {
                    plant: plant.id,
                    hours,
                })
                .unwrap();
            }
        }
        game
    }

    #[test]
    fn battery_forecast_stacks_feasible_discharge_and_subtracts_charging() {
        let game = battery_forecast_game(None);
        let l10n = shipped_l10n();
        let before = game.session().state_hash();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert_eq!(game.session().state_hash(), before);
        let battery = stack_of(&model.stacks, PlantKind::Battery);
        assert!(
            battery.mw.iter().any(|mw| *mw < 0.0),
            "charging lowers supply"
        );
        assert!(
            battery.mw.iter().any(|mw| *mw > 0.0),
            "discharge raises supply"
        );
        assert_eq!(
            model.stacks[0].kind,
            PlantKind::Battery,
            "charging offsets the stack baseline"
        );
        for (hour, mw) in battery.mw.iter().enumerate() {
            let tick = hour * TICKS_PER_HOUR as usize + TICKS_PER_HOUR as usize / 2;
            assert!(
                (mw - (model.storage_discharge[tick][1] - model.storage_charge[tick][1])).abs()
                    < 1e-6
            );
            let mut total = 0.0;
            for stack in &model.stacks {
                assert_eq!(stack.base[hour], total);
                total += stack.mw[hour];
            }
            assert!((total - model.net_supply[tick][1]).abs() < 1e-6);
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn battery_forecast_cannot_discharge_without_feasible_charging() {
        let mut game = battery_forecast_game(None);
        empty_start(&mut game);
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        let battery = stack_of(&model.stacks, PlantKind::Battery);
        assert_eq!(battery.mw, [0.0; 24]);
        assert!(model.storage_discharge.iter().all(|p| p[1] == 0.0));
        assert!(model.storage_charge.iter().all(|p| p[1] == 0.0));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn battery_depletion_inside_an_hour_matches_the_shortfall_warning_and_renders() {
        let game = battery_forecast_game(Some(13.0));
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        let depletion = model
            .storage_discharge
            .windows(2)
            .enumerate()
            .find(|(_, pair)| pair[0][1] > 0.0 && pair[1][1] == 0.0)
            .map(|(tick, _)| tick + 1)
            .expect("a charged battery empties");
        assert_ne!(
            depletion % TICKS_PER_HOUR as usize,
            0,
            "depletion occurs inside an hour"
        );
        assert!(model.net_supply[depletion][1] < model.net_supply[depletion - 1][1]);
        let (tick, shortfall) = model
            .demand_minutes
            .iter()
            .zip(&model.net_supply)
            .enumerate()
            .map(|(tick, (demand, supply))| (tick, demand[1] - supply[1]))
            .fold((0, f64::NEG_INFINITY), |worst, next| {
                if next.1 > worst.1 { next } else { worst }
            });
        let warning = game.session().capacity_shortfall_check().unwrap();
        assert_eq!(tick / TICKS_PER_HOUR as usize, usize::from(warning.hour));
        assert!((shortfall - warning.shortfall.value()).abs() < 1e-6);
        assert!((model.net_supply[tick][1] - warning.deliverable.value()).abs() < 1e-6);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1024.0 / 1.25, 240.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    draw_forecast_chart(ui, &model);
                },
            )
            .drop_without_applying_deltas();
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn battery_reserve_waits_for_a_later_supply_gap_in_chart_schedule_and_warning() {
        let mut game = battery_forecast_game(Some(13.0));
        let mut hours = [false; 24];
        hours[..20].fill(true);
        for plant in game.session().plant_views() {
            if plant.kind.is_dispatchable() && plant.kind != PlantKind::Hydro {
                game.execute(Command::CommitPlant {
                    plant: plant.id,
                    hours,
                })
                .unwrap();
            }
        }
        let before = game.session().state_hash();
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert!(
            model.storage_discharge[..20 * 60]
                .iter()
                .all(|p| p[1] == 0.0)
        );
        assert!(
            model.storage_discharge[20 * 60..]
                .iter()
                .any(|p| p[1] > 0.0)
        );
        let schedule = game.session().battery_schedule();
        for (tick, point) in model.storage_discharge.iter().enumerate() {
            let scheduled: f64 = schedule
                .plants
                .values()
                .map(|p| p.discharge[tick].value())
                .sum();
            assert!((scheduled - point[1]).abs() < 1e-6);
        }
        let (tick, gap) = model
            .demand_minutes
            .iter()
            .zip(&model.net_supply)
            .enumerate()
            .map(|(tick, (demand, supply))| (tick, demand[1] - supply[1]))
            .fold(
                (0, 0.0),
                |worst, next| if next.1 > worst.1 { next } else { worst },
            );
        let warning = game.session().capacity_shortfall_check().unwrap();
        assert_eq!(usize::from(warning.hour), tick / 60);
        assert!((warning.shortfall.value() - gap).abs() < 1e-6);
        assert_eq!(game.session().state_hash(), before);
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn disabled_batteries_add_no_supply_or_charging_deduction() {
        let mut game = battery_forecast_game(None);
        let battery = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Battery)
            .unwrap()
            .id;
        game.execute(Command::SetBatteryEnabled {
            plant: battery,
            on: false,
        })
        .unwrap();
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert!(
            model
                .stacks
                .iter()
                .all(|stack| stack.kind != PlantKind::Battery)
        );
        assert!(model.storage_charge.is_empty() && model.storage_discharge.is_empty());
        assert_eq!(
            model.net_supply.len(),
            48,
            "the remaining fleet still has a net-supply curve"
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn battery_forecast_and_schedule_hold_energy_when_generation_covers_demand() {
        let game = edited_western_game(|text| {
            text.replacen("fleet: [", "fleet: [(Battery, -117.0, 34.5),", 1)
        });
        assert!(
            game.session()
                .battery_schedule()
                .plants
                .values()
                .all(|p| p.discharge.iter().all(|mw| *mw == Megawatts::ZERO))
        );
        let l10n = shipped_l10n();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert!(
            model.storage_discharge.iter().all(|p| p[1] == 0.0),
            "a fully covered fleet does not discharge into a surplus"
        );
        assert!(model.storage_charge.iter().any(|p| p[1] > 0.0));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn expected_solar_is_stacked_once_and_agrees_with_the_shortfall_warning() {
        let l10n = shipped_l10n();
        let mut game = edited_western_game(|text| {
            text.replacen("fleet: [", "fleet: [(Solar, -117.0, 34.5),", 1)
        });
        empty_start(&mut game);
        let before = game.session().state_hash();
        let model = forecast_model(&game, &l10n, &UiState::default());
        assert_eq!(game.session().state_hash(), before);
        let solar = stack_of(&model.stacks, PlantKind::Solar);
        assert_eq!(solar.mw, model.solar);
        assert_eq!(
            model
                .stacks
                .iter()
                .filter(|s| s.kind == PlantKind::Solar)
                .count(),
            1
        );
        assert_eq!(model.solar[0], 0.0);
        assert_eq!(model.solar[23], 0.0);
        assert!(model.solar[12] > 0.0);
        assert!(
            model.solar[12]
                < game
                    .session()
                    .region()
                    .spec(PlantKind::Solar)
                    .capacity
                    .value()
        );

        let session = game.session();
        let mut worst = (f64::NEG_INFINITY, 0, 0.0);
        for (hour, solar_midpoint) in model.solar.iter().enumerate() {
            let mut total = 0.0;
            for stack in &model.stacks {
                assert_eq!(stack.base[hour], total);
                total += stack.mw[hour];
            }
            assert_eq!(
                total, *solar_midpoint,
                "only solar supplies the all-off fleet"
            );
            // The warning samples hour ends, while the chart samples minute 30.
            for minute in [0, TICKS_PER_HOUR - 1] {
                let tick = hour as u32 * TICKS_PER_HOUR + minute;
                let expected_solar = session
                    .power()
                    .expected_solar(session.region(), session.clock().month(), tick)
                    .value();
                let supply = total - solar_midpoint + expected_solar;
                let demand = session
                    .power()
                    .connected_demand_forecast(session.region(), session.clock().month(), tick)
                    .value();
                let shortfall = demand - supply;
                if shortfall > worst.0 {
                    worst = (shortfall, hour as u8, supply);
                }
            }
        }
        let warning = session
            .capacity_shortfall_check()
            .expect("solar alone is short");
        assert_eq!(warning.hour, worst.1);
        assert!((warning.shortfall.value() - worst.0).abs() < 1e-6);
        assert!((warning.deliverable.value() - worst.2).abs() < 1e-6);
        assert_eq!(l10n.problems(), vec![]);
    }

    proptest::proptest! {
    #[test]
    fn solar_stack_preserves_merit_order_and_ignores_unfinished_plants(
        hours in proptest::array::uniform24(proptest::bool::ANY),
    ) {
        let l10n = shipped_l10n();
        let mut game = edited_western_game(|text| {
            text.replacen("fleet: [", "fleet: [(Solar, -117.0, 34.5),", 1)
        });
        let hydro = game.session().plant_views().into_iter()
            .find(|plant| plant.kind == PlantKind::Hydro).unwrap().id;
        game.execute(Command::CommitPlant { plant: hydro, hours }).unwrap();
        let before = stacked_committed_capacity(&game, &l10n);
        order_plant(&mut game, PlantKind::Solar, 0);
        let stacks = stacked_committed_capacity(&game, &l10n);
        proptest::prop_assert_eq!(&stacks, &before, "construction adds no forecast supply");
        let order = crate::hud::plant_list::kinds_in_merit_order(game.session().region());
        let ranks: Vec<_> = stacks
            .iter()
            .map(|s| order.iter().position(|kind| *kind == s.kind).unwrap())
            .collect();
        proptest::prop_assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]));
        proptest::prop_assert!(stack_of(&stacks, PlantKind::Solar).mw[12] > 0.0);
        for hour in 0..24 {
            let mut below = 0.0;
            for stack in &stacks {
                proptest::prop_assert_eq!(stack.base[hour], below);
                below += stack.mw[hour];
            }
        }
        proptest::prop_assert_eq!(l10n.problems(), vec![]);
    }
    }

    #[test]
    fn hydro_forecast_uses_the_water_plan_and_agrees_with_the_shortfall_warning() {
        let l10n = shipped_l10n();
        let mut game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        for plant in game.session().plant_views() {
            if plant.kind.is_dispatchable() && plant.kind != PlantKind::Hydro {
                game.execute(Command::StopPlant { plant: plant.id })
                    .unwrap();
            }
        }
        let model = forecast_model(&game, &l10n, &UiState::default());
        let hydro = &stack_of(&model.stacks, PlantKind::Hydro).mw;
        assert!(hydro[23] < hydro[19], "hydro falls after the evening peak");
        let warning = game
            .session()
            .capacity_shortfall_check()
            .expect("hydro alone is short");
        let start = u32::from(warning.hour) * TICKS_PER_HOUR;
        let shortfall = [start, start + TICKS_PER_HOUR - 1]
            .map(|tick| {
                game.session().forecast_net_load(tick).value() - hydro[usize::from(warning.hour)]
            })
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((shortfall - warning.shortfall.value()).abs() < 1e-6);
        assert!(
            game.session().hydro_schedule().plants.is_empty(),
            "preview leaves the live plan empty"
        );

        game.execute(Command::BeginOperating).unwrap();
        let plan = game.session().hydro_schedule();
        for (hour, actual) in hydro.iter().enumerate() {
            let expected: f64 = plan.plants.values().map(|p| p.hourly[hour].value()).sum();
            assert!((actual - expected).abs() < 1e-6, "hour {hour}");
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn manual_hydro_forecast_counts_its_initial_target_only_in_committed_hours() {
        let l10n = shipped_l10n();
        let mut game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        empty_start(&mut game);
        let plant = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Hydro)
            .unwrap();
        let target = game.session().region().spec(PlantKind::Hydro).capacity * 0.5;
        game.execute(Command::SetDispatchTarget {
            plant: plant.id,
            mw: target,
        })
        .unwrap();
        let mut hours = [false; 24];
        hours[17..21].fill(true);
        game.execute(Command::CommitPlant {
            plant: plant.id,
            hours,
        })
        .unwrap();
        let stacks = stacked_committed_capacity(&game, &l10n);
        for (hour, mw) in stack_of(&stacks, PlantKind::Hydro).mw.iter().enumerate() {
            assert_eq!(*mw, if hours[hour] { target.value() } else { 0.0 });
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn battery_reserves_do_not_reduce_the_hydro_forecast() {
        let l10n = shipped_l10n();
        let mut game = edited_western_game(|text| {
            text.replacen("fleet: [", "fleet: [(Battery, -117.0, 34.5),", 1)
        });
        let before = game.session().state_hash();
        let with_storage = stacked_committed_capacity(&game, &l10n);
        assert_eq!(game.session().state_hash(), before);
        let storage = game.session().battery_schedule();
        assert!(
            storage
                .plants
                .values()
                .all(|p| p.discharge.iter().all(|mw| *mw == Megawatts::ZERO))
        );
        let preview = game.session().hydro_forecast();
        for hour in 0..24 {
            let expected: f64 = preview.values().map(|mw| mw[hour].value()).sum();
            assert!((stack_of(&with_storage, PlantKind::Hydro).mw[hour] - expected).abs() < 1e-6);
        }
        let battery = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Battery)
            .unwrap()
            .id;
        game.execute(Command::SetBatteryEnabled {
            plant: battery,
            on: false,
        })
        .unwrap();
        let without_storage = stacked_committed_capacity(&game, &l10n);
        assert_eq!(
            stack_of(&with_storage, PlantKind::Hydro).mw,
            stack_of(&without_storage, PlantKind::Hydro).mw
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn uncommitted_hydro_adds_no_forecast_capacity() {
        let l10n = shipped_l10n();
        let mut game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        for plant in game.session().plant_views() {
            if plant.kind == PlantKind::Hydro {
                game.execute(Command::StopPlant { plant: plant.id })
                    .unwrap();
            }
        }
        let stacks = stacked_committed_capacity(&game, &l10n);
        assert_eq!(stack_of(&stacks, PlantKind::Hydro).mw, [0.0; 24]);
        for hour in 0..24 {
            let mut below = 0.0;
            for stack in &stacks {
                assert_eq!(stack.base[hour], below);
                below += stack.mw[hour];
            }
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn stacked_committed_capacity_sums_by_hour_and_kind() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let views = game.session().plant_views();
        for plant in &views {
            if plant.kind.is_dispatchable() {
                game.execute(Command::SetAutoFollow {
                    plant: plant.id,
                    on: true,
                })
                .unwrap();
            }
        }
        let of_kind = |kind| -> Vec<_> { views.iter().filter(|p| p.kind == kind).collect() };
        let (nuclear, coal, ccgt, peaker) = (
            of_kind(PlantKind::Nuclear),
            of_kind(PlantKind::Coal),
            of_kind(PlantKind::Ccgt),
            of_kind(PlantKind::Peaker),
        );
        let region = game.session().region().clone();
        let (nuclear_mw, coal_mw, ccgt_mw, peaker_mw) = (
            region.spec(PlantKind::Nuclear).capacity.value(),
            region.spec(PlantKind::Coal).capacity.value(),
            region.spec(PlantKind::Ccgt).capacity.value(),
            region.spec(PlantKind::Peaker).capacity.value(),
        );

        // Nuclear and coal all day; one gas plant all day, another and a peaker
        // 08:00-19:59. Two gas units test aggregation within one stack.
        for plant in &nuclear {
            game.execute(Command::StartPlant { plant: plant.id })
                .unwrap();
        }
        game.execute(Command::StartPlant { plant: coal[0].id })
            .unwrap();
        game.execute(Command::StartPlant { plant: ccgt[0].id })
            .unwrap();
        let mut day = [false; 24];
        day[8..20].fill(true);
        game.execute(Command::CommitPlant {
            plant: ccgt[1].id,
            hours: day,
        })
        .unwrap();
        game.execute(Command::CommitPlant {
            plant: peaker[0].id,
            hours: day,
        })
        .unwrap();

        let stacks = stacked_committed_capacity(&game, &l10n);

        let kinds: Vec<PlantKind> = stacks.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                PlantKind::Nuclear,
                PlantKind::Coal,
                PlantKind::Ccgt,
                PlantKind::Peaker
            ],
            "merit order, and every kind with a plant gets a stack"
        );
        for hour in 0..24 {
            let on = (8..20).contains(&hour);
            let day_plants = if on { 1.0 } else { 0.0 };
            assert_eq!(
                stack_of(&stacks, PlantKind::Nuclear).mw[hour],
                nuclear.len() as f64 * nuclear_mw
            );
            assert_eq!(stack_of(&stacks, PlantKind::Coal).mw[hour], coal_mw);
            assert_eq!(
                stack_of(&stacks, PlantKind::Ccgt).mw[hour],
                ccgt_mw * (1.0 + day_plants)
            );
            assert_eq!(
                stack_of(&stacks, PlantKind::Peaker).mw[hour],
                peaker_mw * day_plants
            );
        }
        // Each bar starts where the cheaper kinds end, so the top of the last is the total.
        for hour in 0..24 {
            let mut running = 0.0;
            for stack in &stacks {
                assert_eq!(stack.base[hour], running, "{:?} at {hour}", stack.kind);
                running += stack.mw[hour];
            }
        }
        let total_at_noon: f64 = stacks.iter().map(|s| s.mw[12]).sum();
        assert_eq!(
            total_at_noon,
            nuclear.len() as f64 * nuclear_mw + coal_mw + 2.0 * ccgt_mw + peaker_mw
        );
        assert_eq!(stack_of(&stacks, PlantKind::Coal).name, "Coal plant");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_plant_committed_in_no_hour_adds_nothing() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);

        let stacks = stacked_committed_capacity(&game, &l10n);

        assert!(
            stacks
                .iter()
                .all(|stack| stack.mw == [0.0; 24] && stack.base == [0.0; 24])
        );
    }

    #[test]
    fn the_forecast_series_come_from_the_session() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let forecast = game.session().hourly_forecast();

        let model = forecast_model(&game, &l10n, &UiState::default());

        for (hour, point) in forecast.iter().enumerate() {
            assert_eq!(model.mean[hour], point.mean.value());
            assert_eq!(model.low[hour], point.low.value());
            assert_eq!(model.high[hour], point.high.value());
            assert!(model.low[hour] <= model.mean[hour] && model.mean[hour] <= model.high[hour]);
            assert_eq!(model.x[hour], hour as f64 + 0.5);
            // No solar farm in the shipped eastern fleet: nothing to expect.
            assert_eq!(model.solar[hour], 0.0);
        }
        assert_eq!(model.forecast_label, "Forecast demand");
        assert_eq!(model.band_label, "Forecast range");
        assert_eq!(model.highlight_hour, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_highlighted_hour_reaches_the_chart() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let ui = UiState {
            highlight_hour: Some(18),
            ..UiState::default()
        };

        assert_eq!(forecast_model(&game, &l10n, &ui).highlight_hour, Some(18));
    }
}
