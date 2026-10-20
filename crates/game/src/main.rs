#![forbid(unsafe_code)]

use bevy::prelude::*;
use game::GamePlugins;
use game::paths::{AssetRoot, CliArgs, CliError, ExitAfterFrames, LogDir, resolve_asset_root};

/// The smallest window the HUD is laid out for.
const MIN_WINDOW_WIDTH: f32 = 1024.0;
const MIN_WINDOW_HEIGHT: f32 = 640.0;

fn main() -> AppExit {
    let args = match CliArgs::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}\n{}", CliError::USAGE);
            return AppExit::from_code(2);
        }
    };

    let log_dir = LogDir(args.log_dir());
    let exit_after_frames = args.exit_after_frames;
    let asset_root = AssetRoot(resolve_asset_root(args.assets));

    let mut app = App::new();
    app.insert_resource(asset_root)
        .insert_resource(log_dir)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Megawatt Mogul".into(),
                // Smaller than this, the map is too small to read and the side panels crowd it.
                resize_constraints: WindowResizeConstraints {
                    min_width: MIN_WINDOW_WIDTH,
                    min_height: MIN_WINDOW_HEIGHT,
                    ..default()
                },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GamePlugins)
        .add_systems(Startup, spawn_ui_camera);
    if let Some(frames) = exit_after_frames {
        app.add_plugins(ExitAfterFrames(frames));
    }
    app.run()
}

/// egui draws through the first camera spawned. Plan 3's `CameraPlugin` replaces this with the
/// diorama camera.
fn spawn_ui_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
