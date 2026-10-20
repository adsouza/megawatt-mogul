//! The shortfall confirmation (spec §16.4) through the real egui pass, at the smallest window,
//! for both shipped regions: *Start the day* on a short forecast asks first, *Keep planning* and a
//! click outside back out, and *Start anyway* begins the day with exactly one `BeginOperating`.
//!
//! Every pass is checked, settled or not, including the press and release of each click and the
//! frames around the phase change: the prompt and the Operating panels never draw together.

use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::session::DayPhase;
use game::hud::{OpenPrompt, Prompt};
use game::sim::{CommandFailed, PlayerCommand, Session};
use game::state::PhaseState;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, sent};
use crate::support::egui_harness::{Widget, button as find_button, has, shows};
use crate::support::egui_pass::EguiPass;
use crate::support::{empty_start_in, hud_app, request, update_n};

const SIZE: (f32, f32) = (1024.0, 640.0);
const QUESTION: &str = "Expect rotating outages if you start the day now.";

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.0, SIZE.1))
}

fn mirrored_phase(app: &App) -> PhaseState {
    *app.world().resource::<State<PhaseState>>().get()
}

fn domain_phase(app: &App) -> DayPhase {
    app.world().resource::<Session>().0.session().phase()
}

fn open_prompt(app: &App) -> OpenPrompt {
    *app.world().resource::<OpenPrompt>()
}

fn prompt_drawn(widgets: &[Widget]) -> bool {
    has(widgets, "Start anyway") || has(widgets, "Keep planning") || shows(widgets, QUESTION)
}

/// Fails if a pass draws the prompt beside an Operating panel (the speed bar, the gauge).
fn one_screen(widgets: &[Widget], step: &str) {
    let operating = has(widgets, "Pause") || has(widgets, "Frequency");
    assert!(
        !(prompt_drawn(widgets) && operating),
        "{step}: the prompt draws over Operating"
    );
}

/// One pass, checked.
fn pass_once(pass: &EguiPass, app: &mut App, step: &str) -> Vec<Widget> {
    let widgets = pass.run(app, SIZE.0, SIZE.1);
    one_screen(&widgets, step);
    widgets
}

/// Two passes, so sizes have settled, each checked.
fn settled(pass: &EguiPass, app: &mut App, step: &str) -> Vec<Widget> {
    pass_once(pass, app, step);
    pass_once(pass, app, step)
}

/// A click at `at`: the pass that presses and the one that releases, each checked, with nothing
/// settled between (that would run `dispatch` again).
fn click(pass: &EguiPass, app: &mut App, at: egui::Pos2, step: &str) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let pressed = pass.run_with(
        app,
        SIZE.0,
        SIZE.1,
        vec![egui::Event::PointerMoved(at), button(true)],
    );
    one_screen(&pressed, &format!("{step}: press"));
    let released = pass.run_with(app, SIZE.0, SIZE.1, vec![button(false)]);
    one_screen(&released, &format!("{step}: release"));
}

fn click_button(pass: &EguiPass, app: &mut App, widgets: &[Widget], label: &str) {
    let at = find_button(widgets, label).rect.center();
    click(pass, app, at, label);
}

/// A new game in `region` with the inherited schedule undone, so the forecast is short.
fn short_app(logs: &TempDir, region: &str) -> App {
    let mut app = hud_app(logs.path());
    request(&mut app, region);
    empty_start_in(&mut app);
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .capacity_shortfall_check()
            .is_some(),
        "{region}: nothing is committed, so the forecast is short"
    );
    app
}

/// *Start the day* on the settled Planning screen, then the passes that size the prompt: the
/// prompt, settled. Sends nothing.
fn ask(pass: &EguiPass, app: &mut App, region: &str) -> Vec<Widget> {
    let widgets = settled(pass, app, "planning");
    assert!(!prompt_drawn(&widgets), "{region}: no prompt yet");
    let before = sent::<PlayerCommand>(app).len();
    click_button(pass, app, &widgets, "Start the day");
    assert_eq!(
        sent::<PlayerCommand>(app)[before..],
        [],
        "{region}: Start the day only asks"
    );
    assert!(
        matches!(open_prompt(app).0, Some(Prompt::ShortfallStart { .. })),
        "{region}: the shortfall prompt is open"
    );
    // The modal's first pass sizes it: run it on its own, then settle for the buttons' places.
    pass_once(pass, app, "the prompt's first pass");
    settled(pass, app, "the prompt")
}

#[test]
fn start_the_day_on_a_short_forecast_asks_and_each_answer_does_what_it_says() {
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = short_app(&logs, region);
        let pass = EguiPass::attach(&mut app);
        let before = sent::<PlayerCommand>(&app).len();

        // 1. Start the day asks: the prompt is up and fits, and nothing was sent.
        let widgets = ask(&pass, &mut app, region);
        let question = widgets
            .iter()
            .find(|w| w.text.contains(QUESTION))
            .unwrap_or_else(|| panic!("{region}: the question is shown"));
        assert!(
            question.text.starts_with("The forecast is short by "),
            "{region}: {}",
            question.text
        );
        for (name, rect) in [
            ("the question", question.rect),
            ("Start anyway", find_button(&widgets, "Start anyway").rect),
            ("Keep planning", find_button(&widgets, "Keep planning").rect),
        ] {
            assert!(
                screen().contains_rect(rect),
                "{region}: {name} {rect:?} is off the screen"
            );
        }
        update_n(&mut app, 2);
        assert_eq!(domain_phase(&app), DayPhase::Planning, "{region}");

        // 2. Keep planning closes it; the game is still in Planning.
        let widgets = settled(&pass, &mut app, "the prompt, frames later");
        click_button(&pass, &mut app, &widgets, "Keep planning");
        assert_eq!(open_prompt(&app), OpenPrompt(None), "{region}");
        update_n(&mut app, 2);
        assert_eq!(mirrored_phase(&app), PhaseState::Planning, "{region}");
        let widgets = settled(&pass, &mut app, "after Keep planning");
        assert!(!prompt_drawn(&widgets), "{region}: the prompt is gone");
        assert!(has(&widgets, "Start the day"), "{region}");

        // A click outside the prompt is Keep planning too.
        let widgets = ask(&pass, &mut app, region);
        let modal_left = find_button(&widgets, "Start anyway").rect.left();
        let outside = egui::pos2(8.0, SIZE.1 / 2.0);
        assert!(
            outside.x < modal_left - 40.0,
            "{region}: left of the prompt"
        );
        click(&pass, &mut app, outside, "outside the prompt");
        assert_eq!(open_prompt(&app), OpenPrompt(None), "{region}");
        assert_eq!(sent::<PlayerCommand>(&app)[before..], [], "{region}");

        // 3. Start the day, then Start anyway: one BeginOperating, and the day begins.
        let widgets = ask(&pass, &mut app, region);
        click_button(&pass, &mut app, &widgets, "Start anyway");
        assert_eq!(
            sent::<PlayerCommand>(&app)[before..],
            [PlayerCommand(Command::BeginOperating)],
            "{region}: exactly one BeginOperating"
        );
        // The frames of the change, unsettled: the domain moves first, then the mirror.
        for frame in 0..3 {
            app.update();
            pass_once(&pass, &mut app, &format!("{region}: frame {frame} after"));
        }
        assert_eq!(mirrored_phase(&app), PhaseState::Operating, "{region}");
        assert_eq!(open_prompt(&app), OpenPrompt(None), "{region}");
        assert_eq!(sent::<CommandFailed>(&app), [], "{region}");
        let widgets = settled(&pass, &mut app, "operating");
        assert!(has(&widgets, "Frequency"), "{region}: Operating draws");
        assert!(!prompt_drawn(&widgets), "{region}");
    }
}
