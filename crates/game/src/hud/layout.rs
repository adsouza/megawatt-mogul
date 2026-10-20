//! Screen space shared between panel systems.
//!
//! egui 0.36 lays panels out inside a [`egui::Ui`], not a context, and each panel system builds
//! its own root `Ui`. [`HudArea`] is the part of the screen no panel has claimed yet this frame,
//! so a side panel added by a later system starts below the top bar instead of under it.

use bevy::prelude::*;
use bevy_egui::egui;

/// The screen space not yet taken by a panel this frame. Reset at the start of every egui pass.
#[derive(Resource, Debug, Default)]
pub struct HudArea {
    free: Option<egui::Rect>,
}

impl HudArea {
    /// A root `Ui` over the unclaimed space (the whole screen if nothing has claimed any), on the
    /// background layer. `id` must be unique per panel system.
    pub fn root_ui(&self, ctx: &egui::Context, id: &'static str) -> egui::Ui {
        let rect = self.free.unwrap_or_else(|| ctx.content_rect());
        egui::Ui::new(
            ctx.clone(),
            egui::Id::new(id),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(rect),
        )
    }

    /// Records what `ui` (from [`Self::root_ui`]) still has free after its panels took their
    /// share. Call it after showing an edge panel; a central panel or an overlay claims nothing.
    pub fn claim(&mut self, ui: &egui::Ui) {
        self.free = Some(ui.available_rect_before_wrap());
    }
}

pub(super) fn reset_hud_area(mut area: ResMut<HudArea>) {
    area.free = None;
}
