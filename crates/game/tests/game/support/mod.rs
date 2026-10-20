//! Shared helpers for the game integration tests.

pub mod egui_harness;

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use game::CorePlugins;
use game::l10n::{LOCALE, Localizer};
use game::paths::{AssetRoot, LogDir};

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
