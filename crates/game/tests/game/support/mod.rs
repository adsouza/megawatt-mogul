//! Shared helpers for the game integration tests.

pub mod egui_harness;
pub mod egui_pass;

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use domain::ids::RegionId;
use game::CorePlugins;
use game::hud::HudPlugin;
use game::l10n::{LOCALE, Localizer};
use game::paths::{AssetRoot, LogDir};
use game::sim::NewGameRequest;
use game::state::AppState;

/// A windowless app with the core (headless-safe) plugins, reading assets from `assets` and
/// writing logs under `logs`.
pub fn headless_app(assets: &Path, logs: &Path) -> App {
    let mut app = App::new();
    app.insert_resource(AssetRoot(assets.to_path_buf()))
        .insert_resource(LogDir(logs.to_path_buf()))
        .add_plugins((MinimalPlugins, StatesPlugin, CorePlugins));
    app
}

/// Runs `n` frames.
pub fn update_n(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// The workspace's shipped `assets/` directory. Tests only read from it.
pub fn shipped_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The shipped en-US translations.
pub fn shipped_l10n() -> Localizer {
    Localizer::load(&shipped_assets().join("locales").join(LOCALE))
        .unwrap_or_else(|error| panic!("the shipped translations must load: {error}"))
}

/// A headless app with the core plugins and the HUD plugin, past boot, in the main menu, writing
/// logs under `logs`. Fixed time stands still, so no ticks run while a test is in game.
pub fn hud_app(logs: &Path) -> App {
    let mut app = headless_app(&shipped_assets(), logs);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ))
    .add_plugins(HudPlugin);
    update_n(&mut app, 3);
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::MainMenu
    );
    app
}

/// Asks for a new game in `region`, seed 7, and runs the frames it takes to start.
pub fn request(app: &mut App, region: &str) {
    app.world_mut().write_message(NewGameRequest {
        region: RegionId::new(region),
        seed: 7,
    });
    update_n(app, 2);
}
