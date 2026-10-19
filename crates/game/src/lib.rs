//! Megawatt Mogul's Bevy client: the outermost adapter over the simulation core (spec §4, §8).
//!
//! The game owns no rules. It reads query views and projections, and writes only by sending
//! player commands through the application layer.
#![forbid(unsafe_code)]

pub mod hud;
pub mod paths;
pub mod state;

use bevy::app::{PluginGroup, PluginGroupBuilder};
use bevy_egui::EguiPlugin;

use crate::hud::HudPlugin;
use crate::state::AppStatePlugin;

/// The headless-safe plugins: app states, and later localisation and the simulation.
///
/// Needs no window or renderer, so tests run it on `MinimalPlugins` (plus `StatesPlugin`).
pub struct CorePlugins;

impl PluginGroup for CorePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>().add(AppStatePlugin)
    }
}

/// Everything the windowed game adds on top of `DefaultPlugins`: [`CorePlugins`], egui and the HUD.
pub struct GamePlugins;

impl PluginGroup for GamePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add_group(CorePlugins)
            .add(EguiPlugin::default())
            .add(HudPlugin)
    }
}
