//! Cached representative-day curves; drawing never queries or changes the simulation.
use super::{ButtonModel, UiAction, button, theme::TextTone};
use crate::l10n::{Localize, Localizer, format};
use bevy_egui::egui;
use domain::{region::PlantKind, session::OperationalDay, time::SimMonth};
use egui_plot::{Line, Plot};

#[derive(Debug, Clone, PartialEq)]
pub struct OutlookDaySeries {
    pub label: String,
    pub color: egui::Color32,
    pub values: Vec<[f64; 2]>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct OutlookDayModel {
    pub has_storage: bool,
    pub title: String,
    pub help: String,
    pub summary: String,
    pub has_shortfall: bool,
    pub power_axis: String,
    pub energy_axis: String,
    pub hour_axis: String,
    pub series: Vec<OutlookDaySeries>,
    pub energy: OutlookDaySeries,
    pub back: ButtonModel,
    pub close: ButtonModel,
}

pub fn outlook_day_model(
    l10n: &Localizer,
    month: SimMonth,
    day: &OperationalDay,
) -> OutlookDayModel {
    let curve = |value: &dyn Fn(&domain::grid::SupplyForecastPoint) -> f64| {
        day.forecast
            .minutes
            .iter()
            .enumerate()
            .map(|(tick, p)| [tick as f64 / 60.0, value(p)])
            .collect()
    };
    let mut series = vec![
        OutlookDaySeries {
            label: l10n.text("outlook-day-demand"),
            color: TextTone::Information.color(),
            values: curve(&|p| p.demand.value()),
        },
        OutlookDaySeries {
            label: l10n.text("outlook-day-supply"),
            color: TextTone::Positive.color(),
            values: curve(&|p| p.net_supply().value()),
        },
    ];
    for kind in [
        PlantKind::Nuclear,
        PlantKind::Coal,
        PlantKind::Ccgt,
        PlantKind::Peaker,
        PlantKind::Hydro,
        PlantKind::Solar,
        PlantKind::Wind,
        PlantKind::Geothermal,
    ] {
        if day
            .forecast
            .minutes
            .iter()
            .any(|p| p.generation.get(&kind).is_some_and(|v| v.value() > 0.0))
        {
            series.push(OutlookDaySeries {
                label: kind.localize(l10n),
                color: super::map::color32(super::map::kind_color(kind)),
                values: curve(&|p| p.generation.get(&kind).map_or(0.0, |v| v.value())),
            });
        }
    }
    if day.forecast.has_storage {
        series.push(OutlookDaySeries {
            label: l10n.text("outlook-day-charge"),
            color: TextTone::Negative.color(),
            values: curve(&|p| -p.battery_charge.value()),
        });
        series.push(OutlookDaySeries {
            label: l10n.text("outlook-day-discharge"),
            color: TextTone::Positive.color(),
            values: curve(&|p| p.battery_discharge.value()),
        });
    }
    if day.shortfall_minutes > 0 {
        series.push(OutlookDaySeries {
            label: l10n.text("outlook-day-unmet"),
            color: TextTone::Negative.color(),
            values: day
                .shortfalls
                .iter()
                .enumerate()
                .map(|(tick, v)| [tick as f64 / 60.0, v.value()])
                .collect(),
        });
    }
    OutlookDayModel {
        has_storage: day.forecast.has_storage,
        title: l10n.args(
            "outlook-day-title",
            &[("month", format::month_year(month).into())],
        ),
        help: l10n.text("outlook-day-help"),
        summary: l10n.args(
            "outlook-day-summary",
            &[
                ("balance", format::megawatts(day.worst_balance).into()),
                ("time", format::clock(day.worst_minute).into()),
                (
                    "duration",
                    format::duration(domain::time::SimDuration::from_ticks(day.shortfall_minutes))
                        .into(),
                ),
                ("energy", format::megawatt_hours(day.unserved_energy).into()),
            ],
        ),
        has_shortfall: day.shortfall_minutes > 0,
        power_axis: l10n.text("forecast-axis-power"),
        energy_axis: l10n.text("outlook-energy-axis"),
        hour_axis: l10n.text("forecast-axis-hour"),
        series,
        energy: OutlookDaySeries {
            label: l10n.text("outlook-day-stored"),
            color: TextTone::Information.color(),
            values: curve(&|p| p.battery_deliverable_energy.value()),
        },
        back: ButtonModel::new(
            l10n.text("outlook-back"),
            UiAction::SelectOutlookMonth(None),
        ),
        close: ButtonModel::new(l10n.text("outlook-close"), UiAction::ShowOutlook(false)),
    }
}

pub fn draw_outlook_day(ui: &mut egui::Ui, model: &OutlookDayModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let popup_was_open = egui::Popup::is_any_open(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.heading(&model.title);
        button(ui, &model.back, &mut actions);
        button(ui, &model.close, &mut actions);
    });
    ui.colored_label(
        if model.has_shortfall {
            TextTone::Negative.color()
        } else {
            TextTone::Positive.color()
        },
        &model.summary,
    );
    ui.horizontal_wrapped(|ui| {
        for s in &model.series {
            ui.colored_label(s.color, &s.label);
        }
    });
    Plot::new("outlook-day-power")
        .height(260.0)
        .include_y(0.0)
        .include_x(0.0)
        .include_x(24.0)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .custom_y_axes(vec![super::chart_style::y_axis(&model.power_axis)])
        .custom_x_axes(vec![super::chart_style::x_axis(&model.hour_axis)])
        .show(ui, |plot| {
            for (i, s) in model.series.iter().enumerate() {
                plot.line(
                    Line::new(&s.label, s.values.clone())
                        .color(s.color)
                        .width(if i < 2 { 2.5 } else { 1.0 }),
                );
            }
        });
    if model.has_storage {
        // The empty-energy graph is also useful for storage fleets that cannot recharge.
        draw_energy(ui, model);
    }
    ui.label(&model.help);
    if !popup_was_open && super::action::cancelled(ui, &model.back) {
        return vec![model.back.action.clone()];
    }
    actions
}

fn draw_energy(ui: &mut egui::Ui, model: &OutlookDayModel) {
    ui.label(&model.energy.label);
    Plot::new("outlook-day-energy")
        .height(130.0)
        .include_y(0.0)
        .include_x(0.0)
        .include_x(24.0)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .custom_y_axes(vec![super::chart_style::y_axis(&model.energy_axis)])
        .custom_x_axes(vec![super::chart_style::x_axis(&model.hour_axis)])
        .show(ui, |plot| {
            plot.line(
                Line::new(&model.energy.label, model.energy.values.clone())
                    .color(model.energy.color),
            );
        });
}
