//! Read-only yearly planning, kept separate from the representative day's dispatch forecast.
use super::TutorialAction;
use super::tutorial::{TutorialAnchor, mark_visible_anchor};
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
use egui_plot::{FilledArea, Line, Plot};

fn cushion_green() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(40, 220, 90, 80)
}
fn shortfall_red() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(255, 25, 25, 185)
}

/// Crossings are explicit vertices, so green and red fills never overlap between months.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CapacityBands {
    pub x: Vec<f64>,
    pub demand: Vec<f64>,
    pub dependable: Vec<f64>,
    pub upper: Vec<f64>,
}
impl CapacityBands {
    fn push(&mut self, x: f64, demand: f64, dependable: f64) {
        self.x.push(x);
        self.demand.push(demand);
        self.dependable.push(dependable);
        self.upper.push(demand.max(dependable));
    }
    fn of(points: &[(f64, f64)]) -> Self {
        let mut bands = Self::default();
        let Some(&(demand, capacity)) = points.first() else {
            return bands;
        };
        bands.push(0.0, demand, capacity);
        for (i, pair) in points.windows(2).enumerate() {
            let a = pair[0].1 - pair[0].0;
            let b = pair[1].1 - pair[1].0;
            if (a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0) {
                let t = a / (a - b);
                let y = pair[0].0 + t * (pair[1].0 - pair[0].0);
                bands.push(i as f64 + t, y, y);
            }
            bands.push((i + 1) as f64, pair[1].0, pair[1].1);
        }
        bands
    }
}

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
    pub bands: CapacityBands,
    pub cushion_label: String,
    pub shortfall_label: String,
    pub cushion_summary: String,
    pub has_shortfall: bool,
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
    let rows: Vec<OutlookRow> = outlook
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
    let bands = CapacityBands::of(&rows.iter().map(|r| (r.peak, r.firm)).collect::<Vec<_>>());
    let worst = rows
        .iter()
        .min_by(|a, b| (a.firm - a.peak).total_cmp(&(b.firm - b.peak)));
    let has_shortfall = worst.is_some_and(|row| row.firm < row.peak);
    let cushion_summary = worst
        .map(|row| {
            let margin = row.firm - row.peak;
            l10n.args(
                if margin < 0.0 {
                    "outlook-shortfall-summary"
                } else if margin == 0.0 {
                    "outlook-zero-summary"
                } else {
                    "outlook-cushion-summary"
                },
                &[
                    (
                        "amount",
                        format::megawatts(domain::units::Megawatts::new(margin.abs())).into(),
                    ),
                    ("month", row.month.clone().into()),
                ],
            )
        })
        .unwrap_or_default();
    OutlookModel {
        bands,
        has_shortfall,
        cushion_summary,
        cushion_label: l10n.text("outlook-cushion"),
        shortfall_label: l10n.text("outlook-shortfall"),
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
    draw_outlook_revealed(ui, model, None)
}
fn draw_outlook_revealed(
    ui: &mut egui::Ui,
    model: &OutlookModel,
    reveal: Option<TutorialAnchor>,
) -> Vec<UiAction> {
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
        let picker = egui::ComboBox::from_id_salt("outlook-city")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (id, name) in &model.cities {
                    if ui.selectable_label(*id == model.city, name).clicked() {
                        actions.push(UiAction::PreviewOutlookCity(*id));
                    }
                }
            });
        mark_visible_anchor(ui, TutorialAnchor::OutlookCity, picker.response.rect);
        if reveal == Some(TutorialAnchor::OutlookCity) {
            picker.response.scroll_to_me(Some(egui::Align::Center));
            actions.push(UiAction::Tutorial(TutorialAction::RevealDone));
        }
    });
    ui.label(&model.route);
    ui.colored_label(
        if model.has_shortfall {
            TextTone::Negative.color()
        } else {
            TextTone::Positive.color()
        },
        &model.cushion_summary,
    );
    let colors = [
        TextTone::Caution.color(),
        TextTone::Negative.color(),
        TextTone::Information.color(),
        TextTone::Positive.color(),
        super::theme::heading_color(),
    ];
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(TextTone::Positive.color(), &model.cushion_label);
        ui.colored_label(TextTone::Negative.color(), &model.shortfall_label);
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
    let chart = Plot::new("year-ahead-outlook")
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
            plot.add(
                FilledArea::new(
                    &model.cushion_label,
                    &model.bands.x,
                    &model.bands.demand,
                    &model.bands.upper,
                )
                .fill_color(cushion_green())
                .allow_hover(false),
            );
            plot.add(
                FilledArea::new(
                    &model.shortfall_label,
                    &model.bands.x,
                    &model.bands.dependable,
                    &model.bands.upper,
                )
                .fill_color(shortfall_red())
                .allow_hover(false),
            );
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
    mark_visible_anchor(ui, TutorialAnchor::OutlookGraph, chart.response.rect);
    if reveal == Some(TutorialAnchor::OutlookGraph) {
        chart.response.scroll_to_me(Some(egui::Align::Center));
        actions.push(UiAction::Tutorial(TutorialAction::RevealDone));
    }
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
    let reveal = sink.ui_state().reveal_anchor;
    egui::Modal::new(egui::Id::new("outlook-dialog"))
        .frame(
            egui::Frame::popup(&ctx.global_style())
                .stroke(egui::Stroke::new(2.0, super::theme::heading_color())),
        )
        .show(ctx, |ui| {
            ui.set_width((screen.x - 64.0).clamp(220.0, 920.0));
            egui::ScrollArea::vertical()
                .max_height((screen.y - 80.0).max(120.0))
                .show(ui, |ui| match cached.value.as_ref() {
                    Some(Ok(outlook)) => {
                        actions = draw_outlook_revealed(
                            ui,
                            &model_from(&session.0, &l10n, outlook),
                            reveal,
                        )
                    }
                    _ => {
                        ui.label(l10n.text("outlook-unavailable"));
                        let close = ButtonModel::new(
                            l10n.text("outlook-close"),
                            UiAction::ShowOutlook(false),
                        );
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacity_bands_split_at_crossings_without_hiding_a_shortage() {
        let bands = CapacityBands::of(&[(100.0, 200.0), (200.0, 100.0)]);
        assert_eq!(bands.x, vec![0.0, 0.5, 1.0]);
        assert_eq!(bands.demand, vec![100.0, 150.0, 200.0]);
        assert_eq!(bands.dependable, vec![200.0, 150.0, 100.0]);
        assert_eq!(bands.upper, vec![200.0, 150.0, 200.0]);
        assert_eq!(CapacityBands::of(&[(100.0, 100.0)]).upper, vec![100.0]);
        assert!(CapacityBands::of(&[]).x.is_empty());
    }
    proptest::proptest! {
        #[test]
        fn bands_are_ordered_exact_and_do_not_mix_surplus_with_shortfall(points in proptest::collection::vec((0.0f64..10000.0, 0.0f64..10000.0), 1..13)) {
            let bands = CapacityBands::of(&points);
            proptest::prop_assert_eq!(bands.x.len(), bands.demand.len());
            proptest::prop_assert_eq!(bands.x.len(), bands.dependable.len());
            proptest::prop_assert_eq!(bands.x.len(), bands.upper.len());
            for i in 0..bands.x.len() {
                proptest::prop_assert_eq!(bands.upper[i], bands.demand[i].max(bands.dependable[i]));
                if i > 0 {
                    proptest::prop_assert!(bands.x[i] > bands.x[i-1]);
                    let a = bands.dependable[i-1] - bands.demand[i-1];
                    let b = bands.dependable[i] - bands.demand[i];
                    proptest::prop_assert!(!((a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0)));
                }
            }
            for (i, &(demand, capacity)) in points.iter().enumerate() {
                let sample = bands.x.iter().position(|x| *x == i as f64).unwrap();
                proptest::prop_assert_eq!(bands.demand[sample], demand);
                proptest::prop_assert_eq!(bands.dependable[sample], capacity);
            }
        }
    }
}

#[cfg(test)]
mod paint_tests {
    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};
    use application::InMemoryCommandLog;

    #[test]
    fn outlook_paints_green_capacity_cushion_and_bright_red_shortfall() {
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut model = outlook_model(&game, &shipped_l10n(), None).unwrap();
        for (i, row) in model.rows.iter_mut().enumerate() {
            row.peak = if i < 6 { 100.0 } else { 300.0 };
            row.firm = 200.0;
            row.existing = 200.0;
            row.scheduled = 0.0;
            row.baseline_peak = row.peak;
        }
        model.bands = CapacityBands::of(
            &model
                .rows
                .iter()
                .map(|r| (r.peak, r.firm))
                .collect::<Vec<_>>(),
        );
        let ctx = egui::Context::default();
        let mut painted = Vec::new();
        for _ in 0..4 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1024.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    draw_outlook(ui, &model);
                },
            );
            painted = output.shapes.clone();
            output.drop_without_applying_deltas();
        }
        fn contains(shape: &egui::Shape, color: egui::Color32) -> bool {
            match shape {
                egui::Shape::Mesh(mesh) => mesh.vertices.iter().any(|v| v.color == color),
                egui::Shape::Vec(shapes) => shapes.iter().any(|s| contains(s, color)),
                _ => false,
            }
        }
        assert!(
            painted.iter().any(|s| contains(
                &s.shape,
                egui::Color32::from_rgba_unmultiplied(40, 220, 90, 80)
            )),
            "green surplus is actually filled on the graph"
        );
        assert!(
            painted.iter().any(|s| contains(
                &s.shape,
                egui::Color32::from_rgba_unmultiplied(255, 25, 25, 185)
            )),
            "the shortage is a bright red fill"
        );
    }
}
