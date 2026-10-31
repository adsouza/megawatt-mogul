//! The opening crawl (first-game tutorial spec §4.1, §4.4): the goal, in perspective text that
//! recedes up the screen over a translucent backdrop, in the style of the Star Wars opening crawl.
//!
//! The text is laid out flat as a galley, tessellated into a mesh, and each vertex projected onto
//! a plane tilted away from the viewer, so lines shrink as they rise towards a vanishing point.
//! Text is clipped at a horizon a quarter of the way down the screen, never faded: the main spec
//! (§8.4) forbids fades that leave visible text unreadable, and at the horizon a line is still at
//! [`HORIZON_SCALE`] of its full size.

use bevy_egui::egui::{self, Color32, epaint};

use super::TutorialStep;
use crate::hud::action::{TutorialAction, UiAction};
use crate::hud::theme::ACCENT_YELLOW;

/// The backdrop: black at 0.8 alpha, so the map shows through but the text keeps 4.5:1 even over
/// white.
const CRAWL_BACKDROP: Color32 = Color32::from_black_alpha(204);
/// The crawl's text colour.
const CRAWL_TEXT: Color32 = ACCENT_YELLOW;
/// Where the horizon sits, as a fraction of the screen's height from its top.
const HORIZON: f32 = 0.25;
/// How big text is at the horizon, against its size at the bottom of the screen.
const HORIZON_SCALE: f32 = 0.4;
/// How far away the viewer stands from the bottom of the screen, in screen heights of flat text:
/// the larger, the gentler the perspective.
const VIEWER_DISTANCE: f32 = 0.6;

/// The crawl: the backdrop, and `text` `progress` of the way through its passage. Its accessible
/// name is the whole text. Any click, which it takes, or Escape (when `escape`) ends it.
pub(super) fn draw_crawl(
    ctx: &egui::Context,
    step: TutorialStep,
    text: &str,
    progress: f32,
    escape: bool,
) -> Vec<UiAction> {
    let screen = ctx.content_rect();
    let area = egui::Area::new(egui::Id::new("tutorial-crawl"))
        .order(egui::Order::Tooltip)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let response = ui.allocate_rect(screen, egui::Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
            ui.painter().rect_filled(screen, 0.0, CRAWL_BACKDROP);
            let mesh = crawl_mesh(ui, screen, text, progress);
            let below_horizon =
                egui::Rect::from_min_max(egui::pos2(screen.left(), horizon_y(screen)), screen.max);
            ui.painter()
                .with_clip_rect(below_horizon)
                .add(egui::Shape::mesh(mesh));
            response.clicked()
        });
    ctx.move_to_top(area.response.layer_id);
    let escaped = escape
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if area.inner || escaped {
        vec![UiAction::Tutorial(TutorialAction::Dismiss(step))]
    } else {
        Vec::new()
    }
}

fn horizon_y(screen: egui::Rect) -> f32 {
    screen.top() + HORIZON * screen.height()
}

/// The crawl's glyphs, laid out flat, tessellated, and projected onto the receding plane.
fn crawl_mesh(ui: &egui::Ui, screen: egui::Rect, text: &str, progress: f32) -> egui::Mesh {
    let size = (screen.height() * 0.05).clamp(18.0, 36.0);
    let mut job = egui::text::LayoutJob::simple(
        // A blank line between sentences, so each reads as its own paragraph.
        text.replace('\n', "\n\n"),
        egui::FontId::proportional(size),
        CRAWL_TEXT,
        (screen.width() * 0.6).min(size * 22.0),
    );
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    let mut flat = egui::Mesh::default();
    let mut tessellator = epaint::Tessellator::new(
        ui.ctx().pixels_per_point(),
        epaint::TessellationOptions::default(),
        ui.ctx().fonts(|fonts| fonts.font_image_size()),
        Vec::new(),
    );
    tessellator.tessellate_text(
        &epaint::TextShape::new(egui::Pos2::ZERO, galley.clone(), CRAWL_TEXT),
        &mut flat,
    );
    let plane = Plane::of(screen);
    // The text's first line enters at the bottom; its last line has passed the horizon at the end.
    let travel = galley.rect.height() + plane.ground_to_horizon();
    let offset = progress.clamp(0.0, 1.0) * travel;
    let centre = galley.rect.center().x;
    for vertex in &mut flat.vertices {
        let ground = offset - (vertex.pos.y - galley.rect.top());
        vertex.pos = plane.project(vertex.pos.x - centre, ground);
    }
    flat
}

/// The receding plane the text lies on, as seen on `screen`.
struct Plane {
    centre_x: f32,
    bottom: f32,
    vanishing_y: f32,
    distance: f32,
}

impl Plane {
    fn of(screen: egui::Rect) -> Self {
        let bottom = screen.bottom();
        let horizon = horizon_y(screen);
        // Where scale `HORIZON_SCALE` falls on the horizon.
        let vanishing_y = (horizon - HORIZON_SCALE * bottom) / (1.0 - HORIZON_SCALE);
        Self {
            centre_x: screen.center().x,
            bottom,
            vanishing_y,
            distance: VIEWER_DISTANCE * screen.height(),
        }
    }

    /// How far along the plane, from the bottom of the screen, the horizon is.
    fn ground_to_horizon(&self) -> f32 {
        self.distance * (1.0 / HORIZON_SCALE - 1.0)
    }

    /// Where a point `x` from the text's centre line, `ground` along the plane from the bottom of
    /// the screen (negative: not yet on screen), appears.
    fn project(&self, x: f32, ground: f32) -> egui::Pos2 {
        // Points well below the screen stay below it rather than flipping over the viewer.
        let ground = ground.max(-0.5 * self.distance);
        let scale = self.distance / (self.distance + ground);
        egui::pos2(
            self.centre_x + x * scale,
            self.vanishing_y + (self.bottom - self.vanishing_y) * scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 640.0))
    }

    #[test]
    fn the_plane_meets_the_bottom_at_full_size_and_the_horizon_at_its_scale() {
        let plane = Plane::of(screen());
        let at_bottom = plane.project(100.0, 0.0);
        assert!((at_bottom.y - 640.0).abs() < 1e-3);
        assert!((at_bottom.x - 740.0).abs() < 1e-3);
        let at_horizon = plane.project(100.0, plane.ground_to_horizon());
        assert!((at_horizon.y - 160.0).abs() < 1e-2, "{at_horizon:?}");
        assert!((at_horizon.x - (640.0 + 100.0 * HORIZON_SCALE)).abs() < 1e-2);
        // Further on, smaller and higher; below the screen, lower than its bottom.
        let beyond = plane.project(100.0, plane.ground_to_horizon() * 2.0);
        assert!(beyond.y < at_horizon.y && beyond.x < at_horizon.x);
        assert!(plane.project(0.0, -1000.0).y > 640.0);
    }

    #[test]
    fn the_backdrop_keeps_the_text_readable_over_white() {
        let linear = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let luminance =
            |c: Color32| 0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b());
        // Premultiplied black over white leaves white times (1 - alpha).
        let left = 255 - CRAWL_BACKDROP.a();
        let behind = Color32::from_gray(left);
        let ratio = (luminance(CRAWL_TEXT) + 0.05) / (luminance(behind) + 0.05);
        assert!(ratio >= crate::hud::theme::MIN_TEXT_CONTRAST, "{ratio}");
    }
}
