//! Read-only, full-utilization investment comparison from the current region's plant catalog.
use application::{
    Game,
    projections::{PlantPayoff, plant_payoff},
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::{
    ids::CellCoord,
    region::{PlantKind, SitingRule},
    units::Money,
};
use egui_plot::{HLine, Line, Plot, Points};

use super::tutorial::{TutorialAnchor, mark_visible_anchor};
use super::{ActionSink, ButtonModel, Selection, UiAction, button, dispatch};
use crate::{
    l10n::{
        Localize, Localizer,
        format::{self, MoneyStyle},
    },
    sim::Session,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PayoffRow {
    pub kind: PlantKind,
    pub name: String,
    pub cash: PlantPayoff,
    pub values: Vec<String>,
    /// Plot coordinates: years since ordering and cumulative cents.
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PayoffModel {
    pub title: String,
    pub context: String,
    pub price: String,
    pub assumptions: Vec<String>,
    pub axis_time: String,
    pub axis_cash: String,
    pub zero: String,
    pub markers: String,
    pub horizon_label: String,
    pub horizon_years: f64,
    pub headings: Vec<String>,
    pub rows: Vec<PayoffRow>,
    pub money_style: MoneyStyle,
    pub close: ButtonModel,
}

pub fn payoff_model(
    game: &Game,
    l10n: &Localizer,
    cell: Option<CellCoord>,
    style: &MoneyStyle,
) -> PayoffModel {
    let session = game.session();
    let region = session.region();
    let cell = cell.filter(|c| region.terrain().get(*c).is_some_and(|c| c.land));
    let tariff = session.company_view().tariff;
    let years = |months: f64| {
        l10n.args(
            "payoff-years",
            &[("years", format!("{:.2}", months / 12.0).into())],
        )
    };
    let mut rows: Vec<_> = region.catalog().iter().filter(|spec| {
        spec.kind != PlantKind::Geothermal || (session.geothermal_available()
            && region.geothermal().is_some()
            && matches!(spec.siting, SitingRule::GeothermalSite { min } if region.terrain().has_geothermal(min)))
    }).filter_map(|base| {
        let spec = cell.and_then(|cell| region.plant_spec_at(base.kind, cell).ok()).unwrap_or_else(|| base.clone());
        let cash = plant_payoff(&spec, tariff, region.geothermal())?;
        let name = spec.kind.localize(l10n);
        let mut values = vec![name.clone(), format::money(spec.capital_cost, style),
            l10n.args("build-months-value", &[("months", spec.build_months.into())])];
        values.extend([cash.monthly_revenue, cash.monthly_fuel, cash.monthly_upkeep,
            cash.monthly_net].map(|m| format::money(m, style)));
        values.push(cash.payback_months.map(years).unwrap_or_else(|| l10n.text("payoff-never")));
        Some(PayoffRow { kind: spec.kind, name, cash, values, points: Vec::new() })
    }).collect();
    // Fit all recoverable investments, with a bounded comparison window for very small margins.
    let horizon_years = rows
        .iter()
        .filter_map(|r| r.cash.payback_months)
        .map(|m| (m / 12.0 * 1.1).ceil())
        .fold(10.0, f64::max)
        .clamp(10.0, 100.0)
        .max(
            rows.iter()
                .map(|r| r.cash.construction.len() as f64 / 12.0)
                .fold(0.0, f64::max),
        );
    for row in &mut rows {
        let end = horizon_years * 12.0;
        let mut months: Vec<f64> = (0..=row.cash.construction.len())
            .map(|m| m as f64)
            .collect();
        if let Some(payback) = row.cash.payback_months.filter(|m| *m < end) {
            months.push(payback);
        }
        months.push(end);
        row.points = months
            .into_iter()
            .map(|m| [m / 12.0, row.cash.balance_at(m)])
            .collect();
    }
    let context = cell
        .and_then(|c| region.economic_zone_at(c).ok().map(|z| (c, z)))
        .map(|(c, zone)| {
            l10n.args(
                "payoff-cell",
                &[
                    ("x", u32::from(c.x).into()),
                    ("y", u32::from(c.y).into()),
                    (
                        "zone",
                        super::economics::zone_name(region, zone, l10n).into(),
                    ),
                ],
            )
        })
        .unwrap_or_else(|| l10n.text("payoff-regional"));
    PayoffModel {
        title: l10n.text("payoff-title"),
        context,
        price: l10n.args(
            "payoff-price",
            &[("price", format::price(tariff, style).into())],
        ),
        assumptions: [
            "payoff-assumptions",
            "payoff-renewables",
            "payoff-geothermal",
        ]
        .map(|k| l10n.text(k))
        .to_vec(),
        axis_time: l10n.text("payoff-axis-time"),
        axis_cash: l10n.text("payoff-axis-cash"),
        zero: l10n.text("payoff-zero"),
        markers: l10n.text("payoff-markers"),
        horizon_label: l10n.args(
            "payoff-horizon",
            &[("years", format!("{horizon_years:.0}").into())],
        ),
        horizon_years,
        headings: [
            "payoff-plant",
            "build-capital",
            "build-months",
            "payoff-revenue",
            "payoff-fuel",
            "payoff-upkeep",
            "payoff-net",
            "payoff-recovery",
        ]
        .map(|k| l10n.text(k))
        .to_vec(),
        rows,
        money_style: style.clone(),
        close: ButtonModel::new(l10n.text("payoff-close"), UiAction::ShowPayoff(false)),
    }
}

pub fn draw_payoff(ui: &mut egui::Ui, model: &PayoffModel) -> Vec<UiAction> {
    draw_payoff_revealed(ui, model, None)
}

fn draw_payoff_revealed(
    ui: &mut egui::Ui,
    model: &PayoffModel,
    reveal: Option<TutorialAnchor>,
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let popup_was_open = egui::Popup::is_any_open(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.heading(&model.title);
        let close = button(ui, &model.close, &mut actions);
        payoff_anchor(
            ui,
            TutorialAnchor::PayoffClose,
            close.rect,
            reveal,
            &mut actions,
        );
    });
    ui.label(&model.context);
    ui.label(&model.price);
    let plot_id = ui.make_persistent_id("plant-payoff-plot");
    let hovered = super::chart_style::draw_key(
        ui,
        plot_id,
        model.rows.iter().map(|row| {
            (
                row.name.as_str(),
                super::map::color32(super::map::kind_color(row.kind)),
                super::chart_style::KeyMark::Line,
            )
        }),
    );
    let style = model.money_style.clone();
    let chart = Plot::new("plant-payoff-plot")
        .id(plot_id)
        .height(260.0)
        .include_x(0.0)
        .include_x(model.horizon_years)
        .include_y(0.0)
        .allow_scroll(false)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .custom_x_axes(vec![super::chart_style::x_axis(&model.axis_time)])
        .custom_y_axes(vec![
            super::chart_style::y_axis(&model.axis_cash).formatter(move |mark, _| {
                format::money(Money::from_cents(mark.value.round() as i64), &style)
            }),
        ])
        .label_formatter({
            let style = model.money_style.clone();
            let time = model.axis_time.clone();
            move |hover| {
                let egui_plot::HoverPosition::NearDataPoint {
                    plot_name,
                    position,
                    ..
                } = hover
                else {
                    return None;
                };
                Some(format!(
                    "{plot_name}\n{time}: {:.2}\n{}",
                    position.x,
                    format::money(Money::from_cents(position.y.round() as i64), &style)
                ))
            }
        })
        .show(ui, |plot| {
            plot.hline(HLine::new(&model.zero, 0.0).color(super::theme::TEXT));
            for row in &model.rows {
                let color = super::map::color32(super::map::kind_color(row.kind));
                let id = egui::Id::new(&row.name);
                plot.line(
                    Line::new(&row.name, row.points.clone())
                        .color(color)
                        .width(if hovered == Some(id) { 3.5 } else { 2.0 }),
                );
                let build = row.cash.construction.len() as f64;
                let mut markers = vec![[build / 12.0, row.cash.balance_at(build)]];
                if let Some(m) = row
                    .cash
                    .payback_months
                    .filter(|m| *m <= model.horizon_years * 12.0)
                {
                    markers.push([m / 12.0, 0.0]);
                }
                plot.points(
                    Points::new(&row.name, markers)
                        .id(id)
                        .color(color)
                        .radius(4.0),
                );
            }
        });
    payoff_anchor(
        ui,
        TutorialAnchor::PayoffGraph,
        chart.response.rect,
        reveal,
        &mut actions,
    );
    ui.label(&model.markers);
    ui.label(&model.horizon_label);
    let table = egui::ScrollArea::horizontal()
        .id_salt("payoff-table-scroll")
        .show(ui, |ui| {
            egui::Grid::new("payoff-table")
                .striped(true)
                .show(ui, |ui| {
                    for heading in &model.headings {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    for row in &model.rows {
                        for (i, value) in row.values.iter().enumerate() {
                            if i == 6 || (i == 7 && row.cash.payback_months.is_none()) {
                                ui.colored_label(
                                    super::theme::TextTone::cash(row.cash.monthly_net.cents())
                                        .color(),
                                    value,
                                );
                            } else {
                                ui.label(value);
                            }
                        }
                        ui.end_row();
                    }
                })
        });
    payoff_anchor(
        ui,
        TutorialAnchor::PayoffTable,
        table.inner.response.rect,
        reveal,
        &mut actions,
    );
    for assumption in &model.assumptions {
        ui.label(assumption);
    }
    if !popup_was_open && super::action::cancelled(ui, &model.close) {
        return vec![model.close.action.clone()];
    }
    actions
}

fn payoff_anchor(
    ui: &egui::Ui,
    anchor: TutorialAnchor,
    rect: egui::Rect,
    reveal: Option<TutorialAnchor>,
    actions: &mut Vec<UiAction>,
) {
    mark_visible_anchor(ui, anchor, rect);
    if reveal == Some(anchor) {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
        actions.push(UiAction::Tutorial(super::TutorialAction::RevealDone));
    }
}

pub(super) fn payoff_panel(
    mut contexts: EguiContexts,
    session: Res<Session>,
    l10n: Res<Localizer>,
    style: Res<MoneyStyle>,
    mut sink: ActionSink,
) -> Result {
    if !sink.ui_state().payoff_open || sink.open_prompt().0.is_some() {
        return Ok(());
    }
    let cell = match sink.selection() {
        Selection::Cell(cell) => Some(cell),
        _ => None,
    };
    let model = payoff_model(&session.0, &l10n, cell, &style);
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect().size();
    let reveal = sink.ui_state().reveal_anchor;
    let mut actions = Vec::new();
    egui::Modal::new(egui::Id::new("payoff-dialog"))
        .frame(
            egui::Frame::popup(&ctx.global_style())
                .stroke(egui::Stroke::new(2.0, super::theme::heading_color())),
        )
        .show(ctx, |ui| {
            ui.set_width((screen.x - 64.0).clamp(220.0, 1100.0));
            egui::ScrollArea::vertical()
                .max_height((screen.y - 80.0).max(120.0))
                .show(ui, |ui| {
                    actions = draw_payoff_revealed(ui, &model, reveal);
                });
        });
    dispatch(actions, &mut sink);
    Ok(())
}
