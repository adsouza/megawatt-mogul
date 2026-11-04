//! Read-only construction cash scenarios from the actual payment queues and recent results.
use application::{
    Game,
    projections::{CashOutlook, cash_outlook},
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::{error::DomainError, ids::CellCoord, region::PlantKind, units::Money};
use egui_plot::{FilledArea, HLine, Line, Plot, VLine};

use super::{
    ActionSink, ButtonModel, UiAction, button, dispatch,
    theme::{self, TextTone},
};
use crate::{
    l10n::{
        Localize, Localizer,
        format::{self, MoneyStyle},
    },
    sim::Session,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CashOutlookModel {
    pub forecast: CashOutlook,
    pub title: String,
    pub chooser: String,
    pub choices: Vec<(Option<PlantKind>, String)>,
    pub plant: Option<PlantKind>,
    pub context: String,
    pub history: String,
    pub summary: String,
    pub risk: Option<String>,
    pub assumptions: Vec<String>,
    pub labels: [String; 5],
    pub dates: Vec<String>,
    pub headings: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub tariffs: Vec<String>,
    pub style: MoneyStyle,
    pub close: ButtonModel,
}

pub fn cash_outlook_model(
    game: &Game,
    l10n: &Localizer,
    cell: Option<CellCoord>,
    plant: Option<PlantKind>,
    style: &MoneyStyle,
) -> Result<CashOutlookModel, DomainError> {
    let session = game.session();
    let mut choices = vec![(None, l10n.text("cash-committed"))];
    if let Some(cell) = cell {
        choices.extend(
            PlantKind::ALL
                .into_iter()
                .filter(|kind| session.plant_quote(*kind, cell).is_ok())
                .map(|kind| (Some(kind), kind.localize(l10n))),
        );
    }
    let plant = plant.filter(|kind| choices.iter().any(|(choice, _)| *choice == Some(*kind)));
    let quote = plant
        .zip(cell)
        .map(|(kind, cell)| session.plant_quote(kind, cell))
        .transpose()?;
    let forecast = cash_outlook(session, game.projections().monthly_reports.history(), quote)?;
    let history = match &forecast.income {
        None => l10n.text("cash-no-history"),
        Some(range) => l10n.args(
            "cash-history",
            &[
                ("count", (range.samples as u32).into()),
                ("low", format::money(range.low, style).into()),
                ("mean", format::money(range.mean, style).into()),
                ("high", format::money(range.high, style).into()),
            ],
        ),
    };
    let lowest = forecast
        .months
        .iter()
        .min_by_key(|m| m.mean)
        .expect("at least twelve months");
    let summary = l10n.args(
        "cash-lowest",
        &[
            ("cash", format::money(lowest.mean, style).into()),
            ("month", format::month_year(lowest.month).into()),
        ],
    );
    let risk = forecast
        .months
        .iter()
        .find(|m| m.low < Money::ZERO)
        .map(|m| {
            l10n.args(
                "cash-risk",
                &[("month", format::month_year(m.month).into())],
            )
        });
    let mut dates = vec![l10n.text("cash-now")];
    dates.extend(forecast.months.iter().map(|m| format::month_year(m.month)));
    let rows = forecast
        .months
        .iter()
        .map(|m| {
            let mut row = vec![format::month_year(m.month)];
            row.extend(
                [m.committed, m.proposed, m.low, m.mean, m.high].map(|v| format::money(v, style)),
            );
            row
        })
        .collect();
    let tariffs = forecast
        .tariff_changes
        .iter()
        .map(|change| {
            l10n.args(
                "cash-tariff-range",
                &[
                    ("month", format::month_year(change.month).into()),
                    ("low", format::price(change.prices[0], style).into()),
                    ("mean", format::price(change.prices[1], style).into()),
                    ("high", format::price(change.prices[2], style).into()),
                ],
            )
        })
        .collect();
    Ok(CashOutlookModel {
        title: l10n.text("cash-title"),
        chooser: l10n.text("cash-preview"),
        choices,
        plant,
        context: cell
            .map(|cell| {
                l10n.args(
                    "cash-cell",
                    &[
                        ("x", u32::from(cell.x).into()),
                        ("y", u32::from(cell.y).into()),
                    ],
                )
            })
            .unwrap_or_else(|| l10n.text("cash-pick-cell")),
        history,
        summary,
        risk,
        assumptions: [
            "cash-scenarios",
            "cash-operations",
            "cash-payments",
            "cash-tariff-help",
        ]
        .map(|key| l10n.text(key))
        .to_vec(),
        labels: [
            "cash-baseline",
            "cash-proposed",
            "cash-capital-only",
            "cash-range",
            "cash-zero",
        ]
        .map(|key| l10n.text(key)),
        headings: [
            "outlook-month",
            "cash-existing-payments",
            "cash-new-payments",
            "cash-low",
            "cash-mean",
            "cash-high",
        ]
        .map(|key| l10n.text(key))
        .to_vec(),
        dates,
        rows,
        tariffs,
        forecast,
        style: style.clone(),
        close: ButtonModel::new(l10n.text("cash-close"), UiAction::CloseCashOutlook),
    })
}

pub fn draw_cash_outlook(ui: &mut egui::Ui, model: &CashOutlookModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let popup_was_open = egui::Popup::is_any_open(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.heading(&model.title);
        button(ui, &model.close, &mut actions);
    });
    ui.label(&model.context);
    let mut plant = model.plant;
    ui.label(&model.chooser);
    egui::ComboBox::from_id_salt("cash-plant")
        .selected_text(
            model
                .choices
                .iter()
                .find(|(kind, _)| *kind == plant)
                .map(|(_, name)| name.as_str())
                .unwrap_or(""),
        )
        .show_ui(ui, |ui| {
            for (kind, name) in &model.choices {
                ui.selectable_value(&mut plant, *kind, name);
            }
        });
    if plant != model.plant {
        actions.push(UiAction::PreviewCashPlant(plant));
    }
    ui.label(&model.history);
    ui.colored_label(
        if model.forecast.months.iter().any(|m| m.mean < Money::ZERO) {
            TextTone::Negative.color()
        } else {
            theme::TEXT
        },
        &model.summary,
    );
    if let Some(risk) = &model.risk {
        ui.colored_label(TextTone::Caution.color(), risk);
    }
    let current = model.forecast.current.cents() as f64;
    let curve = |value: fn(&application::projections::CashMonth) -> Money| {
        std::iter::once([0.0, current])
            .chain(
                model
                    .forecast
                    .months
                    .iter()
                    .enumerate()
                    .map(|(i, month)| [(i + 1) as f64, value(month).cents() as f64]),
            )
            .collect::<Vec<_>>()
    };
    let low = curve(|m| m.low);
    let high = curve(|m| m.high);
    let x: Vec<_> = low.iter().map(|p| p[0]).collect();
    let lower: Vec<_> = low.iter().map(|p| p[1]).collect();
    let upper: Vec<_> = high.iter().map(|p| p[1]).collect();
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            TextTone::Information.color(),
            if model.plant.is_some() {
                &model.labels[1]
            } else {
                &model.labels[0]
            },
        );
        ui.colored_label(TextTone::Caution.color(), &model.labels[2]);
        ui.label(&model.labels[3]);
    });
    let style = model.style.clone();
    let dates = model.dates.clone();
    Plot::new("cash-outlook-plot")
        .height(260.0)
        .include_y(0.0)
        .include_x(0.0)
        .include_x(model.forecast.months.len() as f64)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_zoom(false)
        .custom_y_axes(vec![super::chart_style::y_axis("").formatter(
            move |mark, _| format::money(Money::from_cents(mark.value.round() as i64), &style),
        )])
        .custom_x_axes(vec![super::chart_style::x_axis("").formatter(
            move |mark, _| {
                if mark.value >= 0.0 {
                    dates
                        .get(mark.value.round() as usize)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            },
        )])
        .label_formatter({
            let style = model.style.clone();
            let dates = model.dates.clone();
            move |hover| {
                let egui_plot::HoverPosition::NearDataPoint {
                    plot_name,
                    position,
                    ..
                } = hover
                else {
                    return None;
                };
                let date = if position.x >= 0.0 {
                    dates
                        .get(position.x.round() as usize)
                        .map(String::as_str)
                        .unwrap_or("")
                } else {
                    ""
                };
                Some(format!(
                    "{plot_name}\n{date}\n{}",
                    format::money(Money::from_cents(position.y.round() as i64), &style)
                ))
            }
        })
        .show(ui, |plot| {
            plot.add(
                FilledArea::new(&model.labels[3], &x, &lower, &upper)
                    .fill_color(egui::Color32::from_rgba_unmultiplied(50, 160, 255, 65))
                    .allow_hover(false),
            );
            plot.hline(
                HLine::new(&model.labels[4], 0.0)
                    .color(TextTone::Negative.color())
                    .width(2.0),
            );
            plot.line(
                Line::new(&model.labels[2], curve(|m| m.capital_only))
                    .color(TextTone::Caution.color()),
            );
            if model.plant.is_some() {
                plot.line(Line::new(&model.labels[0], curve(|m| m.baseline)).color(theme::TEXT));
            }
            plot.line(
                Line::new(
                    if model.plant.is_some() {
                        &model.labels[1]
                    } else {
                        &model.labels[0]
                    },
                    curve(|m| m.mean),
                )
                .color(TextTone::Information.color())
                .width(2.5),
            );
            for (i, month) in model.forecast.months.iter().enumerate() {
                if model
                    .forecast
                    .tariff_changes
                    .iter()
                    .any(|change| change.month == month.month)
                {
                    plot.vline(VLine::new(&model.dates[i + 1], i as f64).color(theme::TEXT));
                }
            }
        });
    for note in &model.tariffs {
        ui.label(note);
    }
    for assumption in &model.assumptions {
        ui.label(assumption);
    }
    egui::ScrollArea::horizontal()
        .id_salt("cash-table-scroll")
        .show(ui, |ui| {
            egui::Grid::new("cash-outlook-table")
                .striped(true)
                .show(ui, |ui| {
                    for heading in &model.headings {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    for row in &model.rows {
                        for value in row {
                            ui.label(value);
                        }
                        ui.end_row();
                    }
                });
        });
    if !popup_was_open && super::action::cancelled(ui, &model.close) {
        return vec![UiAction::CloseCashOutlook];
    }
    actions
}

pub(super) fn cash_outlook_panel(
    mut contexts: EguiContexts,
    session: Res<Session>,
    l10n: Res<Localizer>,
    style: Res<MoneyStyle>,
    mut sink: ActionSink,
) -> Result {
    if !sink.ui_state().cash_open || sink.open_prompt().0.is_some() {
        return Ok(());
    }
    let model = cash_outlook_model(
        &session.0,
        &l10n,
        sink.ui_state().cash_cell,
        sink.ui_state().cash_plant,
        &style,
    );
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect().size();
    let mut actions = Vec::new();
    egui::Modal::new(egui::Id::new("cash-outlook-dialog"))
        .frame(
            egui::Frame::popup(&ctx.global_style())
                .stroke(egui::Stroke::new(2.0, theme::heading_color())),
        )
        .show(ctx, |ui| {
            ui.set_width((screen.x - 64.0).clamp(220.0, 1100.0));
            egui::ScrollArea::vertical()
                .max_height((screen.y - 80.0).max(120.0))
                .show(ui, |ui| match &model {
                    Ok(model) => actions = draw_cash_outlook(ui, model),
                    Err(_) => {
                        ui.label(l10n.text("cash-unavailable"));
                        let close =
                            ButtonModel::new(l10n.text("cash-close"), UiAction::CloseCashOutlook);
                        button(ui, &close, &mut actions);
                        if !egui::Popup::is_any_open(ui.ctx())
                            && super::action::cancelled(ui, &close)
                        {
                            actions = vec![UiAction::CloseCashOutlook];
                        }
                    }
                });
        });
    dispatch(actions, &mut sink);
    Ok(())
}
