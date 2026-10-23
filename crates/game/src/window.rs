//! The window's opening size (spec §16.7): about 80% of the primary monitor, centred on it.
//!
//! The size can't be set when the window is created: Bevy 0.20 spawns the `Monitor` entities only
//! once the winit event loop runs. [`WindowSizingPlugin`] waits for the primary monitor, applies
//! the size once and then stops for good, so a resize by the player always wins.

use bevy::prelude::*;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow, WindowPosition};

/// Minimum supported logical display size. Client space must also leave room for desktop chrome.
pub const MIN_SCREEN_SIZE: Vec2 = Vec2::new(1280.0, 720.0);
/// Allow 80 logical pixels vertically for the title bar and system bars, with no side margins.
/// Bevy's Monitor
/// exposes the full display, not the OS work area; this is a layout budget, not a native query.
const DESKTOP_CHROME_SIZE: Vec2 = Vec2::new(0.0, 80.0);
const DESKTOP_TOP_LEFT: Vec2 = Vec2::new(0.0, 40.0);
/// Smallest content area: 1280×640 on the minimum supported display.
pub const MIN_WINDOW_SIZE: Vec2 = Vec2::new(
    MIN_SCREEN_SIZE.x - DESKTOP_CHROME_SIZE.x,
    MIN_SCREEN_SIZE.y - DESKTOP_CHROME_SIZE.y,
);

/// Logical inset and available client size within a full monitor. Keep a reachable fallback on
/// unsupported smaller displays too.
pub(crate) fn screen_work_area(monitor: Vec2) -> (Vec2, Vec2) {
    let size = (monitor - DESKTOP_CHROME_SIZE).max(Vec2::ONE).min(monitor);
    let inset = DESKTOP_TOP_LEFT.min((monitor - size).max(Vec2::ZERO));
    (inset, size)
}
/// The share of the monitor's width and height the window opens at.
pub const OPENING_SHARE: f32 = 0.8;

/// The window's opening logical size on a monitor of logical size `monitor`: [`OPENING_SHARE`] of
/// it, at least [`MIN_WINDOW_SIZE`] and at most the client area after desktop allowances. `None`
/// when no monitor is known (or it reports no size), so the window keeps its default size.
pub fn opening_size(monitor: Option<Vec2>) -> Option<Vec2> {
    let monitor = monitor.filter(|size| size.is_finite() && size.cmpgt(Vec2::ZERO).all())?;
    let (_, available) = screen_work_area(monitor);
    Some(
        (monitor * OPENING_SHARE)
            .max(MIN_WINDOW_SIZE)
            .round()
            .min(available),
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
        // Window positions are physical; sizes and the desktop allowance are logical.
        let (inset, available) = screen_work_area(logical);
        let at = monitor.physical_position.as_vec2()
            + (inset + (available - size) * 0.5) * monitor.scale_factor as f32;
        window.position = WindowPosition::At(at.round().as_ivec2());
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
        // Width reaches the minimum; height retains 80% where desktop allowances permit.
        assert_eq!(
            opening_size(Some(Vec2::new(1280.0, 800.0))),
            Some(Vec2::new(1280.0, 640.0))
        );
        // An unsupported smaller monitor still retains the desktop allowance.
        assert_eq!(
            opening_size(Some(Vec2::new(1024.0, 640.0))),
            Some(Vec2::new(1024.0, 560.0))
        );
        // Even smaller displays retain a bounded fallback inside the desktop allowance.
        assert_eq!(
            opening_size(Some(Vec2::new(800.0, 600.0))),
            Some(Vec2::new(800.0, 520.0))
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

    #[test]
    fn smallest_supported_screen_leaves_room_for_desktop_chrome() {
        assert_eq!(MIN_WINDOW_SIZE, Vec2::new(1280.0, 640.0));
        assert_eq!(
            opening_size(Some(Vec2::new(1280.0, 720.0))),
            Some(MIN_WINDOW_SIZE)
        );
        let (mut app, entity) = app_with_window();
        spawn_primary_monitor(&mut app, UVec2::new(1280, 720), 1.0);
        app.update();
        assert_eq!(
            window(&app, entity).resolution.size(),
            Vec2::new(1280.0, 640.0)
        );
        assert_eq!(
            window(&app, entity).position,
            WindowPosition::At(IVec2::new(0, 40))
        );
    }

    proptest::proptest! {
        #[test]
        fn opening_size_fits_supported_screens_with_desktop_space(
            width in 1280.0_f32..7680.0, height in 720.0_f32..4320.0,
        ) {
            let size = opening_size(Some(Vec2::new(width, height))).unwrap();
            proptest::prop_assert!(size.x >= 1280.0 && size.y >= 640.0);
            proptest::prop_assert!(size.x <= width && size.y <= height - 80.0);
        }
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
        assert_eq!(opened.position, WindowPosition::At(IVec2::new(512, 288)));

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
