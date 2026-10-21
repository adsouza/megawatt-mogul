//! The forecast chart (spec §2.2, §8.4): the coming day's demand forecast with its uncertainty
//! band, the expected solar output, and the capacity committed in each hour, stacked by kind.
//! Read it against the commitment grid: where the stack dips under the forecast, the plan is
//! short.

use application::Game;
use bevy_egui::egui;
use domain::demand::ForecastPoint;
use domain::region::PlantKind;
use domain::time::TICKS_PER_HOUR;
use domain::units::Megawatts;
use egui_plot::{Bar, BarChart, FilledArea, Legend, Line, Plot, VLine};

use super::action::UiAction;
use super::map::{Rgb, color32, kind_color};
use super::plant_list::plants_by_kind;
use super::ui_state::UiState;
use crate::l10n::{Localize, Localizer};

/// The capacity committed in each hour for one kind of plant.
#[derive(Debug, Clone, PartialEq)]
pub struct StackModel {
    pub kind: PlantKind,
    /// `Coal plant`.
    pub name: String,
    pub color: Rgb,
    /// Megawatts: the summed capacity of the plants of this kind committed in each hour.
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
    /// The middle of each hour, in hours: `0.5, 1.5, ...`. Every series is sampled here.
    pub x: [f64; 24],
    /// Forecast demand at minute 30 of each hour, in megawatts.
    pub mean: [f64; 24],
    /// The forecast less two standard deviations of its error.
    pub low: [f64; 24],
    /// The forecast plus two standard deviations of its error.
    pub high: [f64; 24],
    /// Expected solar output: the forecast demand less the forecast net load.
    pub solar: [f64; 24],
    /// The committed capacity of each kind that has a plant, in merit order.
    pub stacks: Vec<StackModel>,
    /// The hour a warning pointed at, if any.
    pub highlight_hour: Option<u8>,
}

/// Capacity committed per hour for each kind of commissioned dispatchable plant, stacked in merit
/// order. Σ over the plants of a kind that are committed in the hour of their capacity.
pub fn stacked_committed_capacity(game: &Game, l10n: &Localizer) -> Vec<StackModel> {
    let region = game.session().region();
    let mut below = [0.0; 24];
    plants_by_kind(game)
        .into_iter()
        .map(|group| {
            let capacity = region.spec(group.kind).capacity.value();
            let mw: [f64; 24] = std::array::from_fn(|hour| {
                let committed = group
                    .plants
                    .iter()
                    .filter(|plant| plant.commitment[hour])
                    .count();
                capacity * committed as f64
            });
            let stack = StackModel {
                kind: group.kind,
                name: group.kind.localize(l10n),
                color: kind_color(group.kind),
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
    let session = game.session();
    let forecast: [ForecastPoint; 24] = session.hourly_forecast();
    let at = |read: fn(&ForecastPoint) -> Megawatts| -> [f64; 24] {
        std::array::from_fn(|hour| read(&forecast[hour]).value())
    };
    let mean = at(|point| point.mean);
    // Expected solar is not a query of its own: the net load is the forecast less expected solar
    // at the same minute 30 the forecast mean is taken at.
    let solar = std::array::from_fn(|hour| {
        let tick = hour as u32 * TICKS_PER_HOUR + TICKS_PER_HOUR / 2;
        (mean[hour] - session.forecast_net_load(tick).value()).max(0.0)
    });
    ForecastModel {
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
        stacks: stacked_committed_capacity(game, l10n),
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
    Plot::new("planning_forecast")
        .height(ui.available_height())
        .legend(Legend::default().background_alpha(1.0))
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_crosshair(false)
        .include_x(0.0)
        .include_x(24.0)
        .include_y(0.0)
        .custom_x_axes(vec![super::chart_style::x_axis(&model.hour_axis)])
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
                    BarChart::new(stack.name.as_str(), bars).color(color32(stack.color)),
                ));
            }
            plot.add(
                FilledArea::new(model.band_label.as_str(), &x, &low, &high).fill_color(band_color),
            );
            plot.line(
                Line::new(model.forecast_label.as_str(), points(&model.mean))
                    .color(line_color)
                    .width(2.0),
            );
            plot.line(
                Line::new(model.solar_label.as_str(), points(&model.solar))
                    .color(solar_color)
                    .width(2.0),
            );
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
    use crate::hud::test_support::{eastern_game, empty_start, shipped_l10n};

    fn stack_of(stacks: &[StackModel], kind: PlantKind) -> &StackModel {
        stacks
            .iter()
            .find(|stack| stack.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} stack"))
    }

    #[test]
    fn stacked_committed_capacity_sums_by_hour_and_kind() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        let views = game.session().plant_views();
        let of_kind = |kind| -> Vec<_> { views.iter().filter(|p| p.kind == kind).collect() };
        let (nuclear, coal, ccgt) = (
            of_kind(PlantKind::Nuclear),
            of_kind(PlantKind::Coal),
            of_kind(PlantKind::Ccgt),
        );
        let region = game.session().region().clone();
        let (nuclear_mw, coal_mw, ccgt_mw) = (
            region.spec(PlantKind::Nuclear).capacity.value(),
            region.spec(PlantKind::Coal).capacity.value(),
            region.spec(PlantKind::Ccgt).capacity.value(),
        );

        // Both nuclear plants all day; the first coal plant all day and the second (a daytime
        // block) only when the sun is up; the first gas plant 08:00-19:59.
        for plant in &nuclear {
            game.execute(Command::StartPlant { plant: plant.id })
                .unwrap();
        }
        game.execute(Command::StartPlant { plant: coal[0].id })
            .unwrap();
        let mut day = [false; 24];
        day[8..20].fill(true);
        game.execute(Command::CommitPlant {
            plant: coal[1].id,
            hours: day,
        })
        .unwrap();
        game.execute(Command::CommitPlant {
            plant: ccgt[0].id,
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
                2.0 * nuclear_mw
            );
            assert_eq!(
                stack_of(&stacks, PlantKind::Coal).mw[hour],
                coal_mw * (1.0 + day_plants)
            );
            assert_eq!(
                stack_of(&stacks, PlantKind::Ccgt).mw[hour],
                ccgt_mw * day_plants
            );
            assert_eq!(stack_of(&stacks, PlantKind::Peaker).mw[hour], 0.0);
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
        assert_eq!(total_at_noon, 2.0 * nuclear_mw + 2.0 * coal_mw + ccgt_mw);
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
