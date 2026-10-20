//! Confirmation prompts: a question before something the player can't undo.
//!
//! A button opens one with `UiAction::Prompt(Some(..))`; [`dispatch`] stores it in [`OpenPrompt`],
//! and [`prompt_overlay`] draws whichever prompt is open as a modal. Adding a prompt means adding
//! a [`Prompt`] variant and its case in [`prompt_model`]; nothing else.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::SessionStatus;

use super::action::{ActionSink, ButtonModel, OpenPrompt, Prompt, UiAction, button, dispatch};
use crate::l10n::Localizer;
use crate::sim::Session;

#[derive(Debug, Clone, PartialEq)]
pub struct PromptModel {
    pub question: String,
    /// Does the thing asked about.
    pub confirm: ButtonModel,
    /// Backs out.
    pub cancel: ButtonModel,
    /// What a click outside the prompt means.
    pub dismiss: UiAction,
}

/// The prompt to show, if any. A prompt that no longer makes sense shows nothing: leaving a game
/// needs a game, and once the game is over its own modal is the only one.
pub fn prompt_model(
    open: OpenPrompt,
    game: Option<&Game>,
    l10n: &Localizer,
) -> Option<PromptModel> {
    match open.0? {
        Prompt::LeaveGame => {
            let game = game?;
            prompt_after(open, game.session().status()).0?;
            let stay = UiAction::Prompt(None);
            Some(PromptModel {
                question: l10n.text("leave-game-question"),
                confirm: ButtonModel::new(l10n.text("leave-game-leave"), UiAction::ReturnToMenu),
                cancel: ButtonModel::new(l10n.text("leave-game-stay"), stay.clone()),
                dismiss: stay,
            })
        }
    }
}

/// What stays open once the game is in `status`: an ended game closes any prompt, so no modal
/// stacks on the game-over one.
pub fn prompt_after(open: OpenPrompt, status: SessionStatus) -> OpenPrompt {
    match status {
        SessionStatus::Active => open,
        SessionStatus::GameOver(_) => OpenPrompt(None),
    }
}

pub fn draw_prompt(ui: &mut egui::Ui, model: &PromptModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.label(model.question.as_str());
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        button(ui, &model.confirm, &mut actions);
        button(ui, &model.cancel, &mut actions);
    });
    actions
}

pub(super) fn prompt_overlay(
    mut contexts: EguiContexts,
    session: Option<Res<Session>>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = prompt_model(sink.open_prompt(), session.as_deref().map(|s| &s.0), &l10n)
    else {
        return Ok(());
    };
    let shown = egui::Modal::new(egui::Id::new("prompt"))
        .show(contexts.ctx_mut()?, |ui| draw_prompt(ui, &model));
    let mut actions = shown.inner;
    if shown.backdrop_response.clicked() {
        actions.push(model.dismiss.clone());
    }
    dispatch(actions, &mut sink);
    Ok(())
}

/// Closes the open prompt when the game ends (see [`prompt_after`]).
pub(super) fn close_prompt_when_over(session: Res<Session>, mut open: ResMut<OpenPrompt>) {
    let after = prompt_after(*open, session.0.session().status());
    if after != *open {
        *open = after;
    }
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;
    use domain::session::GameOverReason;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    const LEAVE: OpenPrompt = OpenPrompt(Some(Prompt::LeaveGame));

    #[test]
    fn menu_asks_before_leaving() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        let prompt = prompt_model(LEAVE, Some(&game), &l10n).expect("the prompt is shown");

        assert_eq!(prompt.question, "Leave this game? It can't be resumed yet.");
        // Leave is the only way out of the game; Stay, or a click outside, just closes the prompt.
        assert_eq!(prompt.confirm.label, "Leave");
        assert_eq!(prompt.confirm.action, UiAction::ReturnToMenu);
        assert_eq!(prompt.cancel.label, "Stay");
        assert_eq!(prompt.cancel.action, UiAction::Prompt(None));
        assert_eq!(prompt.dismiss, UiAction::Prompt(None));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn no_prompt_shows_when_none_is_open_or_there_is_no_game() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        assert_eq!(prompt_model(OpenPrompt(None), Some(&game), &l10n), None);
        assert_eq!(prompt_model(LEAVE, None, &l10n), None);
    }

    #[test]
    fn a_finished_game_closes_the_leave_prompt() {
        for reason in [GameOverReason::Bankrupt, GameOverReason::LicenseRevoked] {
            assert_eq!(
                prompt_after(LEAVE, SessionStatus::GameOver(reason)),
                OpenPrompt(None)
            );
        }
        assert_eq!(prompt_after(LEAVE, SessionStatus::Active), LEAVE);
        assert_eq!(
            prompt_after(OpenPrompt(None), SessionStatus::Active),
            OpenPrompt(None)
        );
    }
}
