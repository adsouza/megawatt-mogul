//! The top bar, shown in every in-game phase (spec §8.4): the date, the phase and the company's
//! scores, the recording-stopped and failed-autosave warnings, the Milestones panel's switch,
//! Save, and the way back to the menu, which asks first.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::time::SimMonth;

use super::action::{ActionSink, ButtonModel, Prompt, UiAction, button, dispatch};
use super::layout::HudArea;
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
    pub season: String,
    pub phase: String,
    pub cash: Stat,
    pub reliability: Stat,
    pub satisfaction: Stat,
    pub strikes: Stat,
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

pub fn top_bar_model(game: &Game, persistence: &Persistence, l10n: &Localizer) -> TopBarModel {
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
        season: grid.month.season().localize(l10n),
        phase: grid.phase.localize(l10n),
        cash: stat("topbar-cash", format::money(company.cash)),
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

pub fn draw_top_bar(ui: &mut egui::Ui, model: &TopBarModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        ui.strong(model.date.as_str());
        ui.label(model.season.as_str());
        ui.separator();
        ui.label(model.phase.as_str());
        for stat in [
            &model.cash,
            &model.reliability,
            &model.satisfaction,
            &model.strikes,
        ] {
            ui.separator();
            ui.weak(stat.label.as_str());
            ui.strong(stat.value.as_str());
        }
        // Right to left: Menu and Milestones are laid out first, so they always keep their
        // place, and the warning takes only the space between them and the scores. It is cut
        // short to fit, so the bar stays one row and the map doesn't move; the whole of it is in
        // its tooltip.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            button(ui, &model.menu, &mut actions);
            let save = button(ui, &model.save, &mut actions);
            if let Some(status) = &model.last_saved {
                save.on_hover_text(status);
            }
            button(ui, &model.milestones, &mut actions);
            if let Some(warnings) = &model.warnings {
                let text =
                    egui::RichText::new(warnings.text.as_str()).color(ui.visuals().warn_fg_color);
                ui.add(
                    egui::Label::new(text)
                        .truncate()
                        .show_tooltip_when_elided(false),
                )
                .on_hover_text(warnings.tooltip.as_str());
                ui.separator();
            }
        });
    });
    actions
}

pub(super) fn top_bar_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let model = top_bar_model(&session.0, sink.persistence(), &l10n);
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
    use application::{CheckpointSummary, Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{FullDisk, eastern_game, shipped_l10n};

    #[test]
    fn top_bar_model_formats_company_view() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let company = session.company_view();
        let month = session.grid_snapshot().month;

        let model = top_bar_model(&game, &Persistence::default(), &l10n);

        assert_eq!(model.date, format::month_year(month));
        assert_eq!(model.season, month.season().localize(&l10n));
        assert_eq!(model.phase, l10n.text("phase-planning"));
        assert_eq!(model.cash.label, l10n.text("topbar-cash"));
        assert_eq!(model.cash.value, format::money(company.cash));
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
        assert_eq!(model.recording_stopped, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn top_bar_shows_recording_stopped() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(FullDisk));
        assert_eq!(
            top_bar_model(&game, &Persistence::default(), &l10n).recording_stopped,
            None
        );

        // The game carries on after the log fails to record this command.
        game.execute(Command::BeginOperating).unwrap();

        let model = top_bar_model(&game, &Persistence::default(), &l10n);
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

        let model = top_bar_model(&game, &persistence, &l10n);

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
            top_bar_model(&game, &Persistence::default(), &l10n).warnings,
            None
        );

        // One warning: the banner is its short form, and the tooltip its whole message.
        let autosave = top_bar_model(
            &game,
            &Persistence::with_failed_autosave(failed.clone()),
            &l10n,
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
        let recording = top_bar_model(&game, &Persistence::default(), &l10n);
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
            top_bar_model(&game, &persistence, &l10n).save.label,
            l10n.text("save-title")
        );
        persistence.last_summary = Some(CheckpointSummary::of(game.session()));

        let model = top_bar_model(&game, &persistence, &l10n);

        assert_eq!(model.autosave_failed, None);
        assert_eq!(model.save.label, l10n.text("save-confirmed"));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn strikes_show_the_revocation_limit() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let limit = game.session().region().regulator().strikes_to_revoke;

        let model = top_bar_model(&game, &Persistence::default(), &l10n);

        assert_eq!(model.strikes.label, l10n.text("topbar-strikes"));
        assert_eq!(model.strikes.value, format!("0/{limit}"));
        assert_eq!(model.strikes.value, "0/3", "the shipped eastern limit");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_menu_button_opens_the_leave_prompt_and_does_not_leave() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = top_bar_model(&game, &Persistence::default(), &l10n);

        assert_eq!(model.menu.action, UiAction::Prompt(Some(Prompt::LeaveGame)));
    }

    #[test]
    fn the_milestones_button_toggles_the_panel() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let model = top_bar_model(&game, &Persistence::default(), &l10n);

        assert_eq!(model.milestones.label, "Milestones");
        assert_eq!(model.milestones.action, UiAction::ToggleMilestones);
        assert!(model.milestones.enabled);
    }
}
