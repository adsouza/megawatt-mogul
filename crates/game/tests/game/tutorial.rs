//! The first-game tutorial's run lifecycle (first-game tutorial spec §3, §4.3): a new game starts
//! it when progress says it is owed, loading a save never does, leaving the game drops it, and
//! leaving the first Review starts the CCGT chapter.

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
    set_reveals(&mut app);
    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 3);
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::MainMenu
    );
    assert!(run(&app).is_none());
    assert_no_reveals(&app);
    assert!(pending(&app), "leaving part-way keeps the tutorial owed");
}

#[test]
fn a_new_session_generation_clears_the_run() {
    let logs = TempDir::new().unwrap();
    let mut app = new_game(&logs, true, true);
    assert!(run(&app).is_some());
    update_n(&mut app, 2);
    assert!(run(&app).is_some(), "the run lasts while its game does");
    set_reveals(&mut app);
    update_n(&mut app, 1);
    assert!(
        ui_state(&app).reveal_plant_card.is_some() && ui_state(&app).reveal_anchor.is_some(),
        "a running tutorial keeps its reveals"
    );
    app.world_mut().resource_mut::<SessionGeneration>().0 += 1;
    update_n(&mut app, 1);
    assert!(run(&app).is_none());
    assert_no_reveals(&app);
    assert!(pending(&app));
}

#[test]
fn leaving_first_review_continues_with_supply_guidance() {
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

    set_reveals(&mut app);
    execute(&mut app, Command::FinishReview);
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Planning
    );
    assert!(run(&app).is_some(), "the CCGT walkthrough follows Review");
    assert!(
        pending(&app),
        "building and connecting supply is still owed"
    );
}

#[test]
fn starting_the_second_day_early_finishes_optional_guidance() {
    let logs = TempDir::new().unwrap();
    let mut app = new_game(&logs, true, true);
    execute(&mut app, Command::BeginOperating);
    execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    execute(&mut app, Command::FinishReview);
    assert!(run(&app).is_some());
    set_reveals(&mut app);
    execute(&mut app, Command::BeginOperating);
    assert!(run(&app).is_none());
    assert!(!pending(&app));
    assert_no_reveals(&app);
}

#[test]
fn payoff_is_scheduled_for_the_next_month_only_after_generator_and_actual_outlook_use() {
    for (generator, outlook) in [(false, false), (true, false), (false, true), (true, true)] {
        let logs = TempDir::new().unwrap();
        let mut app = tutorial_app(&logs);
        execute(&mut app, Command::BeginOperating);
        execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
        execute(&mut app, Command::FinishReview);
        if generator {
            let cell = ccgt_site(&app);
            execute(
                &mut app,
                Command::BuildPlant {
                    kind: PlantKind::Ccgt,
                    cell,
                },
            );
            let session = app.world().resource::<Session>().0.session();
            let from = session.anchor_node();
            let to = session
                .plant_views()
                .into_iter()
                .find(|p| p.cell == cell)
                .unwrap()
                .node;
            execute(&mut app, Command::BuildLine { from, to });
        }
        tutorial(
            &mut app,
            [TutorialAction::Dismiss(TutorialStep::CrystalBall)],
        );
        update_n(&mut app, 2);
        assert!(run(&app).is_some(), "the payoff chapter is still owed");
        if outlook {
            dispatch_in(
                &mut app,
                vec![UiAction::ShowOutlook(true), UiAction::ShowOutlook(false)],
            );
            update_n(&mut app, 2);
        }
        let game = &app.world().resource::<Session>().0;
        let state = run(&app).unwrap();
        assert_eq!(
            state.payoff_month,
            (generator && outlook).then(|| game
                .session()
                .clock()
                .month()
                .checked_add_months(1)
                .unwrap())
        );
        assert_ne!(
            game::hud::tutorial::active_step(state, game, Selection::None).map(|s| s.step),
            Some(TutorialStep::OpenPayoff)
        );
    }
}

#[test]
fn payoff_tutorial_closing_early_finishes_the_final_chapter_and_reopening_is_unguided() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    execute(&mut app, Command::BeginOperating);
    execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    execute(&mut app, Command::FinishReview);
    let month = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .clock()
        .month();
    app.world_mut()
        .resource_mut::<TutorialRun>()
        .0
        .as_mut()
        .unwrap()
        .payoff_month = Some(month);
    at_step(&mut app, TutorialStep::OpenPayoff);
    let cell = ccgt_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    let before = app.world().resource::<Session>().0.session().state_hash();
    dispatch_in(&mut app, vec![UiAction::ShowPayoff(true)]);
    assert!(played(&app).contains(&TutorialStep::OpenPayoff));
    // A lost selection while the modal is open must not strand its remaining guide steps.
    dispatch_in(
        &mut app,
        vec![
            UiAction::Select(Selection::None),
            UiAction::ShowPayoff(false),
        ],
    );
    for step in [
        TutorialStep::PayoffConstruction,
        TutorialStep::PayoffOperating,
        TutorialStep::ClosePayoff,
    ] {
        assert!(played(&app).contains(&step));
    }
    update_n(&mut app, 2);
    assert!(run(&app).is_none());
    assert!(!pending(&app));
    let pass = EguiPass::attach(&mut app);
    dispatch_in(&mut app, vec![UiAction::ShowPayoff(true)]);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    assert!(widgets.iter().any(|w| w.text == "Plant payoff comparison"));
    assert!(
        !widgets
            .iter()
            .any(|w| w.text == crate::support::shipped_l10n().text("tutorial-payoff-operating"))
    );
    press_escape(&pass, &mut app, MIN);
    assert!(!ui_state(&app).payoff_open);
    assert!(run(&app).is_none());
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
}

/// The review's actual Continue button leads into guidance, and the actual CCGT / line buttons
/// queue exactly one of each order. The callouts expose each control at the minimum window size.
#[test]
fn ccgt_walkthrough_after_review_builds_and_connects_through_real_widgets() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let saves = TempDir::new().unwrap();
    app.insert_resource(SaveDir(saves.path().into()));
    let pass = with_anchor_probe(&mut app);
    execute(&mut app, Command::BeginOperating);
    execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    tutorial(
        &mut app,
        [TutorialAction::Dismiss(TutorialStep::Construction)],
    );
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    pass.click_at(
        &mut app,
        MIN.0,
        MIN.1,
        find_button(&widgets, "Continue").rect.center(),
    );
    update_n(&mut app, 2);
    assert_eq!(phase(&app), DayPhase::Planning);
    assert!(run(&app).is_some());
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    widget(
        &widgets,
        "Batteries store electricity. To supply more power, let's build a new power plant.",
    );
    press_escape(&pass, &mut app, MIN);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    widget(
        &widgets,
        "Click a square on the map for your new gas plant.",
    );
    let cell = ccgt_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    for _ in 0..60 {
        pass.run(&mut app, MIN.0, MIN.1);
    }
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    widget(
        &widgets,
        "A CCGT is a gas plant you can build anywhere. It is relatively cheap and takes 4 months to build.",
    );
    press_escape(&pass, &mut app, MIN);
    for _ in 0..60 {
        pass.run(&mut app, MIN.0, MIN.1);
    }
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    let note = widget(
        &widgets,
        "Build a CCGT here. It will take 4 months to build.",
    )
    .rect;
    let target = anchors(&app)[&TutorialAnchor::PlantBuildButton];
    assert!(!note.intersects(target));
    assert_points_at(
        &arrowhead(&painted(&pass, &mut app, MIN)).unwrap(),
        target,
        "CCGT Build plant",
    );
    pass.click_at(&mut app, MIN.0, MIN.1, target.center());
    assert_eq!(
        sent::<PlayerCommand>(&app)
            .iter()
            .filter(|command| matches!(
                command.0,
                Command::BuildPlant {
                    kind: PlantKind::Ccgt,
                    ..
                }
            ))
            .count(),
        1
    );
    update_n(&mut app, 2);
    assert!(
        pending(&app),
        "a plant without a line is unfinished guidance"
    );
    let plant = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|plant| plant.kind == PlantKind::Ccgt && plant.cell == cell)
        .expect("CCGT was queued");
    assert_eq!(
        *app.world().resource::<Selection>(),
        Selection::Plant(plant.id)
    );
    pass.settled(&mut app, MIN.0, MIN.1);
    press_escape(&pass, &mut app, MIN);
    for stage in 0..2 {
        for _ in 0..60 {
            pass.run(&mut app, MIN.0, MIN.1);
        }
        let widgets = pass.settled(&mut app, MIN.0, MIN.1);
        let target = anchors(&app)[&TutorialAnchor::PlantLineButton];
        let note = widget(&widgets, "Build a line from your CCGT to your grid. The plant and line must finish building before they can supply power.").rect;
        assert!(!note.intersects(target));
        assert_points_at(
            &arrowhead(&painted(&pass, &mut app, MIN)).unwrap(),
            target,
            "CCGT connection",
        );
        pass.click_at(&mut app, MIN.0, MIN.1, target.center());
        if stage == 0 {
            assert_eq!(
                *app.world().resource::<Selection>(),
                Selection::LineFrom(plant.node)
            );
        }
        if stage == 1 {
            assert_eq!(
                sent::<PlayerCommand>(&app)
                    .iter()
                    .filter(|command| matches!(command.0,
                Command::BuildLine { from, .. } if from == plant.node))
                    .count(),
                1
            );
        }
        update_n(&mut app, 2);
    }
    assert!(
        run(&app).is_some(),
        "the save caution still needs to be shown"
    );
    assert!(pending(&app));
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    widget(&widgets, SAVE_SNAPSHOT);
    let target = anchors(&app)[&TutorialAnchor::Save];
    let save = widgets
        .iter()
        .find(|widget| widget.role == egui::accesskit::Role::Button && widget.rect == target)
        .expect("the spotlight targets the actual Save button");
    assert!(save.text.starts_with("Save"));
    // As for every spotlight, its dismissal click is consumed. Saving remains the player's action.
    pass.click_at(&mut app, MIN.0, MIN.1, target.center());
    assert!(played(&app).contains(&TutorialStep::SaveSnapshot));
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
    update_n(&mut app, 2);
    assert!(
        run(&app).is_some(),
        "the crystal-ball spotlight follows Save"
    );
    assert!(pending(&app));
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    widget(&widgets, CRYSTAL_BALL);
    let crystal = anchors(&app)[&TutorialAnchor::CrystalBall];
    assert_eq!(
        find_button(&widgets, "Crystal ball: 12-month outlook").rect,
        crystal
    );
    pass.click_at(&mut app, MIN.0, MIN.1, crystal.center());
    assert!(played(&app).contains(&TutorialStep::CrystalBall));
    assert!(
        !ui_state(&app).outlook_open,
        "the spotlight consumes its dismissal click"
    );
    update_n(&mut app, 2);
    assert!(
        run(&app).is_some(),
        "the comparison follows in a later month"
    );
    assert!(pending(&app));
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .outlook_tutorial_pending = false;
    pass.settled(&mut app, MIN.0, MIN.1);
    pass.click_at(&mut app, MIN.0, MIN.1, crystal.center());
    assert!(
        ui_state(&app).outlook_open,
        "the highlighted control opens the city preview"
    );
    pass.settled(&mut app, MIN.0, MIN.1);
    press_escape(&pass, &mut app, MIN);
    assert!(!ui_state(&app).outlook_open);
    update_n(&mut app, 2);
    let scheduled = run(&app)
        .unwrap()
        .payoff_month
        .expect("actual outlook use schedules comparison");
    assert!(
        scheduled
            > app
                .world()
                .resource::<Session>()
                .0
                .session()
                .clock()
                .month()
    );
    assert_eq!(
        game::hud::tutorial::active_step(
            run(&app).unwrap(),
            &app.world().resource::<Session>().0,
            Selection::None
        ),
        None
    );
    execute(&mut app, Command::BeginOperating);
    assert!(
        run(&app).is_some(),
        "the comparison survives the intervening day"
    );
    execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY });
    execute(&mut app, Command::FinishReview);
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .clock()
            .month(),
        scheduled
    );
    assert_eq!(
        game::hud::tutorial::active_step(
            run(&app).unwrap(),
            &app.world().resource::<Session>().0,
            Selection::None
        )
        .unwrap()
        .step,
        TutorialStep::OpenPayoff
    );
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    let compare = find_button(&widgets, "Compare plant payoff").rect;
    assert_eq!(anchors(&app)[&TutorialAnchor::PayoffButton], compare);
    let before_comparison = app.world().resource::<Session>().0.session().state_hash();
    pass.click_at(&mut app, MIN.0, MIN.1, compare.center());
    for (step, key) in [
        (
            TutorialStep::PayoffConstruction,
            "tutorial-payoff-construction",
        ),
        (TutorialStep::PayoffOperating, "tutorial-payoff-operating"),
    ] {
        let widgets = pass.settled(&mut app, MIN.0, MIN.1);
        widget(&widgets, &crate::support::shipped_l10n().text(key));
        press_escape(&pass, &mut app, MIN);
        assert!(played(&app).contains(&step));
        assert!(
            ui_state(&app).payoff_open,
            "Escape dismisses the lesson first"
        );
    }
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    widget(
        &widgets,
        &crate::support::shipped_l10n().text("tutorial-payoff-close"),
    );
    for _ in 0..60 {
        pass.run(&mut app, MIN.0, MIN.1);
    }
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    let close = find_button(&widgets, "Close comparison").rect;
    pass.click_at(&mut app, MIN.0, MIN.1, close.center());
    assert!(!ui_state(&app).payoff_open);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before_comparison
    );
    update_n(&mut app, 2);
    assert!(run(&app).is_none());
    assert!(!pending(&app));
    pass.settled(&mut app, MIN.0, MIN.1);
    let save = anchors(&app)[&TutorialAnchor::Save];
    pass.click_at(&mut app, MIN.0, MIN.1, save.center());
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Save
    );
    assert_no_reveals(&app);
}

/// The tutorial's duration follows local construction terms, including plural formatting,
/// rather than the region-wide default or a fixed number copied into the tutorial.
#[test]
fn ccgt_build_guidance_uses_the_selected_sites_effective_duration() {
    use application::{Game, InMemoryCommandLog};
    use domain::{ids::RegionId, session::GameSettings};
    use game::hud::tutorial::{TutorialView, tutorial_view};
    use infrastructure::region::RonRegionCatalog;

    let dir = TempDir::new().unwrap();
    let source = include_str!("../../../infrastructure/tests/support/economic_region_v4.ron")
        .replace(
            "(kind: Wind, capital_cost: 1000000, build_months: 2)",
            "(kind: Ccgt, capital_cost: 1000000, build_months: 1)",
        )
        .replace(
            "(kind: Wind, capital_cost: 2000000, build_months: 3)",
            "(kind: Ccgt, capital_cost: 2000000, build_months: 3)",
        );
    std::fs::write(dir.path().join("minimal.ron"), source).unwrap();
    let game = Game::start_new(
        &RonRegionCatalog::new(dir.path()),
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    let state = TutorialState::new(1, &game);
    let l10n = crate::support::shipped_l10n();
    for (cell, expected) in [
        (CellCoord { x: 4, y: 4 }, "1 month"),
        (CellCoord { x: 8, y: 5 }, "3 months"),
    ] {
        for step in [TutorialStep::CcgtSpotlight, TutorialStep::BuildCcgt] {
            let def = TUTORIAL_STEPS.iter().find(|def| def.step == step).unwrap();
            let text = match tutorial_view(def, &state, &game, Selection::Cell(cell), &l10n, 1.0) {
                TutorialView::Spotlight { text, .. } | TutorialView::Instruction { text, .. } => {
                    text
                }
                other => panic!("expected a note: {other:?}"),
            };
            assert!(
                text.contains(&format!("{expected} to build")),
                "{cell:?}: {text}"
            );
            assert!(
                !text.contains("30 months"),
                "regional default leaked: {text}"
            );
        }
    }
}

/// Asks the build menu and Review for a reveal, as the tutorial does, that nothing has finished.
fn set_reveals(app: &mut App) {
    let mut ui = app.world_mut().resource_mut::<UiState>();
    ui.reveal_plant_card = Some(PlantKind::Battery);
    ui.reveal_anchor = Some(TutorialAnchor::LineSection);
}

/// A run that ends takes its pending reveals with it.
fn assert_no_reveals(app: &App) {
    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.reveal_plant_card, None);
    assert_eq!(ui.reveal_anchor, None);
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

/// A step change drops the reveals asked for before it, with or without a run, and leaves the
/// cover and what has played alone.
#[test]
fn step_changed_drops_pending_reveals_only() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    tutorial(
        &mut app,
        [
            TutorialAction::Play(TutorialStep::RevealBattery),
            TutorialAction::Reveal(TutorialAnchor::LineSection),
            TutorialAction::Cover(true),
        ],
    );
    tutorial(&mut app, [TutorialAction::StepChanged]);
    let ui = ui_state(&app);
    assert_eq!((ui.reveal_plant_card, ui.reveal_anchor), (None, None));
    assert!(ui.tutorial_cover);
    assert_eq!(played(&app), BTreeSet::from([TutorialStep::RevealBattery]));

    app.world_mut().resource_mut::<TutorialRun>().0 = None;
    set_reveals(&mut app);
    tutorial(&mut app, [TutorialAction::StepChanged]);
    assert_no_reveals(&app);
}

#[test]
fn reveal_asks_the_build_menu_for_a_scrolling_anchor() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::PlantCard)],
    );
    assert_eq!(ui_state(&app).reveal_plant_card, Some(PlantKind::Battery));
    assert_eq!(ui_state(&app).reveal_anchor, None);
    // The card's Build plant button is revealed on its own: the card can be taller than the
    // build menu's view.
    tutorial(&mut app, [TutorialAction::RevealDone]);
    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::PlantBuildButton)],
    );
    assert_eq!(ui_state(&app).reveal_plant_card, None);
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::PlantBuildButton)
    );
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

/// `Cover` sets whether the crawl or a spotlight covers the screen, with or without a run (the
/// overlay clears it on a pass with nothing to show), and plays nothing.
#[test]
fn cover_sets_the_tutorial_cover_and_plays_nothing() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    assert!(!ui_state(&app).tutorial_cover);
    tutorial(&mut app, [TutorialAction::Cover(true)]);
    assert!(ui_state(&app).tutorial_cover);
    assert!(played(&app).is_empty());
    tutorial(&mut app, [TutorialAction::Cover(false)]);
    assert!(!ui_state(&app).tutorial_cover);

    tutorial(&mut app, [TutorialAction::Cover(true)]);
    app.world_mut().resource_mut::<TutorialRun>().0 = None;
    tutorial(&mut app, [TutorialAction::Cover(false)]);
    assert!(!ui_state(&app).tutorial_cover);
}

/// The overlay says on each pass whether it covers the screen: while the crawl or a spotlight is
/// up, not under an instruction, and not once the run has ended.
#[test]
fn the_overlay_covers_the_screen_only_while_the_crawl_or_a_spotlight_is_up() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = EguiPass::attach(&mut app);
    for (step, cover) in [
        (TutorialStep::Crawl, true),
        (TutorialStep::Cash, true),
        (TutorialStep::PickSite, false),
    ] {
        at_step(&mut app, step);
        pass.settled(&mut app, MIN.0, MIN.1);
        assert_eq!(ui_state(&app).tutorial_cover, cover, "{step:?}");
    }
    // Dismissed: the pass that ends it no longer covers.
    at_step(&mut app, TutorialStep::Cash);
    pass.settled(&mut app, MIN.0, MIN.1);
    press_escape(&pass, &mut app, MIN);
    assert!(played(&app).contains(&TutorialStep::Cash));
    assert!(!ui_state(&app).tutorial_cover);
    // A spotlight waiting behind a prompt doesn't cover.
    at_step(&mut app, TutorialStep::Cash);
    dispatch_in(
        &mut app,
        vec![UiAction::Prompt(Some(game::hud::Prompt::LeaveGame))],
    );
    pass.settled(&mut app, MIN.0, MIN.1);
    assert!(!ui_state(&app).tutorial_cover);
    dispatch_in(&mut app, vec![UiAction::Prompt(None)]);
    pass.settled(&mut app, MIN.0, MIN.1);
    assert!(ui_state(&app).tutorial_cover);
    // The run ends with the cover up: ending it clears the cover.
    app.world_mut().resource_mut::<SessionGeneration>().0 += 1;
    update_n(&mut app, 1);
    assert!(run(&app).is_none());
    assert!(!ui_state(&app).tutorial_cover);
}

/// The overlay says whether it covers the screen on every pass, but an idle pass (nothing shown
/// changes, with or without a run) leaves `UiState` unchanged for Bevy's change detection.
#[test]
fn idle_passes_leave_the_ui_state_unchanged() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = EguiPass::attach(&mut app);
    let changed = |app: &App| app.world().resource_ref::<UiState>().last_changed();
    for run in [true, false] {
        if run {
            at_step(&mut app, TutorialStep::PickSite);
        } else {
            app.world_mut().resource_mut::<TutorialRun>().0 = None;
        }
        pass.settled(&mut app, MIN.0, MIN.1);
        let before = changed(&app);
        for _ in 0..5 {
            pass.run(&mut app, MIN.0, MIN.1);
        }
        assert_eq!(changed(&app), before, "run: {run}");
    }
}

/// Without a run the tutorial's actions change nothing; only `RevealDone` (and `StepChanged`)
/// still clear a reveal.
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
    tutorial(&mut app, every_play);
    assert_eq!((ui_state(&app).clone(), focus(&app)), before);
    assert!(run(&app).is_none());
    assert!(pending(&app), "the tutorial is still owed");

    app.world_mut().resource_mut::<UiState>().reveal_plant_card = Some(PlantKind::Battery);
    tutorial(&mut app, [TutorialAction::RevealDone]);
    assert_eq!(ui_state(&app).reveal_plant_card, None);
}

fn any_tutorial_action() -> impl proptest::strategy::Strategy<Value = TutorialAction> {
    use proptest::strategy::Strategy;
    let all: Vec<_> = TUTORIAL_STEPS
        .iter()
        .chain(game::hud::tutorial::OUTLOOK_STEPS.iter())
        .map(|def| def.step)
        .collect();
    (0..all.len(), 0u8..6).prop_map(move |(index, which)| {
        let step = all[index];
        match which {
            0 => TutorialAction::Play(step),
            1 => TutorialAction::Dismiss(step),
            2 => TutorialAction::RevealDone,
            3 => TutorialAction::Cover(index % 2 == 0),
            4 => TutorialAction::StepChanged,
            _ => TutorialAction::Reveal(ANCHORS[index % ANCHORS.len()]),
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

const ANCHORS: [TutorialAnchor; 17] = [
    TutorialAnchor::Cash,
    TutorialAnchor::Map,
    TutorialAnchor::PlantCard,
    TutorialAnchor::PlantBuildButton,
    TutorialAnchor::LineSection,
    TutorialAnchor::PlantLineButton,
    TutorialAnchor::StartDay,
    TutorialAnchor::SpeedControls,
    TutorialAnchor::ReviewConstruction,
    TutorialAnchor::Save,
    TutorialAnchor::CrystalBall,
    TutorialAnchor::OutlookCity,
    TutorialAnchor::OutlookGraph,
    TutorialAnchor::PayoffButton,
    TutorialAnchor::PayoffGraph,
    TutorialAnchor::PayoffTable,
    TutorialAnchor::PayoffClose,
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

fn ccgt_site(app: &App) -> CellCoord {
    let session = app.world().resource::<Session>().0.session();
    let home = session
        .city_views()
        .into_iter()
        .find(|city| city.node == session.anchor_node())
        .unwrap()
        .cell;
    let terrain = session.region().terrain();
    (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .filter(|cell| session.plant_quote(PlantKind::Ccgt, *cell).is_ok())
        .min_by(|a, b| {
            domain::grid::distance_cells(*a, home)
                .total_cmp(&domain::grid::distance_cells(*b, home))
        })
        .expect("a site for a CCGT")
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
        TutorialAnchor::PlantCard,
        TutorialAnchor::PlantBuildButton,
        TutorialAnchor::LineSection,
        TutorialAnchor::StartDay,
    ] {
        let rect = seen.get(&anchor).unwrap_or_else(|| panic!("{anchor:?}"));
        assert!(rect.is_positive(), "{anchor:?}: {rect:?}");
    }
    // The battery's Build plant button, inside its card.
    let card = seen[&TutorialAnchor::PlantCard];
    let build = seen[&TutorialAnchor::PlantBuildButton];
    assert!(card.contains_rect(build), "{build:?} in {card:?}");
    let widgets = pass.settled(&mut app, width, height);
    assert!(
        widgets
            .iter()
            .any(|w| w.role == egui::accesskit::Role::Button
                && w.text == "Build plant"
                && w.rect == build),
        "{build:?} is a Build plant button"
    );
    assert!(!seen.contains_key(&TutorialAnchor::SpeedControls));
    assert!(!seen.contains_key(&TutorialAnchor::ReviewConstruction));

    // Without a selected cell there is no battery card to point at.
    dispatch_in(&mut app, vec![UiAction::Select(Selection::None)]);
    pass.settled(&mut app, width, height);
    assert!(!anchors(&app).contains_key(&TutorialAnchor::PlantCard));
    assert!(!anchors(&app).contains_key(&TutorialAnchor::PlantBuildButton));
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
        !seen.contains_key(&TutorialAnchor::PlantCard),
        "the card starts below the fold: {seen:?}"
    );
    assert!(
        !seen.contains_key(&TutorialAnchor::PlantBuildButton),
        "{seen:?}"
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
        !anchors(&app).contains_key(&TutorialAnchor::PlantCard),
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
    let card = anchors(&app)[&TutorialAnchor::PlantCard];
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

fn open_prompt(app: &App) -> Option<game::hud::Prompt> {
    app.world().resource::<game::hud::OpenPrompt>().0
}

fn phase(app: &App) -> DayPhase {
    app.world().resource::<Session>().0.session().phase()
}

const CRYSTAL_BALL: &str = "Before connecting a new city, use the crystal ball to compare demand and supply for the next 12 months. Choose a city to preview its extra demand once the connection is built.";
const SAVE_SNAPSHOT: &str = "Before risky moves, like connecting a new city, use Save to create a named snapshot. You can load it to return to this point if things go wrong.";
const CASH_NOTE: &str =
    "This is your money. Grow it by selling power, without blacking out your customers.";
const PICK_SITE: &str = "Click a square on the map to see what you can build there.";
const START_NOTE: &str = "Ready? Start the day.";
const BUILD_BATTERY: &str = "Build a battery here.";
const CONNECT: &str = "Build a line from your battery to your grid.";
const CONNECT_NOTE: &str = "A power plant only helps once a line connects it to your grid.";

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
        match tutorial_view(cash, state, game, Selection::None, &l10n, at) {
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
        tutorial_view(
            def(TutorialStep::FrameGrid),
            state,
            game,
            Selection::None,
            &l10n,
            0.0
        ),
        TutorialView::Move(TutorialStep::FrameGrid)
    );
    match tutorial_view(
        def(TutorialStep::Crawl),
        state,
        game,
        Selection::None,
        &l10n,
        9.0,
    ) {
        TutorialView::Crawl { text, progress } => {
            assert!(text.starts_with("You run a power company."), "{text}");
            assert!((progress - 0.2).abs() < 1e-6, "{progress}");
        }
        other => panic!("{other:?}"),
    }
    // Each instruction points at the control it asks for (picking a site, at the map).
    for (step, anchor, text) in [
        (TutorialStep::PickSite, TutorialAnchor::Map, PICK_SITE),
        (
            TutorialStep::BuildBattery,
            TutorialAnchor::PlantBuildButton,
            BUILD_BATTERY,
        ),
        (
            TutorialStep::ConnectBattery,
            TutorialAnchor::PlantLineButton,
            CONNECT,
        ),
        (TutorialStep::StartDay, TutorialAnchor::StartDay, START_NOTE),
    ] {
        assert_eq!(
            tutorial_view(def(step), state, game, Selection::None, &l10n, 0.0),
            TutorialView::Instruction {
                anchor,
                text: text.into(),
            }
        );
    }
    assert_eq!(
        TUTORIAL_STEPS
            .iter()
            .filter(|def| def.kind == StepKind::Instruction)
            .count(),
        9
    );
    assert_eq!(
        def(TutorialStep::BatterySpotlight).anchor,
        Some(TutorialAnchor::PlantCard)
    );
    // The Construction note is chosen from what was ordered: nothing, here.
    match tutorial_view(
        def(TutorialStep::Construction),
        state,
        game,
        Selection::None,
        &l10n,
        1.0,
    ) {
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

/// An instruction never stands between the player and a prompt: while one is open its callout
/// isn't drawn at all, so clicks where it was reach the prompt's backdrop, and the prompt's buttons work.
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
    // A click where the callout was reaches the prompt's backdrop, which closes it.
    pass.click_at(&mut app, width, height, note.center());
    assert_eq!(open_prompt(&app), None);

    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    let widgets = pass.settled(&mut app, width, height);
    assert!(!crate::support::egui_harness::has(&widgets, PICK_SITE));
    let stay = find_button(&widgets, "Stay").rect;
    pass.click_at(&mut app, width, height, stay.center());
    assert_eq!(open_prompt(&app), None);
    assert!(run(&app).is_some());
    // With the prompt closed, the callout is back.
    let widgets = pass.settled(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, PICK_SITE));
}

/// An instruction about the map sits near the map's top, has no buttons, and lets a click on
/// its own text through to the map.
#[test]
fn instruction_sits_on_the_map_and_takes_no_clicks() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::PickSite);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let note = widget(&widgets, PICK_SITE).rect;
    let map = widget(&widgets, "Map").rect;
    assert!(
        map.contains(note.center()),
        "{note:?} sits near the map's top: {map:?}"
    );
    assert!(
        !widgets
            .iter()
            .any(|w| w.role == egui::accesskit::Role::Button && note.contains_rect(w.rect)),
        "the callout has no buttons"
    );
    pass.click_at(&mut app, width, height, note.center());
    assert!(
        matches!(*app.world().resource::<Selection>(), Selection::Cell(_)),
        "the click reached the map"
    );
    assert!(run(&app).is_some());
}

/// The run at the battery instruction as the player meets it: a site picked, the battery card
/// scrolled into view by the move before its spotlight, and the spotlight dismissed.
pub(crate) fn at_battery_instruction(app: &mut App, pass: &EguiPass, (width, height): (f32, f32)) {
    let cell = battery_site(app);
    dispatch_in(app, vec![UiAction::Select(Selection::Cell(cell))]);
    at_step(app, TutorialStep::RevealBattery);
    // The reveal plays, and its scroll animation finishes.
    for _ in 0..60 {
        pass.run(app, width, height);
    }
    assert!(played(app).contains(&TutorialStep::RevealBattery));
    tutorial(
        app,
        [TutorialAction::Dismiss(TutorialStep::BatterySpotlight)],
    );
    pass.settled(app, width, height);
}

/// The arrowhead painted in the accent, as its points.
pub(crate) fn arrowhead(painted: &[egui::epaint::ClippedShape]) -> Option<Vec<egui::Pos2>> {
    painted.iter().find_map(|clipped| match &clipped.shape {
        egui::Shape::Path(path) if path.closed && path.fill == game::hud::theme::ACCENT_YELLOW => {
            Some(path.points.clone())
        }
        _ => None,
    })
}

/// At the largest UI zoom the battery card is taller than the build menu's view, so revealing it
/// for its spotlight leaves its Build plant button out of view. The battery instruction then asks
/// for the button once, and its arrow points at it; scrolled away again, it isn't asked for twice.
#[test]
fn battery_instruction_reveals_its_button_once() {
    const MAX_ZOOM: f32 = 1.5;
    let (width, height) = (MIN.0 / MAX_ZOOM, MIN.1 / MAX_ZOOM);
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    at_step(&mut app, TutorialStep::BatterySpotlight);
    for _ in 0..60 {
        pass.run(&mut app, width, height);
    }
    let seen = anchors(&app);
    assert!(seen.contains_key(&TutorialAnchor::PlantCard), "{seen:?}");
    assert!(
        !seen.contains_key(&TutorialAnchor::PlantBuildButton),
        "the test needs the button out of view: {seen:?}"
    );
    tutorial(
        &mut app,
        [TutorialAction::Dismiss(TutorialStep::BatterySpotlight)],
    );
    pass.run(&mut app, width, height);
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::PlantBuildButton)
    );
    // Let the scroll animation finish.
    for _ in 0..60 {
        pass.run(&mut app, width, height);
    }
    let widgets = pass.settled(&mut app, width, height);
    let button = *anchors(&app)
        .get(&TutorialAnchor::PlantBuildButton)
        .expect("the reveal brought the button into view");
    assert_eq!(ui_state(&app).reveal_anchor, None);
    let note = widget(&widgets, BUILD_BATTERY).rect;
    assert!(
        note.left() >= button.right() + 28.0,
        "{note:?} / {button:?}"
    );
    // Asked once: with the button scrolled away again, the instruction doesn't scroll it back.
    let at = button.center();
    for _ in 0..30 {
        pass.run_with(
            &mut app,
            width,
            height,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 400.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
            ],
        );
    }
    pass.settled(&mut app, width, height);
    assert!(
        !anchors(&app).contains_key(&TutorialAnchor::PlantBuildButton),
        "scrolled away"
    );
    assert_eq!(ui_state(&app).reveal_anchor, None);
}

/// The user's request: "Build a battery here." isn't a button-like card but text to the right of
/// the battery card's Build plant button, vertically centred on it, with an arrow from the text
/// pointing at the button; and the button still builds, even clicked right by the arrow's tip.
#[test]
fn battery_instruction_is_an_arrow_callout_beside_build_plant() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    at_battery_instruction(&mut app, &pass, MIN);
    let widgets = pass.settled(&mut app, width, height);
    let button = *anchors(&app)
        .get(&TutorialAnchor::PlantBuildButton)
        .expect("the battery's Build plant button is in view");
    let note = widget(&widgets, BUILD_BATTERY).rect;
    // To the right, past the arrow (28 points) and the backing's margin (4), centred on it.
    let gap = note.left() - button.right();
    assert!(
        (28.0..=33.0).contains(&gap),
        "gap {gap}: {note:?} / {button:?}"
    );
    assert!(
        (note.center().y - button.center().y).abs() <= 1.0,
        "{note:?} is centred on {button:?}"
    );
    assert!(note.right() <= width - 8.0, "{note:?}");
    let output = pass.paint_with(&mut app, width, height, Vec::new());
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    // The arrow's tip touches the button's right edge, and its head points left, at the button.
    let head = arrowhead(&painted).expect("an arrow");
    let tip = head
        .iter()
        .copied()
        .min_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap();
    assert!(
        (tip.x - button.right()).abs() <= 0.5 && (tip.y - button.center().y).abs() <= 1.0,
        "{tip:?} touches {button:?}"
    );
    assert!(
        head.iter()
            .filter(|p| **p != tip)
            .all(|p| p.x > tip.x + 7.0),
        "{head:?}"
    );
    // No card: nothing is outlined in the accent.
    assert!(
        !painted.iter().any(|clipped| matches!(&clipped.shape,
            egui::Shape::Rect(rect) if rect.stroke.color == game::hud::theme::ACCENT_YELLOW
                && rect.stroke.width > 0.0)),
        "an outlined card was painted"
    );
    // The callout doesn't cover the button: a click by the arrow's tip builds the battery.
    let cell = battery_site(&app);
    pass.click_at(
        &mut app,
        width,
        height,
        egui::pos2(button.right() - 2.0, button.center().y),
    );
    assert_eq!(
        sent::<PlayerCommand>(&app)
            .into_iter()
            .map(|command| command.0)
            .collect::<Vec<_>>(),
        [Command::BuildPlant {
            kind: PlantKind::Battery,
            cell
        }]
    );
}

/// Queueing the battery selects it, so the connect steps point at a line section that starts
/// from the battery: its node is the first start offered. The steps before stay done.
#[test]
fn queueing_the_battery_selects_it_and_the_line_section_starts_from_it() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    at_battery_instruction(&mut app, &pass, MIN);
    let cell = battery_site(&app);
    let button = *anchors(&app)
        .get(&TutorialAnchor::PlantBuildButton)
        .expect("the battery's Build plant button is in view");

    pass.click_at(&mut app, width, height, button.center());
    update_n(&mut app, 1);

    let battery = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|plant| plant.kind == PlantKind::Battery && plant.cell == cell)
        .expect("the battery was queued");
    let selection = *app.world().resource::<Selection>();
    assert_eq!(selection, Selection::Plant(battery.id));
    let game = &app.world().resource::<Session>().0;
    let model = game::hud::build_model(
        game,
        &crate::support::shipped_l10n(),
        selection,
        &crate::support::usd_style(),
    )
    .expect("the build menu is open in Planning");
    let game::hud::LineSection::Start { from, .. } = model.line else {
        panic!("the line section asks where the line starts");
    };
    assert_eq!(
        from[0].action,
        UiAction::Select(Selection::LineFrom(battery.node))
    );
    let state = run(&app).expect("the tutorial runs");
    assert_eq!(
        game::hud::tutorial::active_step(state, game, selection).map(|def| def.step),
        Some(TutorialStep::ConnectSpotlight),
        "PickSite and BuildBattery stay done"
    );
}

/// What a pass paints.
fn painted(
    pass: &EguiPass,
    app: &mut App,
    (width, height): (f32, f32),
) -> Vec<egui::epaint::ClippedShape> {
    let output = pass.paint_with(app, width, height, Vec::new());
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    painted
}

/// Whether anything painted is outlined in the accent, as the old instruction card was.
fn outlined_card(painted: &[egui::epaint::ClippedShape]) -> bool {
    painted.iter().any(|clipped| {
        matches!(&clipped.shape,
            egui::Shape::Rect(rect) if rect.stroke.color == game::hud::theme::ACCENT_YELLOW
                && rect.stroke.width > 0.0)
    })
}

/// The arrowhead's tip touches `target`'s edge, and the head points at it: its other two points
/// lie a head's length (8 points) away from the target.
fn assert_points_at(head: &[egui::Pos2], target: egui::Rect, what: &str) {
    assert_eq!(head.len(), 3, "{what}: {head:?}");
    let tip = head
        .iter()
        .copied()
        .min_by(|a, b| {
            target
                .distance_to_pos(*a)
                .total_cmp(&target.distance_to_pos(*b))
        })
        .unwrap();
    assert!(
        target.expand(0.5).contains(tip) && !target.shrink(0.5).contains(tip),
        "{what}: the tip {tip:?} is on {target:?}'s edge"
    );
    assert!(
        head.iter()
            .filter(|p| **p != tip)
            .all(|p| target.distance_to_pos(*p) >= 7.0),
        "{what}: {head:?} points at {target:?}"
    );
}

/// The user's request, for picking a site: there is no one control to point at, so the callout
/// sits inside the map's top edge, centred on it, its arrow pointing down into the map, and the
/// map still takes clicks, even at the arrow's tip.
#[test]
fn pick_site_instruction_points_down_into_the_map() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::PickSite);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let map = anchors(&app)[&TutorialAnchor::Map];
    let note = widget(&widgets, PICK_SITE).rect;
    assert!(map.contains_rect(note), "{note:?} in {map:?}");
    assert!(
        (note.center().x - map.center().x).abs() <= 1.0,
        "{note:?} is centred on {map:?}"
    );
    // Its backing 10 points below the map's top, the text 4 inside that.
    assert!(
        (note.top() - map.top() - 14.0).abs() <= 1.0,
        "{note:?} is at {map:?}'s top"
    );
    let painted = painted(&pass, &mut app, MIN);
    assert!(!outlined_card(&painted), "an outlined card was painted");
    let head = arrowhead(&painted).expect("an arrow");
    let tip = head
        .iter()
        .copied()
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    assert!(map.contains(tip), "{tip:?} is in {map:?}");
    assert!((tip.x - map.center().x).abs() <= 1.0, "{tip:?}");
    // The arrow runs 28 points down from the backing, 4 below the text.
    assert!(
        (tip.y - (note.bottom() + 4.0 + 28.0)).abs() <= 1.0,
        "{tip:?} / {note:?}"
    );
    assert!(
        head.iter()
            .filter(|p| **p != tip)
            .all(|p| p.y <= tip.y - 7.5),
        "{head:?} points down"
    );
    pass.click_at(&mut app, width, height, tip);
    assert!(
        matches!(*app.world().resource::<Selection>(), Selection::Cell(_)),
        "the click by the tip reached the map"
    );
}

/// The run at the connect instruction as the player meets it: the battery ordered from its card
/// (which selects it) and the connect spotlight dismissed. The battery's node.
fn at_connect_instruction(app: &mut App, pass: &EguiPass, size: (f32, f32)) -> domain::ids::NodeId {
    at_battery_instruction(app, pass, size);
    let cell = battery_site(app);
    let button = anchors(app)[&TutorialAnchor::PlantBuildButton];
    pass.click_at(app, size.0, size.1, button.center());
    update_n(app, 1);
    let battery = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|plant| plant.kind == PlantKind::Battery && plant.cell == cell)
        .expect("the battery was queued");
    assert_eq!(
        *app.world().resource::<Selection>(),
        Selection::Plant(battery.id)
    );
    tutorial(
        app,
        [TutorialAction::Dismiss(TutorialStep::ConnectSpotlight)],
    );
    battery.node
}

/// The connect instruction's arrow points at the button that takes the line on from the battery:
/// *Line from* the battery, then, once the line starts there, the first target's *Build line*.
/// Each is scrolled into view, the callout doesn't cover it, and it still works.
#[test]
fn connect_instruction_points_at_the_line_from_the_battery_then_its_first_target() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let node = at_connect_instruction(&mut app, &pass, MIN);
    for (stage, label) in ["Line from ", "Build line"].into_iter().enumerate() {
        // Let a reveal scroll the button in.
        for _ in 0..60 {
            pass.run(&mut app, width, height);
        }
        let widgets = pass.settled(&mut app, width, height);
        let target = *anchors(&app)
            .get(&TutorialAnchor::PlantLineButton)
            .unwrap_or_else(|| panic!("stage {stage}: the button is in view"));
        let button = widgets
            .iter()
            .find(|w| w.role == egui::accesskit::Role::Button && w.rect == target)
            .unwrap_or_else(|| panic!("stage {stage}: {target:?} is a button"));
        assert!(button.text.starts_with(label), "stage {stage}: {button:?}");
        let note = widget(&widgets, CONNECT).rect;
        assert!(!note.intersects(target), "{note:?} covers {target:?}");
        let painted = painted(&pass, &mut app, MIN);
        assert!(!outlined_card(&painted), "stage {stage}: an outlined card");
        let head = arrowhead(&painted).unwrap_or_else(|| panic!("stage {stage}: an arrow"));
        assert_points_at(&head, target, &format!("stage {stage}"));
        pass.click_at(&mut app, width, height, target.center());
        if stage == 0 {
            assert_eq!(
                *app.world().resource::<Selection>(),
                Selection::LineFrom(node),
                "Line from the battery was clicked"
            );
        } else {
            // The battery's order is still in the message buffer.
            let sent: Vec<Command> = sent::<PlayerCommand>(&app)
                .into_iter()
                .map(|command| command.0)
                .collect();
            assert!(
                matches!(sent.last(), Some(Command::BuildLine { from, .. }) if *from == node),
                "{sent:?}"
            );
        }
    }
}

/// A reveal its step asked for but no panel could finish (here the connect instruction's, with a
/// line started somewhere else) goes with its step: the next step doesn't inherit it, so the
/// build menu doesn't jump when the anchor is drawn later.
#[test]
fn a_reveal_its_step_never_finished_goes_with_the_step() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let node = at_connect_instruction(&mut app, &pass, MIN);
    let home = app.world().resource::<Session>().0.session().anchor_node();
    assert_ne!(home, node);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::LineFrom(home))]);
    for _ in 0..10 {
        pass.run(&mut app, width, height);
    }
    assert_eq!(
        ui_state(&app).reveal_anchor,
        Some(TutorialAnchor::PlantLineButton),
        "the test needs a reveal no panel can finish"
    );
    at_step(&mut app, TutorialStep::StartSpotlight);
    pass.run(&mut app, width, height);
    assert_eq!(ui_state(&app).reveal_anchor, None);
    assert_eq!(ui_state(&app).reveal_plant_card, None);
    // The button it asked for is drawn again: nothing scrolls to it.
    dispatch_in(&mut app, vec![UiAction::Select(Selection::LineFrom(node))]);
    let button = |app: &App| anchors(app).get(&TutorialAnchor::PlantLineButton).copied();
    for _ in 0..2 {
        pass.run(&mut app, width, height);
    }
    let drawn = button(&app);
    assert!(drawn.is_some(), "the test needs the button drawn");
    for _ in 0..60 {
        pass.run(&mut app, width, height);
    }
    assert_eq!(button(&app), drawn, "the build menu scrolled");
}

/// Starting the day: the arrow points at *Start the day*, which still starts it.
#[test]
fn start_instruction_points_at_start_the_day() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::StartDay);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let widgets = pass.settled(&mut app, width, height);
    let target = anchors(&app)[&TutorialAnchor::StartDay];
    assert_eq!(find_button(&widgets, "Start the day").rect, target);
    let note = widget(&widgets, START_NOTE).rect;
    assert!(!note.intersects(target), "{note:?} covers {target:?}");
    let painted = painted(&pass, &mut app, MIN);
    assert!(!outlined_card(&painted), "an outlined card was painted");
    assert_points_at(&arrowhead(&painted).expect("an arrow"), target, "start");
    pass.click_at(&mut app, width, height, target.center());
    // Start the day was clicked: the day begins, or the forecast's shortfall is put to the player.
    let sent: Vec<Command> = sent::<PlayerCommand>(&app)
        .into_iter()
        .map(|command| command.0)
        .collect();
    let prompt = open_prompt(&app);
    assert!(
        matches!(sent.as_slice(), [Command::BeginOperating])
            || (sent.is_empty()
                && matches!(prompt, Some(game::hud::Prompt::ShortfallStart { .. }))),
        "{sent:?}, {prompt:?}"
    );
}

/// With nothing to point at (no battery of the player's to connect), the instruction is still
/// shown: plain text on its backing at the map's top centre, with no arrow and no outline.
#[test]
fn an_instruction_without_its_anchor_is_a_plain_callout_at_the_map_top() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    at_step(&mut app, TutorialStep::ConnectBattery);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    for _ in 0..60 {
        pass.run(&mut app, width, height);
    }
    let widgets = pass.settled(&mut app, width, height);
    let seen = anchors(&app);
    assert!(
        !seen.contains_key(&TutorialAnchor::PlantLineButton),
        "{seen:?}"
    );
    let map = seen[&TutorialAnchor::Map];
    let note = widget(&widgets, CONNECT).rect;
    assert!(map.contains_rect(note), "{note:?} in {map:?}");
    assert!((note.center().x - map.center().x).abs() <= 1.0, "{note:?}");
    assert!((note.top() - map.top() - 14.0).abs() <= 1.0, "{note:?}");
    let painted = painted(&pass, &mut app, MIN);
    assert!(!outlined_card(&painted), "an outlined card was painted");
    assert_eq!(arrowhead(&painted), None, "nothing to point at");
}

/// Prompts come first: while one is open an arrow callout isn't painted at all, text or arrow,
/// and it comes back when the prompt closes.
#[test]
fn an_arrow_callout_steps_aside_for_a_prompt() {
    use game::hud::Prompt;
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    at_battery_instruction(&mut app, &pass, MIN);
    let shows = |pass: &EguiPass, app: &mut App| {
        let painted = painted(pass, app, MIN);
        let text = painted.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(t) if t.galley.text() == BUILD_BATTERY)
        });
        (text, arrowhead(&painted).is_some())
    };
    assert_eq!(shows(&pass, &mut app), (true, true));
    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    pass.settled(&mut app, width, height);
    assert_eq!(shows(&pass, &mut app), (false, false));
    dispatch_in(&mut app, vec![UiAction::Prompt(None)]);
    pass.settled(&mut app, width, height);
    assert_eq!(shows(&pass, &mut app), (true, true));
}

/// A spotlight's clock counts only the passes it is drawn on: hidden mid-hold (its anchor
/// scrolled away), it pauses, and shown again it is still showing and runs out its remaining time.
#[test]
fn a_spotlight_clock_pauses_while_its_anchor_is_hidden() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    at_step(&mut app, TutorialStep::ConnectSpotlight);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let showing = |pass: &EguiPass, app: &mut App| {
        crate::support::egui_harness::has(&pass.run(app, width, height), CONNECT_NOTE)
    };
    // Its reveal scrolls the line section in, and it shows.
    let mut shown = false;
    for _ in 0..60 {
        shown = showing(&pass, &mut app);
    }
    assert!(shown);
    advance_real(&mut app, 2.0);
    assert!(showing(&pass, &mut app));
    // Scrolling the battery card to the middle of the build menu takes the section out of view.
    // The first time, the paused spotlight asks for it back; the second, it waits.
    for _ in 0..2 {
        tutorial(
            &mut app,
            [TutorialAction::Reveal(TutorialAnchor::PlantCard)],
        );
        for _ in 0..60 {
            pass.run(&mut app, width, height);
        }
    }
    assert!(
        !anchors(&app).contains_key(&TutorialAnchor::LineSection),
        "the test needs the section out of view"
    );
    for _ in 0..10 {
        advance_real(&mut app, 1.0);
        assert!(!showing(&pass, &mut app));
    }
    assert!(!played(&app).contains(&TutorialStep::ConnectSpotlight));
    // Back in view: it is still showing, with the rest of its time to run.
    tutorial(
        &mut app,
        [TutorialAction::Reveal(TutorialAnchor::LineSection)],
    );
    let mut shown = false;
    for _ in 0..60 {
        shown = showing(&pass, &mut app);
    }
    assert!(shown, "shown again");
    assert!(!played(&app).contains(&TutorialStep::ConnectSpotlight));
    advance_real(&mut app, 2.0);
    assert!(showing(&pass, &mut app));
    assert!(!played(&app).contains(&TutorialStep::ConnectSpotlight));
    advance_real(&mut app, 0.7);
    pass.run(&mut app, width, height);
    assert!(played(&app).contains(&TutorialStep::ConnectSpotlight));
}

/// A spotlight paused part-way because its anchor was scrolled away asks once (for each
/// selection) for it to be scrolled back, as it did before it started: hidden again under the same
/// selection it waits for the player; under another selection it asks again.
#[test]
fn a_paused_spotlight_asks_for_its_anchor_once_per_selection() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let cell = battery_site(&app);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(cell))]);
    at_step(&mut app, TutorialStep::ConnectSpotlight);
    let pass = with_anchor_probe(&mut app);
    let (width, height) = MIN;
    let showing = |pass: &EguiPass, app: &mut App| {
        crate::support::egui_harness::has(&pass.run(app, width, height), CONNECT_NOTE)
    };
    let shows_within =
        |pass: &EguiPass, app: &mut App, passes: usize| (0..passes).any(|_| showing(pass, app));
    let hide = |pass: &EguiPass, app: &mut App| {
        tutorial(app, [TutorialAction::Reveal(TutorialAnchor::PlantCard)]);
        // The card's reveal scrolls the section away; the overlay sees it gone the pass after.
        for _ in 0..3 {
            pass.run(app, width, height);
        }
    };
    assert!(shows_within(&pass, &mut app, 60));
    advance_real(&mut app, 1.0);
    assert!(showing(&pass, &mut app));
    // Hidden: it asks for the section back, and shows again.
    hide(&pass, &mut app);
    assert!(shows_within(&pass, &mut app, 60), "asked for its anchor");
    // Hidden again under the same selection: it has asked once, so it waits.
    hide(&pass, &mut app);
    assert!(!shows_within(&pass, &mut app, 60), "asked twice");
    assert_eq!(ui_state(&app).reveal_anchor, None);
    // Another selection: it asks again.
    let other = CellCoord {
        x: cell.x + 1,
        y: cell.y,
    };
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Cell(other))]);
    assert!(
        shows_within(&pass, &mut app, 60),
        "asked under the new selection"
    );
    assert!(!played(&app).contains(&TutorialStep::ConnectSpotlight));
}

/// The same for a prompt opened mid-hold: the spotlight steps aside and its clock waits.
#[test]
fn a_spotlight_clock_pauses_while_a_prompt_is_open() {
    use game::hud::Prompt;
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Cash);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    pass.settled(&mut app, width, height);
    advance_real(&mut app, 2.0);
    let widgets = pass.run(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    dispatch_in(&mut app, vec![UiAction::Prompt(Some(Prompt::LeaveGame))]);
    for _ in 0..10 {
        advance_real(&mut app, 1.0);
        let widgets = pass.run(&mut app, width, height);
        assert!(!crate::support::egui_harness::has(&widgets, CASH_NOTE));
    }
    dispatch_in(&mut app, vec![UiAction::Prompt(None)]);
    let widgets = pass.run(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    assert!(!played(&app).contains(&TutorialStep::Cash));
    advance_real(&mut app, 2.0);
    pass.run(&mut app, width, height);
    assert!(!played(&app).contains(&TutorialStep::Cash));
    advance_real(&mut app, 0.7);
    pass.run(&mut app, width, height);
    assert!(played(&app).contains(&TutorialStep::Cash));
}

/// The same while the save screen is open over it (the automatic save-failure screen among them):
/// the overlay doesn't run then, and the time it was away doesn't count when it is back.
#[test]
fn a_spotlight_clock_pauses_while_the_save_screen_is_open() {
    use game::sim::SaveScreen;
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Cash);
    let pass = EguiPass::attach(&mut app);
    let (width, height) = MIN;
    pass.settled(&mut app, width, height);
    advance_real(&mut app, 2.0);
    let widgets = pass.run(&mut app, width, height);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    app.world_mut().resource_mut::<Persistence>().screen = SaveScreen::Automatic;
    for _ in 0..10 {
        advance_real(&mut app, 1.0);
        pass.run(&mut app, width, height);
    }
    assert!(!played(&app).contains(&TutorialStep::Cash));
    app.world_mut().resource_mut::<Persistence>().screen = SaveScreen::Closed;
    let widgets = pass.run(&mut app, width, height);
    assert!(
        crate::support::egui_harness::has(&widgets, CASH_NOTE),
        "still showing"
    );
    assert!(!played(&app).contains(&TutorialStep::Cash));
    advance_real(&mut app, 2.0);
    pass.run(&mut app, width, height);
    assert!(!played(&app).contains(&TutorialStep::Cash));
    advance_real(&mut app, 0.7);
    pass.run(&mut app, width, height);
    assert!(played(&app).contains(&TutorialStep::Cash));
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

// --- Escape and shortcuts under a spotlight or the crawl (only the active layer handles them) ---

fn milestones_open(app: &App) -> bool {
    app.world().resource::<game::hud::MilestonesOpen>().0
}

/// A spotlight covers the Milestones window: Escape ends the spotlight and leaves the window
/// open, and the next Escape closes the window.
#[test]
fn escape_ends_a_spotlight_over_milestones_and_leaves_the_window_open() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Cash);
    dispatch_in(&mut app, vec![UiAction::ToggleMilestones]);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    assert!(crate::support::egui_harness::has(&widgets, CASH_NOTE));
    assert!(crate::support::egui_harness::has(&widgets, "Milestones"));
    press_escape(&pass, &mut app, MIN);
    assert!(played(&app).contains(&TutorialStep::Cash));
    assert!(
        milestones_open(&app),
        "the window under the spotlight stays open"
    );
    // With the spotlight gone, Escape reaches the window.
    pass.settled(&mut app, MIN.0, MIN.1);
    press_escape(&pass, &mut app, MIN);
    assert!(!milestones_open(&app));
}

/// The same for the crawl over Milestones.
#[test]
fn escape_ends_the_crawl_over_milestones_and_leaves_the_window_open() {
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    at_step(&mut app, TutorialStep::Crawl);
    dispatch_in(&mut app, vec![UiAction::ToggleMilestones]);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    assert!(crate::support::egui_harness::has(&widgets, "Milestones"));
    press_escape(&pass, &mut app, MIN);
    assert!(played(&app).contains(&TutorialStep::Crawl));
    assert!(
        milestones_open(&app),
        "the window under the crawl stays open"
    );
}

/// A tutorial run in SGCC, the shipped region with HVDC equipment, so the Links window opens.
fn sgcc_tutorial_app(logs: &TempDir) -> App {
    let mut app = hud_app(logs.path());
    set_progress(&mut app, true, true);
    request(&mut app, "sgcc");
    assert!(run(&app).is_some());
    app
}

/// The crawl and a spotlight each cover the Links window: Escape ends them, not the window.
#[test]
fn escape_ends_the_crawl_or_a_spotlight_over_links_and_leaves_the_window_open() {
    for step in [TutorialStep::Crawl, TutorialStep::Cash] {
        let logs = TempDir::new().unwrap();
        let mut app = sgcc_tutorial_app(&logs);
        at_step(&mut app, step);
        dispatch_in(&mut app, vec![UiAction::ShowLinks(true)]);
        let pass = EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, MIN.0, MIN.1);
        assert!(
            crate::support::egui_harness::has(&widgets, "HVDC equipment"),
            "{step:?}"
        );
        press_escape(&pass, &mut app, MIN);
        assert!(played(&app).contains(&step), "{step:?}");
        assert!(ui_state(&app).links_open, "{step:?}: Links stays open");
    }
}

/// Review Focus 2: at the smallest window and the largest UI zoom, every spotlight's note and
/// every instruction is drawn, and inside the screen. Each instruction is an arrow callout beside
/// its anchor, not over it, with its arrow on screen and touching the anchor; picking a site's
/// sits inside the map's top, its arrow pointing down into the map. The connect instruction is
/// checked at both its stages: choosing where the line starts, and where it ends.
#[test]
fn cards_fit_minimum_window_at_max_zoom() {
    const MAX_ZOOM: f32 = 1.5;
    let (width, height) = (MIN.0 / MAX_ZOOM, MIN.1 / MAX_ZOOM);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    let l10n = crate::support::shipped_l10n();
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    let pass = with_anchor_probe(&mut app);
    let mut cell = battery_site(&app);
    let mut battery = None;
    let mut ccgt = None;
    let mut checked = Vec::new();
    for (chapter, phase_now) in [
        DayPhase::Planning,
        DayPhase::Operating,
        DayPhase::Review,
        DayPhase::Planning,
        DayPhase::Planning,
    ]
    .into_iter()
    .enumerate()
    {
        match phase_now {
            DayPhase::Planning if chapter == 3 => {
                execute(&mut app, Command::FinishReview);
                cell = ccgt_site(&app);
            }
            DayPhase::Planning if chapter == 4 => {
                let month = app
                    .world()
                    .resource::<Session>()
                    .0
                    .session()
                    .clock()
                    .month();
                app.world_mut()
                    .resource_mut::<TutorialRun>()
                    .0
                    .as_mut()
                    .unwrap()
                    .payoff_month = Some(month);
            }
            DayPhase::Planning => {}
            DayPhase::Operating => execute(&mut app, Command::BeginOperating),
            DayPhase::Review => execute(&mut app, Command::AdvanceTicks { n: TICKS_PER_DAY }),
        }
        assert_eq!(phase(&app), phase_now);
        for def in TUTORIAL_STEPS.iter().filter(|def| {
            def.phase == phase_now
                && (phase_now != DayPhase::Planning
                    || if def.step >= TutorialStep::OpenPayoff {
                        chapter == 4
                    } else {
                        (def.step >= TutorialStep::Supply) == (chapter == 3) && chapter != 4
                    })
                && matches!(def.kind, StepKind::Spotlight | StepKind::Instruction)
        }) {
            // From connecting on, the player has ordered a battery, which ordering selects.
            if def.step == TutorialStep::ConnectSpotlight {
                execute(
                    &mut app,
                    Command::BuildPlant {
                        kind: PlantKind::Battery,
                        cell,
                    },
                );
                battery = app
                    .world()
                    .resource::<Session>()
                    .0
                    .session()
                    .plant_views()
                    .into_iter()
                    .find(|plant| plant.kind == PlantKind::Battery && plant.cell == cell);
            }
            if def.step == TutorialStep::ConnectCcgtSpotlight {
                execute(
                    &mut app,
                    Command::BuildPlant {
                        kind: PlantKind::Ccgt,
                        cell,
                    },
                );
                ccgt = app
                    .world()
                    .resource::<Session>()
                    .0
                    .session()
                    .plant_views()
                    .into_iter()
                    .find(|plant| plant.kind == PlantKind::Ccgt && plant.cell == cell);
            }
            if def.step == TutorialStep::SaveSnapshot {
                let from = ccgt.as_ref().unwrap().node;
                let to = app.world().resource::<Session>().0.session().anchor_node();
                execute(&mut app, Command::BuildLine { from, to });
            }
            at_step(&mut app, def.step);
            dispatch_in(
                &mut app,
                vec![UiAction::ShowPayoff(matches!(
                    def.step,
                    TutorialStep::PayoffConstruction
                        | TutorialStep::PayoffOperating
                        | TutorialStep::ClosePayoff
                ))],
            );
            // Picking a site is done once a cell is selected; the later steps need one.
            let selection = match (def.step, &battery) {
                (TutorialStep::PickSite | TutorialStep::PickCcgtSite, _) => Selection::None,
                (TutorialStep::ConnectCcgtSpotlight | TutorialStep::ConnectCcgt, _) => {
                    Selection::Plant(ccgt.as_ref().expect("CCGT ordered").id)
                }
                (TutorialStep::ConnectSpotlight | TutorialStep::ConnectBattery, Some(battery)) => {
                    Selection::Plant(battery.id)
                }
                _ => Selection::Cell(cell),
            };
            dispatch_in(&mut app, vec![UiAction::Select(selection)]);
            let view = game::hud::tutorial::tutorial_view(
                def,
                run(&app).unwrap(),
                &app.world().resource::<Session>().0,
                selection,
                &l10n,
                1.0,
            );
            let text = match view {
                game::hud::tutorial::TutorialView::Spotlight { text, .. }
                | game::hud::tutorial::TutorialView::Instruction { text, .. } => text,
                other => panic!("expected a note: {other:?}"),
            };
            // Let a reveal scroll its anchor in.
            let mut found = None;
            for _ in 0..60 {
                let widgets = pass.run(&mut app, width, height);
                if let Some(note) = widgets.iter().find(|w| w.text == text) {
                    found = Some(note.rect);
                    break;
                }
            }
            let note = found.unwrap_or_else(|| {
                panic!(
                    "{:?} drew no note; anchors={:?}, ui={:?}",
                    def.step,
                    anchors(&app),
                    ui_state(&app)
                )
            });
            assert!(screen.contains_rect(note), "{:?}: {note:?}", def.step);
            if def.kind == StepKind::Instruction {
                check_arrow_callout(&pass, &mut app, (width, height), def, &text);
            }
            if def.step == TutorialStep::ConnectBattery {
                let node = battery.as_ref().expect("the battery was ordered").node;
                dispatch_in(&mut app, vec![UiAction::Select(Selection::LineFrom(node))]);
                check_arrow_callout(&pass, &mut app, (width, height), def, &text);
            }
            if def.step == TutorialStep::ConnectCcgt {
                let node = ccgt.as_ref().unwrap().node;
                dispatch_in(&mut app, vec![UiAction::Select(Selection::LineFrom(node))]);
                check_arrow_callout(&pass, &mut app, (width, height), def, &text);
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

/// An arrow callout's text beside its anchor, clear of it, and its arrow on screen with its tip
/// on the anchor's edge, its head pointing at it; or, for the map, the text inside the map's top
/// and the arrow pointing down into the map. Never an outlined card.
fn check_arrow_callout(
    pass: &EguiPass,
    app: &mut App,
    (width, height): (f32, f32),
    def: &game::hud::tutorial::StepDef,
    text: &str,
) {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    // Until a reveal has scrolled its anchor in, it is the text alone at the map's top.
    for _ in 0..60 {
        pass.run(app, width, height);
    }
    let widgets = pass.settled(app, width, height);
    let note = widget(&widgets, text).rect;
    let anchor = def.anchor.expect("an arrow points at an anchor");
    let target = *anchors(app)
        .get(&anchor)
        .unwrap_or_else(|| panic!("{:?}: {anchor:?} isn't in view", def.step));
    assert!(screen.contains_rect(note), "{:?}: {note:?}", def.step);
    let painted = painted(pass, app, (width, height));
    assert!(!outlined_card(&painted), "{:?}: an outlined card", def.step);
    let head = arrowhead(&painted)
        .unwrap_or_else(|| panic!("{:?} drew no arrow: {note:?} / {target:?}", def.step));
    assert!(head.iter().all(|p| screen.contains(*p)), "{head:?}");
    // Picking a site's callout sits inside the map's top, pointing down into it; on a map too
    // short for that, it points at the map from below, as other callouts do.
    if anchor == TutorialAnchor::Map && target.contains_rect(note) {
        let tip = head
            .iter()
            .copied()
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .unwrap();
        assert!(target.contains(tip), "{tip:?} in the map {target:?}");
        assert!(
            head.iter()
                .filter(|p| **p != tip)
                .all(|p| p.y <= tip.y - 7.5),
            "{head:?} points down"
        );
        return;
    }
    assert!(
        !note.intersects(target),
        "{:?}: {note:?} covers {target:?}",
        def.step
    );
    let beside = note.left() >= target.right() + 28.0
        || note.top() >= target.bottom() + 28.0
        || note.bottom() <= target.top() - 28.0;
    assert!(beside, "{:?}: {note:?} beside {target:?}", def.step);
    assert_points_at(&head, target, &format!("{:?}", def.step));
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
    tutorial_frames(&mut apps, 12, &mut shown);
    let cell = ccgt_site(&apps[0].0);
    for (app, _) in apps.iter_mut() {
        dispatch_in(app, vec![UiAction::Select(Selection::Cell(cell))]);
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for (app, _) in apps.iter_mut() {
        execute(
            app,
            Command::BuildPlant {
                kind: PlantKind::Ccgt,
                cell,
            },
        );
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for step in [
        TutorialStep::Supply,
        TutorialStep::CcgtSpotlight,
        TutorialStep::ConnectCcgtSpotlight,
    ] {
        assert!(shown.contains(&step), "{step:?} wasn't shown");
    }
    for (app, _) in apps.iter_mut() {
        let session = app.world().resource::<Session>().0.session();
        let from = session.anchor_node();
        let to = session
            .plant_views()
            .into_iter()
            .find(|plant| plant.kind == PlantKind::Ccgt && plant.cell == cell)
            .unwrap()
            .node;
        execute(app, Command::BuildLine { from, to });
    }
    tutorial_frames(&mut apps, 4, &mut shown);
    assert!(
        shown.contains(&TutorialStep::SaveSnapshot),
        "the save caution was shown"
    );
    assert!(
        shown.contains(&TutorialStep::CrystalBall),
        "the crystal-ball guidance was shown"
    );
    for (app, _) in apps.iter_mut() {
        app.world_mut()
            .resource_mut::<ClientProgressStore>()
            .value
            .outlook_tutorial_pending = false;
        dispatch_in(
            app,
            vec![UiAction::ShowOutlook(true), UiAction::ShowOutlook(false)],
        );
        update_n(app, 2);
        execute(app, Command::BeginOperating);
        execute(app, Command::AdvanceTicks { n: TICKS_PER_DAY });
        execute(app, Command::FinishReview);
        dispatch_in(app, vec![UiAction::ShowPayoff(true)]);
    }
    tutorial_frames(&mut apps, 12, &mut shown);
    for step in [
        TutorialStep::PayoffConstruction,
        TutorialStep::PayoffOperating,
    ] {
        assert!(shown.contains(&step), "{step:?} wasn't shown");
    }
    for (app, _) in apps.iter_mut() {
        dispatch_in(app, vec![UiAction::ShowPayoff(false)]);
        update_n(app, 2);
    }
    let [(on, _), (off, _)] = &apps;
    assert!(run(on).is_none() && !pending(on), "the tutorial finished");
    assert_eq!(phase(on), DayPhase::Planning);
    assert_ne!(state_hash(on), start, "the script changed the game");
    assert_eq!(state_hash(on), state_hash(off));
}

// --- FrameGrid moves the camera (spec §4.3 step 1) ---

/// A HUD app with the world view and a 1280×720 primary window, on the menus with the tutorial
/// owed, and its egui pass. `CameraPlugin` is left out: it spawns a second primary egui context,
/// and resolving a camera focus needs only `finish_surface`.
fn world_view_app(logs: &TempDir) -> (App, EguiPass) {
    let mut app = crate::support::headless_app(&crate::support::shipped_assets(), logs.path());
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.init_resource::<bevy_egui::EguiGlobalSettings>()
        .add_plugins((game::hud::HudPlugin, game::world::WorldViewPlugin));
    update_n(&mut app, 3);
    app.world_mut().spawn((
        Window {
            resolution: (1280, 720).into(),
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    let pass = EguiPass::attach(&mut app);
    set_progress(&mut app, true, true);
    (app, pass)
}

/// A real frame: `Update`, then the egui pass, as the game runs them.
fn frame(app: &mut App, pass: &EguiPass) {
    app.update();
    pass.run(app, 1280.0, 720.0);
}

/// The orbit, and where the anchor node lands on the map in normalised device coordinates.
fn anchor_on_map(app: &App) -> (game::world::OrbitCamera, Vec3) {
    let orbit = app.world().resource::<WorldControls>().orbit;
    let model = app
        .world()
        .resource::<game::world::WorldScene>()
        .model
        .clone()
        .expect("the scene is built");
    let rect = app
        .world()
        .resource::<game::world::MapSurface>()
        .rect
        .expect("the map is drawn");
    let anchor = app.world().resource::<Session>().0.session().anchor_node();
    let ndc = orbit
        .pose(model.size(), rect.aspect_ratio())
        .project(model.nodes[&anchor])
        .expect("the anchor is in front of the camera");
    (orbit, ndc)
}

/// Single-area regions have no island map at game start: FrameGrid must still move the camera
/// in to the starter grid, not only ask to, and the view must hold through the crawl and
/// ShowForecast.
#[test]
fn frame_grid_zooms_in_on_single_area_starter_grids() {
    for region in ["western_interconnection", "eastern_interconnection"] {
        let logs = TempDir::new().unwrap();
        let (mut app, pass) = world_view_app(&logs);
        for _ in 0..2 {
            frame(&mut app, &pass);
        }
        app.world_mut().write_message(game::sim::NewGameRequest {
            region: domain::ids::RegionId::new(region),
            seed: 7,
        });
        for _ in 0..6 {
            frame(&mut app, &pass);
        }
        assert!(
            app.world()
                .resource::<Session>()
                .0
                .session()
                .power()
                .island_nodes()
                .is_empty(),
            "{region} starts without an island map"
        );
        assert!(played(&app).contains(&TutorialStep::FrameGrid), "{region}");
        assert_eq!(focus(&app), None, "{region}: the focus was resolved");
        let (framed, ndc) = anchor_on_map(&app);
        assert!(
            framed.zoom < 1.0,
            "{region}: the camera moved in, {framed:?}"
        );
        assert!(
            ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
            "{region}: the anchor is on the map, {ndc:?}"
        );

        tutorial(&mut app, [TutorialAction::Dismiss(TutorialStep::Crawl)]);
        for _ in 0..4 {
            frame(&mut app, &pass);
        }
        assert!(
            played(&app).contains(&TutorialStep::ShowForecast),
            "{region}"
        );
        let (after, ndc) = anchor_on_map(&app);
        assert_eq!(after, framed, "{region}: the framing held");
        assert!(
            ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
            "{region}: {ndc:?}"
        );
    }
}

/// The orbit's zoom and whether the map overlay is on.
fn camera_keys_state(app: &App) -> (f32, bool) {
    let controls = app.world().resource::<WorldControls>();
    (controls.orbit.zoom, controls.overlay)
}

/// R and O pressed with the pointer over the map's centre, then released: two real frames.
fn press_map_keys(app: &mut App, pass: &EguiPass) {
    let centre = app
        .world()
        .resource::<game::world::MapSurface>()
        .rect
        .expect("the map is drawn")
        .center();
    app.update();
    pass.run_with(
        app,
        1280.0,
        720.0,
        vec![
            egui::Event::PointerMoved(centre),
            key(egui::Key::R, true),
            key(egui::Key::O, true),
        ],
    );
    app.update();
    pass.run_with(
        app,
        1280.0,
        720.0,
        vec![key(egui::Key::R, false), key(egui::Key::O, false)],
    );
}

/// The map's camera keys, R (reset the view) and O (the overlay), wait while the crawl or a
/// spotlight covers the screen, and work again once it has gone.
#[test]
fn map_keys_wait_under_the_crawl_or_a_spotlight() {
    for (step, note) in [
        (TutorialStep::Crawl, None),
        (TutorialStep::Cash, Some(CASH_NOTE)),
    ] {
        let logs = TempDir::new().unwrap();
        let (mut app, pass) = world_view_app(&logs);
        for _ in 0..2 {
            frame(&mut app, &pass);
        }
        app.world_mut().write_message(game::sim::NewGameRequest {
            region: domain::ids::RegionId::new("western_interconnection"),
            seed: 7,
        });
        for _ in 0..6 {
            frame(&mut app, &pass);
        }
        at_step(&mut app, step);
        // A view R would visibly reset.
        app.world_mut().resource_mut::<WorldControls>().orbit.zoom = 0.5;
        app.update();
        let widgets = pass.settled(&mut app, 1280.0, 720.0);
        if let Some(note) = note {
            assert!(
                crate::support::egui_harness::has(&widgets, note),
                "{step:?}"
            );
        }
        let before = camera_keys_state(&app);
        press_map_keys(&mut app, &pass);
        assert!(!played(&app).contains(&step), "{step:?} is still showing");
        assert_eq!(camera_keys_state(&app), before, "{step:?}");

        // With an instruction instead, which never blocks input, both keys act.
        at_step(&mut app, TutorialStep::PickSite);
        frame(&mut app, &pass);
        frame(&mut app, &pass);
        press_map_keys(&mut app, &pass);
        assert_eq!(camera_keys_state(&app), (1.0, !before.1), "{step:?}");
    }
}

/// Space waits while the Operating spotlight covers the speed controls, and works once it has
/// gone. (The crawl shows only in Planning, where Space does nothing.)
#[test]
fn space_waits_under_the_speed_spotlight() {
    use game::sim::SimSpeed;
    let speed = |app: &App| *app.world().resource::<SimSpeed>();
    let press_space = |app: &mut App| {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Space);
        keys.clear();
    };
    let logs = TempDir::new().unwrap();
    let mut app = tutorial_app(&logs);
    app.init_resource::<ButtonInput<KeyCode>>();
    at_step(&mut app, TutorialStep::Speed);
    execute(&mut app, Command::BeginOperating);
    assert_eq!(phase(&app), DayPhase::Operating);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, MIN.0, MIN.1);
    let note = crate::support::shipped_l10n().text("tutorial-speed");
    assert!(crate::support::egui_harness::has(&widgets, &note));
    let before = speed(&app);
    press_space(&mut app);
    pass.run(&mut app, MIN.0, MIN.1);
    assert_eq!(speed(&app), before, "Space under the spotlight");
    assert!(!played(&app).contains(&TutorialStep::Speed));

    tutorial(&mut app, [TutorialAction::Dismiss(TutorialStep::Speed)]);
    pass.settled(&mut app, MIN.0, MIN.1);
    press_space(&mut app);
    assert_ne!(speed(&app), before, "Space once the spotlight has gone");
}
