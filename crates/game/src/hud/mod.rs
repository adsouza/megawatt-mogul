//! The egui HUD (spec §8.4). Its systems run in bevy_egui's `EguiPrimaryContextPass` schedule.

use bevy::app::{App, Plugin};
use bevy_egui::EguiPrimaryContextPass;

/// The egui panels. Empty until the HUD lands (Plan 2, Task 5).
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        // bevy_egui runs the pass with `try_run_schedule`. If no system was ever added to
        // `EguiPrimaryContextPass` the schedule doesn't exist, the pass errors, and the dropped
        // egui output panics in epaint ("Dropped TexturesDelta with 1 unapplied deltas"). So the
        // schedule must exist even while the HUD is empty.
        app.init_schedule(EguiPrimaryContextPass);
    }
}
