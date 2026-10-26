//! The game-over modal: why the game ended, the final scores, the way to the final month's review
//! (*See the month*), and the way back to the menu.

use crate::l10n::format::MoneyStyle;
use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use domain::session::{CompanyView, GameOverReason, SessionStatus};

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
use super::top_bar::Stat;
use crate::l10n::{Localize, Localizer, format};
use crate::sim::Session;

/// What a finished game shows: the game-over modal, or the final month's review (the lessons and
/// the regulator's section that explain the loss). Written by `dispatch`, reset with each game.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameOverView {
    /// The game-over modal.
    #[default]
    Summary,
    /// The Review modal for the month that ended the game, with *Back* in place of *Continue*.
    FinalMonth,
}

/// The [`GameOverView`] as it was when this egui pass began. The game-over and Review modals both
/// read it, never `GameOverView` itself: `dispatch` writes that partway through the pass, and the
/// modal drawn second would otherwise see the new value and draw too, two modals in one frame.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameOverViewShown(pub GameOverView);

/// Starts each egui pass by taking the snapshot the two modals decide by.
pub(super) fn snapshot_game_over_view(
    view: Res<GameOverView>,
    mut shown: ResMut<GameOverViewShown>,
) {
    shown.0 = *view;
}

pub(super) fn reset_game_over_view(mut view: ResMut<GameOverView>) {
    *view = GameOverView::Summary;
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameOverModel {
    pub title: String,
    pub reason: String,
    pub cash: Stat,
    pub reliability: Stat,
    pub satisfaction: Stat,
    /// Opens the final month's review. `None` when there is no monthly report to show.
    pub see_month: Option<ButtonModel>,
    pub main_menu: ButtonModel,
    pub save: ButtonModel,
}

/// The modal, while the game is over; `None` while it is still running.
pub fn game_over_model(game: &Game, l10n: &Localizer, style: &MoneyStyle) -> Option<GameOverModel> {
    let session = game.session();
    match session.status() {
        SessionStatus::Active => None,
        SessionStatus::GameOver(reason) => {
            // Every game ends at a month close, so a report always exists; the guard keeps the
            // screen from going blank, with neither modal, should that invariant ever break.
            let has_report = game.projections().monthly_reports.latest().is_some();
            Some(game_over_model_for(
                reason,
                &session.company_view(),
                has_report,
                l10n,
                style,
            ))
        }
    }
}

/// The modal for a game that ended for `reason` with the scores in `company`; `has_report` is
/// whether a monthly report exists for *See the month* to open. Split from
/// [`game_over_model`] because a finished `Game` takes months of play to reach.
pub fn game_over_model_for(
    reason: GameOverReason,
    company: &CompanyView,
    has_report: bool,
    l10n: &Localizer,
    style: &MoneyStyle,
) -> GameOverModel {
    let stat = |key: &str, value: String| Stat {
        label: l10n.text(key),
        value,
    };
    GameOverModel {
        title: l10n.text("game-over-title"),
        reason: reason.localize(l10n),
        cash: stat("game-over-final-cash", format::money(company.cash, style)),
        reliability: stat(
            "game-over-reliability",
            format::percent(company.reliability),
        ),
        satisfaction: stat(
            "game-over-satisfaction",
            format::score(company.satisfaction),
        ),
        see_month: has_report
            .then(|| ButtonModel::new(l10n.text("game-over-see-month"), UiAction::ShowFinalMonth)),
        save: ButtonModel::new(
            l10n.text("save-title"),
            UiAction::Persistence(crate::sim::PersistAction::OpenSave { leave: false }),
        ),
        main_menu: ButtonModel::new(
            l10n.text("game-over-main-menu"),
            UiAction::Persistence(crate::sim::PersistAction::OpenLeave),
        ),
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
        if let Some(see_month) = &model.see_month {
            button(ui, see_month, &mut actions);
        }
        button(ui, &model.save, &mut actions);
        button(ui, &model.main_menu, &mut actions);
    });
    actions
}

pub(super) fn game_over_modal(
    mut contexts: EguiContexts,
    session: Res<Session>,
    l10n: Res<Localizer>,
    shown: Res<GameOverViewShown>,
    mut sink: ActionSink,
    style: Res<MoneyStyle>,
) -> Result {
    // The final month's review stands in for the modal until *Back*.
    if shown.0 == GameOverView::FinalMonth {
        return Ok(());
    }
    let Some(model) = game_over_model(&session.0, &l10n, &style) else {
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
    use crate::hud::test_support::usd_style;
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
            let model = game_over_model_for(reason, &company, true, &l10n, &usd_style());

            assert_eq!(model.title, l10n.text("game-over-title"));
            assert_eq!(model.reason, l10n.text(key));
            assert_eq!(model.cash.label, l10n.text("game-over-final-cash"));
            assert_eq!(model.cash.value, format::money(company.cash, &usd_style()));
            assert_eq!(
                model.reliability.value,
                format::percent(company.reliability)
            );
            assert_eq!(
                model.satisfaction.value,
                format::score(company.satisfaction)
            );
            assert_eq!(
                model.main_menu.action,
                UiAction::Persistence(crate::sim::PersistAction::OpenLeave)
            );
        }
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn see_the_month_shows_only_when_there_is_a_report_to_show() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));
        let company = game.session().company_view();

        let with = game_over_model_for(
            GameOverReason::Bankrupt,
            &company,
            true,
            &l10n,
            &usd_style(),
        );
        let without = game_over_model_for(
            GameOverReason::Bankrupt,
            &company,
            false,
            &l10n,
            &usd_style(),
        );

        let button = with.see_month.expect("a report exists");
        assert_eq!(button.label, "See the month");
        assert_eq!(button.action, UiAction::ShowFinalMonth);
        assert!(button.enabled);
        assert_eq!(without.see_month, None);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn the_view_starts_on_the_summary() {
        assert_eq!(GameOverView::default(), GameOverView::Summary);
    }

    #[test]
    fn a_running_game_has_no_game_over_modal() {
        let l10n = shipped_l10n();
        let game = eastern_game(Box::new(InMemoryCommandLog::default()));

        assert_eq!(game_over_model(&game, &l10n, &usd_style()), None);
    }
}
