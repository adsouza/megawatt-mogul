//! Shared helpers for the game integration tests.

use std::path::Path;

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use game::CorePlugins;
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
