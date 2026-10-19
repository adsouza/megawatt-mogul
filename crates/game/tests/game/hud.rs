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
