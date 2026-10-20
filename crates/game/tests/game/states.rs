//! App states, the phase sub-state and asset-root resolution.

use std::path::PathBuf;

use bevy::prelude::*;
use game::paths::resolve_asset_root;
use game::state::{AppState, BootError, PhaseState};
use tempfile::TempDir;

use crate::support::{headless_app, shipped_assets, update_n};

fn app_state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

fn phase_state(app: &App) -> Option<PhaseState> {
    app.world()
        .get_resource::<State<PhaseState>>()
        .map(|state| *state.get())
}

#[test]
fn boot_moves_to_the_main_menu() {
    let dir = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), dir.path());

    update_n(&mut app, 3);

    assert_eq!(app_state(&app), AppState::MainMenu);
}

#[test]
fn boot_waits_for_its_resources() {
    let dir = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), dir.path());
    app.world_mut().remove_resource::<game::paths::LogDir>();

    update_n(&mut app, 3);

    assert_eq!(app_state(&app), AppState::Boot);
}

#[test]
fn a_boot_error_keeps_the_app_in_boot() {
    let dir = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), dir.path());
    app.insert_resource(BootError("regions directory missing".to_owned()));

    update_n(&mut app, 3);

    assert_eq!(app_state(&app), AppState::Boot);
}

#[test]
fn the_phase_substate_exists_only_in_game() {
    let dir = TempDir::new().unwrap();
    let mut app = headless_app(&shipped_assets(), dir.path());

    app.update();
    assert_eq!(app_state(&app), AppState::Boot);
    assert_eq!(phase_state(&app), None);

    update_n(&mut app, 2);
    assert_eq!(app_state(&app), AppState::MainMenu);
    assert_eq!(phase_state(&app), None);

    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    assert_eq!(app_state(&app), AppState::InGame);
    assert_eq!(phase_state(&app), Some(PhaseState::Planning));

    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::MainMenu);
    app.update();
    assert_eq!(phase_state(&app), None);
}

#[test]
fn asset_root_prefers_the_explicit_argument() {
    let dir = TempDir::new().unwrap();

    let root = resolve_asset_root(Some(dir.path().to_path_buf()));

    assert_eq!(root, dir.path());
}

#[test]
fn asset_root_falls_back_to_the_workspace_assets() {
    // Tests run with the crate directory as the working directory, and the test binary lives in
    // target/, so neither `./assets` nor `<exe dir>/assets` exists: the workspace copy is used.
    let root: PathBuf = resolve_asset_root(None);

    assert!(
        root.join("regions").is_dir(),
        "{} has no regions/",
        root.display()
    );
}
