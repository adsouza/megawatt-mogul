//! App states: `Boot` → `MainMenu` → `RegionSelect` → `InGame`, with the day phase as a sub-state.

use bevy::prelude::*;

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

/// What must exist before `Boot` moves on to `MainMenu`. Plugins add entries through
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

/// Registers the states and moves `Boot` to `MainMenu` once the boot checklist is met.
///
/// Requires `StatesPlugin`, which `DefaultPlugins` includes and headless apps add themselves.
pub struct AppStatePlugin;

impl Plugin for AppStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_sub_state::<PhaseState>()
            .require_for_boot::<AssetRoot>()
            .require_for_boot::<LogDir>()
            .add_systems(Update, finish_boot.run_if(in_state(AppState::Boot)));
    }
}

pub(crate) fn finish_boot(world: &mut World) {
    if world.contains_resource::<BootError>() {
        return;
    }
    let ready = world
        .get_resource::<BootChecklist>()
        .is_none_or(|checklist| checklist.unmet(world).is_empty());
    if ready {
        info!("boot complete");
        world
            .resource_mut::<NextState<AppState>>()
            .set(AppState::MainMenu);
    }
}
