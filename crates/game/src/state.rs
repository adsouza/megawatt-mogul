//! App states: boot chooses the opening menu or new-game setup; play has a day-phase sub-state.

use bevy::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
use crate::paths::{AssetRoot, LogDir};

/// The top-level screens.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    /// Waiting for every [`BootChecklist`] entry. A [`BootError`] keeps the app here.
    #[default]
    Boot,
    MainMenu,
    RegionSelect,
    InGame,
}

/// The day phase while in game. It mirrors the domain's `DayPhase` one way only (domain → Bevy):
/// the game never sets it on its own initiative.
#[derive(SubStates, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[source(AppState = AppState::InGame)]
pub enum PhaseState {
    #[default]
    Planning,
    Operating,
    Review,
}

/// Why boot cannot finish. While it exists the app stays in [`AppState::Boot`], which shows it.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct BootError(pub String);

/// What must exist before `Boot` chooses the first menu screen. Plugins add entries through
/// [`BootChecklistAppExt::require_for_boot`], in any order.
#[derive(Resource, Default)]
pub struct BootChecklist {
    requirements: Vec<BootRequirement>,
}

struct BootRequirement {
    name: &'static str,
    is_met: fn(&World) -> bool,
}

impl BootChecklist {
    /// Requires the resource `R` to exist.
    pub fn require_resource<R: Resource>(&mut self) {
        self.requirements.push(BootRequirement {
            name: std::any::type_name::<R>(),
            is_met: |world| world.contains_resource::<R>(),
        });
    }

    /// The type names of the requirements not yet met, in the order they were added.
    pub fn unmet(&self, world: &World) -> Vec<&'static str> {
        self.requirements
            .iter()
            .filter(|requirement| !(requirement.is_met)(world))
            .map(|requirement| requirement.name)
            .collect()
    }
}

/// Lets a plugin add to the [`BootChecklist`] whether or not [`AppStatePlugin`] was added first.
pub trait BootChecklistAppExt {
    /// Keeps the app in `Boot` until the resource `R` exists.
    fn require_for_boot<R: Resource>(&mut self) -> &mut Self;
}

impl BootChecklistAppExt for App {
    fn require_for_boot<R: Resource>(&mut self) -> &mut Self {
        self.init_resource::<BootChecklist>()
            .world_mut()
            .resource_mut::<BootChecklist>()
            .require_resource::<R>();
        self
    }
}

/// Registers the states and chooses the first menu after boot resources and the save scan settle.
///
/// Requires `StatesPlugin`, which `DefaultPlugins` includes and headless apps add themselves.
pub struct AppStatePlugin;

impl Plugin for AppStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_sub_state::<PhaseState>()
            .add_systems(Update, finish_boot.run_if(in_state(AppState::Boot)));
        #[cfg(not(target_arch = "wasm32"))]
        app.require_for_boot::<AssetRoot>()
            .require_for_boot::<LogDir>();
    }
}

pub(crate) fn finish_boot(world: &mut World) {
    if world.contains_resource::<BootError>() {
        return;
    }
    let persistence = world.get_resource::<crate::sim::Persistence>();
    if persistence.is_some_and(|p| p.enabled() && !p.menu_scanned()) {
        return;
    }
    let opening = if persistence
        .is_some_and(|p| p.menu_scanned() && p.error.is_none() && p.entries.is_empty())
    {
        AppState::RegionSelect
    } else {
        AppState::MainMenu
    };
    let ready = world
        .get_resource::<BootChecklist>()
        .is_none_or(|checklist| checklist.unmet(world).is_empty());
    if ready {
        info!("boot complete");
        world.resource_mut::<NextState<AppState>>().set(opening);
    }
}
