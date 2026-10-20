//! The top bar, shown in every in-game phase (spec §8.4): the date, the phase and the company's
//! scores, the recording-stopped warning, and the way back to the menu.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::action::{ActionSink, ButtonModel, UiAction, button, dispatch};
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
    pub menu: ButtonModel,
}

pub fn top_bar_model(game: &Game, l10n: &Localizer) -> TopBarModel {
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
        strikes: stat("topbar-strikes", company.strikes.to_string()),
        recording_stopped: game.log_failure().map(|failure| {
            l10n.args(
                "recording-stopped",
                &[("reason", failure.message.as_str().into())],
            )
        }),
        menu: ButtonModel::new(l10n.text("topbar-menu"), UiAction::ReturnToMenu),
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

pub(super) fn top_bar_panel(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let model = top_bar_model(&session.0, &l10n);
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

        let model = top_bar_model(&game, &l10n);

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
        assert_eq!(model.strikes.value, "0");
        assert_eq!(model.recording_stopped, None);
        assert_eq!(model.menu.action, UiAction::ReturnToMenu);
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn top_bar_shows_recording_stopped() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(FullDisk));
        assert_eq!(top_bar_model(&game, &l10n).recording_stopped, None);

        // The game carries on after the log fails to record this command.
        game.execute(Command::BeginOperating).unwrap();

        let model = top_bar_model(&game, &l10n);
        let warning = model.recording_stopped.expect("the warning is shown");
        assert!(warning.starts_with("Recording stopped"), "{warning}");
        assert!(warning.contains("disk full"), "{warning}");
        assert_eq!(model.phase, l10n.text("phase-operating"));
        assert_eq!(l10n.problems(), vec![]);
    }
}
