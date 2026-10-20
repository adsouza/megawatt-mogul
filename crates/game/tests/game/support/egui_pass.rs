//! The HUD's real egui pass, run headlessly: the plugin's own systems in
//! `EguiPrimaryContextPass`, gated by the real run conditions, drawing into an egui context the
//! test owns. No window and no renderer: the context `EguiPlugin` would attach to the window is
//! spawned by hand, and the pass is begun and ended around the schedule as bevy_egui does.

use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPrimaryContextPass, EguiUserTextures, PrimaryEguiContext, egui};

use super::egui_harness::{Widget, widgets_of};

/// The primary egui context of a headless app.
pub struct EguiPass {
    ctx: egui::Context,
}

impl EguiPass {
    /// Gives `app` a primary egui context, as `EguiPlugin` does for the window.
    pub fn attach(app: &mut App) -> Self {
        app.init_resource::<EguiUserTextures>();
        let world = app.world_mut();
        let entity = world.spawn(PrimaryEguiContext).id();
        let ctx = world
            .get_mut::<EguiContext>(entity)
            .expect("PrimaryEguiContext requires an EguiContext")
            .get_mut()
            .clone();
        ctx.enable_accesskit();
        Self { ctx }
    }

    /// The context, to inspect or steer egui's memory between passes.
    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Runs the HUD's egui pass once on a `width`×`height` screen: the widgets it drew.
    pub fn run(&self, app: &mut App, width: f32, height: f32) -> Vec<Widget> {
        self.ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..egui::RawInput::default()
        });
        app.world_mut().run_schedule(EguiPrimaryContextPass);
        let output = self.ctx.end_pass();
        let widgets = widgets_of(&output);
        // There is no renderer to take the font texture.
        output.drop_without_applying_deltas();
        widgets
    }

    /// Runs the pass twice, so sizes egui takes from the previous pass have settled.
    pub fn settled(&self, app: &mut App, width: f32, height: f32) -> Vec<Widget> {
        self.run(app, width, height);
        self.run(app, width, height)
    }
}
