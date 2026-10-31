//! The first-game tutorial's run lifecycle (first-game tutorial spec §3, §4.3): a new game starts
//! it when progress says it is owed, loading a save never does, leaving the game drops it, and
//! leaving the first Review finishes it.

use std::collections::{BTreeMap, BTreeSet};

use crate::hud_smoke::{dispatch_in, sent};
use crate::save_resume::{open_browser, settle};
use crate::support::egui_harness::{button as find_button, widget};
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, update_n};
use application::Command;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use domain::ids::CellCoord;
use domain::region::PlantKind;
use domain::session::DayPhase;
use domain::time::TICKS_PER_DAY;
use game::hud::tutorial::{
    StepKind, TUTORIAL_STEPS, TutorialAnchor, TutorialRun, TutorialState, TutorialStep, anchor_rect,
};
use game::hud::{HudSet, PlanningTab, Selection, TutorialAction, UiAction, UiState};
use game::paths::SaveDir;
use game::progress::ClientProgressStore;
use game::sim::{
    PersistAction, Persistence, PlayerCommand, ReturnToMenu, Session, SessionGeneration,
};
use game::state::AppState;
use game::world::{CameraFocus, WorldControls};
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

// --- Tutorial actions through `dispatch`, anchors and the battery-card reveal (spec §4.1) ---

/// A new western game in a HUD app, with the tutorial running.
fn tutorial_app(logs: &TempDir) -> App {
    let app = new_game(logs, true, true);
    assert!(run(&app).is_some());
    app
}

fn tutorial(app: &mut App, actions: impl IntoIterator<Item = TutorialAction>) {
    dispatch_in(app, actions.into_iter().map(UiAction::Tutorial).collect());
}

fn played(app: &App) -> BTreeSet<TutorialStep> {
    run(app).expect("the tutorial runs").played.clone()
}

fn ui_state(app: &App) -> &UiState {
    app.world().resource::<UiState>()
}

fn focus(app: &App) -> Option<CameraFocus> {
    app.world().resource::<WorldControls>().focus()
}

#[test]
fn playing_frame_grid_frames_the_home_zone() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let anchor = app.world().resource::<Session>().0.session().anchor_node();
    assert_ne!(focus(&app), Some(CameraFocus::Zone(anchor)));
    tutorial(&mut app, [TutorialAction::Play(TutorialStep::FrameGrid)]);
    assert_eq!(focus(&app), Some(CameraFocus::Zone(anchor)));
    assert_eq!(played(&app), BTreeSet::from([TutorialStep::FrameGrid]));
}

#[test]
fn playing_show_forecast_opens_the_forecast_tab() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    assert_ne!(ui_state(&app).tab, PlanningTab::Forecast);
    tutorial(&mut app, [TutorialAction::Play(TutorialStep::ShowForecast)]);
    assert_eq!(ui_state(&app).tab, PlanningTab::Forecast);
    assert_eq!(played(&app), BTreeSet::from([TutorialStep::ShowForecast]));
}

#[test]
fn playing_reveal_battery_asks_the_build_menu_for_the_battery_card() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    assert_eq!(ui_state(&app).reveal_plant_card, None);
    tutorial(
        &mut app,
        [TutorialAction::Play(TutorialStep::RevealBattery)],
    );
    assert_eq!(ui_state(&app).reveal_plant_card, Some(PlantKind::Battery));
    assert_eq!(played(&app), BTreeSet::from([TutorialStep::RevealBattery]));
}

#[test]
fn playing_a_crawl_or_spotlight_only_marks_it_played() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let before = (ui_state(&app).clone(), focus(&app));
    tutorial(
        &mut app,
        [
            TutorialAction::Play(TutorialStep::Crawl),
            TutorialAction::Play(TutorialStep::Cash),
        ],
    );
    assert_eq!((ui_state(&app).clone(), focus(&app)), before);
    assert_eq!(
        played(&app),
        BTreeSet::from([TutorialStep::Crawl, TutorialStep::Cash])
    );
}

#[test]
fn dismissing_a_step_marks_it_played_and_moves_nothing() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let before = (ui_state(&app).clone(), focus(&app));
    tutorial(
        &mut app,
        [
            TutorialAction::Dismiss(TutorialStep::FrameGrid),
            TutorialAction::Dismiss(TutorialStep::BatterySpotlight),
        ],
    );
    assert_eq!((ui_state(&app).clone(), focus(&app)), before);
    assert_eq!(
        played(&app),
        BTreeSet::from([TutorialStep::FrameGrid, TutorialStep::BatterySpotlight])
    );
}

/// An instruction stays until its step is done: it never joins `played`, however it is asked.
#[test]
fn instructions_never_join_played() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let instructions: Vec<TutorialStep> = TUTORIAL_STEPS
        .iter()
        .filter(|def| def.kind == StepKind::Instruction)
        .map(|def| def.step)
        .collect();
    assert!(instructions.contains(&TutorialStep::PickSite));
    tutorial(
        &mut app,
        instructions
            .iter()
            .flat_map(|step| [TutorialAction::Play(*step), TutorialAction::Dismiss(*step)]),
    );
    assert!(played(&app).is_empty());
}

#[test]
fn reveal_done_clears_the_reveal() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    tutorial(
        &mut app,
        [TutorialAction::Play(TutorialStep::RevealBattery)],
    );
    tutorial(&mut app, [TutorialAction::RevealDone]);
    assert_eq!(ui_state(&app).reveal_plant_card, None);
    assert_eq!(played(&app), BTreeSet::from([TutorialStep::RevealBattery]));
}

#[test]
fn end_finishes_the_tutorial() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    tutorial(&mut app, [TutorialAction::End]);
    assert!(run(&app).is_none());
    assert!(!pending(&app));
}

/// Without a run the tutorial's actions change nothing; only `RevealDone` still clears a reveal.
#[test]
fn without_a_run_tutorial_actions_do_nothing() {
    let logs = TempDir::new().unwrap();
    let mut app = new_game(&logs, true, false);
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .tutorial_pending = true;
    assert!(run(&app).is_none());
    let before = (ui_state(&app).clone(), focus(&app));
    let every_play = TUTORIAL_STEPS.iter().flat_map(|def| {
        [
            TutorialAction::Play(def.step),
            TutorialAction::Dismiss(def.step),
        ]
    });
    tutorial(&mut app, every_play.chain([TutorialAction::End]));
    assert_eq!((ui_state(&app).clone(), focus(&app)), before);
    assert!(run(&app).is_none());
    assert!(pending(&app), "End without a run leaves the tutorial owed");

    app.world_mut().resource_mut::<UiState>().reveal_plant_card = Some(PlantKind::Battery);
    tutorial(&mut app, [TutorialAction::RevealDone]);
    assert_eq!(ui_state(&app).reveal_plant_card, None);
}

fn any_tutorial_action() -> impl proptest::strategy::Strategy<Value = TutorialAction> {
    use proptest::strategy::Strategy;
    (0..TUTORIAL_STEPS.len(), 0u8..4).prop_map(|(index, which)| {
        let step = TUTORIAL_STEPS[index].step;
        match which {
            0 => TutorialAction::Play(step),
            1 => TutorialAction::Dismiss(step),
            2 => TutorialAction::RevealDone,
            _ => TutorialAction::End,
        }
    })
}

/// The tutorial never plays the game for the player: whatever it asks, no `PlayerCommand` is sent.
#[test]
fn tutorial_actions_never_send_player_commands() {
    let logs = TempDir::new().unwrap();
    let app = tutorial_app(&logs);
    let start = run(&app).cloned();
    let app = std::cell::RefCell::new(app);
    let config = proptest::test_runner::Config {
        test_name: Some(concat!(
            module_path!(),
            "::tutorial_actions_never_send_player_commands"
        )),
        ..proptest::test_runner::Config::with_cases(32)
    };
    proptest::proptest!(config, |(actions in proptest::collection::vec(any_tutorial_action(), 0..=32))| {
        let mut app = app.borrow_mut();
        app.world_mut().resource_mut::<TutorialRun>().0 = start.clone();
        app.world_mut().resource_mut::<Messages<PlayerCommand>>().clear();
        tutorial(&mut app, actions);
        proptest::prop_assert!(sent::<PlayerCommand>(&app).is_empty());
    });
}

/// The anchors marked in the egui pass under way, read back by a probe in `HudSet::Tutorial`
/// (where the overlay reads them), since a rect is only good for the pass that marked it.
#[derive(Resource, Default)]
struct SeenAnchors(BTreeMap<TutorialAnchor, egui::Rect>);

const ANCHORS: [TutorialAnchor; 7] = [
    TutorialAnchor::Cash,
    TutorialAnchor::Map,
    TutorialAnchor::BatteryCard,
    TutorialAnchor::LineSection,
    TutorialAnchor::StartDay,
    TutorialAnchor::SpeedControls,
    TutorialAnchor::ReviewConstruction,
];

fn probe_anchors(mut contexts: EguiContexts, mut seen: ResMut<SeenAnchors>) -> Result {
    let ctx = contexts.ctx_mut()?;
    seen.0 = ANCHORS
        .into_iter()
        .filter_map(|anchor| anchor_rect(ctx, anchor).map(|rect| (anchor, rect)))
        .collect();
    Ok(())
}

fn with_anchor_probe(app: &mut App) -> EguiPass {
    app.init_resource::<SeenAnchors>().add_systems(
        EguiPrimaryContextPass,
        probe_anchors.in_set(HudSet::Tutorial),
    );
    EguiPass::attach(app)
}

fn anchors(app: &App) -> BTreeMap<TutorialAnchor, egui::Rect> {
    app.world().resource::<SeenAnchors>().0.clone()
}

/// A land cell a battery can be built on.
fn battery_site(app: &App) -> CellCoord {
    let session = app.world().resource::<Session>().0.session();
    let terrain = session.region().terrain();
    (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .find(|cell| session.plant_site_check(PlantKind::Battery, *cell).is_ok())
        .expect("a battery can go somewhere")
}

const MIN: (f32, f32) = (
    game::window::MIN_WINDOW_SIZE.x,
    game::window::MIN_WINDOW_SIZE.y,
);

#[test]
fn anchors_are_marked_in_planning() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    let (width, height) = MIN;
    pass.settled(&mut app, width, height);
    let seen = anchors(&app);
    for anchor in [
        TutorialAnchor::Cash,
        TutorialAnchor::Map,
        TutorialAnchor::BatteryCard,
        TutorialAnchor::LineSection,
        TutorialAnchor::StartDay,
    ] {
        let rect = seen.get(&anchor).unwrap_or_else(|| panic!("{anchor:?}"));
        assert!(rect.is_positive(), "{anchor:?}: {rect:?}");
    }
    assert!(!seen.contains_key(&TutorialAnchor::SpeedControls));
    assert!(!seen.contains_key(&TutorialAnchor::ReviewConstruction));

    // Without a selected cell there is no battery card to point at.
    dispatch_in(&mut app, vec![UiAction::Select(Selection::None)]);
    pass.settled(&mut app, width, height);
    assert!(!anchors(&app).contains_key(&TutorialAnchor::BatteryCard));
}

#[test]
fn speed_and_review_anchors_are_marked() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;

    execute(&mut app, Command::BeginOperating);
    pass.settled(&mut app, width, height);
    let seen = anchors(&app);
    assert!(seen[&TutorialAnchor::SpeedControls].is_positive());
    assert!(!seen.contains_key(&TutorialAnchor::StartDay));
    assert!(!seen.contains_key(&TutorialAnchor::ReviewConstruction));

    execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Review
    );
    pass.settled(&mut app, width, height);
    let seen = anchors(&app);
    assert!(seen[&TutorialAnchor::ReviewConstruction].is_positive());
    assert!(!seen.contains_key(&TutorialAnchor::SpeedControls));
}

/// At the smallest window the battery card starts below the build menu's fold; the reveal scrolls
/// it into the menu's view once and then says so, so the player can scroll away again.
#[test]
fn reveal_scrolls_battery_card_into_view() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    // The build menu's scrolling part: right of the map, below its title and above the Planning
    // panel's tab row.
    let map = widget(&widgets, "Map").rect;
    let title = widgets
        .iter()
        .find(|w| w.text == "Build" && w.rect.left() >= map.right())
        .expect("the build menu's title")
        .rect;
    let view = egui::Rect::from_min_max(
        egui::pos2(map.right(), title.bottom()),
        egui::pos2(width, find_button(&widgets, "Start the day").rect.top()),
    );
    let card = anchors(&app)[&TutorialAnchor::BatteryCard];
    assert!(
        !view.contains_rect(card),
        "the test needs the card out of view first: {card:?} in {view:?}"
    );

    tutorial(
        &mut app,
        [TutorialAction::Play(TutorialStep::RevealBattery)],
    );
    pass.run(&mut app, width, height);
    assert_eq!(
        ui_state(&app).reveal_plant_card,
        None,
        "the build menu reports the reveal done on the pass it scrolls"
    );
    // Let the scroll animation finish.
    for _ in 0..60 {
        pass.run(&mut app, width, height);
    }
    let card = anchors(&app)[&TutorialAnchor::BatteryCard];
    assert!(view.contains_rect(card), "{card:?} is not in {view:?}");
}
