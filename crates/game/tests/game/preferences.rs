use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, update_n};
use bevy::prelude::*;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow, WindowPosition};
use game::{
    paths::SaveDir,
    preferences::{ClientSettings, PreferencesPlugin},
    window::WindowSizingPlugin,
};
use infrastructure::client_preferences::{
    ClientPreferences, PreferenceWindow, read_preferences, write_preferences,
};
use tempfile::TempDir;

#[test]
fn preferences_restore_window_and_clamp_a_missing_monitor() {
    let root = TempDir::new().unwrap();
    let p = ClientPreferences {
        window: Some(PreferenceWindow {
            width: 1400.0,
            height: 900.0,
            x: -9000,
            y: 300,
        }),
        zoom: 1.2,
        speed: 8,
        ..Default::default()
    };
    write_preferences(root.path(), &p).unwrap();
    let mut app = App::new();
    app.insert_resource(SaveDir(root.path().into()))
        .add_plugins((MinimalPlugins, PreferencesPlugin, WindowSizingPlugin));
    app.world_mut().spawn((
        Monitor {
            name: None,
            physical_height: 1080,
            physical_width: 1920,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: None,
            scale_factor: 1.0,
            video_modes: vec![],
        },
        PrimaryMonitor,
    ));
    let id = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    update_n(&mut app, 3);
    let w = app.world().get::<Window>(id).unwrap();
    assert_eq!(w.width(), 1400.0);
    let WindowPosition::At(at) = w.position else {
        panic!("restored position")
    };
    assert!(at.x >= 0 && at.x + 1400 <= 1920 && at.y >= 0 && at.y + 900 <= 1080);
}

#[test]
fn preferences_real_egui_pass_restores_and_retains_panels_zoom_and_day_speed_on_exit() {
    let root = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    let mut p = ClientPreferences {
        zoom: 1.25,
        speed: 4,
        ..Default::default()
    };
    p.panels.insert("planning".into(), [1600.0, 320.0]);
    write_preferences(root.path(), &p).unwrap();
    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(root.path().into()))
        .add_plugins(PreferencesPlugin);
    update_n(&mut app, 2);
    let pass = EguiPass::attach(&mut app);
    // Render the main menu before starting: its default LastSpeed must not replace the loaded one.
    pass.settled(&mut app, 1600.0, 1000.0);
    assert_eq!(app.world().resource::<ClientSettings>().value.speed, 4);
    request(&mut app, "western_interconnection");
    pass.settled(&mut app, 1600.0, 1000.0);
    assert_eq!(pass.ctx().zoom_factor(), 1.25);
    assert_eq!(
        app.world().resource::<game::hud::LastSpeed>().0,
        game::sim::SimSpeed::X4
    );
    assert!(
        bevy_egui::egui::PanelState::load(pass.ctx(), bevy_egui::egui::Id::new("planning"))
            .unwrap()
            .size()
            .y
            >= 319.0
    );
    pass.ctx().set_zoom_factor(1.5);
    pass.settled(&mut app, 1600.0, 1000.0);
    crate::hud_smoke::dispatch_in(
        &mut app,
        vec![game::hud::UiAction::SetSpeed(game::sim::SimSpeed::X8)],
    );
    pass.run(&mut app, 1600.0, 1000.0);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    let restored = read_preferences(root.path()).unwrap();
    assert_eq!(restored.zoom, 1.5);
    assert_eq!(restored.speed, 8);
    assert!(restored.panels.contains_key("build"));
    assert!(app.world().resource::<ClientSettings>().loaded);
}

#[test]
fn preferences_real_sidebar_and_bottom_drags_survive_a_fresh_app() {
    use bevy_egui::egui;
    let root = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(root.path().into()))
        .add_plugins(PreferencesPlugin);
    update_n(&mut app, 2);
    request(&mut app, "eastern_interconnection");
    let pass = EguiPass::attach(&mut app);
    pass.settled(&mut app, 1920.0, 1080.0);
    for id in ["build", "planning"] {
        let panel = egui::PanelState::load(pass.ctx(), egui::Id::new(id))
            .unwrap()
            .outer_rect;
        let from = if id == "build" {
            panel.left_center()
        } else {
            panel.center_top()
        };
        let to = from
            + if id == "build" {
                egui::vec2(-70.0, 0.0)
            } else {
                egui::vec2(0.0, -60.0)
            };
        pass.drag(&mut app, 1920.0, 1080.0, from, to);
        pass.settled(&mut app, 1920.0, 1080.0);
    }
    let before = app
        .world()
        .resource::<ClientSettings>()
        .value
        .panels
        .clone();
    assert!(before["build"][0] > game::hud::BUILD_PANEL_WIDTH + 40.0);
    assert!(before["planning"][1] > game::hud::PLANNING_PANEL_HEIGHT + 40.0);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    drop(app);
    let mut fresh = hud_app(logs.path());
    fresh
        .insert_resource(SaveDir(root.path().into()))
        .add_plugins(PreferencesPlugin);
    update_n(&mut fresh, 2);
    request(&mut fresh, "eastern_interconnection");
    let pass = EguiPass::attach(&mut fresh);
    pass.settled(&mut fresh, 1920.0, 1080.0);
    for id in ["build", "planning"] {
        let size = egui::PanelState::load(pass.ctx(), egui::Id::new(id))
            .unwrap()
            .size();
        let axis = usize::from(id == "planning");
        assert!(
            (size[axis] - before[id][axis]).abs() <= 1.0,
            "{id}: {size:?} vs {:?}",
            before[id]
        );
    }
}

proptest::proptest! {
    #[test]
    fn restored_window_is_always_inside_the_chosen_monitor(x in -100000i32..100000, y in -100000i32..100000, w in 64.0f32..12000.0, h in 64.0f32..12000.0, scale in 0.75f64..3.0) {
        let original = PreferenceWindow { width: w, height:h,x,y };
        let origin = IVec2::new(-1920, 120);
        let pixels = UVec2::new(1920,1080);
        let fit = game::preferences::fit_window(&original,origin,pixels,scale);
        proptest::prop_assert!(fit.width.is_finite() && fit.height.is_finite());
        proptest::prop_assert!(fit.x >= origin.x && fit.y >= origin.y);
        proptest::prop_assert!(fit.x as f64 + f64::from(fit.width)*scale <= (origin.x+1920) as f64 + 1.0);
        proptest::prop_assert!(fit.y as f64 + f64::from(fit.height)*scale <= (origin.y+1080) as f64 + 1.0);
    }
}
