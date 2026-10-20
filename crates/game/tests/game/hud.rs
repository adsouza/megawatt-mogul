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
use bevy::time::TimeUpdateStrategy;
use domain::ids::{CellCoord, RegionId};
use game::hud::{
    CommitmentForm, MapLayer, MenuForm, OpenPrompt, PlanningTab, Prompt, RegionListings, Selection,
    ToastQueue, UiAction, UiState, toasts_model,
};
use game::l10n::{LocalizeWith, Localizer, Names};
use game::sim::{NewGameRequest, ReturnToMenu};
use game::state::AppState;
use tempfile::TempDir;

use crate::support::{headless_app, shipped_assets, shipped_l10n, update_n};

/// A headless app with the core plugins and the HUD plugin, past boot, in the main menu. Fixed
/// time stands still, so no ticks run while a test is in game.
fn hud_app(logs: &TempDir) -> App {
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ))
    .add_plugins(HudPlugin);
    update_n(&mut app, 3);
    assert_eq!(state(&app), AppState::MainMenu);
    app
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

fn request(app: &mut App, region: &str) {
    app.world_mut().write_message(NewGameRequest {
        region: RegionId::new(region),
        seed: 7,
    });
    update_n(app, 2);
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
    *world.resource_mut::<OpenPrompt>() = OpenPrompt(Some(Prompt::LeaveGame));
    world.resource_mut::<MenuForm>().seed = "the old seed".to_owned();
    world.write_message(ReturnToMenu);
    update_n(&mut app, 2);

    assert_eq!(state(&app), AppState::MainMenu);
    let world = app.world();
    assert_eq!(*world.resource::<Selection>(), Selection::None);
    assert_eq!(*world.resource::<MapLayer>(), MapLayer::Terrain);
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
    *app.world_mut().resource_mut::<OpenPrompt>() = OpenPrompt(Some(Prompt::LeaveGame));

    request(&mut app, "eastern_interconnection");

    assert_eq!(state(&app), AppState::InGame);
    assert_eq!(*app.world().resource::<Selection>(), Selection::None);
    assert_eq!(*app.world().resource::<MapLayer>(), MapLayer::Terrain);
    assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
}

/// Every egui system's parameters must be disjoint. Bevy checks that when the schedule is
/// initialised, which for the egui pass is the first frame of a real window, so without this a
/// `Res<T>` beside an `ActionSink` (which holds `ResMut<T>`) would panic the game on launch.
#[test]
fn the_egui_systems_have_no_conflicting_parameters() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);

    app.world_mut()
        .schedule_scope(EguiPrimaryContextPass, |world, schedule| {
            schedule.initialize(world).expect(
                "the egui pass initialises: its systems build, and their parameters are disjoint",
            );
        });
}

// ---- the Planning panel's state -------------------------------------------------------------

use application::Command;
use domain::region::PlantKind;
use game::sim::{PlayerCommand, Session};

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

/// What the HUD does for a click on Apply: it records the draft and the pending commit
/// (`dispatch` does this through `UiState::apply`) and sends the command.
fn send_commit(app: &mut App, plant: domain::ids::PlantId, hours: [bool; 24]) {
    let world = app.world_mut();
    let commit = Command::CommitPlant { plant, hours };
    let mut ui = world.resource_mut::<UiState>();
    ui.apply(&UiAction::EditCommitment { plant, hours });
    ui.apply(&UiAction::Command(commit.clone()));
    world.write_message(PlayerCommand(commit));
}

#[test]
fn leaving_a_game_resets_the_planning_panel_state() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    request(&mut app, "eastern_interconnection");
    let plant = plant_of(&app, PlantKind::Coal);
    {
        let world = app.world_mut();
        let mut ui = world.resource_mut::<UiState>();
        ui.apply(&UiAction::SetPlanningTab(PlanningTab::Tariff));
        ui.apply(&UiAction::ShowHour(Some(7)));
        ui.apply(&UiAction::EditCommitment {
            plant,
            hours: [true; 24],
        });
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

#[test]
fn a_new_game_starts_on_the_commitments_tab() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(&logs);
    app.world_mut()
        .resource_mut::<UiState>()
        .apply(&UiAction::SetPlanningTab(PlanningTab::Forecast));

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
    // Nuclear takes 48 hours to start, so any off-gap is too short.
    let mut infeasible = [false; 24];
    infeasible[..12].fill(true);

    send_commit(&mut app, nuclear, infeasible);
    update_n(&mut app, 1);

    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.draft, None, "the row is back to the committed pattern");
    assert_eq!(ui.pending, None);
    assert_eq!(
        commitment_of(&app, nuclear),
        [false; 24],
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
    update_n(&mut app, 1);

    let ui = app.world().resource::<UiState>();
    assert_eq!(ui.draft, None);
    assert_eq!(ui.pending, None);
    assert_eq!(commitment_of(&app, peaker), evening);
    assert_eq!(toasts(&app), Vec::<String>::new());
}
