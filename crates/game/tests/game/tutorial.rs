//! The first-game tutorial's run lifecycle (first-game tutorial spec §3, §4.3): a new game starts
//! it when progress says it is owed, loading a save never does, leaving the game drops it, and
//! leaving the first Review finishes it.

use crate::hud_smoke::dispatch_in;
use crate::save_resume::{open_browser, settle};
use crate::support::{hud_app, request, update_n};
use application::Command;
use bevy::prelude::*;
use domain::session::DayPhase;
use domain::time::TICKS_PER_DAY;
use game::hud::UiAction;
use game::hud::tutorial::{TutorialRun, TutorialState};
use game::paths::SaveDir;
use game::progress::ClientProgressStore;
use game::sim::{PersistAction, Persistence, ReturnToMenu, Session, SessionGeneration};
use game::state::AppState;
use tempfile::TempDir;

/// Sets the progress the app has loaded: loaded or not, and whether the tutorial is owed.
fn set_progress(app: &mut App, loaded: bool, pending: bool) {
    let mut store = app.world_mut().resource_mut::<ClientProgressStore>();
    store.loaded = loaded;
    store.value.tutorial_pending = pending;
}

fn run(app: &App) -> Option<&TutorialState> {
    app.world().resource::<TutorialRun>().0.as_ref()
}

fn pending(app: &App) -> bool {
    app.world()
        .resource::<ClientProgressStore>()
        .value
        .tutorial_pending
}

fn execute(app: &mut App, command: Command) {
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(command)
        .expect("the command applies");
    update_n(app, 2);
}

/// A new western game in a HUD app whose progress is `loaded` with `pending`.
fn new_game(logs: &TempDir, loaded: bool, pending: bool) -> App {
    let mut app = hud_app(logs.path());
    set_progress(&mut app, loaded, pending);
    request(&mut app, "western_interconnection");
    assert!(app.world().contains_resource::<Session>());
    app
}

#[test]
fn new_game_with_pending_progress_starts_tutorial() {
    let logs = TempDir::new().unwrap();
    let app = new_game(&logs, true, true);
    let generation = app.world().resource::<SessionGeneration>().0;
    let state = run(&app).expect("the tutorial runs");
    assert_eq!(state.generation, generation);
    assert!(state.played.is_empty());
    assert!(!state.seen_review);
    // The western starter fleet has no battery.
    assert!(state.starter_batteries.is_empty());
    assert!(pending(&app));
}

#[test]
fn not_pending_means_no_tutorial() {
    let logs = TempDir::new().unwrap();
    let app = new_game(&logs, true, false);
    assert!(run(&app).is_none());

    // Progress that hasn't loaded yet doesn't start one either, whatever its in-memory default.
    let logs = TempDir::new().unwrap();
    let app = new_game(&logs, false, true);
    assert!(run(&app).is_none());
}

#[test]
fn loading_a_save_mid_tutorial_stops_it() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(saves.path().into()));
    set_progress(&mut app, true, true);
    request(&mut app, "western_interconnection");
    assert!(run(&app).is_some());

    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::OpenSave {
            leave: true,
        })],
    );
    settle(&mut app);
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::SaveNew(
            "Mid-tutorial".into(),
        ))],
    );
    settle(&mut app);
    let slot = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .expect("the save was written")
        .metadata
        .slot
        .clone();
    update_n(&mut app, 3);
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::MainMenu
    );
    assert!(run(&app).is_none());

    open_browser(&mut app);
    settle(&mut app);
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::Load(slot))],
    );
    settle(&mut app);
    update_n(&mut app, 6);
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::InGame
    );
    assert!(app.world().contains_resource::<Session>());
    assert!(
        run(&app).is_none(),
        "loading a save never starts a tutorial"
    );
    assert!(pending(&app), "the tutorial is still owed");
}

#[test]
fn returning_to_menu_clears_the_run() {
    let logs = TempDir::new().unwrap();
    let mut app = new_game(&logs, true, true);
    assert!(run(&app).is_some());
    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 3);
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::MainMenu
    );
    assert!(run(&app).is_none());
    assert!(pending(&app), "leaving part-way keeps the tutorial owed");
}

#[test]
fn a_new_session_generation_clears_the_run() {
    let logs = TempDir::new().unwrap();
    let mut app = new_game(&logs, true, true);
    assert!(run(&app).is_some());
    update_n(&mut app, 2);
    assert!(run(&app).is_some(), "the run lasts while its game does");
    app.world_mut().resource_mut::<SessionGeneration>().0 += 1;
    update_n(&mut app, 1);
    assert!(run(&app).is_none());
    assert!(pending(&app));
}

#[test]
fn leaving_first_review_finishes() {
    let logs = TempDir::new().unwrap();
    let mut app = new_game(&logs, true, true);

    execute(&mut app, Command::BeginOperating);
    let state = run(&app).expect("the run survives starting the day");
    assert!(!state.seen_review);

    execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Review
    );
    assert!(run(&app).expect("the run reaches Review").seen_review);
    assert!(pending(&app), "the tutorial isn't finished in Review");

    execute(&mut app, Command::FinishReview);
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Planning
    );
    assert!(run(&app).is_none());
    assert!(!pending(&app), "leaving the first Review finishes it");
}
