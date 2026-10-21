//! Megawatt Mogul's Bevy client: the outermost adapter over the simulation core (spec §4, §8).
//!
//! The game owns no rules. It reads query views and projections, and writes only by sending
//! player commands through the application layer.
#![forbid(unsafe_code)]

pub mod hud;
pub mod l10n;
pub mod paths;
pub mod preferences;
pub mod sim;
pub mod state;
pub mod window;
pub mod world;

use bevy::app::{PluginGroup, PluginGroupBuilder};
use bevy_egui::EguiPlugin;

use crate::hud::HudPlugin;
use crate::l10n::LocalizationPlugin;
use crate::sim::SimulationPlugin;
use crate::state::AppStatePlugin;

/// The headless-safe plugins: app states, localisation and the simulation bridge.
///
/// Needs no window or renderer, so tests run it on `MinimalPlugins` (plus `StatesPlugin`).
pub struct CorePlugins;

impl PluginGroup for CorePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(AppStatePlugin)
            .add(LocalizationPlugin)
            .add(SimulationPlugin)
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
            .add(preferences::PreferencesPlugin)
    }
}
