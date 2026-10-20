//! The hydro panel (spec §12.6): for each scheduled hydro plant, the water level `λ` ("hydro
//! covers everything above 31.0 GW") and its 24 hourly allocations. The hours already played are
//! frozen: each shows the allocation as it stood when the hour began, so the plan can be read
//! against what happened.

use application::Game;
use bevy_egui::egui;
use domain::grid::HydroScheduleView;
use domain::ids::PlantId;
use domain::region::PlantKind;
use domain::time::TICKS_PER_HOUR;
use egui_plot::{Bar, BarChart, Legend, Plot};

use super::action::UiAction;
use super::map::{Rgb, color32, kind_color};
use crate::l10n::{Localizer, Names, format};

/// One scheduled plant.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroRow {
    pub plant: PlantId,
    pub name: String,
    /// `Hydro plant 5: Hydro covers everything above 31.0 GW`, or that it has no water to spare.
    pub level: String,
    pub has_level: bool,
    /// The allocation of each hour, in megawatts.
    pub hourly: [f64; 24],
    pub color: Rgb,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HydroModel {
    pub hour_axis: String,
    pub power_axis: String,
    pub rows: Vec<HydroRow>,
    /// The hours before this one are played and frozen.
    pub frozen_hours: u8,
    /// `Hours already played are frozen`.
    pub frozen_note: String,
    /// Shown instead when no plant is scheduled.
    pub empty: Option<String>,
}

pub fn hydro_model(game: &Game, l10n: &Localizer) -> HydroModel {
    let session = game.session();
    let view = session.hydro_schedule();
    let names = if view.plants.is_empty() {
        Names::empty(l10n)
    } else {
        Names::new(l10n, session)
    };
    let hour = session.clock().tick() / TICKS_PER_HOUR;
    hydro_model_for(&view, &names, u8::try_from(hour).unwrap_or(24), l10n)
}

/// The panel for a schedule, with the hours before `frozen_hours` played.
pub fn hydro_model_for(
    view: &HydroScheduleView,
    names: &Names,
    frozen_hours: u8,
    l10n: &Localizer,
) -> HydroModel {
    let rows: Vec<HydroRow> = view
        .plants
        .iter()
        .enumerate()
        .map(|(index, (plant, plan))| {
            let name = names.plant(*plant);
            let level = plan.level.map_or_else(
                || l10n.text("hydro-no-level"),
                |level| l10n.args("hydro-level", &[("level", format::megawatts(level).into())]),
            );
            HydroRow {
                plant: *plant,
                level: l10n.args(
                    "hydro-row",
                    &[("plant", name.as_str().into()), ("level", level.into())],
                ),
                name,
                has_level: plan.level.is_some(),
                hourly: plan.hourly.map(|mw| mw.value()),
                color: hydro_shade(index),
            }
        })
        .collect();
    HydroModel {
        hour_axis: l10n.text("forecast-axis-hour"),
        power_axis: l10n.text("forecast-axis-power"),
        empty: rows.is_empty().then(|| l10n.text("hydro-none")),
        rows,
        frozen_hours,
        frozen_note: l10n.text("hydro-frozen"),
    }
}

/// The width of the column of water levels beside the chart.
pub const HYDRO_TEXT_WIDTH: f32 = 230.0;

/// The levels on the left, the allocations stacked by plant on the right, played hours faded.
pub fn draw_hydro(ui: &mut egui::Ui, model: &HydroModel) -> Vec<UiAction> {
    if let Some(empty) = &model.empty {
        ui.weak(empty.as_str());
        return Vec::new();
    }
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        let column = egui::Layout::top_down(egui::Align::Min);
        ui.allocate_ui_with_layout(egui::vec2(HYDRO_TEXT_WIDTH, height), column, |ui| {
            ui.set_width(HYDRO_TEXT_WIDTH);
            egui::ScrollArea::vertical()
                .id_salt("hydro_levels")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for row in &model.rows {
                        if row.has_level {
                            ui.label(row.level.as_str());
                        } else {
                            ui.colored_label(ui.visuals().warn_fg_color, row.level.as_str());
                        }
                    }
                    ui.weak(model.frozen_note.as_str());
                });
        });
        let mut below = [0.0; 24];
        Plot::new("operating_hydro")
            .height(height)
            .legend(Legend::default())
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .show_crosshair(false)
            .include_x(0.0)
            .include_x(24.0)
            .include_y(0.0)
            .x_axis_label(model.hour_axis.as_str())
            .y_axis_label(model.power_axis.as_str())
            .show(ui, |plot| {
                for row in &model.rows {
                    let color = color32(row.color);
                    let bars = (0..24u8)
                        .map(|hour| {
                            let h = usize::from(hour);
                            let fill = if hour < model.frozen_hours {
                                color.gamma_multiply(0.35)
                            } else {
                                color
                            };
                            Bar::new(f64::from(hour) + 0.5, row.hourly[h])
                                .base_offset(below[h])
                                .width(0.85)
                                .fill(fill)
                        })
                        .collect();
                    for (total, add) in below.iter_mut().zip(row.hourly) {
                        *total += add;
                    }
                    plot.bar_chart(BarChart::new(row.name.as_str(), bars).color(color));
                }
            });
    });
    Vec::new()
}

/// A shade of the hydro colour per plant, so stacked plants can be told apart.
pub fn hydro_shade(index: usize) -> Rgb {
    let [r, g, b] = kind_color(PlantKind::Hydro);
    let dim = |c: u8| (f32::from(c) * (1.0 - 0.18 * (index % 4) as f32)) as u8;
    [dim(r), dim(g), dim(b)]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use application::{Command, InMemoryCommandLog};
    use domain::grid::PlantHydroScheduleView;
    use domain::units::{MegawattHours, Megawatts};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_game, shipped_l10n};

    #[test]
    fn hydro_panel_shows_level_or_no_level() {
        let l10n = shipped_l10n();
        let names = Names::empty(&l10n);
        let mut hourly = [Megawatts::ZERO; 24];
        hourly[17] = Megawatts::new(900.0);
        hourly[18] = Megawatts::new(1_200.0);
        let plan = |level| PlantHydroScheduleView {
            level,
            hourly,
            budget_remaining: MegawattHours::new(2_100.0),
        };
        let view = HydroScheduleView {
            plants: BTreeMap::from([
                (PlantId(4), plan(Some(Megawatts::new(31_000.0)))),
                (PlantId(9), plan(None)),
            ]),
        };

        let model = hydro_model_for(&view, &names, 6, &l10n);

        assert_eq!(model.empty, None);
        let rows: Vec<(PlantId, &str, bool)> = model
            .rows
            .iter()
            .map(|row| (row.plant, row.level.as_str(), row.has_level))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    PlantId(4),
                    format!(
                        "{}: Hydro covers everything above 31.0 GW",
                        names.plant(PlantId(4))
                    )
                    .as_str(),
                    true
                ),
                (
                    PlantId(9),
                    format!(
                        "{}: Hydro has no water to spare today",
                        names.plant(PlantId(9))
                    )
                    .as_str(),
                    false
                ),
            ]
        );
        assert_eq!(model.rows[0].hourly[18], 1_200.0);
        assert_eq!(model.rows[0].hourly[0], 0.0);
        assert_eq!(model.rows[0].name, names.plant(PlantId(4)));
        assert_eq!(model.rows[0].color, hydro_shade(0));
        assert_ne!(model.rows[1].color, model.rows[0].color);
        assert_eq!(model.frozen_hours, 6);
        assert_eq!(model.frozen_note, "Hours already played are frozen");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn no_scheduled_plant_says_so() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();

        let model = hydro_model(&game, &l10n);

        assert!(model.rows.is_empty());
        assert_eq!(
            model.empty.as_deref(),
            Some("No hydro plant is scheduled today.")
        );
    }

    #[test]
    fn the_western_schedule_freezes_the_hours_played() {
        let l10n = shipped_l10n();
        let mut game = shipped_game(
            "western_interconnection",
            Box::new(InMemoryCommandLog::default()),
        );
        for plant in game.session().plant_views() {
            if crate::hud::is_commitable(&plant) {
                game.execute(Command::StartPlant { plant: plant.id })
                    .unwrap();
            }
            // Only hydro under AGC is scheduled (spec §12.3).
            if plant.kind == PlantKind::Hydro {
                game.execute(Command::SetAutoFollow {
                    plant: plant.id,
                    on: true,
                })
                .unwrap();
            }
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks {
            n: 2 * TICKS_PER_HOUR + 10,
        })
        .unwrap();
        let view = game.session().hydro_schedule();
        assert!(
            !view.plants.is_empty(),
            "the western hydro fleet is scheduled"
        );

        let model = hydro_model(&game, &l10n);

        assert_eq!(model.frozen_hours, 2, "00:00 and 01:00 are played");
        let ids: Vec<PlantId> = model.rows.iter().map(|row| row.plant).collect();
        assert_eq!(ids, view.plants.keys().copied().collect::<Vec<_>>());
        for (row, plan) in model.rows.iter().zip(view.plants.values()) {
            let expected: Vec<f64> = plan.hourly.iter().map(|mw| mw.value()).collect();
            assert_eq!(row.hourly.to_vec(), expected);
            assert_eq!(row.has_level, plan.level.is_some());
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn levels_are_formatted_as_power() {
        assert_eq!(format::megawatts(Megawatts::new(31_000.0)), "31.0 GW");
    }
}
