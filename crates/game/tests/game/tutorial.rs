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
pub(crate) fn tutorial_app(logs: &TempDir) -> App {
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
    // A reveal still pending when the tutorial ends must not scroll the build menu later.
    tutorial(
        &mut app,
        [
            TutorialAction::Play(TutorialStep::RevealBattery),
            TutorialAction::Reveal(TutorialAnchor::LineSection),
        ],
    );
    assert_eq!(ui_state(&app).reveal_plant_card, Some(PlantKind::Battery));
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::LineSection)
    );
    tutorial(&mut app, [TutorialAction::End]);
    assert!(run(&app).is_none());
    assert!(!pending(&app));
    assert_eq!(ui_state(&app).reveal_plant_card, None);
    assert_eq!(ui_state(&app).reveal_anchor, None);
}

#[test]
fn reveal_asks_the_build_menu_for_a_scrolling_anchor() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::BatteryCard)],
    );
    assert_eq!(ui_state(&app).reveal_plant_card, Some(PlantKind::Battery));
    assert_eq!(ui_state(&app).reveal_anchor, None);
    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::LineSection)],
    );
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::LineSection)
    );
    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::ReviewConstruction)],
    );
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::ReviewConstruction)
    );
    // Anchors that don't scroll have nothing to reveal; and nothing joins `played`.
    let before = ui_state(&app).clone();
    tutorial(
        &mut app,
        [
            TutorialAction::Reveal(TutorialAnchor::Cash),
            TutorialAction::Reveal(TutorialAnchor::StartDay),
        ],
    );
    assert_eq!(ui_state(&app).clone(), before);
    assert!(played(&app).is_empty());
    tutorial(&mut app, [TutorialAction::RevealDone]);
    assert_eq!(ui_state(&app).reveal_plant_card, None);
    assert_eq!(ui_state(&app).reveal_anchor, None);
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
    let every_play = every_play.chain(ANCHORS.map(TutorialAction::Reveal));
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
    (0..TUTORIAL_STEPS.len(), 0u8..5).prop_map(|(index, which)| {
        let step = TUTORIAL_STEPS[index].step;
        match which {
            0 => TutorialAction::Play(step),
            1 => TutorialAction::Dismiss(step),
            2 => TutorialAction::RevealDone,
            3 => TutorialAction::Reveal(ANCHORS[index % ANCHORS.len()]),
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
    // A tall window shows the whole build menu, battery card and line section included.
    let (width, height) = (MIN.0, 3000.0);
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

/// The build menu's scrolling part at the smallest window: right of the map, below its title and
/// above the Planning panel's tab row.
fn build_menu_view(widgets: &[crate::support::egui_harness::Widget]) -> egui::Rect {
    let map = widget(widgets, "Map").rect;
    let title = widgets
        .iter()
        .find(|w| w.text == "Build" && w.rect.left() >= map.right())
        .expect("the build menu's title")
        .rect;
    egui::Rect::from_min_max(
        egui::pos2(map.right(), title.bottom()),
        egui::pos2(MIN.0, find_button(widgets, "Start the day").rect.top()),
    )
}

/// A card scrolled out of the build menu's view has no anchor, so a spotlight never frames
/// space the player can't see; a card partly in view is marked only where it shows.
#[test]
fn a_scrolled_away_battery_card_has_no_anchor() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    let (width, height) = MIN;
    pass.settled(&mut app, width, height);
    let seen = anchors(&app);
    assert!(
        !seen.contains_key(&TutorialAnchor::BatteryCard),
        "the card starts below the fold: {seen:?}"
    );
    assert!(!seen.contains_key(&TutorialAnchor::LineSection), "{seen:?}");
    // Every anchor marked is on screen.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    for (anchor, rect) in &seen {
        assert!(screen.contains_rect(*rect), "{anchor:?}: {rect:?}");
    }
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
    let view = build_menu_view(&widgets);
    assert!(
        !anchors(&app).contains_key(&TutorialAnchor::BatteryCard),
        "the test needs the card out of view first"
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
    assert!(card.is_positive());
    assert!(
        view.expand(1.0).contains_rect(card),
        "{card:?} is not in {view:?}"
    );
}

#[test]
fn reveal_scrolls_line_section_into_view() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let view = build_menu_view(&widgets);
    assert!(!anchors(&app).contains_key(&TutorialAnchor::LineSection));

    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::LineSection)],
    );
    pass.run(&mut app, width, height);
    assert!(
        ui_state(&app).reveal_anchor.is_none(),
        "the build menu reports the reveal done on the pass it scrolls"
    );
    for _ in 0..60 {
        pass.run(&mut app, width, height);
    }
    let section = anchors(&app)[&TutorialAnchor::LineSection];
    assert!(section.is_positive());
    // Taller than the view, so its top is brought in and it fills the view below that.
    assert!(
        view.expand(1.0).contains_rect(section),
        "{section:?} is not in {view:?}"
    );
    assert!(section.height() > view.height() / 2.0, "{section:?}");
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

// --- The overlay: crawl, spotlights and instructions (spec §4.1) ---

/// Puts the run at `step`: every step before it in the table counts as played (instructions
/// too, which only the test can do), so `step` is the active one in its phase.
pub(crate) fn at_step(app: &mut App, step: TutorialStep) {
    let mut run = app.world_mut().resource_mut::<TutorialRun>();
    let state = run.0.as_mut().expect("the tutorial runs");
    state.played = TUTORIAL_STEPS
        .iter()
        .map(|def| def.step)
        .take_while(|before| *before != step)
        .collect();
}

/// Moves the real clock, which the overlay times spotlights and the crawl by, on `seconds`.
pub(crate) fn advance_real(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_duration(std::time::Duration::from_secs_f32(seconds));
}

fn key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn press_escape(pass: &EguiPass, app: &mut App, (width, height): (f32, f32)) {
    pass.run_with(app, width, height, vec![key(egui::Key::Escape, true)]);
    pass.run_with(app, width, height, vec![key(egui::Key::Escape, false)]);
}

/// The instruction's End tutorial link.
fn end_link(
    widgets: &[crate::support::egui_harness::Widget],
) -> Option<&crate::support::egui_harness::Widget> {
    widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::Button && w.text == "End tutorial")
}

fn open_prompt(app: &App) -> Option<game::hud::Prompt> {
    app.world().resource::<game::hud::OpenPrompt>().0
}

fn phase(app: &App) -> DayPhase {
    app.world().resource::<Session>().0.session().phase()
}

const CASH_NOTE: &str =
    "This is your money. Grow it by selling power, without blacking out your customers.";
const PICK_SITE: &str = "Click a square on the map to see what you can build there.";
const START_NOTE: &str = "Ready? Start the day.";

#[test]
fn tutorial_view_opacity_curve() {
    use game::hud::tutorial::{TutorialView, tutorial_view};
    let logs = TempDir::new().unwrap();
    let app = tutorial_app(&logs);
    let state = run(&app).unwrap();
    let game = &app.world().resource::<Session>().0;
    let l10n = crate::support::shipped_l10n();
    let cash = TUTORIAL_STEPS
        .iter()
        .find(|def| def.step == TutorialStep::Cash)
        .unwrap();
    for (at, expected) in [(0.0, 0.0), (0.15, 0.5), (2.0, 1.0), (4.45, 0.5), (4.7, 0.0)] {
        match tutorial_view(cash, state, game, &l10n, at) {
            TutorialView::Spotlight {
                step,
                anchor,
                text,
                opacity,
            } => {
                assert_eq!(step, TutorialStep::Cash);
                assert_eq!(anchor, TutorialAnchor::Cash);
                assert_eq!(text, CASH_NOTE);
                assert!(
                    (opacity - expected).abs() < 1e-4,
                    "at {at} s: {opacity}, not {expected}"
                );
            }
            other => panic!("{other:?}"),
        }
    }
    // The other kinds, for completeness.
    let def = |step| TUTORIAL_STEPS.iter().find(|def| def.step == step).unwrap();
    assert_eq!(
        tutorial_view(def(TutorialStep::FrameGrid), state, game, &l10n, 0.0),
        TutorialView::Move(TutorialStep::FrameGrid)
    );
    match tutorial_view(def(TutorialStep::Crawl), state, game, &l10n, 9.0) {
        TutorialView::Crawl { text, progress } => {
            assert!(text.starts_with("You run a power company."), "{text}");
            assert!((progress - 0.2).abs() < 1e-6, "{progress}");
        }
        other => panic!("{other:?}"),
    }
    match tutorial_view(def(TutorialStep::PickSite), state, game, &l10n, 0.0) {
        TutorialView::Instruction { anchor, text, end } => {
            assert_eq!(anchor, TutorialAnchor::Map);
            assert_eq!(text, PICK_SITE);
            assert_eq!(end.label, "End tutorial");
            assert_eq!(end.action, UiAction::Tutorial(TutorialAction::End));
        }
        other => panic!("{other:?}"),
    }
    // The Construction note is chosen from what was ordered: nothing, here.
    match tutorial_view(def(TutorialStep::Construction), state, game, &l10n, 1.0) {
        TutorialView::Spotlight { text, .. } => assert_eq!(
            text,
            "Construction is what you pay each month for things you've ordered."
        ),
        other => panic!("{other:?}"),
    }
}

/// The overlay plays moves through `dispatch` as soon as they are the active step.
#[test]
fn moves_play_by_themselves() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = EguiPass::attach(&mut app);
    let anchor = app.world().resource::<Session>().0.session().anchor_node();
    pass.run(&mut app, MIN.0, MIN.1);
    assert_eq!(played(&app), BTreeSet::from([TutorialStep::FrameGrid]));
    assert_eq!(focus(&app), Some(CameraFocus::Zone(anchor)));
}

#[test]
fn spotlight_click_is_consumed() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::StartSpotlight);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, START_NOTE));
    assert!(!played(&app).contains(&TutorialStep::StartSpotlight));
    let start = find_button(&widgets, "Start the day").rect;
    pass.click_at(&mut app, width, height, start.center());
    assert!(sent::<PlayerCommand>(&app).is_empty());
    assert_eq!(open_prompt(&app), None);
    update_n(&mut app, 2);
    assert_eq!(phase(&app), DayPhase::Planning);
    assert!(played(&app).contains(&TutorialStep::StartSpotlight));
    // The instruction follows, and it doesn't block the button.
    let widgets = pass.settled(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, START_NOTE));
    let start = find_button(&widgets, "Start the day").rect;
    pass.click_at(&mut app, width, height, start.center());
    assert!(
        !sent::<PlayerCommand>(&app).is_empty() || open_prompt(&app).is_some(),
        "Start the day was clicked"
    );
}

#[test]
fn escape_ends_a_spotlight() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Cash);
    let pass = EguiPass::attach(&mut app);
    pass.settled(&mut app, MIN.0, MIN.1);
    assert!(!played(&app).contains(&TutorialStep::Cash));
    press_escape(&pass, &mut app, MIN);
    assert!(played(&app).contains(&TutorialStep::Cash));
}

#[test]
fn spotlight_ends_by_itself_after_hold() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Cash);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    advance_real(&mut app, 2.0);
    let widgets = pass.run(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    assert!(!played(&app).contains(&TutorialStep::Cash));
    advance_real(&mut app, 2.7);
    pass.run(&mut app, width, height);
    assert!(played(&app).contains(&TutorialStep::Cash));
    let widgets = pass.run(&mut app, width, height);
    assert!(!crate::support::egui_harness::has(&widgets, CASH_NOTE));
    assert!(crate::support::egui_harness::has(&widgets, PICK_SITE));
}

/// A spotlight whose anchor is scrolled out of the build menu asks for it, and its clock starts
/// only once the anchor is drawn.
#[test]
fn spotlight_waits_for_its_anchor() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    at_step(&mut app, TutorialStep::ConnectSpotlight);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let note = "A power plant only helps once a line connects it to your grid.";
    let widgets = pass.run(&mut app, width, height);
    assert!(!crate::support::egui_harness::has(&widgets, note));
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::LineSection)
    );
    // However long the wait, the clock hasn't started.
    advance_real(&mut app, 10.0);
    let mut shown = false;
    for _ in 0..60 {
        let widgets = pass.run(&mut app, width, height);
        shown |= crate::support::egui_harness::has(&widgets, note);
    }
    assert!(shown);
    assert!(anchors(&app).contains_key(&TutorialAnchor::LineSection));
    assert!(!played(&app).contains(&TutorialStep::ConnectSpotlight));
    advance_real(&mut app, 4.7);
    pass.run(&mut app, width, height);
    assert!(played(&app).contains(&TutorialStep::ConnectSpotlight));
}

/// Review Focus 3: only the active dialog handles Escape. An open prompt takes it, a spotlight
/// waits for the prompt to close, and the run is unchanged.
#[test]
fn escape_goes_to_open_prompt_not_tutorial() {
    use game::hud::Prompt;
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::PickSite);
    let pass = EguiPass::attach(&mut app);
    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    assert!(!crate::support::egui_harness::has(&widgets, PICK_SITE));
    let before = run(&app).cloned();
    press_escape(&pass, &mut app, MIN);
    assert_eq!(open_prompt(&app), None);
    assert_eq!(run(&app).cloned(), before);

    // A spotlight doesn't start while a prompt is open.
    at_step(&mut app, TutorialStep::Cash);
    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    assert!(!crate::support::egui_harness::has(&widgets, CASH_NOTE));
    let before = run(&app).cloned();
    advance_real(&mut app, 5.0);
    press_escape(&pass, &mut app, MIN);
    assert_eq!(open_prompt(&app), None);
    assert_eq!(run(&app).cloned(), before);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    assert!(!played(&app).contains(&TutorialStep::Cash));
}

/// Whether a pass paints `text`: its glyphs, not only an accessible name.
fn paints(pass: &EguiPass, app: &mut App, (width, height): (f32, f32), text: &str) -> bool {
    let output = pass.paint_with(app, width, height, Vec::new());
    let painted = output
        .shapes
        .iter()
        .any(|clipped| matches!(&clipped.shape, egui::Shape::Text(t) if t.galley.text() == text));
    output.drop_without_applying_deltas();
    painted
}

/// An instruction never stands between the player and a prompt: while one is open its card isn't
/// drawn at all, so clicks where it was reach the prompt's backdrop, and the prompt's buttons work.
#[test]
fn prompt_stays_clickable_under_instruction() {
    use game::hud::Prompt;
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::PickSite);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let note = widget(&widgets, PICK_SITE).rect;
    assert!(paints(&pass, &mut app, MIN, PICK_SITE));

    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    let widgets = pass.settled(&mut app, width, height);
    assert!(!paints(&pass, &mut app, MIN, PICK_SITE));
    assert!(!crate::support::egui_harness::has(&widgets, PICK_SITE));
    assert!(end_link(&widgets).is_none());
    // A click where the card was reaches the prompt's backdrop, which closes it.
    pass.click_at(&mut app, width, height, note.center());
    assert_eq!(open_prompt(&app), None);

    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    let widgets = pass.settled(&mut app, width, height);
    assert!(!crate::support::egui_harness::has(&widgets, PICK_SITE));
    let stay = find_button(&widgets, "Stay").rect;
    pass.click_at(&mut app, width, height, stay.center());
    assert_eq!(open_prompt(&app), None);
    assert!(run(&app).is_some());
    // With the prompt closed, the card is back.
    let widgets = pass.settled(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, PICK_SITE));
}

/// The instruction's End link ends the tutorial; elsewhere the map still takes clicks.
#[test]
fn instruction_end_link_ends_the_tutorial() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::PickSite);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let end = end_link(&widgets).expect("the End tutorial link").rect;
    let note = widget(&widgets, PICK_SITE).rect;
    let map = widget(&widgets, "Map").rect;
    assert!(
        map.contains(note.center()),
        "{note:?} sits near the map's top: {map:?}"
    );
    pass.click_at(&mut app, width, height, end.center());
    assert!(run(&app).is_none());
    assert!(!pending(&app));
    let widgets = pass.settled(&mut app, width, height);
    assert!(!crate::support::egui_harness::has(&widgets, PICK_SITE));
}

#[test]
fn crawl_has_full_accessible_name_and_ends_on_click() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Crawl);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let l10n = crate::support::shipped_l10n();
    let text = l10n.text("tutorial-crawl");
    assert!(text.lines().count() >= 4, "{text}");
    // Fluent keeps the warning's own line break, and so does the accessible name.
    assert!(
        text.ends_with("Keep supply equal to demand,\nor the lights go out."),
        "{text:?}"
    );
    let crawl = widget(&widgets, &text);
    assert_eq!(
        crawl.rect,
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height))
    );
    // A click anywhere ends it, and doesn't reach the map.
    let map = widget(&widgets, "Map").rect;
    pass.click_at(&mut app, width, height, map.center());
    assert!(played(&app).contains(&TutorialStep::Crawl));
    assert_eq!(*app.world().resource::<Selection>(), Selection::None);
}

#[test]
fn crawl_ends_by_itself_or_on_escape() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Crawl);
    let pass = EguiPass::attach(&mut app);
    pass.settled(&mut app, MIN.0, MIN.1);
    advance_real(&mut app, 44.0);
    pass.run(&mut app, MIN.0, MIN.1);
    assert!(!played(&app).contains(&TutorialStep::Crawl));
    advance_real(&mut app, 1.5);
    pass.run(&mut app, MIN.0, MIN.1);
    assert!(played(&app).contains(&TutorialStep::Crawl));

    at_step(&mut app, TutorialStep::Crawl);
    pass.settled(&mut app, MIN.0, MIN.1);
    press_escape(&pass, &mut app, MIN);
    assert!(played(&app).contains(&TutorialStep::Crawl));
}

/// Review Focus 2: at the smallest window and the largest UI zoom, every spotlight's note and
/// every instruction's card is drawn, and inside the screen.
#[test]
fn cards_fit_minimum_window_at_max_zoom() {
    const MAX_ZOOM: f32 = 1.5;
    let (width, height) = (MIN.0 / MAX_ZOOM, MIN.1 / MAX_ZOOM);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    let l10n = crate::support::shipped_l10n();
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = EguiPass::attach(&mut app);
    let cell = battery_site(&app);
    let mut checked = Vec::new();
    for phase_now in [DayPhase::Planning, DayPhase::Operating, DayPhase::Review] {
        match phase_now {
            DayPhase::Planning => {}
            DayPhase::Operating => execute(&mut app, Command::BeginOperating),
            DayPhase::Review => execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY }),
        }
        assert_eq!(phase(&app), phase_now);
        for def in TUTORIAL_STEPS.iter().filter(|def| {
            def.phase == phase_now
                && matches!(def.kind, StepKind::Spotlight | StepKind::Instruction)
        }) {
            at_step(&mut app, def.step);
            // Picking a site is done once a cell is selected; the later steps need one.
            let selection = if def.step == TutorialStep::PickSite {
                Selection::None
            } else {
                Selection::Cell(cell)
            };
            dispatch_in(&mut app, vec![UiAction::Select(selection)]);
            let text = def
                .text_key
                .map(|key| l10n.text(key))
                .unwrap_or_else(|| l10n.text("tutorial-construction-general"));
            // Let a reveal scroll its anchor in.
            let mut found = None;
            for _ in 0..60 {
                let widgets = pass.run(&mut app, width, height);
                if let Some(note) = widgets.iter().find(|w| w.text == text) {
                    let end = end_link(&widgets).map(|w| w.rect);
                    found = Some((note.rect, end));
                    break;
                }
            }
            let (note, end) = found.unwrap_or_else(|| panic!("{:?} drew no note", def.step));
            assert!(screen.contains_rect(note), "{:?}: {note:?}", def.step);
            if def.kind == StepKind::Instruction {
                let end = end.unwrap_or_else(|| panic!("{:?} has no End link", def.step));
                assert!(screen.contains_rect(end), "{:?}: {end:?}", def.step);
            }
            checked.push(def.step);
        }
    }
    assert_eq!(
        checked.len(),
        TUTORIAL_STEPS
            .iter()
            .filter(|def| matches!(def.kind, StepKind::Spotlight | StepKind::Instruction))
            .count()
    );
}

// --- The tutorial leaves the simulation alone (spec §7) ---

/// Runs the HUD's egui pass `passes` times in each app, a second of real time apart, as the same
/// frames. Whatever crawl or spotlight the tutorial in `apps[0]` shows is drawn, then dismissed in
/// both apps alike (without a run, tutorial actions do nothing), and recorded in `shown`.
fn tutorial_frames(
    apps: &mut [(App, EguiPass); 2],
    passes: usize,
    shown: &mut BTreeSet<TutorialStep>,
) {
    for _ in 0..passes {
        for (app, pass) in apps.iter_mut() {
            advance_real(app, 1.0);
            pass.run(app, MIN.0, MIN.1);
        }
        let on = &apps[0].0;
        let showing = run(on).and_then(|state| {
            let game = &on.world().resource::<Session>().0;
            game::hud::tutorial::active_step(state, game, *on.world().resource::<Selection>())
                .filter(|def| matches!(def.kind, StepKind::Crawl | StepKind::Spotlight))
                .map(|def| def.step)
        });
        if let Some(step) = showing {
            shown.insert(step);
            for (app, _) in apps.iter_mut() {
                tutorial(app, [TutorialAction::Dismiss(step)]);
            }
        }
        for (app, _) in apps.iter_mut() {
            update_n(app, 1);
        }
    }
}

fn state_hash(app: &App) -> u64 {
    app.world().resource::<Session>().0.session().state_hash()
}

/// The tutorial only steers the UI: a first game played with it and the same game played without
/// it, command for command and frame for frame, end in the same simulation state.
#[test]
fn tutorial_on_and_off_give_identical_state() {
    let (logs_on, logs_off) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let on = new_game(&logs_on, true, true);
    let off = new_game(&logs_off, true, false);
    assert!(run(&on).is_some() && run(&off).is_none());
    assert_eq!(state_hash(&on), state_hash(&off));
    let start = state_hash(&on);
    let mut apps = [on, off].map(|mut app| {
        let pass = EguiPass::attach(&mut app);
        (app, pass)
    });
    let mut shown = BTreeSet::new();
    let cell = battery_site(&apps[0].0);

    tutorial_frames(&mut apps, 12, &mut shown);
    for (app, _) in apps.iter_mut() {
        dispatch_in(app, vec![UiAction::Select(Selection::Cell(cell))]);
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for (app, _) in apps.iter_mut() {
        execute(
            app,
            Command::BuildPlant {
                kind: PlantKind::Battery,
                cell,
            },
        );
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for (app, _) in apps.iter_mut() {
        let session = app.world().resource::<Session>().0.session();
        let from = session.anchor_node();
        let to = session
            .plant_views()
            .into_iter()
            .find(|plant| plant.kind == PlantKind::Battery)
            .expect("the battery was queued")
            .node;
        execute(app, Command::BuildLine { from, to });
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for (app, _) in apps.iter_mut() {
        execute(app, Command::BeginOperating);
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for (app, _) in apps.iter_mut() {
        execute(app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    assert_eq!(phase(&apps[0].0), DayPhase::Review);
    // The tutorial ran the whole way: its moves played, and its crawl and spotlights were shown.
    let played = played(&apps[0].0);
    for step in [
        TutorialStep::FrameGrid,
        TutorialStep::ShowForecast,
        TutorialStep::RevealBattery,
    ] {
        assert!(played.contains(&step), "{step:?} didn't play: {played:?}");
    }
    for step in [
        TutorialStep::Crawl,
        TutorialStep::Cash,
        TutorialStep::BatterySpotlight,
        TutorialStep::ConnectSpotlight,
        TutorialStep::StartSpotlight,
        TutorialStep::Speed,
        TutorialStep::Construction,
    ] {
        assert!(shown.contains(&step), "{step:?} wasn't shown: {shown:?}");
    }
    for (app, _) in apps.iter_mut() {
        execute(app, Command::FinishReview);
    }
    tutorial_frames(&mut apps, 4, &mut shown);
    let [(on, _), (off, _)] = &apps;
    assert!(run(on).is_none() && !pending(on), "the tutorial finished");
    assert_eq!(phase(on), DayPhase::Planning);
    assert_ne!(state_hash(on), start, "the script changed the game");
    assert_eq!(state_hash(on), state_hash(off));
}
