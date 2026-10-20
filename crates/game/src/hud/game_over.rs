//! The game-over modal: why the game ended, the final scores, and the way back to the menu.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::{CompanyView, GameOverReason, SessionStatus};

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::top_bar::Stat;
use crate::l10n::{Localize, Localizer, format};
use crate::sim::Session;

#[derive(Debug, Clone, PartialEq)]
pub struct GameOverModel {
    pub title: String,
    pub reason: String,
    pub cash: Stat,
    pub reliability: Stat,
    pub satisfaction: Stat,
    pub main_menu: ButtonModel,
}

/// The modal, while the game is over; `None` while it is still running.
pub fn game_over_model(game: &Game, l10n: &Localizer) -> Option<GameOverModel> {
    let session = game.session();
    match session.status() {
        SessionStatus::Active => None,
        SessionStatus::GameOver(reason) => {
            Some(game_over_model_for(reason, &session.company_view(), l10n))
        }
    }
}

/// The modal for a game that ended for `reason` with the scores in `company`. Split from
/// [`game_over_model`] because a finished `Game` takes months of play to reach.
pub fn game_over_model_for(
    reason: GameOverReason,
    company: &CompanyView,
    l10n: &Localizer,
) -> GameOverModel {
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    GameOverModel {
        title: l10n.text("game-over-title"),
        reason: reason.localize(l10n),
        cash: stat("game-over-final-cash", format::money(company.cash)),
        reliability: stat(
            "game-over-reliability",
            format::percent(company.reliability),
        ),
        satisfaction: stat(
            "game-over-satisfaction",
            format::score(company.satisfaction),
        ),
        main_menu: ButtonModel::new(l10n.text("game-over-main-menu"), UiAction::ReturnToMenu),
    }
}

pub fn draw_game_over(ui: &mut egui::Ui, model: &GameOverModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.set_max_width(420.0);
    ui.vertical_centered(|ui| {
        ui.heading(model.title.as_str());
        ui.add_space(8.0);
        ui.label(model.reason.as_str());
        ui.add_space(12.0);
    });
    egui::Grid::new("game_over_scores")
        .num_columns(2)
        .show(ui, |ui| {
            for stat in [&model.cash, &model.reliability, &model.satisfaction] {
                ui.label(stat.label.as_str());
                ui.strong(stat.value.as_str());
                ui.end_row();
            }
        });
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        button(ui, &model.main_menu, &mut actions);
    });
    actions
}

pub(super) fn game_over_modal(
    mut contexts: EguiContexts,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let Some(model) = game_over_model(&session.0, &l10n) else {
        return Ok(());
    };
    let actions = egui::Modal::new(egui::Id::new("game_over"))
        .show(contexts.ctx_mut()?, |ui| draw_game_over(ui, &model))
        .inner;
    dispatch(actions, &mut sink);
    Ok(())
}

#[cfg(test)]
mod tests {
    use application::InMemoryCommandLog;

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn game_over_model_names_the_reason() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let company = game.session().company_view();

        for (reason, key) in [
            (GameOverReason::Bankrupt, "game-over-bankrupt"),
            (GameOverReason::LicenseRevoked, "game-over-license-revoked"),
        ] {
            let model = game_over_model_for(reason, &company, &l10n);

            assert_eq!(model.title, l10n.text("game-over-title"));
            assert_eq!(model.reason, l10n.text(key));
            assert_eq!(model.cash.label, l10n.text("game-over-final-cash"));
            assert_eq!(model.cash.value, format::money(company.cash));
            assert_eq!(
                model.reliability.value,
                format::percent(company.reliability)
            );
            assert_eq!(
                model.satisfaction.value,
                format::score(company.satisfaction)
            );
            assert_eq!(model.main_menu.action, UiAction::ReturnToMenu);
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn a_running_game_has_no_game_over_modal() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        assert_eq!(game_over_model(&game, &l10n), None);
    }
}
