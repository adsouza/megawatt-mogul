//! The Review modal (spec §8.4): the month just closed, over the dimmed map.
//!
//! The summary column shows P&L (weighted by the month's days, §2.3), reliability, incidents,
//! lessons, regulation and the annual rate review. Merit order and battery storage have their
//! own independently scrolling columns.
//! *Continue* is the way out: it sends `FinishReview`. The backdrop dims the map but not the top
//! bar, so the menu and the milestones stay in reach.
//!
//! A game that ended at this month close shows this modal only when the player asks for it from
//! the game-over modal (*See the month*, [`GameOverView::FinalMonth`]). Then the footer is *Back*,
//! which returns to the game-over modal, and `FinishReview` is never sent: the game is over.

use application::projections::Lesson;
use application::{Command, Game};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::events::DomainEvent;
use domain::session::{DayPhase, MonthlyReport, SessionStatus};
use domain::units::MegawattHours;

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::game_over::{GameOverView, GameOverViewShown};
use super::layout::HudArea;
use super::top_bar::Stat;
use super::{SupplyHistoryModel, UiState, draw_supply_history, supply_history_model};
use crate::l10n::{Localize, LocalizeWith, Localizer, Names, Toast, format};
use crate::sim::Session;

/// The space kept clear between the modal and the edges of the dimmed area.
pub const REVIEW_MARGIN: f32 = 16.0;
/// The height of the separator above the footer button, reserved under the scroll area.
const FOOTER_SEPARATOR: f32 = 6.0;
/// How much the backdrop darkens the map.
const BACKDROP: egui::Color32 = egui::Color32::from_black_alpha(100);

/// One plant's line in the merit order used.
#[derive(Debug, Clone, PartialEq)]
pub struct MeritRow {
    pub plant: String,
    pub kind: String,
    pub marginal_cost: String,
    pub energy: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewModel {
    pub storage_heading: String,
    pub storage_note: String,
    pub storage: Vec<(String, Vec<Stat>)>,
    pub title: String,
    /// `March 2027`.
    pub month: String,
    pub pnl_heading: String,
    /// The month's figures are one played day times the month's days (§2.3).
    pub days_note: String,
    /// Revenue, fuel, start-ups, fixed O&M, construction, then net, as signed money.
    pub pnl: Vec<Stat>,
    pub merit_heading: String,
    /// Plant, kind, marginal cost, energy.
    pub merit_columns: [String; 4],
    /// Cheapest first, as the month dispatched them.
    pub merit: Vec<MeritRow>,
    pub reliability_heading: String,
    /// Served, unserved by planned rotating outages, unserved by protection, time out of band.
    pub reliability: Vec<Stat>,
    pub incidents_heading: String,
    /// `06:12: Protection shed ...`, oldest first.
    pub incidents: Vec<String>,
    /// Shown when there were none.
    pub incidents_empty: Option<String>,
    pub lessons_heading: String,
    pub lessons: Vec<String>,
    pub regulatory_heading: String,
    /// Each strike with its reason, each notice, a lapsed strike, a revoked licence.
    pub regulatory: Vec<String>,
    /// Shown when the regulator did nothing.
    pub regulatory_empty: Option<String>,
    pub rate_review_heading: String,
    /// The annual review's result, in December only.
    pub rate_review: Option<String>,
    /// *Continue*, which sends `FinishReview`; after game over, *Back*, which returns to the
    /// game-over modal.
    pub footer_button: ButtonModel,
    pub supply_button: ButtonModel,
    pub supply: Option<SupplyHistoryModel>,
}

/// The modal while the month is in review; `None` otherwise. Once the game is over, its own modal
/// is the only one, unless `view` asks for the final month.
pub fn review_model(game: &Game, view: GameOverView, l10n: &Localizer) -> Option<ReviewModel> {
    review_model_with_history(game, view, &UiState::default(), l10n)
}

fn review_model_with_history(
    game: &Game,
    view: GameOverView,
    ui: &UiState,
    l10n: &Localizer,
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
        session.status() != SessionStatus::Active,
        l10n,
    );
    if ui.review_supply {
        model.supply = Some(
            supply_history_model(game, ui.supply_month, l10n).with_layout(ui.supply_chart_layout),
        );
        model.supply_button = ButtonModel::new(
            l10n.text("review-summary"),
            UiAction::ShowReviewSupply(false),
        );
    }
    Some(model)
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
pub fn review_model_for(
    report: &MonthlyReport,
    lessons: &[Lesson],
    regulatory: &[DomainEvent],
    names: &Names,
    game_over: bool,
    l10n: &Localizer,
) -> ReviewModel {
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    // `rotating_outage` is part of `unserved` (both are monthly sums, so the difference can carry
    // float noise below zero).
    let unplanned = MegawattHours::new((report.unserved - report.rotating_outage).value().max(0.0));
    let incidents: Vec<String> = report
        .incidents
        .iter()
        .map(|incident| {
            l10n.args(
                "incident-row",
                &[
                    ("time", format::clock(incident.tick).into()),
                    ("incident", incident.kind.localize_with(l10n, names).into()),
                ],
            )
        })
        .collect();
    let regulatory: Vec<String> = regulatory
        .iter()
        .filter_map(|event| event.toast(l10n, names))
        .collect();
    ReviewModel {
        storage_heading: l10n.text("battery-review-heading"),
        storage_note: l10n.text("battery-review-note"),
        storage: storage_review_rows(report, names, l10n),
        title: l10n.text("review-title"),
        month: format::month_year(report.month),
        pnl_heading: l10n.text("review-pnl"),
        days_note: l10n.args(
            "review-days-note",
            &[("days", report.days.to_string().into())],
        ),
        pnl: vec![
            stat("review-revenue", format::money(report.revenue)),
            stat("review-fuel", format::money(report.fuel)),
            stat("review-start-up", format::money(report.start_up)),
            stat("review-fixed-om", format::money(report.fixed_om)),
            stat("review-construction", format::money(report.construction)),
            stat("review-net", format::money(report.net)),
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
                marginal_cost: format::price(entry.marginal_cost),
                energy: format::megawatt_hours(entry.energy),
            })
            .collect(),
        reliability_heading: l10n.text("review-reliability"),
        reliability: vec![
            stat("review-served", format::megawatt_hours(report.served)),
            stat(
                "review-unserved-planned",
                format::megawatt_hours(report.rotating_outage),
            ),
            stat(
                "review-unserved-unplanned",
                format::megawatt_hours(unplanned),
            ),
            stat(
                "review-out-of-band",
                format::percent(report.out_of_band_share),
            ),
        ],
        incidents_heading: l10n.text("review-incidents"),
        incidents_empty: incidents
            .is_empty()
            .then(|| l10n.text("review-incidents-none")),
        incidents,
        lessons_heading: l10n.text("review-lessons"),
        lessons: lessons.iter().map(|lesson| lesson.localize(l10n)).collect(),
        regulatory_heading: l10n.text("review-regulatory"),
        regulatory_empty: regulatory
            .is_empty()
            .then(|| l10n.text("review-regulatory-none")),
        regulatory,
        rate_review_heading: l10n.text("review-rate-review"),
        rate_review: report.rate_review.as_ref().map(|outcome| {
            l10n.args(
                "review-rate-review-result",
                &[
                    ("tariff", format::price(outcome.tariff).into()),
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
        ui.heading(model.title.as_str());
        ui.label(model.month.as_str());
    });
    ui.separator();
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

fn draw_summary(ui: &mut egui::Ui, model: &ReviewModel) {
    let heading = |ui: &mut egui::Ui, text: &str| {
        ui.add_space(6.0);
        ui.strong(text);
    };
    let lines = |ui: &mut egui::Ui, lines: &[String], empty: Option<&String>| {
        if let Some(empty) = empty {
            ui.weak(empty.as_str());
        }
        for line in lines {
            ui.label(line.as_str());
        }
    };

    heading(ui, &model.pnl_heading);
    ui.weak(model.days_note.as_str());
    draw_stats(ui, "review_pnl", &model.pnl);
    heading(ui, &model.reliability_heading);
    draw_stats(ui, "review_reliability", &model.reliability);

    heading(ui, &model.incidents_heading);
    lines(ui, &model.incidents, model.incidents_empty.as_ref());

    heading(ui, &model.lessons_heading);
    for lesson in &model.lessons {
        ui.label(lesson.as_str());
    }

    heading(ui, &model.regulatory_heading);
    lines(ui, &model.regulatory, model.regulatory_empty.as_ref());

    if let Some(result) = &model.rate_review {
        heading(ui, &model.rate_review_heading);
        ui.label(result.as_str());
    }
}

fn draw_stats(ui: &mut egui::Ui, id: &str, stats: &[Stat]) {
    egui::Grid::new(id)
        .num_columns(2)
        .max_col_width((ui.available_width() - ui.spacing().item_spacing.x) / 2.0)
        .show(ui, |ui| {
            for stat in stats {
                ui.label(stat.label.as_str());
                ui.label(stat.value.as_str());
                ui.end_row();
            }
        });
}

fn draw_storage(ui: &mut egui::Ui, model: &ReviewModel) {
    if model.storage.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.strong(&model.storage_heading);
    ui.label(&model.storage_note);
    for (index, (name, rows)) in model.storage.iter().enumerate() {
        ui.strong(name);
        draw_stats(ui, &format!("storage_review_{index}"), rows);
    }
}

fn draw_merit(ui: &mut egui::Ui, model: &ReviewModel) {
    ui.add_space(6.0);
    ui.strong(&model.merit_heading);
    egui::Grid::new("review_merit")
        .num_columns(4)
        .max_col_width((ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0)
        .striped(true)
        .show(ui, |ui| {
            for column in &model.merit_columns {
                ui.weak(column.as_str());
            }
            ui.end_row();
            for row in &model.merit {
                ui.label(row.plant.as_str());
                ui.label(row.kind.as_str());
                ui.label(row.marginal_cost.as_str());
                ui.label(row.energy.as_str());
                ui.end_row();
            }
        });
}

/// Dims the map under the top bar (a backdrop that takes clicks and does nothing with them, so
/// only the footer button leaves Review) and draws the modal over it, in one layer under the
/// floating windows.
pub(super) fn review_modal(
    mut contexts: EguiContexts,
    area: Res<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    shown: Res<GameOverViewShown>,
    mut sink: ActionSink,
) -> Result {
    // The pass's snapshot, not the live view: see `GameOverViewShown`.
    let Some(model) = review_model_with_history(&session.0, shown.0, sink.ui_state(), &l10n) else {
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
            let (rect, _) = ui.allocate_exact_size(behind.size(), egui::Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, BACKDROP);
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
) -> Vec<(String, Vec<Stat>)> {
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
        .map(|(key, energy)| Stat {
            label: l10n.text(key),
            value: format::megawatt_hours(energy),
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
    std::iter::once((l10n.text("battery-fleet"), rows(total)))
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
    use std::collections::BTreeMap;

    use application::InMemoryCommandLog;
    use domain::generation::PlantStatus;
    use domain::regulation::{RateReviewOutcome, StrikeReason};
    use domain::session::{GameOverReason, Incident, IncidentKind};
    use domain::time::{SimMonth, TICKS_PER_DAY};
    use domain::units::{Megawatts, Money, Price, Ratio};

    use super::*;
    use crate::hud::test_support::{eastern_game, empty_start, shipped_l10n};

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
            false,
            l10n,
        )
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
        fn text_rect(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, text)),
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some(shape.visual_bounding_rect())
                }
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
                    let at = |text| {
                        output
                            .shapes
                            .iter()
                            .find_map(|shape| text_rect(&shape.shape, text))
                            .unwrap_or_else(|| panic!("missing {text}"))
                    };
                    let summary = at(&model.pnl_heading);
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

        let model =
            review_model(&game, GameOverView::Summary, &l10n).expect("the month is in review");

        let lines: Vec<(&str, &str)> = model
            .pnl
            .iter()
            .map(|stat| (stat.label.as_str(), stat.value.as_str()))
            .collect();
        let money = |amount| format::money(amount);
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
                marginal_cost: format::price(first.marginal_cost),
                energy: format::megawatt_hours(first.energy),
            }
        );
        assert_eq!(
            model.lessons,
            game.projections()
                .lessons
                .latest()
                .iter()
                .map(|lesson| lesson.localize(&l10n))
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
            false,
            &l10n,
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

        let model = review_model(&game, GameOverView::Summary, &l10n).unwrap();

        let names = Names::new(&l10n, game.session());
        let expected: Vec<String> = events
            .iter()
            .filter_map(|event| event.toast(&l10n, &names))
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

        let model = review_model(&game, GameOverView::Summary, &l10n).unwrap();

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

        let over = review_model_for(&report(), &lessons, &[], &names, true, &l10n);
        let running = review_model_for(&report(), &lessons, &[], &names, false, &l10n);

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
            review_model(&game, GameOverView::Summary, &l10n),
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

        assert_eq!(review_model(&game, GameOverView::Summary, &l10n), None);
        let model =
            review_model(&game, GameOverView::FinalMonth, &l10n).expect("the final month's review");

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
