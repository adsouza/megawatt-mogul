//! Screen space shared between panel systems.
//!
//! egui 0.36 lays panels out inside a [`egui::Ui`], not a context, and each panel system builds
//! its own root `Ui`. [`HudArea`] is the part of the screen no panel has claimed yet this frame,
//! so a side panel added by a later system starts below the top bar instead of under it.
//!
//! The outer panels ([`OuterPanel`]) are resizable by dragging their inner edge (spec §16.7):
//! each opens at its fixed size, which is also its minimum, and grows up to [`panel_max`], which
//! leaves the map at least [`MAP_MIN_SIZE`]. egui keeps each panel's size in its memory, so it
//! lasts for the run, through selection, tab and phase changes.

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
        let rect = self.free_rect(ctx);
        egui::Ui::new(
            ctx.clone(),
            egui::Id::new(id),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(rect),
        )
    }

    /// The space no panel has claimed so far this frame: the whole screen if none has. An overlay
    /// that must leave the bars uncovered (the Review backdrop) or sit below them reads it.
    pub fn free_rect(&self, ctx: &egui::Context) -> egui::Rect {
        self.free.unwrap_or_else(|| ctx.content_rect())
    }

    /// Records what `ui` (from [`Self::root_ui`]) still has free after its panels took their
    /// share. Call it after showing an edge panel; a central panel or an overlay claims nothing.
    pub fn claim(&mut self, ui: &egui::Ui) {
        self.free = Some(ui.available_rect_before_wrap());
    }

    /// Shows `panel` on its edge of the unclaimed space, resizable between its opening size and
    /// [`panel_max`], and claims its share. `reserved` is the size of the outer panels still to
    /// come on the same axis this frame (see [`OuterPanel::current_size`]): the map must keep
    /// its room beside them too.
    pub fn show_panel<R>(
        &mut self,
        ctx: &egui::Context,
        panel: OuterPanel,
        reserved: f32,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let max = panel_max(
            panel.edge,
            panel.size,
            ctx.content_rect().size(),
            self.free_rect(ctx).size(),
            reserved,
        );
        let mut ui = self.root_ui(ctx, panel.id);
        let container = match panel.edge {
            Edge::Left => egui::Panel::left(panel.id),
            Edge::Right => egui::Panel::right(panel.id),
            Edge::Bottom => egui::Panel::bottom(panel.id),
        };
        let inner = container
            .resizable(true)
            .default_size(panel.size)
            .min_size(panel.size)
            .max_size(max)
            .show(&mut ui, |ui| fill(ui, add_contents))
            .inner;
        self.claim(&ui);
        inner
    }
}

/// Lays `add_contents` out over exactly the space `ui` has, and reports that as its size.
///
/// egui sizes a resizable panel by its content: content that falls short of a dragged size would
/// shrink the panel back on release, and content that overflows would grow it on its own, moving
/// the map. Drawn in a child that the parent doesn't allocate, then covering `ui`'s whole space,
/// the panel keeps exactly the size it was dragged to.
fn fill<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rect = ui.max_rect();
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let inner = add_contents(&mut content);
    ui.expand_to_include_rect(rect);
    inner
}

pub(super) fn reset_hud_area(mut area: ResMut<HudArea>) {
    area.free = None;
}

/// The least room the outer panels leave the map (spec §16.7): enough to see it and drag on it.
pub const MAP_MIN_SIZE: egui::Vec2 = egui::vec2(320.0, 200.0);
/// The most of the screen's width a side panel can be dragged to.
pub const SIDE_PANEL_MAX_SHARE: f32 = 0.4;
/// The most of the screen's height a bottom panel can be dragged to.
pub const BOTTOM_PANEL_MAX_SHARE: f32 = 0.5;

/// The screen edge an outer panel sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Bottom,
}

/// The largest size an outer panel on `edge` can be dragged to, along its axis.
///
/// It is at most its share of the `screen`, and it leaves the map at least [`MAP_MIN_SIZE`] of
/// the `free` space (what no panel has claimed when this one is shown) after `reserved`, the
/// panels still to come on the same axis. It is never below `min`, the panel's opening size.
pub fn panel_max(edge: Edge, min: f32, screen: egui::Vec2, free: egui::Vec2, reserved: f32) -> f32 {
    let axis = edge.axis();
    let share = screen[axis] * edge.max_share();
    let room = free[axis] - reserved - MAP_MIN_SIZE[axis];
    share.min(room).max(min)
}

impl Edge {
    /// 0 for a side panel's width, 1 for a bottom panel's height.
    fn axis(self) -> usize {
        match self {
            Self::Left | Self::Right => 0,
            Self::Bottom => 1,
        }
    }

    fn max_share(self) -> f32 {
        match self {
            Self::Left | Self::Right => SIDE_PANEL_MAX_SHARE,
            Self::Bottom => BOTTOM_PANEL_MAX_SHARE,
        }
    }
}

/// An outer HUD panel the player can resize: its egui id (unique per panel system), its edge,
/// and its opening size along its axis, which is also its minimum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuterPanel {
    pub id: &'static str,
    pub edge: Edge,
    pub size: f32,
}

impl OuterPanel {
    /// Its size along its axis as egui last laid it out (as the player left it): its opening size
    /// before it is first shown. A panel shown earlier in the frame reads a later one's with this,
    /// to leave the map room beside it.
    pub fn current_size(&self, ctx: &egui::Context) -> f32 {
        egui::PanelState::load(ctx, egui::Id::new(self.id))
            .map_or(self.size, |state| state.size()[self.edge.axis()])
            .max(self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: egui::Vec2 = egui::vec2(1024.0, 640.0);
    const LARGE: egui::Vec2 = egui::vec2(2560.0, 1440.0);

    #[test]
    fn panel_limits_keep_the_map_usable() {
        // Below the top bar (and, in Operating, the speed bar): a little less than the screen.
        let free = |screen: egui::Vec2| egui::vec2(screen.x, screen.y - 60.0);

        // At 1024×640 a side panel takes at most 40% of the width, and the inspector leaves the
        // map 320 beside the build menu; a bottom panel takes at most half the height.
        assert_eq!(
            panel_max(Edge::Right, 300.0, SMALL, free(SMALL), 0.0),
            409.6
        );
        assert_eq!(
            panel_max(Edge::Left, 260.0, SMALL, free(SMALL), 300.0),
            1024.0 - 300.0 - 320.0
        );
        assert_eq!(
            panel_max(Edge::Bottom, 240.0, SMALL, free(SMALL), 0.0),
            320.0
        );

        // At 2560×1440 the shares bind: there is room to spare beside them.
        assert_eq!(
            panel_max(Edge::Left, 260.0, LARGE, free(LARGE), 300.0),
            1024.0
        );
        assert_eq!(
            panel_max(Edge::Right, 300.0, LARGE, free(LARGE), 0.0),
            1024.0
        );
        assert_eq!(
            panel_max(Edge::Bottom, 200.0, LARGE, free(LARGE), 0.0),
            720.0
        );

        // Where the map's room binds, the panel leaves it exactly its minimum.
        let short = egui::vec2(1024.0, 400.0);
        assert_eq!(panel_max(Edge::Bottom, 120.0, SMALL, short, 0.0), 200.0);
        assert_eq!(
            short.y - panel_max(Edge::Bottom, 120.0, SMALL, short, 0.0),
            MAP_MIN_SIZE.y
        );

        // The maximum is never below the minimum, even with no room to spare.
        for screen in [SMALL, LARGE] {
            for (edge, min) in [
                (Edge::Left, 260.0),
                (Edge::Right, 300.0),
                (Edge::Bottom, 240.0),
            ] {
                for reserved in [0.0, 300.0, 1_000.0, 5_000.0] {
                    for free in [free(screen), egui::vec2(100.0, 100.0), egui::Vec2::ZERO] {
                        let max = panel_max(edge, min, screen, free, reserved);
                        assert!(max >= min, "{edge:?} at {screen:?}: {max} < {min}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_panel_at_its_maximum_leaves_the_map_its_minimum_room() {
        // Both Planning side panels dragged as far as they go, in either order, at either size.
        for screen in [SMALL, LARGE] {
            let free = egui::vec2(screen.x, screen.y - 60.0);
            for build in [300.0, 350.0, 409.6] {
                let inspector = panel_max(Edge::Left, 260.0, screen, free, build);
                let build_max = panel_max(
                    Edge::Right,
                    300.0,
                    screen,
                    egui::vec2(free.x - inspector, free.y),
                    0.0,
                );
                assert!(
                    build <= build_max + 1e-3,
                    "{screen:?}: the build menu is never squeezed"
                );
                assert!(
                    free.x - inspector - build_max >= MAP_MIN_SIZE.x - 1e-3,
                    "{screen:?}: the map keeps its width"
                );
            }
            let bottom = panel_max(Edge::Bottom, 240.0, screen, free, 0.0);
            assert!(free.y - bottom >= MAP_MIN_SIZE.y, "{screen:?}");
        }
    }
}
