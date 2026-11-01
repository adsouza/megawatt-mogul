//! Read-only yearly planning, kept separate from the representative day's dispatch forecast.
use super::{ActionSink, ButtonModel, UiAction, button, dispatch, theme::TextTone};
use crate::{
    l10n::{Localizer, Names, format},
    sim::{Session, SessionGeneration},
};
use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::{
    error::DomainError,
    ids::CityId,
    session::{CapacityEstimate, YearAheadOutlook},
};
use egui_plot::{Line, Plot};

#[derive(Debug, Clone, PartialEq)]
pub struct OutlookRow {
    pub month: String,
    pub baseline_peak: f64,
    pub peak: f64,
    pub existing: f64,
    pub scheduled: f64,
    pub firm: f64,
    pub values: Vec<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct OutlookModel {
    pub title: String,
    pub help: String,
    pub power_label: String,
    pub route: String,
    pub city_label: String,
    pub city: Option<CityId>,
    pub cities: Vec<(Option<CityId>, String)>,
    pub headings: Vec<String>,
    pub series: Vec<String>,
    pub rows: Vec<OutlookRow>,
    pub close: ButtonModel,
}

/// Cities with existing or ordered paths belong to the baseline, not the what-if chooser.
pub(super) fn preview_city(
    session: &domain::session::GameSession,
    city: Option<CityId>,
) -> Option<CityId> {
    city.filter(|id| {
        session.power().cities().contains_key(id) && session.city_connection_months(*id).is_none()
    })
}

pub fn outlook_model(
    game: &Game,
    l10n: &Localizer,
    city: Option<CityId>,
) -> Result<OutlookModel, DomainError> {
    let city = preview_city(game.session(), city);
    let outlook = game.session().year_ahead_outlook(city)?;
    Ok(model_from(game, l10n, &outlook))
}
fn model_from(game: &Game, l10n: &Localizer, outlook: &YearAheadOutlook) -> OutlookModel {
    let session = game.session();
    let names = Names::new(l10n, session);
    let mut cities = vec![(None, l10n.text("outlook-current-grid"))];
    cities.extend(
        session
            .city_views()
            .iter()
            .filter(|c| preview_city(session, Some(c.id)).is_some())
            .map(|c| (Some(c.id), names.city(c.id))),
    );
    let route = match outlook.connection {
        None => l10n.text("outlook-select-city"),
        Some(c) if c.already_scheduled => l10n.args(
            "outlook-already-ordered",
            &[("city", names.city(c.city).into())],
        ),
        Some(c) => match (c.from, c.months) {
            (Some(from), Some(months)) => {
                let first = outlook.months[0].month;
                let ready = first
                    .checked_add_months(months)
                    .ok()
                    .map(format::month_year)
                    .unwrap_or_default();
                let source = session
                    .power()
                    .network()
                    .node(from)
                    .map(|node| match node.kind {
                        domain::grid::NodeKind::City(id) => names.city(id),
                        domain::grid::NodeKind::Plant(id) => names.plant(id),
                    })
                    .unwrap_or_default();
                l10n.args(
                    "outlook-route",
                    &[
                        ("city", names.city(c.city).into()),
                        ("source", source.into()),
                        (
                            "time",
                            l10n.args("build-months-value", &[("months", months.into())])
                                .into(),
                        ),
                        ("month", ready.into()),
                    ],
                )
            }
            _ => l10n.args("outlook-no-route", &[("city", names.city(c.city).into())]),
        },
    };
    let rows = outlook
        .months
        .iter()
        .map(|point| {
            let CapacityEstimate {
                peak_demand,
                existing_capacity,
                scheduled_capacity,
                firm_capacity,
            } = point.with_city.unwrap_or(point.baseline);
            let month = format::month_year_short(point.month);
            let mut values = vec![month.clone(), format::megawatts(point.baseline.peak_demand)];
            if outlook.connection.is_some() {
                values.push(format::megawatts(peak_demand));
            }
            values.extend([
                format::megawatts(existing_capacity),
                format::megawatts(scheduled_capacity),
                format::megawatts(existing_capacity + scheduled_capacity),
                format::megawatts(firm_capacity),
                format::megawatts(firm_capacity - peak_demand),
            ]);
            OutlookRow {
                month,
                baseline_peak: point.baseline.peak_demand.value(),
                peak: peak_demand.value(),
                existing: existing_capacity.value(),
                scheduled: scheduled_capacity.value(),
                firm: firm_capacity.value(),
                values,
            }
        })
        .collect();
    let mut headings = vec![l10n.text("outlook-month"), l10n.text("outlook-demand")];
    if outlook.connection.is_some() {
        headings.push(l10n.text("outlook-with-city"));
    }
    headings.extend(
        [
            "outlook-existing",
            "outlook-scheduled",
            "outlook-total",
            "outlook-firm",
            "outlook-headroom",
        ]
        .map(|key| l10n.text(key)),
    );
    OutlookModel {
        title: l10n.text("outlook-title"),
        help: l10n.text("outlook-help"),
        power_label: l10n.text("forecast-axis-power"),
        route,
        city_label: l10n.text("outlook-city"),
        city: outlook.connection.map(|c| c.city),
        cities,
        rows,
        headings,
        series: [
            "outlook-demand",
            "outlook-with-city",
            "outlook-existing",
            "outlook-total",
            "outlook-firm",
        ]
        .map(|key| l10n.text(key))
        .to_vec(),
        close: ButtonModel::new(l10n.text("outlook-close"), UiAction::ShowOutlook(false)),
    }
}

pub fn draw_outlook(ui: &mut egui::Ui, model: &OutlookModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let popup_was_open = egui::Popup::is_any_open(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.heading(&model.title);
        button(ui, &model.close, &mut actions);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(&model.city_label);
        let selected = model
            .cities
            .iter()
            .find(|(id, _)| *id == model.city)
            .map(|(_, name)| name.as_str())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("outlook-city")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (id, name) in &model.cities {
                    if ui.selectable_label(*id == model.city, name).clicked() {
                        actions.push(UiAction::PreviewOutlookCity(*id));
                    }
                }
            });
    });
    ui.label(&model.route);
    let colors = [
        TextTone::Caution.color(),
        TextTone::Negative.color(),
        TextTone::Information.color(),
        TextTone::Positive.color(),
        super::theme::heading_color(),
    ];
    ui.horizontal_wrapped(|ui| {
        for (i, label) in model
            .series
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1 || model.city.is_some())
        {
            ui.colored_label(colors[i], label);
        }
    });
    let labels: Vec<_> = model.rows.iter().map(|r| r.month.clone()).collect();
    Plot::new("year-ahead-outlook")
        .height(210.0)
        .include_y(0.0)
        .include_x(0.0)
        .include_x(11.0)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .custom_y_axes(vec![super::chart_style::y_axis(&model.power_label)])
        .custom_x_axes(vec![super::chart_style::x_axis("").formatter(
            move |mark, _| {
                if mark.value < 0.0 || mark.value > 11.0 {
                    return String::new();
                }
                labels
                    .get(mark.value.round() as usize)
                    .cloned()
                    .unwrap_or_default()
            },
        )])
        .show(ui, |plot| {
            for (i, (name, color)) in model.series.iter().zip(colors).enumerate() {
                if i == 1 && model.city.is_none() {
                    continue;
                }
                let values: Vec<[f64; 2]> = model
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(m, r)| {
                        [
                            m as f64,
                            match i {
                                0 => r.baseline_peak,
                                1 => r.peak,
                                2 => r.existing,
                                3 => r.existing + r.scheduled,
                                _ => r.firm,
                            },
                        ]
                    })
                    .collect();
                plot.line(
                    Line::new(name.clone(), values)
                        .color(color)
                        .width(if i < 2 { 2.5 } else { 1.5 }),
                );
            }
        });
    // The table makes the chart's exact numbers readable without hover and supports narrow screens.
    egui::ScrollArea::horizontal()
        .id_salt("outlook-table")
        .show(ui, |ui| {
            egui::Grid::new("outlook-values")
                .striped(true)
                .show(ui, |ui| {
                    for heading in &model.headings {
                        ui.label(egui::RichText::new(heading).strong());
                    }
                    ui.end_row();
                    for row in &model.rows {
                        for (i, value) in row.values.iter().enumerate() {
                            let color = if i + 1 == row.values.len() && row.firm < row.peak {
                                TextTone::Negative.color()
                            } else {
                                TextTone::Neutral.color()
                            };
                            ui.colored_label(color, value);
                        }
                        ui.end_row();
                    }
                });
        });
    ui.label(&model.help);
    if !popup_was_open && super::action::cancelled(ui, &model.close) {
        return vec![model.close.action.clone()];
    }
    actions
}

#[derive(Default)]
pub(super) struct OutlookCache {
    source: Option<(u32, u64, Option<CityId>)>,
    value: Option<Result<YearAheadOutlook, DomainError>>,
}
pub(super) fn outlook_panel(
    mut contexts: EguiContexts,
    session: Res<Session>,
    generation: Res<SessionGeneration>,
    l10n: Res<Localizer>,
    mut cached: Local<OutlookCache>,
    mut sink: ActionSink,
) -> Result {
    if !sink.ui_state().outlook_open || sink.open_prompt().0.is_some() {
        return Ok(());
    }
    let source = (
        session.last_changed().get(),
        generation.0,
        preview_city(session.0.session(), sink.ui_state().outlook_city),
    );
    if cached.source != Some(source) {
        cached.value = Some(session.0.session().year_ahead_outlook(source.2));
        cached.source = Some(source);
    }
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect().size();
    let mut actions = Vec::new();
    egui::Modal::new(egui::Id::new("outlook-dialog")).show(ctx, |ui| {
        ui.set_width((screen.x - 64.0).clamp(220.0, 920.0));
        egui::ScrollArea::vertical()
            .max_height((screen.y - 80.0).max(120.0))
            .show(ui, |ui| match cached.value.as_ref() {
                Some(Ok(outlook)) => {
                    actions = draw_outlook(ui, &model_from(&session.0, &l10n, outlook))
                }
                _ => {
                    ui.label(l10n.text("outlook-unavailable"));
                    let close =
                        ButtonModel::new(l10n.text("outlook-close"), UiAction::ShowOutlook(false));
                    button(ui, &close, &mut actions);
                    if super::action::cancelled(ui, &close) {
                        actions.push(close.action);
                    }
                }
            });
    });
    dispatch(actions, &mut sink);
    Ok(())
}
