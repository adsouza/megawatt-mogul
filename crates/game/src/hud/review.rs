//! The Review screen (spec §8.4): the month just closed, on an opaque background.
//!
//! Three headline figures lead the report. The summary column puts regulation, the annual rate
//! review and lessons before the detailed P&L (weighted by the month's days, §2.3), reliability
//! and incidents. Merit order and battery storage have their own independently scrolling columns.
//! *Continue* is the way out: it sends `FinishReview`. The top bar stays visible, so the menu
//! and the milestones stay in reach. The 3D scene is not rendered during Review.
//!
//! A game that ended at this month close shows this modal only when the player asks for it from
//! the game-over modal (*See the month*, [`GameOverView::FinalMonth`]). Then the footer is *Back*,
//! which returns to the game-over modal, and `FinishReview` is never sent: the game is over.

use crate::l10n::format::MoneyStyle;
use application::projections::Lesson;
use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::events::DomainEvent;
use domain::region::RegionDefinition;
use domain::session::{DayPhase, MonthlyReport, SessionStatus};
use domain::units::MegawattHours;

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::game_over::{GameOverView, GameOverViewShown};
use super::incidents::incident_tone;
use super::layout::HudArea;
pub use super::theme::TextTone as ReviewTone;
use super::theme::{marginal_cost_fraction, severity_color as marginal_cost_color};
use super::{SupplyHistoryModel, UiState, draw_supply_history};
use crate::l10n::{LessonMoneyText, Localize, LocalizeWith, Localizer, Names, Toast, format};
use crate::sim::Session;

/// The space kept clear between the modal and the edges of the dimmed area.
pub const REVIEW_MARGIN: f32 = 16.0;
/// The height of the separator above the footer button, reserved under the scroll area.
const FOOTER_SEPARATOR: f32 = 6.0;
/// Monthly energy sums accumulate about 1e-13 relative rounding error across 1,440 ticks.
/// This allows numerical noise while staying far below the percentage's display precision.
const ENERGY_ROUNDING_RELATIVE_TOLERANCE: f64 = 1e-12;

/// A review figure with presentation semantics derived before formatting.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewStat {
    pub label: String,
    pub value: String,
    pub tone: ReviewTone,
}

impl ReviewStat {
    fn with_tone(mut self, tone: ReviewTone) -> Self {
        self.tone = tone;
        self
    }
}

fn heading_color() -> egui::Color32 {
    super::theme::heading_color()
}

/// One plant's line in the merit order used.
#[derive(Debug, Clone, PartialEq)]
pub struct MeritRow {
    pub plant: String,
    pub kind: String,
    pub marginal_cost: String,
    /// Marginal cost relative to the region's most expensive generation, in `0..=1`.
    pub marginal_cost_fraction: f32,
    pub energy: String,
    pub has_energy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewModel {
    pub storage_heading: String,
    pub storage_note: String,
    /// Individual batteries, preceded by fleet totals only when there are multiple batteries.
    pub storage: Vec<(String, Vec<ReviewStat>)>,
    pub title: String,
    /// `March 2027`.
    pub month: String,
    /// Net cash change, share of demand served, and time out of frequency band, for this month.
    pub highlights: [ReviewStat; 3],
    pub pnl_heading: String,
    /// The month's figures are one played day times the month's days (§2.3).
    pub days_note: String,
    /// Revenue, fuel, start-ups, fixed O&M, construction, then net, as signed money.
    pub pnl: Vec<ReviewStat>,
    pub merit_heading: String,
    /// Plant, kind, marginal cost, energy.
    pub merit_columns: [String; 4],
    /// Cheapest first, as the month dispatched them.
    pub merit: Vec<MeritRow>,
    pub reliability_heading: String,
    /// Served, unserved by planned rotating outages, unserved by protection, time out of band.
    pub reliability: Vec<ReviewStat>,
    pub incidents_heading: String,
    /// `06:12: Protection shed ...`, oldest first.
    pub incidents: Vec<String>,
    pub incident_tones: Vec<ReviewTone>,
    /// Shown when there were none.
    pub incidents_empty: Option<String>,
    pub lessons_heading: String,
    pub lessons: Vec<String>,
    pub lesson_tones: Vec<ReviewTone>,
    pub regulatory_heading: String,
    /// Each strike with its reason, each notice, a lapsed strike, a revoked licence.
    pub regulatory: Vec<String>,
    pub regulatory_tones: Vec<ReviewTone>,
    /// Shown when the regulator did nothing.
    pub regulatory_empty: Option<String>,
    pub rate_review_heading: String,
    /// The annual review's result, in December only.
    pub rate_review: Option<String>,
    pub satisfaction_adjustment: Option<String>,
    /// *Continue*, which sends `FinishReview`; after game over, *Back*, which returns to the
    /// game-over modal.
    pub footer_button: ButtonModel,
    pub supply_button: ButtonModel,
    pub supply: Option<SupplyHistoryModel>,
}

/// The modal while the month is in review; `None` otherwise. Once the game is over, its own modal
/// is the only one, unless `view` asks for the final month.
pub fn review_model(
    game: &Game,
    view: GameOverView,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> Option<ReviewModel> {
    review_model_with_history(game, view, &UiState::default(), l10n, style)
}

fn review_model_with_history(
    game: &Game,
    view: GameOverView,
    ui: &UiState,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> Option<ReviewModel> {
    let session = game.session();
    if !review_applies(session.status(), session.phase(), view) {
        return None;
    }
    let projections = game.projections();
    let report = projections.monthly_reports.latest()?;
    let mut model = review_model_for(
        report,
        projections.lessons.latest(),
        projections.regulatory.latest(),
        &Names::new(l10n, session),
        session.region(),
        session.status() != SessionStatus::Active,
        (l10n, style),
    );
    model.satisfaction_adjustment = current_satisfaction_adjustment(session, report, l10n);
    if ui.review_supply {
        model.supply = Some(
            super::supply_history::supply_history_model_scoped(
                game,
                ui.supply_month,
                ui.island,
                l10n,
                style,
            )
            .with_layout(ui.supply_chart_layout),
        );
        model.supply_button = ButtonModel::new(
            l10n.text("review-summary"),
            UiAction::ShowReviewSupply(false),
        );
    }
    Some(model)
}

/// Only the live closed month has a current adjustment; archived outcomes keep their own copy.
fn current_satisfaction_adjustment(
    session: &domain::session::GameSession,
    report: &MonthlyReport,
    l10n: &Localizer,
) -> Option<String> {
    (report.rate_review.is_some()
        && session.phase() == DayPhase::Review
        && report.month == session.clock().month())
    .then(|| {
        l10n.args(
            "review-satisfaction-adjustment",
            &[(
                "points",
                format::signed_points(session.satisfaction_return_adjustment()).into(),
            )],
        )
    })
}

/// Whether the Review modal shows for a game in `status` and `phase`, with `view` the finished
/// game's screen: an active game in Review, or a finished game (which ends at a month close, in
/// Review) whose player asked to see the final month. Never over the game-over modal.
pub fn review_applies(status: SessionStatus, phase: DayPhase, view: GameOverView) -> bool {
    if phase != DayPhase::Review {
        return false;
    }
    match status {
        SessionStatus::Active => true,
        SessionStatus::GameOver(_) => view == GameOverView::FinalMonth,
    }
}

/// The modal for `report`, its `lessons`, and the `regulatory` events of the same month close.
/// A strike's reason comes from its `StrikeIssued` event, never from the frequency lesson's
/// `notice` flag, which can't tell which test the month's one strike was for. `game_over` is
/// whether the game ended at this close, which changes the footer to *Back*.
/// `region` anchors the cost colors to its catalog, even when expensive plants did not run.
pub fn review_model_for(
    report: &MonthlyReport,
    lessons: &[Lesson],
    regulatory: &[DomainEvent],
    names: &Names,
    region: &RegionDefinition,
    game_over: bool,
    text: (&Localizer, &MoneyStyle),
) -> ReviewModel {
    let (l10n, style) = text;
    let stat = |key: &str, value: String| ReviewStat {
        label: l10n.text(key),
        value,
        tone: ReviewTone::Information,
    };
    let cash = |key: &str, amount: domain::units::Money| {
        stat(key, format::money(amount, style)).with_tone(ReviewTone::cash(amount.cents()))
    };
    // Match the one-decimal percentage shown in both the highlight and reliability table.
    let frequency_tone = if (report.out_of_band_share.value() * 1000.0).round() == 0.0 {
        ReviewTone::Positive
    } else if report.frequency_failed {
        ReviewTone::Negative
    } else {
        ReviewTone::Caution
    };
    let demand = report.demand.value();
    let served = report.served.value();
    let energy_tolerance = demand.abs().max(served.abs()) * ENERGY_ROUNDING_RELATIVE_TOLERANCE;
    let fully_served = report.unserved.value().max(demand - served) <= energy_tolerance;
    let demand_served = if fully_served {
        l10n.text("review-demand-served-full")
    } else if (report.reliability.value() * 1000.0).round() == 1000.0 {
        l10n.text("review-demand-served-below-full")
    } else {
        format::percent(report.reliability)
    };
    // `rotating_outage` is part of `unserved` (both are monthly sums, so the difference can carry
    // float noise below zero).
    let unplanned = MegawattHours::new((report.unserved - report.rotating_outage).value().max(0.0));
    let (incidents, incident_tones): (Vec<_>, Vec<_>) = report
        .incidents
        .iter()
        .map(|incident| {
            let text = l10n.args(
                "incident-row",
                &[
                    ("time", format::clock(incident.tick).into()),
                    ("incident", incident.kind.localize_with(l10n, names).into()),
                ],
            );
            (text, incident_tone(incident.kind))
        })
        .unzip();
    let (regulatory, regulatory_tones): (Vec<_>, Vec<_>) = regulatory
        .iter()
        .filter_map(|event| {
            event
                .toast(l10n, names, style)
                .map(|text| (text, regulatory_tone(event)))
        })
        .unzip();
    let (lessons, lesson_tones) = lessons
        .iter()
        .map(|lesson| (lesson.localize_money(l10n, style), lesson_tone(lesson)))
        .unzip();
    ReviewModel {
        storage_heading: l10n.text("battery-review-heading"),
        storage_note: l10n.text("battery-review-note"),
        storage: storage_review_rows(report, names, l10n),
        title: l10n.text("review-title"),
        month: format::month_year(report.month),
        highlights: [
            cash("review-net-cash", report.net),
            stat("review-demand-served", demand_served).with_tone(if fully_served {
                ReviewTone::Positive
            } else {
                ReviewTone::Caution
            }),
            stat(
                "review-out-of-band",
                format::percent(report.out_of_band_share),
            )
            .with_tone(frequency_tone),
        ],
        pnl_heading: l10n.text("review-pnl"),
        days_note: l10n.args(
            "review-days-note",
            &[("days", report.days.to_string().into())],
        ),
        pnl: vec![
            cash("review-revenue", report.revenue),
            cash("review-fuel", report.fuel),
            cash("review-start-up", report.start_up),
            cash("review-fixed-om", report.fixed_om),
            cash("review-construction", report.construction),
            cash("review-net", report.net),
        ],
        merit_heading: l10n.text("review-merit-order"),
        merit_columns: [
            "review-merit-plant",
            "review-merit-kind",
            "review-merit-cost",
            "review-merit-energy",
        ]
        .map(|key| l10n.text(key)),
        merit: report
            .merit_order
            .iter()
            .map(|entry| MeritRow {
                plant: names.plant(entry.plant),
                kind: entry.kind.localize(l10n),
                marginal_cost: format::price(entry.marginal_cost, style),
                marginal_cost_fraction: marginal_cost_fraction(region, entry.marginal_cost),
                energy: format::megawatt_hours(entry.energy),
                has_energy: entry.energy > MegawattHours::ZERO,
            })
            .collect(),
        reliability_heading: l10n.text("review-reliability"),
        reliability: vec![
            stat("review-served", format::megawatt_hours(report.served)),
            stat(
                "review-unserved-planned",
                format::megawatt_hours(report.rotating_outage),
            )
            .with_tone(if report.rotating_outage.value() > 0.0 {
                ReviewTone::Caution
            } else {
                ReviewTone::Positive
            }),
            stat(
                "review-unserved-unplanned",
                format::megawatt_hours(unplanned),
            )
            .with_tone(if unplanned.value() > 0.0 {
                ReviewTone::Negative
            } else {
                ReviewTone::Positive
            }),
            stat(
                "review-out-of-band",
                format::percent(report.out_of_band_share),
            )
            .with_tone(frequency_tone),
        ]
        .into_iter()
        .chain(
            report
                .curtailed_by_kind
                .iter()
                .map(|(kind, energy)| ReviewStat {
                    label: l10n.args(
                        "review-curtailed-kind",
                        &[("kind", kind.localize(l10n).into())],
                    ),
                    value: format::megawatt_hours(*energy),
                    tone: if energy.value() > 0.0 {
                        ReviewTone::Caution
                    } else {
                        ReviewTone::Neutral
                    },
                }),
        )
        .collect(),
        incidents_heading: l10n.text("review-incidents"),
        incidents_empty: incidents
            .is_empty()
            .then(|| l10n.text("review-incidents-none")),
        incidents,
        incident_tones,
        lessons_heading: l10n.text("review-lessons"),
        lessons,
        lesson_tones,
        regulatory_heading: l10n.text("review-regulatory"),
        regulatory_empty: regulatory
            .is_empty()
            .then(|| l10n.text("review-regulatory-none")),
        regulatory,
        regulatory_tones,
        satisfaction_adjustment: None,
        rate_review_heading: l10n.text("review-rate-review"),
        rate_review: report.rate_review.as_ref().map(|outcome| {
            l10n.args(
                "review-rate-review-result",
                &[
                    ("tariff", format::price(outcome.tariff, style).into()),
                    (
                        "allowed_return",
                        format::percent(outcome.allowed_return).into(),
                    ),
                ],
            )
        }),
        footer_button: if game_over {
            ButtonModel::new(l10n.text("ui-back"), UiAction::ShowGameOverSummary)
        } else {
            ButtonModel::new(
                l10n.text("ui-continue"),
                UiAction::Command(Command::FinishReview),
            )
        },
        supply_button: ButtonModel::new(
            l10n.text("review-supply"),
            UiAction::ShowReviewSupply(true),
        ),
        supply: None,
    }
}

fn regulatory_tone(event: &DomainEvent) -> ReviewTone {
    match event {
        DomainEvent::StrikeExpired { .. } => ReviewTone::Positive,
        DomainEvent::FrequencyNoticeIssued { .. } => ReviewTone::Caution,
        DomainEvent::StrikeIssued { .. } | DomainEvent::LicenseRevoked => ReviewTone::Negative,
        _ => ReviewTone::Information,
    }
}

fn lesson_tone(lesson: &Lesson) -> ReviewTone {
    match lesson {
        Lesson::GeothermalSuccess { .. } => ReviewTone::Positive,
        Lesson::GeothermalDry { .. } => ReviewTone::Negative,
        Lesson::GeothermalDecline { .. } => ReviewTone::Caution,
        Lesson::AllClear => ReviewTone::Positive,
        Lesson::RateReview { .. } => ReviewTone::Information,
        Lesson::UnservedEnergy { .. } | Lesson::FrequencyPerformance { notice: false, .. } => {
            ReviewTone::Negative
        }
        Lesson::Overgeneration { trips, .. } if *trips > 0 => ReviewTone::Negative,
        Lesson::ExpensivePeaking { .. }
        | Lesson::RotatingOutages { .. }
        | Lesson::Overgeneration { .. }
        | Lesson::FrequencyPerformance { notice: true, .. }
        | Lesson::SolarCurtailed { .. }
        | Lesson::RenewableCurtailed { .. }
        | Lesson::StartUpCosts { .. } => ReviewTone::Caution,
    }
}

/// Fill the available area under the top bar, leaving a small margin on every side.
pub fn review_rect(behind: egui::Rect) -> egui::Rect {
    egui::Rect::from_center_size(
        behind.center(),
        (behind.size() - egui::Vec2::splat(2.0 * REVIEW_MARGIN)).max(egui::Vec2::ZERO),
    )
}

/// The heading, independently scrolling report columns, and footer buttons pinned below them.
/// Fills the height it is given, so the system sets that.
pub fn draw_review(ui: &mut egui::Ui, model: &ReviewModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        ui.heading(egui::RichText::new(&model.title).color(heading_color()));
        ui.colored_label(ReviewTone::Caution.color(), &model.month);
    });
    ui.separator();
    if model.supply.is_none() {
        ui.columns(model.highlights.len(), |columns| {
            for (column, stat) in columns.iter_mut().zip(&model.highlights) {
                egui::Frame::group(column.style()).show(column, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(&stat.label).color(ReviewTone::Neutral.color()));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&stat.value)
                                .size(24.0)
                                .strong()
                                .color(stat.tone.color()),
                        )
                        .wrap(),
                    );
                });
            }
        });
        ui.add_space(6.0);
    }
    let spacing = ui.spacing();
    // The separator and the footer button's row below the scroll area, with the gaps between them.
    let footer = FOOTER_SEPARATOR + spacing.interact_size.y + 3.0 * spacing.item_spacing.y;
    let content_height = (ui.available_height() - footer).max(0.0);
    if let Some(history) = &model.supply {
        ui.allocate_ui(egui::vec2(ui.available_width(), content_height), |ui| {
            actions.extend(draw_supply_history(ui, history));
        });
    } else {
        if ui.available_width() >= 680.0 {
            let count = if model.storage.is_empty() { 2 } else { 3 };
            ui.columns(count, |columns| {
                egui::ScrollArea::vertical()
                    .id_salt("review_sections")
                    .auto_shrink([false, false])
                    .max_height(content_height)
                    .show(&mut columns[0], |ui| draw_summary(ui, model));
                egui::ScrollArea::vertical()
                    .id_salt("review_merit_rows")
                    .auto_shrink([false, false])
                    .max_height(content_height)
                    .show(&mut columns[1], |ui| draw_merit(ui, model));
                if count == 3 {
                    egui::ScrollArea::vertical()
                        .id_salt("review_storage_rows")
                        .auto_shrink([false, false])
                        .max_height(content_height)
                        .show(&mut columns[2], |ui| draw_storage(ui, model));
                }
            });
        } else {
            // At extreme UI zoom retain every section inside the window's width.
            egui::ScrollArea::vertical()
                .id_salt("review_narrow_sections")
                .auto_shrink([false, false])
                .max_height(content_height)
                .show(ui, |ui| {
                    draw_summary(ui, model);
                    draw_merit(ui, model);
                    draw_storage(ui, model);
                });
        }
    }
    ui.add(egui::Separator::default().spacing(FOOTER_SEPARATOR));
    ui.horizontal(|ui| {
        button(ui, &model.supply_button, &mut actions);
        button(ui, &model.footer_button, &mut actions);
    });
    actions
}

fn review_heading(ui: &mut egui::Ui, text: &str) {
    review_heading_with_color(ui, text, heading_color());
}

fn review_heading_with_color(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.add_space(6.0);
    ui.heading(egui::RichText::new(text).strong().color(color));
}

fn draw_summary(ui: &mut egui::Ui, model: &ReviewModel) {
    review_heading(ui, &model.regulatory_heading);
    if let Some(empty) = &model.regulatory_empty {
        ui.colored_label(ReviewTone::Positive.color(), empty);
    }
    for (action, tone) in model.regulatory.iter().zip(&model.regulatory_tones) {
        ui.label(egui::RichText::new(action).strong().color(tone.color()));
    }

    if let Some(result) = &model.rate_review {
        review_heading(ui, &model.rate_review_heading);
        ui.strong(result);
        if let Some(adjustment) = &model.satisfaction_adjustment {
            ui.label(adjustment);
        }
    }

    review_heading(ui, &model.lessons_heading);
    for (lesson, tone) in model.lessons.iter().zip(&model.lesson_tones) {
        ui.colored_label(tone.color(), lesson);
    }

    ui.separator();
    review_heading(ui, &model.pnl_heading);
    ui.weak(model.days_note.as_str());
    draw_stats(ui, "review_pnl", &model.pnl);
    review_heading(ui, &model.reliability_heading);
    draw_stats(ui, "review_reliability", &model.reliability);

    review_heading(ui, &model.incidents_heading);
    if let Some(empty) = &model.incidents_empty {
        ui.colored_label(ReviewTone::Positive.color(), empty);
    }
    for (incident, tone) in model.incidents.iter().zip(&model.incident_tones) {
        ui.colored_label(tone.color(), incident);
    }
}

fn draw_stats(ui: &mut egui::Ui, id: &str, stats: &[ReviewStat]) {
    egui::Grid::new(id)
        .num_columns(2)
        .max_col_width((ui.available_width() - ui.spacing().item_spacing.x) / 2.0)
        .show(ui, |ui| {
            for stat in stats {
                ui.colored_label(ReviewTone::Neutral.color(), &stat.label);
                ui.colored_label(stat.tone.color(), &stat.value);
                ui.end_row();
            }
        });
}

fn draw_storage(ui: &mut egui::Ui, model: &ReviewModel) {
    if model.storage.is_empty() {
        return;
    }
    review_heading(ui, &model.storage_heading);
    ui.weak(&model.storage_note);
    for (index, (name, rows)) in model.storage.iter().enumerate() {
        if index > 0 {
            ui.separator();
        }
        review_heading_with_color(ui, name, ReviewTone::Information.color());
        draw_stats(ui, &format!("storage_review_{index}"), rows);
    }
}

fn draw_merit(ui: &mut egui::Ui, model: &ReviewModel) {
    review_heading(ui, &model.merit_heading);
    let content_width = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x).max(0.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color())
            .size()
            .x
            .ceil()
    };
    let mut widths = model.merit_columns.each_ref().map(|text| text_width(text));
    for row in &model.merit {
        for (width, text) in
            widths
                .iter_mut()
                .zip([&row.plant, &row.kind, &row.marginal_cost, &row.energy])
        {
            *width = width.max(text_width(text));
        }
    }
    // Fit the text first; only wrap when the whole table needs more than the available width.
    let scale = (content_width / widths.iter().sum::<f32>()).min(1.0);
    let widths = widths.map(|width| width * scale);
    let cell = |ui: &mut egui::Ui, text: &str, width, header, color| {
        let text = egui::RichText::new(text).color(color);
        // Allocate the laid-out text directly so the grid aligns and sizes the full cell,
        // including wrapped lines, before painting the next row's background.
        let galley = egui::WidgetText::from(if header { text.strong() } else { text }).into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            width,
            egui::TextStyle::Body,
        );
        ui.add(egui::Label::new(galley));
    };
    egui::Grid::new("review_merit")
        .num_columns(4)
        .min_col_width(0.0)
        .striped(true)
        .show(ui, |ui| {
            for (column, width) in model.merit_columns.iter().zip(widths) {
                cell(ui, column, width, true, heading_color());
            }
            ui.end_row();
            for row in &model.merit {
                for (index, (text, width)) in
                    [&row.plant, &row.kind, &row.marginal_cost, &row.energy]
                        .into_iter()
                        .zip(widths)
                        .enumerate()
                {
                    let color = match index {
                        1 => ReviewTone::Neutral.color(),
                        2 => marginal_cost_color(row.marginal_cost_fraction),
                        3 if row.has_energy => ReviewTone::Information.color(),
                        3 => ReviewTone::Neutral.color(),
                        _ => ui.visuals().text_color(),
                    };
                    cell(ui, text, width, false, color);
                }
                ui.end_row();
            }
        });
}

/// Opaque even while a save dialog or the game-over summary covers the report.
pub(super) fn review_background(mut contexts: EguiContexts, area: Res<HudArea>) -> Result {
    let ctx = contexts.ctx_mut()?;
    ctx.layer_painter(egui::LayerId::background()).rect_filled(
        area.free_rect(ctx),
        0.0,
        ctx.global_style().visuals.panel_fill,
    );
    Ok(())
}

/// The report takes background clicks without leaving Review and stays below floating windows.
pub(super) fn review_modal(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    shown: Res<GameOverViewShown>,
    mut sink: ActionSink,
    style: Res<MoneyStyle>,
) -> Result {
    // The pass's snapshot, not the live view: see `GameOverViewShown`.
    let Some(model) =
        review_model_with_history(&session.0, shown.0, sink.ui_state(), &l10n, &style)
    else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;
    let behind = area.free_rect(ctx);
    let modal = review_rect(behind);
    let frame = egui::Frame::window(&ctx.global_style());
    let inner = (modal.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
    // One layer for the backdrop and the modal, at `Middle`: egui raises a pressed area above the
    // others of its order, so a modal of its own at `Foreground` would rise over the Milestones
    // window when clicked.
    let actions = egui::Area::new(egui::Id::new("review"))
        .order(egui::Order::Middle)
        .fixed_pos(behind.min)
        .show(ctx, |ui| {
            ui.allocate_exact_size(behind.size(), egui::Sense::click_and_drag());
            let mut modal_ui = ui.new_child(egui::UiBuilder::new().max_rect(modal));
            frame
                .show(&mut modal_ui, |ui| {
                    ui.set_width(inner.x);
                    ui.set_height(inner.y);
                    draw_review(ui, &model)
                })
                .inner
        })
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

fn storage_review_rows(
    report: &MonthlyReport,
    names: &Names,
    l10n: &Localizer,
) -> Vec<(String, Vec<ReviewStat>)> {
    use domain::generation::BatteryTotals;
    let rows = |total: BatteryTotals| {
        [
            ("battery-input", total.input),
            ("battery-delivered", total.output),
            ("battery-charge-loss", total.charge_loss),
            ("battery-discharge-loss", total.discharge_loss),
            ("battery-unused", total.unused),
            (
                "battery-unused-daily",
                total.unused * (1.0 / f64::from(report.days)),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (key, energy))| ReviewStat {
            label: l10n.text(key),
            value: format::megawatt_hours(energy),
            tone: if energy.value() == 0.0 {
                ReviewTone::Neutral
            } else {
                match index {
                    0 => ReviewTone::Information,
                    1 => ReviewTone::Positive,
                    _ => ReviewTone::Caution,
                }
            },
        })
        .collect()
    };
    if report.storage.is_empty() {
        return Vec::new();
    }
    let mut total = BatteryTotals::default();
    for b in report.storage.values() {
        total.input += b.input;
        total.output += b.output;
        total.charge_loss += b.charge_loss;
        total.discharge_loss += b.discharge_loss;
        total.unused += b.unused;
    }
    (report.storage.len() > 1)
        .then(|| (l10n.text("battery-fleet"), rows(total)))
        .into_iter()
        .chain(
            report
                .storage
                .iter()
                .map(|(id, b)| (names.plant(*id), rows(*b))),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;

    #[test]
    fn review_history_scopes_each_island_and_all_restores_region_totals() {
        let game = crate::hud::test_support::sgcc_review_with_remote_island();
        let day = game.projections().timeline.completed_days().last().unwrap();
        let l10n = crate::hud::test_support::shipped_l10n();
        let style = usd_style();
        let mut ui = UiState {
            review_supply: true,
            supply_month: Some(day.month),
            ..Default::default()
        };
        for &id in game.session().power().island_nodes().keys() {
            ui.island = Some(id);
            let history =
                review_model_with_history(&game, GameOverView::default(), &ui, &l10n, &style)
                    .unwrap()
                    .supply
                    .unwrap();
            let chart = history.chart.unwrap();
            assert_eq!(
                chart.demand,
                day.points
                    .iter()
                    .map(|p| p.islands[&id].demand.value())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                chart.served,
                day.points
                    .iter()
                    .map(|p| p.islands[&id].served.value())
                    .collect::<Vec<_>>()
            );
            assert_eq!(chart.transfers.len(), 2);
            assert_eq!(
                chart.forecast[0][1],
                day.island_forecasts[&id][0].mean.value()
            );
        }
        ui.island = None;
        let history = review_model_with_history(&game, GameOverView::default(), &ui, &l10n, &style)
            .unwrap()
            .supply
            .unwrap();
        assert_eq!(
            history.chart.unwrap().demand,
            day.points
                .iter()
                .map(|p| p.demand.value())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_local_shortage_changes_only_its_zones_archived_retrospective() {
        use application::{Command, InMemoryCommandLog};
        let mut game =
            crate::hud::test_support::shipped_game("sgcc", Box::new(InMemoryCommandLog::default()));
        let city = game
            .session()
            .city_views()
            .into_iter()
            .find(|c| c.name_key == "city-chengdu")
            .unwrap();
        let local_nodes = game.session().power().island_nodes()[&city.node].clone();
        for plant in game
            .session()
            .plant_views()
            .into_iter()
            .filter(|p| local_nodes.contains(&p.node))
        {
            game.execute(Command::StopPlant { plant: plant.id })
                .unwrap();
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        let l10n = crate::hud::test_support::shipped_l10n();
        let checkpoint = game.checkpoint();
        let day = game.projections().timeline.completed_days().last().unwrap();
        let mut unserved_chart = None;
        for name in ["city-chengdu", "city-lanzhou", "city-changchun"] {
            let id = game
                .session()
                .city_views()
                .into_iter()
                .find(|c| c.name_key == name)
                .unwrap()
                .node;
            let ui = UiState {
                island: Some(id),
                review_supply: true,
                ..Default::default()
            };
            let history =
                review_model_with_history(&game, GameOverView::default(), &ui, &l10n, &usd_style())
                    .unwrap()
                    .supply
                    .unwrap();
            let chart = history.chart.unwrap();
            assert_eq!(
                chart.demand,
                day.points
                    .iter()
                    .map(|p| p.islands[&id].demand.value())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                chart.served,
                day.points
                    .iter()
                    .map(|p| p.islands[&id].served.value())
                    .collect::<Vec<_>>()
            );
            for stack in &chart.stacks {
                for (index, point) in day.points.iter().enumerate() {
                    let actual = point.islands[&id]
                        .by_kind
                        .get(&stack.kind)
                        .map_or(0.0, |mw| mw.value());
                    assert!((stack.high[index] - stack.low[index] - actual).abs() < 1e-6);
                }
            }
            if id == city.node {
                assert!(chart.served.iter().all(|mw| *mw == 0.0));
                assert!(chart.stacks.is_empty());
                unserved_chart = Some(chart.served);
            } else {
                assert!(chart.served.iter().all(|mw| *mw > 0.0));
                assert_ne!(Some(&chart.served), unserved_chart.as_ref());
                assert!(!chart.stacks.is_empty());
            }
        }
        assert_eq!(game.checkpoint(), checkpoint);
    }
    use domain::region::PlantKind;
    use std::collections::BTreeMap;

    use application::InMemoryCommandLog;
    use domain::generation::PlantStatus;
    use domain::regulation::{RateReviewOutcome, StrikeReason};
    use domain::session::{GameOverReason, Incident, IncidentKind};
    use domain::time::{SimMonth, TICKS_PER_DAY};
    use domain::units::{Megawatts, Money, Price, Ratio};

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_l10n};

    #[test]
    fn merit_cost_and_energy_text_use_meaningful_colors() {
        use domain::{ids::PlantId, region::PlantKind, session::MeritOrderEntry};

        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let region = game.session().region();
        let l10n = shipped_l10n();
        let mut report = report();
        report.merit_order = [
            PlantKind::Hydro,
            PlantKind::Solar,
            PlantKind::Ccgt,
            PlantKind::Peaker,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| MeritOrderEntry {
            plant: PlantId(index as u32 + 1),
            kind,
            marginal_cost: region.spec(kind).marginal_cost,
            energy: if index == 0 {
                MegawattHours::ZERO
            } else {
                MegawattHours::new(100.0)
            },
        })
        .collect();
        let model = review_model_for(
            &report,
            &[],
            &[],
            &Names::empty(&l10n),
            region,
            false,
            (&l10n, &usd_style()),
        );
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 800.0),
                )),
                ..Default::default()
            },
            |ui| draw_merit(ui, &model),
        );
        let shapes = output.shapes.clone();
        output.drop_without_applying_deltas();
        let colors: Vec<_> = model
            .merit
            .iter()
            .map(|row| {
                shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == row.marginal_cost => {
                            Some(text.galley.job.sections[0].format.color)
                        }
                        _ => None,
                    })
                    .unwrap()
            })
            .collect();
        assert_eq!(colors[0], ReviewTone::Positive.color());
        assert_eq!(colors[1], ReviewTone::Positive.color());
        assert_eq!(colors[3], ReviewTone::Negative.color());
        assert_ne!(colors[2], colors[0]);
        assert_ne!(colors[2], colors[3]);
        for (row, tone) in model.merit.iter().zip([
            ReviewTone::Neutral,
            ReviewTone::Information,
            ReviewTone::Information,
            ReviewTone::Information,
        ]) {
            let color = shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == row.energy => {
                        Some(text.galley.job.sections[0].format.color)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(color, tone.color(), "{}", row.energy);
        }
        report.merit_order.pop();
        let without_peaker = review_model_for(
            &report,
            &[],
            &[],
            &Names::empty(&l10n),
            region,
            false,
            (&l10n, &usd_style()),
        );
        assert_eq!(
            without_peaker.merit[2].marginal_cost_fraction,
            model.merit[2].marginal_cost_fraction
        );
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn review_news_is_colored_by_the_event_and_lesson_severity() {
        let l10n = shipped_l10n();
        let mut report = report();
        report.incidents = [
            IncidentKind::Blackout,
            IncidentKind::RotatingOutage {
                share: Ratio::new(0.1).unwrap(),
            },
            IncidentKind::BlackoutRestored,
        ]
        .into_iter()
        .enumerate()
        .map(|(tick, kind)| Incident {
            tick: tick as u32,
            kind,
        })
        .collect();
        let events = [
            DomainEvent::StrikeIssued {
                strikes: 1,
                reason: StrikeReason::UnservedEnergy,
            },
            DomainEvent::FrequencyNoticeIssued {
                month: report.month,
            },
            DomainEvent::StrikeExpired { strikes: 0 },
        ];
        let lessons = [
            Lesson::UnservedEnergy {
                energy: MegawattHours::new(10.0),
                first_incident_tick: 0,
            },
            Lesson::StartUpCosts {
                total: Money::from_dollars(100),
                share: Ratio::new(0.2).unwrap(),
            },
            Lesson::AllClear,
        ];
        let model = review_model_for(
            &report,
            &lessons,
            &events,
            &Names::empty(&l10n),
            &test_region(),
            false,
            (&l10n, &usd_style()),
        );
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| draw_summary(ui, &model),
        );
        let shapes = output.shapes.clone();
        output.drop_without_applying_deltas();
        let color = |label: &str| {
            shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        let color = text.galley.job.sections[0].format.color;
                        Some(if color == egui::Color32::PLACEHOLDER {
                            text.fallback_color
                        } else {
                            color
                        })
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing {label}"))
        };
        for messages in [&model.regulatory, &model.lessons, &model.incidents] {
            for (message, tone) in messages.iter().zip([
                ReviewTone::Negative,
                ReviewTone::Caution,
                ReviewTone::Positive,
            ]) {
                assert_eq!(color(message), tone.color(), "{message}");
            }
        }
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn merit_row_backgrounds_contain_their_text_without_covering_previous_rows() {
        let game = reviewed_game();
        let l10n = shipped_l10n();
        let model = review_model(&game, GameOverView::Summary, &l10n, &usd_style()).unwrap();
        fn flatten<'a>(shape: &'a egui::Shape, shapes: &mut Vec<&'a egui::Shape>) {
            match shape {
                egui::Shape::Vec(children) => {
                    for child in children {
                        flatten(child, shapes);
                    }
                }
                _ => shapes.push(shape),
            }
        }
        for width in [240.0, 400.0, 900.0] {
            let ctx = egui::Context::default();
            for frame in 0..4 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 2000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| draw_merit(ui, &model),
                );
                let painted = output.shapes.clone();
                output.drop_without_applying_deltas();
                if frame != 3 {
                    continue;
                }
                let mut shapes = Vec::new();
                for shape in &painted {
                    flatten(&shape.shape, &mut shapes);
                }
                let mut texts = Vec::new();
                let mut backgrounds = Vec::new();
                for shape in shapes {
                    match shape {
                        egui::Shape::Text(text) => texts.push(text),
                        egui::Shape::Rect(background) => {
                            for text in &texts {
                                assert!(
                                    !background.rect.intersects(text.visual_bounding_rect()),
                                    "{width}: a later row background covers {:?}",
                                    text.galley.text()
                                );
                            }
                            backgrounds.push(background.rect);
                        }
                        _ => {}
                    }
                }
                // Heading, four column headers, then four cells per plant.
                assert_eq!(texts.len(), 5 + 4 * model.merit.len());
                assert_eq!(backgrounds.len(), model.merit.len().div_ceil(2));
                for (row, background) in backgrounds.iter().enumerate() {
                    for text in &texts[5 + row * 8..9 + row * 8] {
                        assert!(
                            background
                                .expand(0.5)
                                .contains_rect(text.visual_bounding_rect()),
                            "{width}: {:?} sits outside its row background {background:?}",
                            text.galley.text()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn merit_table_keeps_every_cell_on_one_line_when_the_text_fits() {
        let game = reviewed_game();
        let l10n = shipped_l10n();
        let model = review_model(&game, GameOverView::Summary, &l10n, &usd_style()).unwrap();
        fn check(shape: &egui::Shape, width: f32) {
            match shape {
                egui::Shape::Text(text) => {
                    assert_eq!(
                        text.galley.rows.len(),
                        1,
                        "{width}: {:?} wraps despite available space",
                        text.galley.job.text
                    );
                    assert!(text.pos.x + text.galley.size().x <= width + 0.5);
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        check(shape, width);
                    }
                }
                _ => {}
            }
        }
        for width in [400.0, 500.0, 600.0, 900.0] {
            let ctx = egui::Context::default();
            for _ in 0..3 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 2000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| draw_merit(ui, &model),
                );
                let shapes = output.shapes.clone();
                output.drop_without_applying_deltas();
                for shape in &shapes {
                    check(&shape.shape, width);
                }
            }
        }
    }

    /// An eastern game with every plant that can run committed all day, its first day played
    /// through to the month's close: in Review.
    fn reviewed_game() -> Game {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        for plant in game.session().plant_views() {
            if plant.kind.is_dispatchable()
                && !matches!(plant.status, PlantStatus::UnderConstruction { .. })
            {
                game.execute(Command::StartPlant { plant: plant.id })
                    .unwrap();
            }
        }
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        assert_eq!(game.session().phase(), DayPhase::Review);
        game
    }

    /// An eastern game with nothing committed, played until the regulator revokes the licence at
    /// a month close: over, with its final month's report as the latest.
    fn finished_game() -> Game {
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        for _ in 0..3 {
            game.execute(Command::BeginOperating).unwrap();
            game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
                .unwrap();
            if matches!(game.session().status(), SessionStatus::GameOver(_)) {
                return game;
            }
            game.execute(Command::FinishReview).unwrap();
        }
        panic!("the licence is revoked by the third close");
    }

    /// A quiet month to vary one field at a time.
    fn report() -> MonthlyReport {
        MonthlyReport {
            islands: Default::default(),
            storage: Default::default(),
            month: SimMonth::new(2027, 3).unwrap(),
            days: 31,
            revenue: Money::from_dollars(9_000_000),
            fuel: -Money::from_dollars(4_000_000),
            start_up: -Money::from_dollars(250_000),
            fixed_om: -Money::from_dollars(1_000_000),
            construction: Money::ZERO,
            net: Money::from_dollars(3_750_000),
            energy_by_kind: BTreeMap::new(),
            merit_order: Vec::new(),
            demand: MegawattHours::new(100_000.0),
            served: MegawattHours::new(99_900.0),
            unserved: MegawattHours::new(100.0),
            curtailed: MegawattHours::ZERO,
            curtailed_by_kind: Default::default(),
            imbalance: MegawattHours::ZERO,
            overgeneration: MegawattHours::ZERO,
            rotating_outage: MegawattHours::new(40.0),
            peak_outage_share: Ratio::new(0.05).unwrap(),
            peak_demand: Megawatts::new(5_000.0),
            reliability: Ratio::new(0.999).unwrap(),
            incidents: Vec::new(),
            over_frequency_trips: 0,
            out_of_band_share: Ratio::new(0.02).unwrap(),
            frequency_failed: false,
            frequency_notice: false,
            rate_review: None,
        }
    }

    fn test_region() -> std::sync::Arc<RegionDefinition> {
        eastern_game(Box::new(InMemoryCommandLog::default()))
            .session()
            .region()
            .clone()
    }

    fn model_of(
        report: &MonthlyReport,
        regulatory: &[DomainEvent],
        l10n: &Localizer,
    ) -> ReviewModel {
        review_model_for(
            report,
            &[Lesson::AllClear],
            regulatory,
            &Names::empty(l10n),
            &test_region(),
            false,
            (l10n, &usd_style()),
        )
    }

    #[test]
    fn review_highlights_the_months_cash_and_grid_outcomes() {
        let l10n = shipped_l10n();
        let mut report = report();
        report.net = -Money::from_dollars(500_000);
        let model = model_of(&report, &[], &l10n);
        let highlights: Vec<_> = model
            .highlights
            .iter()
            .map(|stat| (stat.label.as_str(), stat.value.as_str()))
            .collect();
        assert_eq!(
            highlights,
            [
                ("Net cash change", "−$500k"),
                ("Demand served", "99.9%"),
                ("Time out of frequency band", "2.0%"),
            ]
        );
        assert!(l10n.problems().is_empty());
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(32))]
        #[test]
        fn higher_marginal_costs_never_look_greener(first in 0.0f32..=1.0, second in 0.0f32..=1.0) {
            let cheaper = marginal_cost_color(first.min(second));
            let dearer = marginal_cost_color(first.max(second));
            proptest::prop_assert!(cheaper.r() <= dearer.r());
            proptest::prop_assert!(cheaper.g() >= dearer.g());
        }

        #[test]
        fn frequency_figures_are_green_when_they_display_zero(share in 0.0f64..=1.0) {
            let l10n = shipped_l10n();
            for share in [share, 0.0, f64::EPSILON, 0.00049, 0.0005, 0.00051] {
                for failed in [false, true] {
                    let mut report = report();
                    report.out_of_band_share = Ratio::new(share).unwrap();
                    report.frequency_failed = failed;
                    let model = model_of(&report, &[], &l10n);
                    let expected = if model.highlights[2].value == format::percent(Ratio::ZERO) {
                        ReviewTone::Positive
                    } else if failed {
                        ReviewTone::Negative
                    } else {
                        ReviewTone::Caution
                    };
                    proptest::prop_assert_eq!(model.highlights[2].tone, expected, "share: {}", share);
                    proptest::prop_assert_eq!(model.reliability[3].tone, expected, "share: {}", share);
                }
            }
            proptest::prop_assert!(l10n.problems().is_empty());
        }

        #[test]
        fn demand_served_distinguishes_real_shortfalls_from_float_noise(
            demand in 1e-6f64..1e12,
            shortfall_share in 1e-9f64..=1.0,
        ) {
            let l10n = shipped_l10n();
            for (lost_share, noise_only) in [
                (shortfall_share, false), (0.0001, false), (1e-9, false),
                (0.0, true), (f64::EPSILON, true), (1e-14, true),
            ] {
                let mut report = report();
                report.demand = MegawattHours::new(demand);
                report.served = report.demand * (1.0 - lost_share);
                report.unserved = report.demand * lost_share;
                report.rotating_outage = MegawattHours::ZERO;
                report.reliability = Ratio::new(1.0 - lost_share).unwrap();
                let model = model_of(&report, &[], &l10n);
                let highlight = &model.highlights[1];
                if noise_only {
                    proptest::prop_assert_eq!(highlight.tone, ReviewTone::Positive);
                    proptest::prop_assert_eq!(&highlight.value, "100%");
                } else {
                    proptest::prop_assert_eq!(highlight.tone, ReviewTone::Caution);
                    let rounded = format::percent(report.reliability);
                    let expected = if rounded == format::percent(Ratio::ONE) { "<100%" } else { &rounded };
                    proptest::prop_assert_eq!(&highlight.value, expected);
                }
            }
            proptest::prop_assert!(l10n.problems().is_empty());
        }

        #[test]
        fn review_cash_colors_follow_the_amount_before_formatting(cents in proptest::prelude::any::<i64>()) {
            let l10n = shipped_l10n();
            for cents in [cents, 0, -1, 1] {
                let mut report = report();
                report.net = Money::from_cents(cents);
                let model = model_of(&report, &[], &l10n);
                let expected = if cents < 0 {
                    ReviewTone::Negative
                } else if cents > 0 {
                    ReviewTone::Positive
                } else {
                    ReviewTone::Neutral
                };
                proptest::prop_assert_eq!(model.highlights[0].tone, expected);
                proptest::prop_assert_eq!(model.pnl.last().unwrap().tone, expected);
            }
            proptest::prop_assert!(l10n.problems().is_empty());
        }
    }

    #[test]
    fn a_december_review_shows_its_satisfaction_adjustment_only_while_current() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        for month in 1..=12 {
            game.execute(Command::BeginOperating).unwrap();
            game.execute(Command::AdvanceTicks {
                n: domain::time::TICKS_PER_DAY,
            })
            .unwrap();
            if month != 12 {
                game.execute(Command::FinishReview).unwrap();
            }
        }
        let december = game.projections().monthly_reports.latest().unwrap().clone();
        let model = review_model(&game, GameOverView::Summary, &l10n, &usd_style()).unwrap();
        let expected = format!(
            "Satisfaction moved your allowed return by {}.",
            format::signed_points(game.session().satisfaction_return_adjustment())
        );
        assert_eq!(
            model.satisfaction_adjustment.as_deref(),
            Some(expected.as_str())
        );
        game.execute(Command::FinishReview).unwrap();
        assert_eq!(
            current_satisfaction_adjustment(game.session(), &december, &l10n),
            None
        );
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks {
            n: domain::time::TICKS_PER_DAY,
        })
        .unwrap();
        assert_eq!(
            current_satisfaction_adjustment(game.session(), &december, &l10n),
            None
        );
        assert_eq!(
            review_model(&game, GameOverView::Summary, &l10n, &usd_style())
                .unwrap()
                .satisfaction_adjustment,
            None
        );
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn storage_review_only_adds_fleet_totals_for_multiple_batteries() {
        use domain::{generation::BatteryTotals, ids::PlantId};

        let l10n = shipped_l10n();
        for count in 0..=3 {
            let mut report = report();
            for id in 1..=count {
                report.storage.insert(
                    PlantId(id),
                    BatteryTotals {
                        input: MegawattHours::new(100.0),
                        ..Default::default()
                    },
                );
            }
            let model = model_of(&report, &[], &l10n);
            assert_eq!(model.storage.len(), count as usize + usize::from(count > 1));
            if count == 0 {
                continue;
            }
            let individuals = if count > 1 {
                assert_eq!(model.storage[0].0, l10n.text("battery-fleet"));
                assert_eq!(
                    model.storage[0].1[0].value,
                    format::megawatt_hours(MegawattHours::new(100.0 * f64::from(count)))
                );
                &model.storage[1..]
            } else {
                &model.storage[..]
            };
            for (index, (name, rows)) in individuals.iter().enumerate() {
                assert_eq!(*name, Names::empty(&l10n).plant(PlantId(index as u32 + 1)));
                assert_eq!(
                    rows[0].value,
                    format::megawatt_hours(MegawattHours::new(100.0))
                );
            }
        }
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn review_headings_stand_out_and_fleet_totals_have_a_visual_break() {
        let l10n = shipped_l10n();
        let mut report = report();
        for id in 1..=2 {
            report.storage.insert(
                domain::ids::PlantId(id),
                domain::generation::BatteryTotals::default(),
            );
        }
        let mut model = model_of(&report, &[], &l10n);
        model.rate_review = Some(l10n.text("review-rate-review"));
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 4000.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw_summary(ui, &model);
                draw_merit(ui, &model);
                draw_storage(ui, &model);
            },
        );
        let shapes = output.shapes.clone();
        output.drop_without_applying_deltas();
        let texts: Vec<_> = shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        let text = |label: &str| {
            texts
                .iter()
                .find(|text| text.galley.text() == label)
                .unwrap()
        };
        let fleet = text(&model.storage[0].0).visual_bounding_rect();
        let battery = text(&model.storage[1].0).visual_bounding_rect();
        let fleet_bottom = texts
            .iter()
            .map(|text| text.visual_bounding_rect())
            .filter(|rect| rect.top() > fleet.bottom() && rect.bottom() < battery.top())
            .map(|rect| rect.bottom())
            .fold(fleet.bottom(), f32::max);
        assert!(
            shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::LineSegment { points, .. }
                    if points[0].y > fleet_bottom && points[0].y < battery.top()
            )),
            "fleet totals need a separator before the first battery"
        );
        let body_size = egui::TextStyle::Body.resolve(&ctx.global_style()).size;
        for heading in [
            &model.regulatory_heading,
            &model.rate_review_heading,
            &model.lessons_heading,
            &model.pnl_heading,
            &model.reliability_heading,
            &model.incidents_heading,
            &model.merit_heading,
            &model.storage_heading,
        ]
        .into_iter()
        .chain(model.storage.iter().map(|(name, _)| name))
        {
            assert!(
                text(heading).galley.job.sections[0].format.font_id.size > body_size,
                "{heading} needs heading-sized text"
            );
        }
        assert!(l10n.problems().is_empty());
    }

    #[test]
    fn battery_storage_has_a_third_review_column_with_a_visible_footer() {
        let l10n = shipped_l10n();
        let mut report = report();
        report.storage.insert(
            domain::ids::PlantId(1),
            domain::generation::BatteryTotals::default(),
        );
        let model = model_of(&report, &[], &l10n);
        fn text_shape<'a>(
            shape: &'a egui::Shape,
            text: &str,
        ) -> Option<&'a egui::epaint::TextShape> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_shape(shape, text)),
                egui::Shape::Text(shape) if shape.galley.text() == text => Some(shape),
                _ => None,
            }
        }
        for width in [1024.0, 1024.0 / 1.25, 1920.0] {
            let ctx = egui::Context::default();
            let screen =
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 640.0 / 1.25));
            for frame in 0..3 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ui| {
                        draw_review(ui, &model);
                    },
                );
                if frame == 2 {
                    let painted_text = |text| {
                        output
                            .shapes
                            .iter()
                            .find_map(|shape| text_shape(&shape.shape, text))
                            .unwrap_or_else(|| panic!("missing {text}"))
                    };
                    let at = |text| painted_text(text).visual_bounding_rect();
                    let summary = at(&model.regulatory_heading);
                    let merit = at(&model.merit_heading);
                    let storage = at(&model.storage_heading);
                    assert!(
                        summary.left() < merit.left() && merit.left() < storage.left(),
                        "storage needs its own third column at {width}"
                    );
                    assert!(
                        (summary.top() - storage.top()).abs() <= 2.0,
                        "{width}: summary {summary:?}, storage {storage:?}"
                    );
                    assert!(screen.contains_rect(at(&model.footer_button.label)));
                    let pnl = at(&model.pnl_heading);
                    assert!(summary.bottom() < pnl.top(), "regulation leads the detail");
                    assert!(at(&model.lessons_heading).bottom() < pnl.top());
                    for stat in &model.highlights {
                        let label = at(&stat.label);
                        let value = at(&stat.value);
                        assert!(screen.contains_rect(label), "{width}: {label:?}");
                        assert!(screen.contains_rect(value), "{width}: {value:?}");
                        assert!(value.bottom() < summary.top());
                        // Compare font sizes: glyph bounds vary with the characters in the text.
                        assert!(
                            painted_text(&stat.value).galley.job.sections[0]
                                .format
                                .font_id
                                .size
                                > painted_text(&model.regulatory_heading).galley.job.sections[0]
                                    .format
                                    .font_id
                                    .size
                        );
                    }
                }
                output.drop_without_applying_deltas();
            }
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn review_model_lists_pl_lines_in_order() {
        let l10n = shipped_l10n();
        let game = reviewed_game();
        let report = game.projections().monthly_reports.latest().unwrap();

        let model = review_model(&game, GameOverView::Summary, &l10n, &usd_style())
            .expect("the month is in review");

        let lines: Vec<(&str, &str)> = model
            .pnl
            .iter()
            .map(|stat| (stat.label.as_str(), stat.value.as_str()))
            .collect();
        let money = |amount| format::money(amount, &usd_style());
        assert_eq!(
            lines,
            [
                ("Revenue", money(report.revenue).as_str()),
                ("Fuel", money(report.fuel).as_str()),
                ("Start-ups", money(report.start_up).as_str()),
                (
                    "Fixed operations and maintenance",
                    money(report.fixed_om).as_str()
                ),
                ("Construction", money(report.construction).as_str()),
                ("Net", money(report.net).as_str()),
            ]
        );
        assert!(report.revenue > Money::ZERO && report.fuel < Money::ZERO);
        assert_eq!(model.pnl_heading, "Profit and loss");
        assert_eq!(
            model.days_note,
            format!(
                "Each played day stands for {} days of the month.",
                report.days
            )
        );
        assert_eq!(model.title, "Month in review");
        assert_eq!(model.month, format::month_year(report.month));
        // The merit order used, cheapest first, names the plants that ran.
        assert_eq!(model.merit.len(), report.merit_order.len());
        assert!(!model.merit.is_empty());
        let names = Names::new(&l10n, game.session());
        let first = &report.merit_order[0];
        assert_eq!(
            model.merit[0],
            MeritRow {
                plant: names.plant(first.plant),
                kind: first.kind.localize(&l10n),
                marginal_cost: format::price(first.marginal_cost, &usd_style()),
                marginal_cost_fraction: (first.marginal_cost.value()
                    / game
                        .session()
                        .region()
                        .spec(PlantKind::Peaker)
                        .marginal_cost
                        .value()) as f32,
                energy: format::megawatt_hours(first.energy),
                has_energy: first.energy > MegawattHours::ZERO,
            }
        );
        assert_eq!(
            model.lessons,
            game.projections()
                .lessons
                .latest()
                .iter()
                .map(|lesson| lesson.localize_money(&l10n, &usd_style()))
                .collect::<Vec<_>>()
        );
        assert!(!model.lessons.is_empty());
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn review_model_splits_planned_and_unplanned_unserved() {
        let l10n = shipped_l10n();

        let model = model_of(&report(), &[], &l10n);

        let lines: Vec<(&str, &str)> = model
            .reliability
            .iter()
            .map(|stat| (stat.label.as_str(), stat.value.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                ("Served", "99.9 GWh"),
                ("Unserved (planned rotating outages)", "40 MWh"),
                ("Unserved (protection shedding)", "60 MWh"),
                ("Time out of frequency band", "2.0%"),
            ]
        );

        // Summed separately over the month, the two can differ by noise below zero: that is none.
        let mut all_planned = report();
        all_planned.rotating_outage = MegawattHours::new(100.000_000_000_1);
        let model = model_of(&all_planned, &[], &l10n);
        assert_eq!(model.reliability[2].value, "0.0 MWh");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn review_model_shows_strike_reason_from_events() {
        let l10n = shipped_l10n();
        // The month failed both tests: one strike, for unserved energy, and the frequency
        // lesson says "no notice" though frequency drew no strike of its own.
        let mut failed = report();
        failed.frequency_failed = true;
        failed.frequency_notice = false;
        let strike = DomainEvent::StrikeIssued {
            strikes: 1,
            reason: StrikeReason::UnservedEnergy,
        };
        let lessons = [Lesson::FrequencyPerformance {
            out_of_band: failed.out_of_band_share,
            notice: false,
        }];

        let model = review_model_for(
            &failed,
            &lessons,
            &[strike],
            &Names::empty(&l10n),
            &test_region(),
            false,
            (&l10n, &usd_style()),
        );

        assert_eq!(model.regulatory.len(), 1);
        let line = &model.regulatory[0];
        assert!(line.contains("strike 1"), "{line}");
        assert!(
            line.contains(&l10n.text("strike-reason-unserved-energy")),
            "{line}"
        );
        assert!(
            !line.contains(&l10n.text("strike-reason-frequency-performance")),
            "{line}"
        );
        assert_eq!(model.regulatory_empty, None);

        // A notice is shown as a notice; a month the regulator let pass says so.
        let notice = DomainEvent::FrequencyNoticeIssued {
            month: failed.month,
        };
        let model = model_of(&failed, &[notice], &l10n);
        assert!(
            model.regulatory[0].contains("formal notice"),
            "{:?}",
            model.regulatory
        );
        let quiet = model_of(&report(), &[], &l10n);
        assert_eq!(quiet.regulatory, Vec::<String>::new());
        assert_eq!(
            quiet.regulatory_empty.as_deref(),
            Some("The regulator took no action this month.")
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_played_months_strike_reaches_the_review_from_the_regulators_events() {
        let l10n = shipped_l10n();
        // Nothing committed: the day is one long rotating outage, and the regulator strikes.
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        empty_start(&mut game);
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: TICKS_PER_DAY })
            .unwrap();
        let events = game.projections().regulatory.latest();
        assert!(
            events.iter().any(|event| matches!(
                event,
                DomainEvent::StrikeIssued {
                    reason: StrikeReason::UnservedEnergy,
                    ..
                }
            )),
            "{events:?}"
        );

        let model = review_model(&game, GameOverView::Summary, &l10n, &usd_style()).unwrap();

        let names = Names::new(&l10n, game.session());
        let expected: Vec<String> = events
            .iter()
            .filter_map(|event| event.toast(&l10n, &names, &usd_style()))
            .collect();
        assert_eq!(model.regulatory, expected);
        assert!(
            model.regulatory[0].contains(&l10n.text("strike-reason-unserved-energy")),
            "{:?}",
            model.regulatory
        );
        assert_eq!(model.regulatory_empty, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn continue_emits_finish_review() {
        let l10n = shipped_l10n();
        let game = reviewed_game();

        let model = review_model(&game, GameOverView::Summary, &l10n, &usd_style()).unwrap();

        assert_eq!(model.footer_button.label, "Continue");
        assert_eq!(
            model.footer_button.action,
            UiAction::Command(Command::FinishReview)
        );
        assert!(model.footer_button.enabled);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn after_game_over_the_footer_is_back_to_the_summary_never_finish_review() {
        let l10n = shipped_l10n();
        let lessons = [Lesson::AllClear];
        let names = Names::empty(&l10n);

        let region = test_region();
        let over = review_model_for(
            &report(),
            &lessons,
            &[],
            &names,
            &region,
            true,
            (&l10n, &usd_style()),
        );
        let running = review_model_for(
            &report(),
            &lessons,
            &[],
            &names,
            &region,
            false,
            (&l10n, &usd_style()),
        );

        assert_eq!(over.footer_button.label, "Back");
        assert_eq!(over.footer_button.action, UiAction::ShowGameOverSummary);
        assert!(over.footer_button.enabled);
        assert_eq!(running.footer_button.label, "Continue");
        assert_eq!(
            running.footer_button.action,
            UiAction::Command(Command::FinishReview)
        );
        // Only the footer differs.
        assert_eq!(
            ReviewModel {
                storage_heading: l10n.text("battery-review-heading"),
                storage_note: l10n.text("battery-review-note"),
                storage: Vec::new(),
                footer_button: running.footer_button.clone(),
                ..over
            },
            running
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_review_shows_only_in_review_and_never_over_game_over() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        assert_eq!(
            review_model(&game, GameOverView::Summary, &l10n, &usd_style()),
            None,
            "Planning"
        );

        use GameOverView::{FinalMonth, Summary};
        // A running game: in Review only, whatever the (stale) game-over view says.
        for view in [Summary, FinalMonth] {
            assert!(review_applies(
                SessionStatus::Active,
                DayPhase::Review,
                view
            ));
            for phase in [DayPhase::Planning, DayPhase::Operating] {
                assert!(!review_applies(SessionStatus::Active, phase, view));
            }
        }
        // A finished game: only when the player asked for the final month.
        for reason in [GameOverReason::Bankrupt, GameOverReason::LicenseRevoked] {
            let over = SessionStatus::GameOver(reason);
            assert!(review_applies(over, DayPhase::Review, FinalMonth));
            assert!(!review_applies(over, DayPhase::Review, Summary));
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_finished_game_reviews_its_final_month_only_on_request() {
        let l10n = shipped_l10n();
        let game = finished_game();
        let report = game.projections().monthly_reports.latest().unwrap();

        assert_eq!(
            review_model(&game, GameOverView::Summary, &l10n, &usd_style()),
            None
        );
        let model = review_model(&game, GameOverView::FinalMonth, &l10n, &usd_style())
            .expect("the final month's review");

        assert_eq!(model.month, format::month_year(report.month));
        assert_eq!(model.footer_button.label, "Back");
        assert_eq!(model.footer_button.action, UiAction::ShowGameOverSummary);
        assert!(
            model
                .regulatory
                .iter()
                .any(|line| line.contains(&l10n.text("strike-reason-unserved-energy"))),
            "{:?}",
            model.regulatory
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_timeline_is_oldest_first_and_december_shows_the_rate_review() {
        let l10n = shipped_l10n();
        let mut december = report();
        december.incidents = vec![
            Incident {
                tick: 372,
                kind: IncidentKind::Blackout,
            },
            Incident {
                tick: 1_030,
                kind: IncidentKind::BlackoutRestored,
            },
        ];
        december.rate_review = Some(RateReviewOutcome {
            tariff: Price::per_mwh(101.0),
            allowed_return: Ratio::new(0.09).unwrap(),
            revenue_requirement: Money::from_dollars(1_000_000),
            cost_per_mwh: Some(60.0),
        });

        let model = model_of(&december, &[], &l10n);

        assert_eq!(
            model.incidents,
            [
                "06:12: Blackout: protection tripped every plant.",
                "17:10: Power restored after the blackout.",
            ]
        );
        assert_eq!(model.incidents_empty, None);
        assert_eq!(model.rate_review_heading, "Rate review");
        assert_eq!(
            model.rate_review.as_deref(),
            Some("Your tariff is now $101.00/MWh and your allowed return is 9.0%.")
        );
        let quiet = model_of(&report(), &[], &l10n);
        assert_eq!(quiet.rate_review, None);
        assert_eq!(
            quiet.incidents_empty.as_deref(),
            Some("No incidents this month.")
        );
        assert_eq!(l10n.problems(), vec![]);
    }

    proptest::proptest! {
        #[test]
        fn the_review_fills_available_width_below_the_top_bar(
            width in 200u16..4000, height in 80u16..2400, top in 0u16..100,
        ) {
            let behind = egui::Rect::from_min_size(
                egui::pos2(0.0, f32::from(top)),
                egui::vec2(f32::from(width), f32::from(height)),
            );
            let modal = review_rect(behind);
            proptest::prop_assert_eq!(modal.left(), behind.left() + REVIEW_MARGIN);
            proptest::prop_assert_eq!(modal.right(), behind.right() - REVIEW_MARGIN);
            proptest::prop_assert_eq!(modal.top(), behind.top() + REVIEW_MARGIN);
            proptest::prop_assert_eq!(modal.bottom(), behind.bottom() - REVIEW_MARGIN);
        }
    }
}
