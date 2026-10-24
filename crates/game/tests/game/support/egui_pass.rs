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
        self.run_with(app, width, height, Vec::new())
    }

    /// Runs the pass once with `events` as the frame's input: the widgets it drew. Whatever the
    /// systems ask for goes through the real `dispatch`, so a test asserts on the resources and
    /// messages it wrote.
    pub fn run_with(
        &self,
        app: &mut App,
        width: f32,
        height: f32,
        events: Vec<egui::Event>,
    ) -> Vec<Widget> {
        let output = self.paint_with(app, width, height, events);
        let widgets = widgets_of(&output);
        // There is no renderer to take the font texture.
        output.drop_without_applying_deltas();
        widgets
    }

    /// The actual paint output, for colour/contrast checks. The caller must consume texture deltas.
    pub fn paint_with(
        &self,
        app: &mut App,
        width: f32,
        height: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        self.ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            events,
            ..egui::RawInput::default()
        });
        app.world_mut().run_schedule(EguiPrimaryContextPass);
        self.ctx.end_pass()
    }

    /// Clicks with the primary button at `at`: a pass that presses there, then one that releases.
    /// Run it on a settled screen (see [`Self::settled`]) so the target is where `at` says, and
    /// don't settle around it: that would run the systems, and `dispatch`, again.
    pub fn click_at(&self, app: &mut App, width: f32, height: f32, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.run_with(
            app,
            width,
            height,
            vec![egui::Event::PointerMoved(at), button(true)],
        );
        self.run_with(app, width, height, vec![button(false)]);
    }

    /// Drags with the primary button from `from` to `to`: a pass that presses at `from`, one per
    /// step of the way (halfway, then `to`), and one that releases at `to`. Like
    /// [`Self::click_at`], run it on a settled screen and don't settle around it.
    pub fn drag(&self, app: &mut App, width: f32, height: f32, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.run_with(
            app,
            width,
            height,
            vec![egui::Event::PointerMoved(from), button(from, true)],
        );
        for at in [from.lerp(to, 0.5), to] {
            self.run_with(app, width, height, vec![egui::Event::PointerMoved(at)]);
        }
        self.run_with(app, width, height, vec![button(to, false)]);
    }

    /// Runs the pass twice, so sizes egui takes from the previous pass have settled.
    pub fn settled(&self, app: &mut App, width: f32, height: f32) -> Vec<Widget> {
        self.run(app, width, height);
        self.run(app, width, height)
    }
}
