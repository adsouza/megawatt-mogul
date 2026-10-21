//! The HUD plugin's wiring (the panels themselves are tested as presenters).

use bevy::ecs::schedule::Schedules;
use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use game::hud::HudPlugin;

/// bevy_egui panics on the first frame if the pass schedule doesn't exist, which is the case
/// until some system is added to it. The HUD plugin creates it even while it has no panels.
#[test]
fn the_egui_pass_schedule_exists_without_any_panels() {
    let mut app = App::new();
    app.add_plugins(HudPlugin);

    assert!(
        app.world()
            .resource::<Schedules>()
            .contains(EguiPrimaryContextPass)
    );
}

// ---- the plugin's non-egui systems, in a headless app -------------------------------------
//
// No `EguiPlugin`: the egui pass never runs, so this covers exactly the glue around it (listing
// regions, turning refusals into toasts, resetting HUD state between games).

use application::{CommandError, RegionLoadError};
use domain::ids::{CellCoord, RegionId};
use game::hud::{
    ActionSink, CommitmentForm, LegendChoice, MapLayer, MenuForm, OpenPrompt, PlanningTab, Prompt,
    RegionListings, Selection, ToastQueue, UiAction, UiState, dispatch, toasts_model,
};
use game::l10n::{LocalizeWith, Localizer, Names};
use game::sim::ReturnToMenu;
use game::state::AppState;
use tempfile::TempDir;

use crate::support::egui_harness::has;
use crate::support::egui_pass::EguiPass;
use crate::support::{request, shipped_l10n, update_n};

fn hud_app(logs: &TempDir) -> App {
    crate::support::hud_app(logs.path())
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

/// The toasts on screen now, as the overlay would draw them.
fn toasts(app: &App) -> Vec<String> {
    let world = app.world();
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    toasts_model(
        world.resource::<ToastQueue>(),
        world.resource::<Localizer>(),
        now,
    )
    .toasts
}

#[test]
fn entering_the_main_menu_lists_the_shipped_regions() {
    let logs = TempDir::new().unwrap();
    let app = hud_app(&logs);

    let RegionListings(listings) = app.world().resource::<RegionListings>();
    let listings = listings.as_ref().expect("the shipped regions folder lists");
    let ids: Vec<&str> = listings.iter().map(|listing| listing.id.as_str()).collect();
    assert_eq!(ids, ["eastern_interconnection", "western_interconnection"]);
    assert!(listings.iter().all(|listing| listing.entry.is_ok()));
}

#[test]
fn a_refused_new_game_becomes_a_localised_toast() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);

    request(&mut app, "no_such_region");

    let l10n = shipped_l10n();
    let refusal = CommandError::Region(RegionLoadError::NotFound(RegionId::new("no_such_region")))
        .localize_with(&l10n, &Names::empty(&l10n));
    assert!(refusal.contains("no_such_region"), "{refusal}");
    assert_eq!(toasts(&app), vec![refusal]);
    assert_eq!(state(&app), AppState::MainMenu);
}

#[test]
fn leaving_a_game_resets_the_selection_the_prompt_and_the_seed() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    assert_eq!(state(&app), AppState::InGame);

    let world = app.world_mut();
    *world.resource_mut::<Selection>() = Selection::Cell(CellCoord { x: 3, y: 4 });
    *world.resource_mut::<MapLayer>() = MapLayer::Irradiance;
    *world.resource_mut::<LegendChoice>() = LegendChoice(Some(true));
    *world.resource_mut::<OpenPrompt>() = OpenPrompt(Some(Prompt::LeaveGame));
    world.resource_mut::<MenuForm>().seed = "the old seed".to_owned();
    world.write_message(ReturnToMenu);
    update_n(&mut app, 2);

    assert_eq!(state(&app), AppState::MainMenu);
    let world = app.world();
    assert_eq!(*world.resource::<Selection>(), Selection::None);
    assert_eq!(*world.resource::<MapLayer>(), MapLayer::Terrain);
    assert_eq!(*world.resource::<LegendChoice>(), LegendChoice(None));
    assert_eq!(*world.resource::<OpenPrompt>(), OpenPrompt(None));
    let form = world.resource::<MenuForm>();
    assert!(form.parsed_seed().is_some(), "re-seeded: {:?}", form.seed);
}

#[test]
fn entering_a_game_starts_with_nothing_selected_and_no_prompt() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    // Left over from the menu, which shouldn't happen but mustn't leak into the game.
    *app.world_mut().resource_mut::<Selection>() = Selection::Cell(CellCoord { x: 1, y: 1 });
    *app.world_mut().resource_mut::<MapLayer>() = MapLayer::HydroPotential;
    *app.world_mut().resource_mut::<LegendChoice>() = LegendChoice(Some(false));
    *app.world_mut().resource_mut::<OpenPrompt>() = OpenPrompt(Some(Prompt::LeaveGame));

    request(&mut app, "eastern_interconnection");

    assert_eq!(state(&app), AppState::InGame);
    assert_eq!(*app.world().resource::<Selection>(), Selection::None);
    assert_eq!(*app.world().resource::<MapLayer>(), MapLayer::Terrain);
    assert_eq!(*app.world().resource::<LegendChoice>(), LegendChoice(None));
    assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
}

/// Every egui system's parameters must be disjoint, and every system must run. Bevy checks the
/// parameters when the schedule is initialised, which for the egui pass is the first frame of a
/// real window, so without this a `Res<T>` beside an `ActionSink` (which holds `ResMut<T>`) would
/// panic the game on launch. The pass runs for real here, in the menu, Planning, Operating and
/// Review, with the Milestones panel open.
#[test]
fn the_egui_systems_have_no_conflicting_parameters() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    let pass = EguiPass::attach(&mut app);

    assert!(has(&pass.run(&mut app, 1024.0, 640.0), "New game"));
    request(&mut app, "eastern_interconnection");
    assert!(has(&pass.run(&mut app, 1024.0, 640.0), "Start the day"));
    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 3);
    assert!(has(&pass.run(&mut app, 1024.0, 640.0), "Frequency"));
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::AdvanceTicks { n: 1_440 }),
            UiAction::ToggleMilestones,
        ],
    );
    update_n(&mut app, 3);
    let widgets = pass.run(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, "Month in review") && has(&widgets, "Connect 10 cities"));
}

/// The real systems draw each phase's panels, by their run conditions, and none of the other
/// phases'.
#[test]
fn the_real_pass_draws_each_phases_panels_and_only_those() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    let pass = EguiPass::attach(&mut app);
    request(&mut app, "eastern_interconnection");
    // Planning: the commitment grid, the inspector and the build menu, around the map.
    let planning_panels = [
        "Start the day",
        "Commitments",
        "Select a cell to see what can be built there",
        "Select a cell on the map to build a plant there.",
    ];
    // Operating: the speed bar, the frequency gauge, the plant list and the Supply/Hydro tabs.
    let operating_panels = ["Pause", "8×", "Frequency", "Plants", "Supply", "Hydro"];
    let review_modal = "Month in review";

    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, "Map") && has(&widgets, "Menu"));
    for text in planning_panels {
        assert!(has(&widgets, text), "Planning shows {text}");
    }
    for text in operating_panels {
        assert!(!has(&widgets, text), "Planning does not show {text}");
    }

    assert!(!has(&widgets, review_modal), "Planning has no review");

    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Operating);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, "Map") && has(&widgets, "Menu"));
    for text in operating_panels {
        assert!(has(&widgets, text), "Operating shows {text}");
    }
    for text in planning_panels {
        assert!(!has(&widgets, text), "Operating does not show {text}");
    }
    assert!(!has(&widgets, review_modal), "Operating has no review");

    // Review: the modal over the map, with the top bar, and neither phase's panels.
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 1_440 })],
    );
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Review);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, "Map"), "the map draws behind the modal");
    assert!(has(&widgets, "Menu") && has(&widgets, "Milestones"));
    assert!(has(&widgets, review_modal) && has(&widgets, "Continue"));
    for text in planning_panels.iter().chain(&operating_panels) {
        assert!(!has(&widgets, text), "Review does not show {text}");
    }

    // The next month's Planning, with a closed month on record: no review.
    dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Planning);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(has(&widgets, "Start the day"));
    assert!(!has(&widgets, review_modal), "Planning has no review");
}

// ---- the Planning panel's state -------------------------------------------------------------

use application::Command;
use bevy::ecs::system::RunSystemOnce;
use domain::region::PlantKind;
use game::sim::Session;

/// The id of the first plant of `kind` in the running game.
fn plant_of(app: &App, kind: PlantKind) -> domain::ids::PlantId {
    app.world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|plant| plant.kind == kind)
        .unwrap_or_else(|| panic!("the fleet has a {kind:?} plant"))
        .id
}

fn commitment_of(app: &App, plant: domain::ids::PlantId) -> [bool; 24] {
    app.world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.id == plant)
        .unwrap()
        .commitment
}

/// Runs `actions` through the real `dispatch`, as a panel system would after drawing.
fn dispatch_in(app: &mut App, actions: Vec<UiAction>) {
    app.world_mut()
        .run_system_once(move |mut sink: ActionSink| dispatch(actions.clone(), &mut sink))
        .unwrap();
}

/// What the HUD does for an edit followed by a click on Apply, through `dispatch`: it records the
/// draft and the pending commit and sends the command.
fn send_commit(app: &mut App, plant: domain::ids::PlantId, hours: [bool; 24]) {
    dispatch_in(
        app,
        vec![
            UiAction::EditCommitment { plant, hours },
            UiAction::Command(Command::CommitPlant { plant, hours }),
        ],
    );
}

#[test]
fn leaving_a_game_resets_the_planning_panel_state() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    let plant = plant_of(&app, PlantKind::Coal);
    dispatch_in(
        &mut app,
        vec![
            UiAction::SetPlanningTab(PlanningTab::Tariff),
            UiAction::ShowHour(Some(7)),
            UiAction::EditCommitment {
                plant,
                hours: [true; 24],
            },
        ],
    );
    {
        let world = app.world_mut();
        world
            .resource_mut::<CommitmentForm>()
            .targets
            .insert(plant, "400".to_owned());
        world.write_message(ReturnToMenu);
    }
    update_n(&mut app, 2);

    assert_eq!(state(&app), AppState::MainMenu);
    assert_eq!(*app.world().resource::<UiState>(), UiState::default());
    assert_eq!(
        *app.world().resource::<CommitmentForm>(),
        CommitmentForm::default()
    );
}

/// `dispatch` is the one writer of the Planning panel's state: every Planning interaction reaches
/// `UiState` through it.
#[test]
fn dispatch_writes_the_planning_state() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    let coal = plant_of(&app, PlantKind::Coal);
    let mut hours = [false; 24];
    hours[6..18].fill(true);

    dispatch_in(
        &mut app,
        vec![UiAction::EditCommitment { plant: coal, hours }],
    );
    assert_eq!(
        app.world().resource::<UiState>().draft_of(coal),
        Some(hours)
    );

    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::CommitPlant {
            plant: coal,
            hours,
        })],
    );
    let ui = app.world().resource::<UiState>();
    assert_eq!(
        ui.pending.map(|pending| (pending.plant, pending.hours)),
        Some((coal, hours))
    );
    assert_eq!(
        ui.draft_of(coal),
        Some(hours),
        "the draft waits for the game's answer"
    );

    dispatch_in(
        &mut app,
        vec![
            UiAction::ShowHour(Some(18)),
            UiAction::SetPlanningTab(PlanningTab::Forecast),
        ],
    );
    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.highlight_hour, Some(18));
    assert_eq!(ui.tab, PlanningTab::Forecast);

    dispatch_in(&mut app, vec![UiAction::DiscardCommitmentDraft]);
    assert_eq!(app.world().resource::<UiState>().draft, None);
}

/// The Construction tab (spec §16.9) opens through the real `dispatch`, which hands
/// `SetPlanningTab` to `UiState::apply`, and a new game goes back to Commitments.
#[test]
fn dispatch_opens_the_construction_tab() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    assert_eq!(
        app.world().resource::<UiState>().tab,
        PlanningTab::Commitments
    );

    dispatch_in(
        &mut app,
        vec![UiAction::SetPlanningTab(PlanningTab::Construction)],
    );
    assert_eq!(
        app.world().resource::<UiState>().tab,
        PlanningTab::Construction
    );

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);
    request(&mut app, "eastern_interconnection");
    assert_eq!(
        app.world().resource::<UiState>().tab,
        PlanningTab::Commitments
    );
}

#[test]
fn a_new_game_starts_on_the_commitments_tab() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    dispatch_in(
        &mut app,
        vec![UiAction::SetPlanningTab(PlanningTab::Forecast)],
    );

    request(&mut app, "eastern_interconnection");

    assert_eq!(
        app.world().resource::<UiState>().tab,
        PlanningTab::Commitments
    );
}

/// A refused pattern: the game is untouched, the player is told why in one toast, and the row
/// is back to what the domain holds.
#[test]
fn an_infeasible_commit_reverts_the_row_and_shows_the_toast() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    let nuclear = plant_of(&app, PlantKind::Nuclear);
    let committed = commitment_of(&app, nuclear);
    // Nuclear takes 48 hours to start, so any off-gap is too short.
    let mut infeasible = [false; 24];
    infeasible[..12].fill(true);

    send_commit(&mut app, nuclear, infeasible);
    // Sent, not yet answered: the row shows the attempt and the commit is pending.
    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.draft_of(nuclear), Some(infeasible));
    assert_eq!(ui.pending.map(|pending| pending.plant), Some(nuclear));
    update_n(&mut app, 1);

    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.draft, None, "the row is back to the committed pattern");
    assert_eq!(ui.pending, None);
    assert_eq!(
        commitment_of(&app, nuclear),
        committed,
        "the game is unchanged"
    );
    let toasts = toasts(&app);
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert!(toasts[0].contains("too short"), "{}", toasts[0]);
    assert!(toasts[0].contains("to ramp down"), "{}", toasts[0]);
}

#[test]
fn a_feasible_commit_lands_and_clears_the_draft_without_a_toast() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    let peaker = plant_of(&app, PlantKind::Peaker);
    let mut evening = [false; 24];
    evening[16..22].fill(true);

    send_commit(&mut app, peaker, evening);
    assert_eq!(
        app.world().resource::<UiState>().draft_of(peaker),
        Some(evening)
    );
    update_n(&mut app, 1);

    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.draft, None);
    assert_eq!(ui.pending, None);
    assert_eq!(commitment_of(&app, peaker), evening);
    assert_eq!(toasts(&app), Vec::<String>::new());
}

/// A highlighted hour and an edit belong to the month being planned: once the day begins they go.
#[test]
fn beginning_the_day_resets_the_planning_panel_state() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    let coal = plant_of(&app, PlantKind::Coal);
    dispatch_in(
        &mut app,
        vec![
            UiAction::SetPlanningTab(PlanningTab::Forecast),
            UiAction::ShowHour(Some(9)),
            UiAction::EditCommitment {
                plant: coal,
                hours: [true; 24],
            },
        ],
    );
    app.world_mut()
        .resource_mut::<CommitmentForm>()
        .targets
        .insert(coal, "300".to_owned());
    assert_ne!(*app.world().resource::<UiState>(), UiState::default());

    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 3);

    let phase = *app
        .world()
        .resource::<State<game::state::PhaseState>>()
        .get();
    assert_eq!(phase, game::state::PhaseState::Operating);
    assert_eq!(*app.world().resource::<UiState>(), UiState::default());
    assert_eq!(
        *app.world().resource::<CommitmentForm>(),
        CommitmentForm::default()
    );
}

// ---- the shortfall confirmation --------------------------------------------------------------

use game::hud::{planning_model, prompt_model};
use game::sim::{CommandFailed, PlayerCommand};

use crate::hud_smoke::sent;
use crate::support::empty_start_in;

/// *Start the day*'s action, from the Planning panel's model.
fn start_the_day(app: &App) -> UiAction {
    let world = app.world();
    planning_model(
        &world.resource::<Session>().0,
        world.resource::<Localizer>(),
        world.resource::<UiState>(),
        world.resource::<CommitmentForm>(),
    )
    .expect("the game is planning")
    .begin
    .action
}

/// The open prompt's model, as the overlay would build it.
fn open_prompt_model(app: &App) -> game::hud::PromptModel {
    let world = app.world();
    prompt_model(
        *world.resource::<OpenPrompt>(),
        Some(&world.resource::<Session>().0),
        world.resource::<Localizer>(),
    )
    .expect("a prompt is open")
}

/// On a short forecast, *Start the day* opens the prompt and sends nothing; *Keep planning*
/// closes it; *Start anyway* sends one `BeginOperating`, and the prompt is closed by the frame
/// that begins the day, before any Operating panel draws.
#[test]
fn the_shortfall_prompt_opens_and_confirms_through_dispatch() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    empty_start_in(&mut app);
    let worst = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .capacity_shortfall_check()
        .expect("nothing is committed");
    let asks = Prompt::ShortfallStart {
        hour: worst.hour,
        shortfall: worst.shortfall,
    };
    let begin = start_the_day(&app);
    assert_eq!(begin, UiAction::Prompt(Some(asks)));
    // With fixed time standing still the message buffers don't turn over: count from here.
    let before = sent::<PlayerCommand>(&app).len();

    dispatch_in(&mut app, vec![begin.clone()]);
    assert_eq!(
        *app.world().resource::<OpenPrompt>(),
        OpenPrompt(Some(asks))
    );
    assert_eq!(sent::<PlayerCommand>(&app)[before..], []);

    // Keep planning, and a click outside, close it and send nothing.
    let prompt = open_prompt_model(&app);
    for close in [prompt.cancel.action.clone(), prompt.dismiss.clone()] {
        dispatch_in(&mut app, vec![begin.clone()]);
        dispatch_in(&mut app, vec![close]);
        assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
    }
    assert_eq!(sent::<PlayerCommand>(&app)[before..], []);
    update_n(&mut app, 2);
    assert_eq!(phase(&app), PhaseState::Planning);

    // Start anyway.
    dispatch_in(&mut app, vec![begin]);
    let start = open_prompt_model(&app).confirm.action;
    dispatch_in(&mut app, vec![start]);
    assert_eq!(
        sent::<PlayerCommand>(&app)[before..],
        [PlayerCommand(
            Command::BeginOperating,
            app.world().resource::<game::sim::SessionGeneration>().0
        )]
    );
    // The frame that applies it begins the day and closes the prompt.
    app.update();
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        domain::session::DayPhase::Operating
    );
    assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
    update_n(&mut app, 2);
    assert_eq!(phase(&app), PhaseState::Operating);
    assert_eq!(sent::<CommandFailed>(&app), []);
}

/// The leave prompt stays open across a phase change: only the shortfall prompt is Planning's.
#[test]
fn beginning_the_day_leaves_the_leave_prompt_open() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");

    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Prompt(Some(Prompt::LeaveGame)),
        ],
    );
    update_n(&mut app, 3);

    assert_eq!(phase(&app), PhaseState::Operating);
    assert_eq!(
        *app.world().resource::<OpenPrompt>(),
        OpenPrompt(Some(Prompt::LeaveGame))
    );
}

// ---- the Operating screen's state ----------------------------------------------------------

use bevy::input::ButtonInput;
use game::hud::{LastSpeed, OperatingTab, PlantPanelForm};
use game::sim::SimSpeed;
use game::state::PhaseState;

/// A game in Operating, with the coal fleet committed all day and no tick run (fixed time stands
/// still), and a keyboard to press.
fn operating_app(logs: &TempDir) -> App {
    let mut app = hud_app(logs);
    request(&mut app, "eastern_interconnection");
    let coal: Vec<_> = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .filter(|plant| plant.kind == PlantKind::Coal)
        .map(|plant| plant.id)
        .collect();
    let mut actions: Vec<UiAction> = coal
        .into_iter()
        .map(|plant| UiAction::Command(Command::StartPlant { plant }))
        .collect();
    actions.push(UiAction::Command(Command::BeginOperating));
    dispatch_in(&mut app, actions);
    app.init_resource::<ButtonInput<KeyCode>>();
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Operating);
    assert_eq!(speed(&app), SimSpeed::X1, "the day starts at 1×");
    app
}

fn phase(app: &App) -> PhaseState {
    *app.world().resource::<State<PhaseState>>().get()
}

fn speed(app: &App) -> SimSpeed {
    *app.world().resource::<SimSpeed>()
}

/// Presses and releases Space over one frame, as `InputPlugin` would report it.
fn press_space(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(KeyCode::Space);
    keys.clear();
}

#[test]
fn space_toggles_pause() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);

    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::Paused);
    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::X1, "1× when no speed was picked");

    // After a speed button, Space resumes that speed.
    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::X4)]);
    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::Paused);
    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::X4);

    // Holding Space down is one press.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert_eq!(speed(&app), SimSpeed::Paused, "toggled once, not twice");
}

#[test]
fn space_is_ignored_while_a_prompt_is_open() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);
    *app.world_mut().resource_mut::<OpenPrompt>() = OpenPrompt(Some(Prompt::LeaveGame));

    press_space(&mut app);

    assert_eq!(speed(&app), SimSpeed::X1);
    // And once it closes, Space works again.
    *app.world_mut().resource_mut::<OpenPrompt>() = OpenPrompt(None);
    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::Paused);
}

#[test]
fn dispatch_remembers_the_last_running_speed_for_space() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);
    assert_eq!(
        *app.world().resource::<LastSpeed>(),
        LastSpeed(SimSpeed::X1)
    );

    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::X8)]);
    assert_eq!(speed(&app), SimSpeed::X8);
    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::Paused)]);

    assert_eq!(speed(&app), SimSpeed::Paused);
    assert_eq!(
        *app.world().resource::<LastSpeed>(),
        LastSpeed(SimSpeed::X8),
        "pausing is not a speed to resume"
    );
}

/// The Operating panel's clicks reach the game through `dispatch`: selecting a plant, its AGC,
/// stop and start, and a target.
#[test]
fn operating_clicks_reach_the_game_through_dispatch() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);
    let coal = plant_of(&app, PlantKind::Coal);
    let view = |app: &App| {
        app.world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .into_iter()
            .find(|plant| plant.id == coal)
            .unwrap()
    };
    let spec = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .region()
        .spec(PlantKind::Coal)
        .clone();
    let target = spec.min_output() + domain::units::Megawatts::new(10.0);

    dispatch_in(
        &mut app,
        vec![
            UiAction::Select(Selection::Plant(coal)),
            UiAction::Command(Command::SetDispatchTarget {
                plant: coal,
                mw: target,
            }),
        ],
    );
    assert_eq!(*app.world().resource::<Selection>(), Selection::Plant(coal));
    app.update();
    assert!(matches!(
        view(&app).status,
        domain::generation::PlantStatus::Online { target: t, .. } if t == target
    ));

    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::SetAutoFollow {
            plant: coal,
            on: true,
        })],
    );
    app.update();
    assert!(view(&app).auto_follow);

    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::StopPlant { plant: coal })],
    );
    app.update();
    assert!(matches!(
        view(&app).status,
        domain::generation::PlantStatus::Stopping { .. }
    ));
}

#[test]
fn dispatch_opens_the_operating_tab_and_a_new_game_starts_on_supply() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);

    dispatch_in(
        &mut app,
        vec![UiAction::SetOperatingTab(OperatingTab::Hydro)],
    );
    assert_eq!(*app.world().resource::<OperatingTab>(), OperatingTab::Hydro);

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);
    request(&mut app, "eastern_interconnection");
    assert_eq!(
        *app.world().resource::<OperatingTab>(),
        OperatingTab::Supply
    );
}

#[test]
fn leaving_a_game_resets_the_operating_state() {
    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);
    let coal = plant_of(&app, PlantKind::Coal);
    dispatch_in(
        &mut app,
        vec![
            UiAction::SetSpeed(SimSpeed::X8),
            UiAction::SetOperatingTab(OperatingTab::Hydro),
        ],
    );
    *app.world_mut().resource_mut::<PlantPanelForm>() = PlantPanelForm {
        plant: Some(coal),
        value: 500.0,
        held: true,
    };

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);

    assert_eq!(state(&app), AppState::MainMenu);
    let world = app.world();
    assert_eq!(*world.resource::<LastSpeed>(), LastSpeed::default());
    assert_eq!(*world.resource::<OperatingTab>(), OperatingTab::default());
    assert_eq!(
        *world.resource::<PlantPanelForm>(),
        PlantPanelForm::default()
    );
    assert_eq!(*world.resource::<SimSpeed>(), SimSpeed::Paused);
}

/// Space is the player's only while egui isn't taking the keyboard: with a widget focused it would
/// also press that widget.
#[test]
fn space_is_ignored_while_egui_has_the_keyboard() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy_egui::egui;
    use bevy_egui::input::{EguiWantsInput, write_egui_wants_input_system};

    let logs = TempDir::new().unwrap();
    let mut app = operating_app(&logs);
    let pass = EguiPass::attach(&mut app);
    app.init_resource::<EguiWantsInput>();
    pass.run(&mut app, 1024.0, 640.0);
    let field = egui::Id::new("a focused text field");
    pass.ctx().memory_mut(|memory| memory.request_focus(field));
    app.world_mut()
        .run_system_once(write_egui_wants_input_system)
        .unwrap();
    assert!(
        app.world()
            .resource::<EguiWantsInput>()
            .wants_keyboard_input()
    );

    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::X1, "the keystroke was egui's");

    // Once egui lets go of the keyboard, Space is the player's again.
    pass.ctx()
        .memory_mut(|memory| memory.surrender_focus(field));
    app.world_mut()
        .run_system_once(write_egui_wants_input_system)
        .unwrap();
    press_space(&mut app);
    assert_eq!(speed(&app), SimSpeed::Paused);
}

// ---- the Milestones panel ------------------------------------------------------------------

use game::hud::MilestonesOpen;

#[test]
fn the_milestones_panel_starts_closed_in_each_game() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    // Left open from somewhere, which mustn't leak into the game.
    *app.world_mut().resource_mut::<MilestonesOpen>() = MilestonesOpen(true);
    request(&mut app, "eastern_interconnection");
    assert_eq!(
        *app.world().resource::<MilestonesOpen>(),
        MilestonesOpen(false)
    );

    dispatch_in(&mut app, vec![UiAction::ToggleMilestones]);
    assert_eq!(
        *app.world().resource::<MilestonesOpen>(),
        MilestonesOpen(true)
    );
    // Ending Planning resets the Planning panel's state, not this.
    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 3);
    assert_eq!(phase(&app), PhaseState::Operating);
    assert_eq!(
        *app.world().resource::<MilestonesOpen>(),
        MilestonesOpen(true)
    );

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);
    assert_eq!(state(&app), AppState::MainMenu);
    assert_eq!(
        *app.world().resource::<MilestonesOpen>(),
        MilestonesOpen(false)
    );
}

/// A toast belongs to its game: one game's refusal is not on screen in the menu after it, nor in
/// the next game, though its real-time seconds are not up.
#[test]
fn toasts_from_one_game_do_not_follow_into_the_next() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    // Refused in Planning: there is no review to finish.
    dispatch_in(
        &mut app,
        vec![UiAction::Command(application::Command::FinishReview)],
    );
    update_n(&mut app, 1);
    let refusal = toasts(&app);
    assert_eq!(refusal.len(), 1, "{refusal:?}");

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);
    assert_eq!(state(&app), AppState::MainMenu);
    assert_eq!(toasts(&app), Vec::<String>::new(), "gone in the menu");

    // A refusal in the menu, then a game: the menu's toast stays behind too.
    request(&mut app, "no_such_region");
    assert_eq!(toasts(&app).len(), 1);
    request(&mut app, "western_interconnection");
    assert_eq!(state(&app), AppState::InGame);
    assert_eq!(
        toasts(&app),
        Vec::<String>::new(),
        "the new game starts clean"
    );
}

// ---- the game-over view ---------------------------------------------------------------------

use game::hud::GameOverView;

fn game_over_view(app: &App) -> GameOverView {
    *app.world().resource::<GameOverView>()
}

#[test]
fn show_final_month_and_show_game_over_summary_write_the_view_through_dispatch() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    assert_eq!(game_over_view(&app), GameOverView::Summary);

    dispatch_in(&mut app, vec![UiAction::ShowFinalMonth]);
    assert_eq!(game_over_view(&app), GameOverView::FinalMonth);

    dispatch_in(&mut app, vec![UiAction::ShowGameOverSummary]);
    assert_eq!(game_over_view(&app), GameOverView::Summary);

    // In order, in one batch: the last one wins.
    dispatch_in(
        &mut app,
        vec![UiAction::ShowFinalMonth, UiAction::ShowGameOverSummary],
    );
    assert_eq!(game_over_view(&app), GameOverView::Summary);
    dispatch_in(
        &mut app,
        vec![UiAction::ShowGameOverSummary, UiAction::ShowFinalMonth],
    );
    assert_eq!(game_over_view(&app), GameOverView::FinalMonth);
}

/// The view belongs to its game: one that ended on its final month's review does not leave the
/// next game, or the menu after it, on that screen.
#[test]
fn each_game_starts_on_the_game_over_summary() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    // Left over from somewhere, which mustn't leak into the game.
    *app.world_mut().resource_mut::<GameOverView>() = GameOverView::FinalMonth;
    request(&mut app, "eastern_interconnection");
    assert_eq!(game_over_view(&app), GameOverView::Summary);

    dispatch_in(&mut app, vec![UiAction::ShowFinalMonth]);
    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);
    assert_eq!(state(&app), AppState::MainMenu);
    assert_eq!(game_over_view(&app), GameOverView::Summary);
}
