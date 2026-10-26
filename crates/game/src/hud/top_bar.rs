//! The top bar, shown in every in-game phase (spec §8.4): the date, the phase and the company's
//! scores, the recording-stopped and failed-autosave warnings, the Milestones panel's switch,
//! Save, and the way back to the menu, which asks first.

use crate::l10n::format::MoneyStyle;
use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::time::SimMonth;

use super::action::{ActionSink, ButtonModel, Prompt, UiAction, button, dispatch};
use super::layout::HudArea;
use super::theme::{TextTone, heading_color};
use crate::l10n::{Localize, Localizer, format};
use crate::sim::{PersistAction, Persistence, Session};

/// A labelled value, such as *Cash* and `$1.23M`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub label: String,
    pub value: String,
}

/// The one warning slot in the bar, beside Milestones. Truncated to fit; `tooltip` is the whole.
#[derive(Debug, Clone, PartialEq)]
pub struct WarningsBanner {
    /// One warning's message, or with two or more a count such as `⚠ 2 warnings`: two banners
    /// cut short side by side at 1024×640 read "Recordi…" and "Autosave f…", which say nothing.
    pub text: String,
    /// Every warning's whole message, one per line.
    pub tooltip: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopBarModel {
    /// `March 2027`.
    pub date: String,
    pub time: String,
    pub season: String,
    pub phase: String,
    pub cash: Stat,
    pub reliability: Stat,
    pub satisfaction: Stat,
    pub strikes: Stat,
    /// Cash, reliability, satisfaction and strikes, classified against this region's rules.
    pub score_tones: [TextTone; 4],
    /// Set once the command log has failed: the game goes on, but its recording is incomplete
    /// (Review Focus 4). It stays for the rest of the game.
    pub recording_stopped: Option<String>,
    /// Set while the latest month-close autosave has failed and no save has covered it since
    /// (spec §17.5). It stays after *Continue without saving*. This is the short form the bar
    /// shows, month first so the month survives truncation at 1024×640; the banner's tooltip
    /// adds the advice.
    pub autosave_failed: Option<String>,
    /// What the bar draws for the warnings above; `None` when there are none.
    pub warnings: Option<WarningsBanner>,
    /// Opens or closes the Milestones panel.
    pub milestones: ButtonModel,
    /// Opens the leave prompt (see `prompt.rs`); it does not leave by itself.
    pub menu: ButtonModel,
    pub save: ButtonModel,
    pub last_saved: Option<String>,
}

pub fn top_bar_model(
    game: &Game,
    persistence: &Persistence,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> TopBarModel {
    let session = game.session();
    let grid = session.grid_snapshot();
    let company = session.company_view();
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    let recording_stopped = game.log_failure().map(|failure| {
        l10n.args(
            "recording-stopped",
            &[("reason", failure.message.as_str().into())],
        )
    });
    // The banner names the month short so it fits whole at 1024×640; the tooltip names it in full.
    let autosave_message = |key: &str, month: fn(SimMonth) -> String| {
        persistence
            .autosave_failure()
            .map(|failed| l10n.args(key, &[("month", month(failed.month).into())]))
    };
    let autosave_failed = autosave_message("save-autosave-failed", format::month_year_short);
    let warnings = warnings_banner(
        &[
            autosave_failed.clone().zip(autosave_message(
                "save-autosave-failed-detail",
                format::month_year,
            )),
            recording_stopped
                .clone()
                .map(|message| (message.clone(), message)),
        ],
        l10n,
    );
    TopBarModel {
        date: format::month_year(grid.month),
        time: format::zoned_clock(session.clock().tick(), session.region().system_zone(), l10n),
        season: session.region().season(grid.month).localize(l10n),
        phase: grid.phase.localize(l10n),
        cash: stat("topbar-cash", format::money(company.cash, style)),
        reliability: stat("topbar-reliability", format::percent(company.reliability)),
        satisfaction: stat("topbar-satisfaction", format::score(company.satisfaction)),
        strikes: stat(
            "topbar-strikes",
            l10n.args(
                "topbar-strikes-value",
                &[
                    ("strikes", company.strikes.to_string().into()),
                    (
                        "limit",
                        session
                            .region()
                            .regulator()
                            .strikes_to_revoke
                            .to_string()
                            .into(),
                    ),
                ],
            ),
        ),
        score_tones: score_tones(&company, session.region().regulator()),
        recording_stopped,
        autosave_failed,
        warnings,
        milestones: ButtonModel::new(l10n.text("topbar-milestones"), UiAction::ToggleMilestones),
        last_saved: persistence.last_summary.as_ref().map(|summary| {
            l10n.args(
                "save-last",
                &[
                    ("month", format::month_year(summary.month).into()),
                    ("phase", summary.phase.localize(l10n).into()),
                    ("time", format::clock(summary.tick).into()),
                ],
            )
        }),
        save: ButtonModel::new(
            // "(saved)" would contradict the warning: the latest boundary is not on disk.
            l10n.text(
                if persistence.last_summary.is_some() && persistence.autosave_failure().is_none() {
                    "save-confirmed"
                } else {
                    "save-title"
                },
            ),
            UiAction::Persistence(PersistAction::OpenSave { leave: false }),
        ),
        menu: ButtonModel::new(
            l10n.text("topbar-menu"),
            UiAction::Prompt(Some(Prompt::LeaveGame)),
        ),
    }
}

fn score_tones(
    company: &domain::session::CompanyView,
    regulator: &domain::region::RegulatorParams,
) -> [TextTone; 4] {
    let cash = if company.cash.is_negative() {
        TextTone::Negative
    } else if company.cash == domain::units::Money::ZERO {
        TextTone::Caution
    } else {
        TextTone::Positive
    };
    // The rolling score uses the region's target and monthly shortfall threshold as signposts.
    // A red score signals poor standing; it does not claim a new strike has been issued.
    let reliability = if company.reliability >= regulator.reliability_target {
        TextTone::Positive
    } else if company.reliability.value() < 1.0 - regulator.strike_threshold.value() {
        TextTone::Negative
    } else {
        TextTone::Caution
    };
    // Serious dissatisfaction consumes all the base return's headroom above its minimum,
    // considering the satisfaction contribution alone.
    let satisfaction = if company.satisfaction >= regulator.satisfaction_target {
        TextTone::Positive
    } else if regulator.base_return.value()
        + domain::regulation::satisfaction_adjustment(company.satisfaction, regulator)
        <= regulator.min_return.value()
    {
        TextTone::Negative
    } else {
        TextTone::Caution
    };
    let strikes = if company.licence == domain::regulation::Licence::Revoked {
        TextTone::Negative
    } else if company.strikes == 0 {
        TextTone::Positive
    } else if company.strikes.saturating_add(1) >= regulator.strikes_to_revoke {
        TextTone::Negative
    } else {
        TextTone::Caution
    };
    [cash, reliability, satisfaction, strikes]
}

/// One warning is shown as itself; two or more as a count. Each warning is the short form the
/// bar shows and the whole message, which the tooltip gives, one per line.
fn warnings_banner(
    warnings: &[Option<(String, String)>],
    l10n: &Localizer,
) -> Option<WarningsBanner> {
    let shown: Vec<&(String, String)> = warnings.iter().flatten().collect();
    let tooltip = shown
        .iter()
        .map(|(_, whole)| whole.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    match shown.as_slice() {
        [] => None,
        [(short, _)] => Some(WarningsBanner {
            text: short.clone(),
            tooltip,
        }),
        several => Some(WarningsBanner {
            text: l10n.args(
                "topbar-warnings",
                &[("count", (several.len() as f64).into())],
            ),
            tooltip,
        }),
    }
}

/// Measure the localized content before assigning the warning its remaining space. Long
/// seasons, currency fallbacks and UI zoom can need a second row even at the minimum window.
fn top_bar_width(ui: &egui::Ui, model: &TopBarModel) -> f32 {
    let stats = [
        &model.cash,
        &model.reliability,
        &model.satisfaction,
        &model.strikes,
    ];
    let mut texts = vec![
        model.date.as_str(),
        &model.time,
        &model.season,
        &model.phase,
        &model.menu.label,
        &model.save.label,
        &model.milestones.label,
    ];
    for stat in stats {
        texts.extend([stat.label.as_str(), stat.value.as_str()]);
    }
    if let Some(warning) = &model.warnings
        && model.autosave_failed.is_some()
    {
        texts.push(&warning.text);
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width: f32 = texts
        .iter()
        .map(|text| {
            ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap((*text).to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
        })
        .sum();
    // A recording error can contain arbitrarily long diagnostics. Its tooltip retains the
    // full reason; reserve a compact indicator rather than wrapping for diagnostic text.
    let recording = if model.warnings.is_some() && model.autosave_failed.is_none() {
        64.0
    } else {
        0.0
    };
    width + texts.len() as f32 * 4.0 + 6.0 * ui.spacing().button_padding.x + 40.0 + recording
}

fn draw_scores(ui: &mut egui::Ui, model: &TopBarModel) {
    for (stat, tone) in [
        &model.cash,
        &model.reliability,
        &model.satisfaction,
        &model.strikes,
    ]
    .into_iter()
    .zip(model.score_tones)
    {
        ui.separator();
        ui.colored_label(TextTone::Neutral.color(), &stat.label);
        ui.label(
            egui::RichText::new(&stat.value)
                .strong()
                .color(tone.color()),
        );
    }
}

fn draw_warning(ui: &mut egui::Ui, model: &TopBarModel) {
    if let Some(warnings) = &model.warnings {
        ui.add(
            egui::Label::new(egui::RichText::new(&warnings.text).color(TextTone::Caution.color()))
                .truncate()
                .show_tooltip_when_elided(false),
        )
        .on_hover_text(&warnings.tooltip);
        ui.separator();
    }
}

pub fn draw_top_bar(ui: &mut egui::Ui, model: &TopBarModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let two_rows = top_bar_width(ui, model) > ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            egui::RichText::new(&model.date)
                .strong()
                .color(TextTone::Caution.color()),
        );
        ui.colored_label(TextTone::Information.color(), &model.time);
        ui.colored_label(TextTone::Neutral.color(), &model.season);
        ui.separator();
        ui.colored_label(heading_color(), &model.phase);
        if !two_rows {
            draw_scores(ui, model);
        }
        // Actions keep the right edge. When space is tight the scores and warning share a
        // separate row, preserving the complete failed month rather than hiding it in a tooltip.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            button(ui, &model.menu, &mut actions);
            let save = button(ui, &model.save, &mut actions);
            if let Some(status) = &model.last_saved {
                save.on_hover_text(status);
            }
            button(ui, &model.milestones, &mut actions);
            if !two_rows {
                draw_warning(ui, model);
            }
        });
    });
    if two_rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            draw_scores(ui, model);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                draw_warning(ui, model)
            });
        });
    }
    actions
}

pub(super) fn top_bar_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
    style: Res<MoneyStyle>,
) -> Result {
    let model = top_bar_model(&session.0, sink.persistence(), &l10n, &style);
    let mut ui = area.root_ui(contexts.ctx_mut()?, "top_bar");
    let actions = egui::Panel::top("top_bar")
        .show(&mut ui, |ui| draw_top_bar(ui, &model))
        .inner;
    area.claim(&ui);
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::hud::test_support::usd_style;
    use application::{CheckpointSummary, Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{FullDisk, eastern_game, shipped_l10n};

    #[test]
    fn top_bar_shows_system_time_with_zone() {
        let l10n = crate::hud::test_support::global_l10n();
        let mut game = crate::hud::test_support::global_game();
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 860 }).unwrap();
        assert_eq!(
            top_bar_model(&game, &Persistence::default(), &l10n, &usd_style()).time,
            "14:20 MSK"
        );
        let legacy = crate::hud::test_support::legacy_eastern_game();
        assert_eq!(
            top_bar_model(&legacy, &Persistence::default(), &l10n, &usd_style()).time,
            "00:00"
        );
    }
    #[test]
    fn long_regional_labels_keep_the_failed_autosave_month_and_actions_visible() {
        let l10n = crate::hud::test_support::global_l10n();
        let mut game = crate::hud::test_support::edited_global_game(|source| {
            source
                .replace("currency: EUR", "currency: INR")
                .replace("month: 4", "month: 9")
                .replace("Spring", "PostMonsoon")
        });
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 860 }).unwrap();
        let style = MoneyStyle::resolve(game.session().region().currency(), &l10n, |_| false);
        let persistence = Persistence::with_failed_autosave(CheckpointSummary::of(game.session()));
        let model = top_bar_model(&game, &persistence, &l10n, &style);
        assert_eq!(model.season, "Post-monsoon");
        assert!(model.cash.value.starts_with("INR "));
        for zoom in [1.0, 1.25] {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(zoom);
            let screen = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0 / zoom, 640.0 / zoom),
            );
            for frame in 0..4 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..default()
                    },
                    |ui| {
                        egui::Panel::top("top_bar").show(ui, |ui| draw_top_bar(ui, &model));
                    },
                );
                let shapes = output.shapes.clone();
                output.drop_without_applying_deltas();
                if frame < 2 {
                    continue;
                }
                for label in [
                    model.warnings.as_ref().unwrap().text.as_str(),
                    model.menu.label.as_str(),
                    model.save.label.as_str(),
                    model.milestones.label.as_str(),
                ] {
                    let text = shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == label => Some(text),
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("missing {label} at zoom {zoom}"));
                    let glyphs: String = text
                        .galley
                        .rows
                        .iter()
                        .flat_map(|row| row.glyphs.iter().map(|g| g.chr))
                        .collect();
                    assert_eq!(glyphs, label, "complete persistence text at zoom {zoom}");
                    assert!(
                        screen.contains_rect(text.visual_bounding_rect()),
                        "{label} fits at zoom {zoom}"
                    );
                }
            }
        }
    }

    #[test]
    fn season_label_follows_the_region_calendar() {
        let game = crate::hud::test_support::global_game();
        assert_eq!(
            top_bar_model(
                &game,
                &Persistence::default(),
                &shipped_l10n(),
                &usd_style()
            )
            .season,
            "Autumn"
        );
    }

    #[test]
    fn top_bar_model_formats_company_view() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let company = session.company_view();
        let month = session.grid_snapshot().month;

        let model = top_bar_model(&game, &Persistence::default(), &l10n, &usd_style());

        assert_eq!(model.date, format::month_year(month));
        assert_eq!(model.season, month.season().localize(&l10n));
        assert_eq!(model.phase, l10n.text("phase-planning"));
        assert_eq!(model.cash.label, l10n.text("topbar-cash"));
        assert_eq!(model.cash.value, format::money(company.cash, &usd_style()));
        assert!(model.cash.value.starts_with('$'), "{}", model.cash.value);
        assert_eq!(
            model.reliability.value,
            format::percent(company.reliability)
        );
        assert!(model.reliability.value.ends_with('%'));
        assert_eq!(
            model.satisfaction.value,
            format::score(company.satisfaction)
        );
        assert_eq!(
            model.score_tones,
            [
                TextTone::Positive,
                TextTone::Positive,
                TextTone::Caution,
                TextTone::Positive
            ]
        );
        assert_eq!(model.recording_stopped, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn top_bar_scores_use_region_targets_and_remaining_strikes() {
        use domain::{
            regulation::Licence,
            units::{Money, Ratio},
        };

        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut company = game.session().company_view();
        let mut regulator = game.session().region().regulator().clone();
        // Different targets prove these are region data, not shipped constants.
        regulator.reliability_target = Ratio::new(0.98).unwrap();
        regulator.strike_threshold = Ratio::new(0.04).unwrap();
        regulator.satisfaction_target = 80.0;
        regulator.strikes_to_revoke = 5;
        company.cash = Money::ZERO;
        company.reliability = Ratio::new(0.97).unwrap();
        company.satisfaction = 70.0;
        company.strikes = 3;
        assert_eq!(score_tones(&company, &regulator), [TextTone::Caution; 4]);

        company.cash = Money::from_cents(-1);
        company.reliability = Ratio::new(0.95).unwrap();
        company.satisfaction = 0.0;
        company.strikes = 4;
        assert_eq!(score_tones(&company, &regulator), [TextTone::Negative; 4]);

        company.cash = Money::from_cents(1);
        company.reliability = regulator.reliability_target;
        company.satisfaction = regulator.satisfaction_target;
        company.strikes = 0;
        assert_eq!(score_tones(&company, &regulator), [TextTone::Positive; 4]);
        company.licence = Licence::Revoked;
        assert_eq!(score_tones(&company, &regulator)[3], TextTone::Negative);
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(32))]
        #[test]
        fn worsening_top_bar_scores_never_improves_their_color(
            cash in -1_000_000i64..1_000_000,
            loss in 0i64..1_000_000,
            reliability in 0.0f64..=1.0,
            other_reliability in 0.0f64..=1.0,
            satisfaction in 0.0f64..=100.0,
            other_satisfaction in 0.0f64..=100.0,
            strikes in 0u8..=3,
            other_strikes in 0u8..=3,
        ) {
            let game = eastern_game(Box::new(InMemoryCommandLog::default()));
            let mut company = game.session().company_view();
            company.cash = domain::units::Money::from_cents(cash);
            company.reliability = domain::units::Ratio::new(reliability).unwrap();
            company.satisfaction = satisfaction;
            company.strikes = strikes;
            let regulator = game.session().region().regulator();
            let before = score_tones(&company, regulator);
            company.cash = domain::units::Money::from_cents(cash - loss);
            company.reliability = domain::units::Ratio::new(reliability.min(other_reliability)).unwrap();
            company.satisfaction = satisfaction.min(other_satisfaction);
            company.strikes = strikes.max(other_strikes);
            let rank = |tone| match tone {
                TextTone::Positive => 0,
                TextTone::Caution => 1,
                TextTone::Negative => 2,
                _ => panic!("scores need a status color"),
            };
            for (before, after) in before.into_iter().zip(score_tones(&company, regulator)) {
                proptest::prop_assert!(rank(after) >= rank(before));
            }
        }
    }

    #[test]
    fn top_bar_shows_recording_stopped() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(FullDisk));
        assert_eq!(
            top_bar_model(&game, &Persistence::default(), &l10n, &usd_style()).recording_stopped,
            None
        );

        // The game carries on after the log fails to record this command.
        game.execute(Command::BeginOperating).unwrap();

        let model = top_bar_model(&game, &Persistence::default(), &l10n, &usd_style());
        let warning = model.recording_stopped.expect("the warning is shown");
        assert!(warning.starts_with("Recording stopped"), "{warning}");
        assert!(warning.contains("disk full"), "{warning}");
        assert_eq!(model.phase, l10n.text("phase-operating"));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_failed_autosave_warns_with_its_month_and_the_save_label_does_not_say_saved() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let failed = CheckpointSummary::of(game.session());
        let mut persistence = Persistence::with_failed_autosave(failed.clone());
        // A save was confirmed earlier: without the failure the button would say "(saved)".
        persistence.last_summary = Some(failed.clone());

        let model = top_bar_model(&game, &persistence, &l10n, &usd_style());

        // The warning and the label never contradict each other (Review Focus 2).
        assert_ne!(model.save.label, l10n.text("save-confirmed"));
        assert_eq!(model.save.label, l10n.text("save-title"));
        let month = format::month_year(failed.month);
        let short = format::month_year_short(failed.month);
        let warning = model.autosave_failed.expect("the warning is shown");
        // Short enough to fit whole at 1024×640: what failed, and the abbreviated month.
        assert_eq!(warning, format!("Autosave failed: {short}"));
        // The tooltip gives the whole month and the advice.
        let tooltip = model.warnings.expect("the bar shows it").tooltip;
        assert!(tooltip.contains(&month), "{tooltip}");
        assert!(tooltip.contains("Save to keep your progress"), "{tooltip}");
        assert!(model.last_saved.is_some(), "the last confirmed save stays");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn one_warning_is_its_own_banner_and_two_become_one_counted_indicator() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(FullDisk));
        let failed = CheckpointSummary::of(game.session());
        assert_eq!(
            top_bar_model(&game, &Persistence::default(), &l10n, &usd_style()).warnings,
            None
        );

        // One warning: the banner is its short form, and the tooltip its whole message.
        let autosave = top_bar_model(
            &game,
            &Persistence::with_failed_autosave(failed.clone()),
            &l10n,
            &usd_style(),
        );
        let message = l10n.args(
            "save-autosave-failed-detail",
            &[("month", format::month_year(failed.month).into())],
        );
        assert_eq!(
            autosave.warnings,
            Some(WarningsBanner {
                text: autosave.autosave_failed.clone().unwrap(),
                tooltip: message.clone(),
            })
        );
        game.execute(Command::BeginOperating).unwrap();
        let recording = top_bar_model(&game, &Persistence::default(), &l10n, &usd_style());
        let reason = recording.recording_stopped.clone().unwrap();
        assert_eq!(
            recording.warnings,
            Some(WarningsBanner {
                text: reason.clone(),
                tooltip: reason.clone(),
            })
        );

        // Two: one indicator with the count; the tooltip has each message on its own line.
        let both = top_bar_model(
            &game,
            &Persistence::with_failed_autosave(failed.clone()),
            &l10n,
            &usd_style(),
        );
        let banner = both.warnings.expect("an indicator");
        assert_eq!(banner.text, "⚠ 2 warnings");
        assert_eq!(banner.tooltip, format!("{message}\n{reason}"));
        assert!(banner.tooltip.contains(&format::month_year(failed.month)));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn without_a_failed_autosave_a_confirmed_save_says_saved() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let mut persistence = Persistence::default();
        assert_eq!(
            top_bar_model(&game, &persistence, &l10n, &usd_style())
                .save
                .label,
            l10n.text("save-title")
        );
        persistence.last_summary = Some(CheckpointSummary::of(game.session()));

        let model = top_bar_model(&game, &persistence, &l10n, &usd_style());

        assert_eq!(model.autosave_failed, None);
        assert_eq!(model.save.label, l10n.text("save-confirmed"));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn strikes_show_the_revocation_limit() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let limit = game.session().region().regulator().strikes_to_revoke;

        let model = top_bar_model(&game, &Persistence::default(), &l10n, &usd_style());

        assert_eq!(model.strikes.label, l10n.text("topbar-strikes"));
        assert_eq!(model.strikes.value, format!("0/{limit}"));
        assert_eq!(model.strikes.value, "0/3", "the shipped eastern limit");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_menu_button_opens_the_leave_prompt_and_does_not_leave() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = top_bar_model(&game, &Persistence::default(), &l10n, &usd_style());

        assert_eq!(model.menu.action, UiAction::Prompt(Some(Prompt::LeaveGame)));
    }

    #[test]
    fn the_milestones_button_toggles_the_panel() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = top_bar_model(&game, &Persistence::default(), &l10n, &usd_style());

        assert_eq!(model.milestones.label, "Milestones");
        assert_eq!(model.milestones.action, UiAction::ToggleMilestones);
        assert!(model.milestones.enabled);
    }
}
