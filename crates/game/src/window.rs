//! The window's opening size (spec §16.7): about 80% of the primary monitor, centred on it.
//!
//! The size can't be set when the window is created: Bevy 0.20 spawns the `Monitor` entities only
//! once the winit event loop runs. [`WindowSizingPlugin`] waits for the primary monitor, applies
//! the size once and then stops for good, so a resize by the player always wins.

use bevy::prelude::*;
use bevy::window::{Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, WindowPosition};

/// The smallest window the HUD is laid out for. Smaller than this, the map is too small to read
/// and the side panels crowd it.
pub const MIN_WINDOW_SIZE: Vec2 = Vec2::new(1024.0, 640.0);
/// The share of the monitor's width and height the window opens at.
pub const OPENING_SHARE: f32 = 0.8;

/// The window's opening logical size on a monitor of logical size `monitor`: [`OPENING_SHARE`] of
/// it, at least [`MIN_WINDOW_SIZE`] and at most the monitor. `None` when no monitor is known (or
/// it reports no size), so the window keeps its default size.
pub fn opening_size(monitor: Option<Vec2>) -> Option<Vec2> {
    let monitor = monitor.filter(|size| size.is_finite() && size.cmpgt(Vec2::ZERO).all())?;
    Some(
        (monitor * OPENING_SHARE)
            .max(MIN_WINDOW_SIZE)
            .min(monitor)
            .round(),
    )
}

/// Opens the primary window at its [`opening_size`], centred, once the primary monitor is known.
pub struct WindowSizingPlugin;

impl Plugin for WindowSizingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            apply_opening_size
                .after(crate::preferences::PreferenceSet::Load)
                .run_if(not(resource_exists::<OpeningSizeApplied>)),
        );
    }
}

/// Present once the opening size has been applied (or found unknowable): the sizing system never
/// runs again, so it can't undo a resize by the player.
#[derive(Resource, Debug)]
pub(crate) struct OpeningSizeApplied;

/// Waits for the primary monitor, then sizes and centres the primary window on it, once.
fn apply_opening_size(
    mut commands: Commands,
    monitors: Query<(&Monitor, Option<&PrimaryMonitor>)>,
    settings: Option<Res<crate::preferences::ClientSettings>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if settings.as_ref().is_some_and(|s| !s.loaded) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let Some((primary, _)) = monitors.iter().find(|(_, p)| p.is_some()) else {
        return;
    };
    if let Some(saved) = settings.as_ref().and_then(|s| s.value.window.as_ref()) {
        let monitor = monitors
            .iter()
            .map(|(m, _)| m)
            .find(|m| {
                let at = IVec2::new(saved.x, saved.y).as_i64vec2();
                let min = m.physical_position.as_i64vec2();
                at.cmpge(min).all() && at.cmplt(min + m.physical_size().as_i64vec2()).all()
            })
            .unwrap_or(primary);
        let restored = crate::preferences::fit_window(
            saved,
            monitor.physical_position,
            monitor.physical_size(),
            monitor.scale_factor,
        );
        window.resolution.set(restored.width, restored.height);
        window.position = WindowPosition::At(IVec2::new(restored.x, restored.y));
        commands.insert_resource(OpeningSizeApplied);
        return;
    }
    let monitor = primary;
    let logical = monitor.physical_size().as_vec2() / monitor.scale_factor as f32;
    if let Some(size) = opening_size(Some(logical)) {
        window.resolution.set(size.x, size.y);
        // Centred on the new size: winit places the window before it resizes it.
        window.position = WindowPosition::Centered(MonitorSelection::Primary);
    }
    commands.insert_resource(OpeningSizeApplied);
}

#[cfg(test)]
mod tests {
    use bevy::window::WindowResolution;

    use super::*;

    #[test]
    fn opening_size_is_80_percent_of_the_monitor_clamped() {
        // A large monitor: 80% of it.
        assert_eq!(
            opening_size(Some(Vec2::new(2560.0, 1440.0))),
            Some(Vec2::new(2048.0, 1152.0))
        );
        // 80% of 1280×800 is 1024×640, the minimum window.
        assert_eq!(
            opening_size(Some(Vec2::new(1280.0, 800.0))),
            Some(MIN_WINDOW_SIZE)
        );
        // 80% of the minimum is below it: clamped up to the minimum, which is the monitor.
        assert_eq!(
            opening_size(Some(Vec2::new(1024.0, 640.0))),
            Some(Vec2::new(1024.0, 640.0))
        );
        // A monitor smaller than the minimum: clamped up, then down to the monitor.
        assert_eq!(
            opening_size(Some(Vec2::new(800.0, 600.0))),
            Some(Vec2::new(800.0, 600.0))
        );
        // Clamped in each direction on its own.
        assert_eq!(
            opening_size(Some(Vec2::new(3440.0, 720.0))),
            Some(Vec2::new(2752.0, 640.0))
        );
        // No monitor, or one that reports no size: the default size stays.
        assert_eq!(opening_size(None), None);
        assert_eq!(opening_size(Some(Vec2::ZERO)), None);
        assert_eq!(opening_size(Some(Vec2::new(f32::NAN, 1440.0))), None);
    }

    fn app_with_window() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WindowSizingPlugin));
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: WindowResolution::new(1280, 720),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        (app, window)
    }

    fn window(app: &App, entity: Entity) -> &Window {
        app.world().get::<Window>(entity).unwrap()
    }

    fn spawn_primary_monitor(app: &mut App, physical: UVec2, scale_factor: f64) -> Entity {
        app.world_mut()
            .spawn((
                Monitor {
                    name: None,
                    physical_height: physical.y,
                    physical_width: physical.x,
                    physical_position: IVec2::ZERO,
                    refresh_rate_millihertz: None,
                    scale_factor,
                    video_modes: Vec::new(),
                },
                PrimaryMonitor,
            ))
            .id()
    }

    #[test]
    fn with_no_monitor_the_window_keeps_its_default_size() {
        // The headless case: there is no event loop, so no monitor ever appears.
        let (mut app, entity) = app_with_window();
        for _ in 0..3 {
            app.update();
        }
        let window = window(&app, entity);
        assert_eq!(window.resolution.size(), Vec2::new(1280.0, 720.0));
        assert_eq!(window.position, WindowPosition::Automatic);
    }

    #[test]
    fn the_opening_size_is_applied_once_the_primary_monitor_appears_and_then_never_again() {
        let (mut app, entity) = app_with_window();
        app.update();
        assert_eq!(
            window(&app, entity).resolution.size(),
            Vec2::new(1280.0, 720.0)
        );

        // A 5K monitor at scale 2 is 2560×1440 logical.
        let monitor = spawn_primary_monitor(&mut app, UVec2::new(5120, 2880), 2.0);
        app.update();
        let opened = window(&app, entity);
        assert_eq!(opened.resolution.size(), Vec2::new(2048.0, 1152.0));
        assert_eq!(
            opened.position,
            WindowPosition::Centered(MonitorSelection::Primary)
        );

        // The player resizes the window: their size stays, frame after frame.
        app.world_mut()
            .get_mut::<Window>(entity)
            .unwrap()
            .resolution
            .set(1500.0, 900.0);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            window(&app, entity).resolution.size(),
            Vec2::new(1500.0, 900.0)
        );

        // Even another primary monitor later doesn't size it again.
        app.world_mut().despawn(monitor);
        spawn_primary_monitor(&mut app, UVec2::new(3840, 2160), 1.0);
        app.update();
        assert_eq!(
            window(&app, entity).resolution.size(),
            Vec2::new(1500.0, 900.0)
        );
    }

    #[test]
    fn a_monitor_that_is_not_the_primary_one_is_ignored() {
        let (mut app, entity) = app_with_window();
        app.world_mut().spawn(Monitor {
            name: None,
            physical_height: 1440,
            physical_width: 2560,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: None,
            scale_factor: 1.0,
            video_modes: Vec::new(),
        });
        app.update();
        assert_eq!(
            window(&app, entity).resolution.size(),
            Vec2::new(1280.0, 720.0)
        );
    }
}
