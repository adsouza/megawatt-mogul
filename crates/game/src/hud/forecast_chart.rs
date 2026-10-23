//! The forecast chart (spec §2.2, §8.4): the coming day's demand forecast with its uncertainty
//! band, the expected solar output, and the capacity committed in each hour, stacked by kind.
//! Hydro counts its water-aware allocation on AGC, or its initial target on manual dispatch.
//! Expected solar contributes to the stack as well as its reference curve.
//! Read it against the commitment grid: where the stack dips under the forecast, the plan is
//! short.

use application::Game;
use bevy_egui::egui;
use domain::demand::ForecastPoint;
use domain::region::PlantKind;
use domain::time::TICKS_PER_HOUR;
use domain::units::Megawatts;
use egui_plot::{Bar, BarChart, FilledArea, Line, Plot, VLine};

use super::action::UiAction;
use super::map::{Rgb, color32, kind_color};
use super::plant_list::{kinds_in_merit_order, plants_by_kind};
use super::ui_state::UiState;
use crate::l10n::{Localize, Localizer};

/// The forecast supply in each hour for one kind of plant.
#[derive(Debug, Clone, PartialEq)]
pub struct StackModel {
    pub kind: PlantKind,
    /// `Coal plant`.
    pub name: String,
    pub color: Rgb,
    /// Megawatts: committed capacity, water-aware hydro, or expected solar output.
    pub mw: [f64; 24],
    /// Where this kind's bar starts: the capacity of the kinds below it (cheaper fuel first).
    pub base: [f64; 24],
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForecastModel {
    pub hour_axis: String,
    pub power_axis: String,
    pub forecast_label: String,
    pub band_label: String,
    pub solar_label: String,
    pub storage_charge_label: String,
    pub storage_discharge_label: String,
    pub storage_charge: Vec<[f64; 2]>,
    pub storage_discharge: Vec<[f64; 2]>,
    /// The middle of each hour, in hours: `0.5, 1.5, ...`. Every series is sampled here.
    pub x: [f64; 24],
    /// Forecast demand at minute 30 of each hour, in megawatts.
    pub mean: [f64; 24],
    /// The forecast less two standard deviations of its error.
    pub low: [f64; 24],
    /// The forecast plus two standard deviations of its error.
    pub high: [f64; 24],
    /// Expected solar output at each hour's midpoint, also included in the supply stack.
    pub solar: [f64; 24],
    /// Committed capacity and expected solar supply, in merit order.
    pub stacks: Vec<StackModel>,
    /// The hour a warning pointed at, if any.
    pub highlight_hour: Option<u8>,
}

/// Capacity committed per hour for each kind of commissioned dispatchable plant, stacked in merit
/// order. Hydro uses the shortfall check's water-aware preview on AGC and initial targets
/// on manual dispatch; disconnected hydro contributes nothing. Expected solar from connected,
/// commissioned plants is included independently of commitment, as in the shortfall check.
pub fn stacked_committed_capacity(game: &Game, l10n: &Localizer) -> Vec<StackModel> {
    let session = game.session();
    let region = session.region();
    let hydro = session.hydro_forecast();
    let mut stacks: Vec<_> = plants_by_kind(game)
        .into_iter()
        .map(|group| {
            let capacity = region.spec(group.kind).capacity.value();
            let mw: [f64; 24] = std::array::from_fn(|hour| {
                group
                    .plants
                    .iter()
                    .filter(|plant| plant.commitment[hour])
                    .map(|plant| {
                        if group.kind != PlantKind::Hydro {
                            capacity
                        } else if !plant.connected {
                            0.0
                        } else if plant.auto_follow {
                            hydro.get(&plant.id).map_or(0.0, |mw| mw[hour].value())
                        } else {
                            plant.initial_target.value()
                        }
                    })
                    .sum()
            });
            StackModel {
                kind: group.kind,
                name: group.kind.localize(l10n),
                color: kind_color(group.kind),
                mw,
                base: [0.0; 24],
            }
        })
        .collect();
    let solar = std::array::from_fn(|hour| {
        let tick = hour as u32 * TICKS_PER_HOUR + TICKS_PER_HOUR / 2;
        session
            .power()
            .expected_solar(region, session.clock().month(), tick)
            .value()
    });
    if solar.iter().any(|mw| *mw > 0.0) {
        stacks.push(StackModel {
            kind: PlantKind::Solar,
            name: PlantKind::Solar.localize(l10n),
            color: kind_color(PlantKind::Solar),
            mw: solar,
            base: [0.0; 24],
        });
    }
    let order = kinds_in_merit_order(region);
    stacks.sort_by_key(|stack| order.iter().position(|kind| *kind == stack.kind));
    let mut below = [0.0; 24];
    for stack in &mut stacks {
        stack.base = below;
        for (total, mw) in below.iter_mut().zip(stack.mw) {
            *total += mw;
        }
    }
    stacks
}

pub fn forecast_model(game: &Game, l10n: &Localizer, ui: &UiState) -> ForecastModel {
    let session = game.session();
    let forecast: [ForecastPoint; 24] = session.hourly_forecast();
    let at = |read: fn(&ForecastPoint) -> Megawatts| -> [f64; 24] {
        std::array::from_fn(|hour| read(&forecast[hour]).value())
    };
    let mean = at(|point| point.mean);
    let stacks = stacked_committed_capacity(game, l10n);
    let solar = stacks
        .iter()
        .find(|stack| stack.kind == PlantKind::Solar)
        .map_or([0.0; 24], |stack| stack.mw);
    let storage = session.battery_schedule();
    let storage_series = |charge: bool| {
        (0..1440)
            .map(|tick| {
                [
                    tick as f64 / 60.0,
                    storage
                        .plants
                        .values()
                        .map(|p| {
                            if charge {
                                p.charge[tick].value()
                            } else {
                                p.discharge[tick].value()
                            }
                        })
                        .sum(),
                ]
            })
            .collect()
    };
    ForecastModel {
        storage_charge_label: l10n.text("battery-preview-charge"),
        storage_discharge_label: l10n.text("battery-preview-discharge"),
        storage_charge: if storage.plants.is_empty() {
            Vec::new()
        } else {
            storage_series(true)
        },
        storage_discharge: if storage.plants.is_empty() {
            Vec::new()
        } else {
            storage_series(false)
        },
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

/// The chart, filling the space it is given. It asks for nothing: the plot is not interactive, so
/// the pointer scrolls past it instead of moving it.
pub fn draw_forecast_chart(ui: &mut egui::Ui, model: &ForecastModel) -> Vec<UiAction> {
    let line_color = ui.visuals().strong_text_color();
    let band_color = egui::Color32::from_rgba_unmultiplied(160, 160, 170, 60);
    let solar_color = color32(kind_color(PlantKind::Solar));
    let points = |ys: &[f64; 24]| -> Vec<[f64; 2]> {
        model.x.iter().zip(ys).map(|(x, y)| [*x, *y]).collect()
    };
    let (x, high, low) = (model.x.to_vec(), model.high.to_vec(), model.low.to_vec());
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
            for stack in &model.stacks {
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
                Line::new(model.forecast_label.as_str(), points(&model.mean))
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
    Vec::new()
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{
        eastern_game, edited_western_game, empty_start, order_plant, shipped_game, shipped_l10n,
    };

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
    fn hydro_forecast_includes_expected_battery_discharge_and_refreshes_after_edits() {
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
                .any(|p| p.discharge.iter().any(|mw| *mw > Megawatts::ZERO))
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
        assert_ne!(
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
