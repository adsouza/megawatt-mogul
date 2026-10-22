//! The speed controls (spec §8.4), Operating only: the time of day, pause and 1× to 32×, and Space
//! to toggle pause.
//!
//! Space switches between `Paused` and the last speed the day ran at, [`LastSpeed`], which
//! [`dispatch`] records whenever it sets a running speed. It is ignored while egui is taking the
//! keyboard (a focused text field, an open popup) and while a prompt or the game-over modal is up.

use application::Game;
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use bevy_egui::{EguiContexts, egui};
use domain::session::SessionStatus;

use super::action::{ActionSink, ButtonModel, ToggleModel, UiAction, dispatch, toggle_button};
use super::layout::HudArea;
use crate::l10n::{Localizer, format};
use crate::sim::{Session, SimSpeed};

/// The speeds the controls offer, in order, with their labels.
const SPEEDS: [(SimSpeed, &str); 7] = [
    (SimSpeed::Paused, "speed-pause"),
    (SimSpeed::X1, "speed-1x"),
    (SimSpeed::X2, "speed-2x"),
    (SimSpeed::X4, "speed-4x"),
    (SimSpeed::X8, "speed-8x"),
    (SimSpeed::X16, "speed-16x"),
    (SimSpeed::X32, "speed-32x"),
];

/// The last running speed: what Space resumes. `X1` until the player picks another this game.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastSpeed(pub SimSpeed);

impl Default for LastSpeed {
    fn default() -> Self {
        Self(SimSpeed::X1)
    }
}

pub(super) fn reset_last_speed(
    mut last: ResMut<LastSpeed>,
    settings: Option<Res<crate::preferences::ClientSettings>>,
) {
    *last = LastSpeed(settings.map_or(SimSpeed::X1, |s| s.speed()));
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeedModel {
    /// The time of day: `06:00`.
    pub clock: String,
    /// One per speed, in order; the current one is on.
    pub speeds: Vec<ToggleModel>,
}

pub fn speed_model(game: &Game, l10n: &Localizer, current: SimSpeed) -> SpeedModel {
    SpeedModel {
        clock: format::clock(game.session().clock().tick()),
        speeds: SPEEDS
            .into_iter()
            .map(|(speed, key)| ToggleModel {
                button: ButtonModel::new(l10n.text(key), UiAction::SetSpeed(speed)),
                on: speed == current,
            })
            .collect(),
    }
}

/// What Space asks for, if anything: pause a running day, or resume a paused one at `last`.
/// `blocked` when egui has the keyboard or a prompt or modal is open.
pub fn space_action(current: SimSpeed, last: LastSpeed, blocked: bool) -> Option<UiAction> {
    if blocked {
        return None;
    }
    Some(UiAction::SetSpeed(if current == SimSpeed::Paused {
        last.0
    } else {
        SimSpeed::Paused
    }))
}

pub fn draw_speed(ui: &mut egui::Ui, model: &SpeedModel) -> Vec<UiAction> {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        ui.strong(model.clock.as_str());
        ui.separator();
        for speed in &model.speeds {
            toggle_button(ui, speed, &mut actions);
        }
    });
    actions
}

pub(super) fn speed_bar(
    mut contexts: EguiContexts,
    mut area: ResMut<HudArea>,
    session: Res<Session>,
    l10n: Res<Localizer>,
    mut sink: ActionSink,
) -> Result {
    let model = speed_model(&session.0, &l10n, sink.speed());
    let mut ui = area.root_ui(contexts.ctx_mut()?, "speed");
    let actions = egui::Panel::top("speed")
        .resizable(false)
        .show(&mut ui, |ui| draw_speed(ui, &model))
        .inner;
    area.claim(&ui);
    dispatch(actions, &mut sink);
    Ok(())
}

/// Space toggles pause. Reads the keyboard through `ButtonInput`, so a headless test can press it.
pub(super) fn pause_on_space(
    keys: Res<ButtonInput<KeyCode>>,
    egui: Option<Res<EguiWantsInput>>,
    session: Res<Session>,
    mut sink: ActionSink,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }
    let blocked = egui.is_some_and(|egui| egui.wants_any_keyboard_input())
        || sink.open_prompt().0.is_some()
        || session.0.session().status() != SessionStatus::Active;
    if let Some(action) = space_action(sink.speed(), sink.last_speed(), blocked) {
        dispatch([action], &mut sink);
    }
}

#[cfg(test)]
mod tests {
    use application::{Command, InMemoryCommandLog};

    use super::*;
    use crate::hud::test_support::{eastern_game, shipped_l10n};

    #[test]
    fn the_speed_buttons_ask_for_their_speed_and_the_current_one_is_on() {
        let l10n = shipped_l10n();
        let mut game = eastern_game(Box::new(InMemoryCommandLog::default()));
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 75 }).unwrap();

        let model = speed_model(&game, &l10n, SimSpeed::X4);

        assert_eq!(model.clock, "01:15");
        let buttons: Vec<(&str, &UiAction, bool)> = model
            .speeds
            .iter()
            .map(|speed| (speed.button.label.as_str(), &speed.button.action, speed.on))
            .collect();
        assert_eq!(
            buttons,
            [
                ("Pause", &UiAction::SetSpeed(SimSpeed::Paused), false),
                ("1×", &UiAction::SetSpeed(SimSpeed::X1), false),
                ("2×", &UiAction::SetSpeed(SimSpeed::X2), false),
                ("4×", &UiAction::SetSpeed(SimSpeed::X4), true),
                ("8×", &UiAction::SetSpeed(SimSpeed::X8), false),
                ("16×", &UiAction::SetSpeed(SimSpeed::X16), false),
                ("32×", &UiAction::SetSpeed(SimSpeed::X32), false),
            ]
        );
        assert!(model.speeds.iter().all(|speed| speed.button.enabled));
        assert_eq!(l10n.problems(), vec![]);
    }

    #[test]
    fn space_pauses_a_running_day_and_resumes_the_last_speed() {
        let last = LastSpeed(SimSpeed::X4);
        for running in [
            SimSpeed::X1,
            SimSpeed::X2,
            SimSpeed::X4,
            SimSpeed::X8,
            SimSpeed::X16,
            SimSpeed::X32,
        ] {
            assert_eq!(
                space_action(running, LastSpeed(running), false),
                Some(UiAction::SetSpeed(SimSpeed::Paused))
            );
            assert_eq!(
                space_action(SimSpeed::Paused, LastSpeed(running), false),
                Some(UiAction::SetSpeed(running))
            );
        }
        assert_eq!(
            space_action(SimSpeed::Paused, last, false),
            Some(UiAction::SetSpeed(SimSpeed::X4))
        );
        assert_eq!(
            space_action(SimSpeed::Paused, LastSpeed::default(), false),
            Some(UiAction::SetSpeed(SimSpeed::X1)),
            "1× if no speed was picked this game"
        );
    }

    #[test]
    fn space_does_nothing_while_blocked() {
        for current in [SimSpeed::Paused, SimSpeed::X2] {
            assert_eq!(space_action(current, LastSpeed(SimSpeed::X8), true), None);
        }
    }
}
