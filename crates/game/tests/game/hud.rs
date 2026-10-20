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
    MapLayer, MenuForm, OpenPrompt, Prompt, RegionListings, Selection, ToastQueue, toasts_model,
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
