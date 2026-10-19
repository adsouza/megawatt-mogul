#![forbid(unsafe_code)]

use bevy::prelude::*;
use game::GamePlugins;
use game::paths::{AssetRoot, CliArgs, CliError, LogDir, resolve_asset_root};

fn main() -> AppExit {
    let args = match CliArgs::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}\n{}", CliError::USAGE);
            return AppExit::from_code(2);
        }
    };

    let log_dir = LogDir(args.log_dir());
    let asset_root = AssetRoot(resolve_asset_root(args.assets));

    App::new()
        .insert_resource(asset_root)
        .insert_resource(log_dir)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Megawatt Mogul".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GamePlugins)
        .add_systems(Startup, spawn_ui_camera)
        .run()
}

/// egui draws through the first camera spawned. Plan 3's `CameraPlugin` replaces this with the
/// diorama camera.
fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
