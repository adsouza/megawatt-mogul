//! The frame in which the day's phase changes, drawn by the real egui pass.
//!
//! The domain's phase changes in `Update`, as the command runs; `PhaseState` follows at the next
//! frame's state transition. The egui pass of the frame in between must still draw a phase's
//! panels, so the screen doesn't flash the bare map for a frame.

use application::Command;
use bevy::prelude::*;
use domain::session::DayPhase;
use game::hud::UiAction;
use game::sim::Session;
use game::state::PhaseState;
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in};
use crate::support::egui_harness::{Widget, button as find_button, has, shows, widget};
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, update_n};

const SIZE: (f32, f32) = (1024.0, 640.0);

fn domain_phase(app: &App) -> DayPhase {
    app.world().resource::<Session>().0.session().phase()
}

fn mirrored_phase(app: &App) -> PhaseState {
    *app.world().resource::<State<PhaseState>>().get()
}

/// Sends `command` and runs the one frame that applies it, then that frame's egui pass: the
/// widgets drawn while the domain has moved on and `PhaseState` has not yet.
fn change_frame(app: &mut App, pass: &EguiPass, command: Command, to: DayPhase) -> Vec<Widget> {
    let before = mirrored_phase(app);
    dispatch_in(app, vec![UiAction::Command(command)]);
    app.update();
    assert_eq!(domain_phase(app), to, "the command ran this frame");
    assert_eq!(
        mirrored_phase(app),
        before,
        "the mirror follows at the next frame's transition"
    );
    let (width, height) = SIZE;
    pass.run(app, width, height)
}

#[test]
fn planning_to_operating_draws_the_operating_panels_in_the_frame_it_happens() {
    let (width, height) = SIZE;
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region);
        let pass = EguiPass::attach(&mut app);
        assert!(has(&pass.settled(&mut app, width, height), "Start the day"));

        let widgets = change_frame(
            &mut app,
            &pass,
            Command::BeginOperating,
            DayPhase::Operating,
        );

        for text in ["Frequency", "Plants", "Supply", "8×"] {
            assert!(shows(&widgets, text), "{region}: {text} is not drawn");
        }
        assert!(!has(&widgets, "Start the day"), "{region}");
        let map = widget(&widgets, "Map").rect;
        update_n(&mut app, 1);
        assert_eq!(mirrored_phase(&app), PhaseState::Operating);
        let settled = widget(&pass.settled(&mut app, width, height), "Map").rect;
        assert_eq!(map, settled, "{region}: the map does not jump");
    }
}

#[test]
fn review_to_planning_draws_the_planning_panels_in_the_frame_it_happens() {
    let (width, height) = SIZE;
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = crate::hud_smoke::operating_hour_app(&logs, region);
        dispatch_in(
            &mut app,
            vec![UiAction::Command(Command::AdvanceTicks { n: 1_440 })],
        );
        update_n(&mut app, 3);
        assert_eq!(mirrored_phase(&app), PhaseState::Review);
        let pass = EguiPass::attach(&mut app);
        assert!(has(&pass.settled(&mut app, width, height), "Continue"));

        let widgets = change_frame(&mut app, &pass, Command::FinishReview, DayPhase::Planning);

        assert!(!find_button(&widgets, "Start the day").disabled, "{region}");
        for text in ["Commitments", "Forecast", "Tariff"] {
            assert!(shows(&widgets, text), "{region}: {text} is not drawn");
        }
        assert!(!has(&widgets, "Continue"), "{region}");
        let map = widget(&widgets, "Map").rect;
        update_n(&mut app, 1);
        assert_eq!(mirrored_phase(&app), PhaseState::Planning);
        let settled = widget(&pass.settled(&mut app, width, height), "Map").rect;
        assert_eq!(map, settled, "{region}: the map does not jump");
    }
}
