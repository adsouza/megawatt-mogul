//! Opt-in inspector restricted to render entities and visual tuning. No aggregate access.
use super::{TuningConfig, WorldEntity};
use crate::{hud::HudSet, l10n::Localizer};
use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, PrimaryEguiContext, egui};
use bevy_inspector_egui::{
    DefaultInspectorConfigPlugin,
    bevy_inspector::{self, Filter},
};

pub struct DevToolsPlugin;
impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        // Bevy's default features (including reflect_auto_register) are deliberately off.
        // The inspector assumes these registrations exist before installing its type data.
        app.register_type::<Name>()
            .register_type::<bevy::asset::uuid::Uuid>()
            .register_type::<Transform>()
            .register_type::<GlobalTransform>()
            .register_type::<Visibility>()
            .register_type::<Mesh3d>()
            .register_type::<MeshMaterial3d<StandardMaterial>>()
            .register_type::<DirectionalLight>()
            .register_type::<bevy::render::view::ColorGrading>()
            .register_type::<TuningConfig>()
            .add_plugins(DefaultInspectorConfigPlugin)
            .add_systems(
                EguiPrimaryContextPass,
                inspector
                    .after(HudSet::Overlays)
                    .before(super::ui::finish_surface),
            );
    }
}
fn inspector(world: &mut World) {
    let Some(l10n) = world.get_resource::<Localizer>() else {
        return;
    };
    let title = l10n.text("world-dev-title");
    let help = l10n.text("world-dev-boundary");
    let entities = l10n.text("world-dev-entities");
    let Ok(context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single(world)
    else {
        return;
    };
    let mut context = context.clone();
    let ctx = context.get_mut();
    // F12 deliberately uses egui input, preserving keyboard capture in all production controls.
    let id = egui::Id::new("diorama_dev_open");
    let mut open = ctx.data_mut(|d| *d.get_temp_mut_or_default::<bool>(id));
    if !ctx.egui_wants_keyboard_input()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F12))
    {
        open = !open;
    }
    if open {
        egui::Window::new(title)
            .id(egui::Id::new("diorama_dev"))
            .default_width(340.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(help);
                bevy_inspector::ui_for_resource::<TuningConfig>(world, ui);
                ui.collapsing(entities, |ui| {
                    bevy_inspector::ui_for_entities_filtered(
                        world,
                        ui,
                        false,
                        &Filter::<With<WorldEntity>>::all(),
                    );
                });
            });
    }
    ctx.data_mut(|d| d.insert_temp(id, open));
}
