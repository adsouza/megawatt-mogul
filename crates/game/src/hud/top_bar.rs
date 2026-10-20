//! The top bar, shown in every in-game phase (spec §8.4): the date, the phase and the company's
//! scores, the recording-stopped warning, and the way back to the menu, which asks first.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::action::{ActionSink, ButtonModel, OpenPrompt, Prompt, UiAction, button, dispatch};
use super::layout::HudArea;
use crate::l10n::{Localize, Localizer, format};
use crate::sim::Session;

/// A labelled value, such as *Cash* and `$1.23M`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub label: String,
    pub value: String,
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
    /// Opens the leave prompt; it does not leave by itself.
    pub menu: ButtonModel,
    /// Shown while the player is being asked whether to leave.
    pub leave_prompt: Option<LeavePromptModel>,
}

/// "Leave this game?": leaving drops the game, which can't be resumed yet.
#[derive(Debug, Clone, PartialEq)]
pub struct LeavePromptModel {
    pub question: String,
    pub leave: ButtonModel,
    pub stay: ButtonModel,
}

pub fn top_bar_model(game: &Game, l10n: &Localizer, prompt: OpenPrompt) -> TopBarModel {
    let session = game.session();
    let grid = session.grid_snapshot();
    let company = session.company_view();
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
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
        recording_stopped: game.log_failure().map(|failure| {
            l10n.args(
                "recording-stopped",
                &[("reason", failure.message.as_str().into())],
            )
        }),
        menu: ButtonModel::new(
            l10n.text("topbar-menu"),
            UiAction::Prompt(Some(Prompt::LeaveGame)),
        ),
        leave_prompt: (prompt.0 == Some(Prompt::LeaveGame)).then(|| LeavePromptModel {
            question: l10n.text("leave-game-question"),
            leave: ButtonModel::new(l10n.text("leave-game-leave"), UiAction::ReturnToMenu),
            stay: ButtonModel::new(l10n.text("leave-game-stay"), UiAction::Prompt(None)),
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
        if let Some(warning) = &model.recording_stopped {
            ui.separator();
            ui.colored_label(ui.visuals().warn_fg_color, warning.as_str());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            button(ui, &model.menu, &mut actions);
        });
    });
    actions
}

pub fn draw_leave_prompt(ui: &mut egui::Ui, model: &LeavePromptModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.label(model.question.as_str());
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        button(ui, &model.leave, &mut actions);
        button(ui, &model.stay, &mut actions);
    });
    actions
}

pub(super) fn top_bar_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    prompt: Res<OpenPrompt>,
    mut sink: ActionSink,
) -> Result {
    let model = top_bar_model(&session.0, &l10n, *prompt);
    let ctx = contexts.ctx_mut()?;
    let mut ui = area.root_ui(ctx, "top_bar");
    let mut actions = egui::Panel::top("top_bar")
        .show(&mut ui, |ui| draw_top_bar(ui, &model))
        .inner;
    area.claim(&ui);
    if let Some(leave) = &model.leave_prompt {
        let shown = egui::Modal::new(egui::Id::new("leave_prompt"))
            .show(ctx, |ui| draw_leave_prompt(ui, leave));
        actions.extend(shown.inner);
        // A click outside the prompt is a "Stay".
        if shown.backdrop_response.clicked() {
            actions.push(leave.stay.action.clone());
        }
    }
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{FullDisk, eastern_game, shipped_l10n};

    #[test]
    fn top_bar_model_formats_company_view() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let session = game.session();
        let company = session.company_view();
        let month = session.grid_snapshot().month;

        let model = top_bar_model(&game, &l10n, OpenPrompt::default());

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
            top_bar_model(&game, &l10n, OpenPrompt::default()).recording_stopped,
            None
        );

        // The game carries on after the log fails to record this command.
        game.execute(Command::BeginOperating).unwrap();

        let model = top_bar_model(&game, &l10n, OpenPrompt::default());
        let warning = model.recording_stopped.expect("the warning is shown");
        assert!(warning.starts_with("Recording stopped"), "{warning}");
        assert!(warning.contains("disk full"), "{warning}");
        assert_eq!(model.phase, l10n.text("phase-operating"));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn strikes_show_the_revocation_limit() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let limit = game.session().region().regulator().strikes_to_revoke;

        let model = top_bar_model(&game, &l10n, OpenPrompt::default());

        assert_eq!(model.strikes.label, l10n.text("topbar-strikes"));
        assert_eq!(model.strikes.value, format!("0/{limit}"));
        assert_eq!(model.strikes.value, "0/3", "the shipped eastern limit");
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn menu_asks_before_leaving() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        // Menu opens the prompt and nothing else: it never leaves by itself.
        let closed = top_bar_model(&game, &l10n, OpenPrompt::default());
        assert_eq!(
            closed.menu.action,
            UiAction::Prompt(Some(Prompt::LeaveGame))
        );
        assert_eq!(closed.leave_prompt, None);

        let open = top_bar_model(&game, &l10n, OpenPrompt(Some(Prompt::LeaveGame)));
        let prompt = open.leave_prompt.expect("the prompt is shown while open");
        assert_eq!(prompt.question, "Leave this game? It can't be resumed yet.");
        // Leave is the only way out of the game; Stay just closes the prompt.
        assert_eq!(prompt.leave.label, "Leave");
        assert_eq!(prompt.leave.action, UiAction::ReturnToMenu);
        assert_eq!(prompt.stay.label, "Stay");
        assert_eq!(prompt.stay.action, UiAction::Prompt(None));
        assert_eq!(l10n.problems(), vec![]);
    }
}
