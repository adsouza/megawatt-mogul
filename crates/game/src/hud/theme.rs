//! A single dark text palette, with a 4.5:1 minimum contrast for every text size and state.
//! Semantic chart/marker colours stay separate from the colours used to write text.
use bevy_egui::{EguiContexts, egui};
use egui::Color32;

pub const MIN_TEXT_CONTRAST: f64 = 4.5;
pub const TEXT: Color32 = Color32::from_gray(230);
pub const LABEL_TEXT: Color32 = Color32::WHITE;
pub const LABEL_BACKING: Color32 = Color32::from_gray(24);
/// The brightest text-bearing surface in the HUD palette, including hovered widgets.
const LIGHTEST_SURFACE: Color32 = Color32::from_gray(70);

fn luminance(color: Color32) -> f64 {
    let linear = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

fn contrast(a: Color32, b: Color32) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Opaque black or white, whichever has more contrast against an opaque fill.
pub fn contrasting_text(background: Color32) -> Color32 {
    if contrast(Color32::WHITE, background) >= contrast(Color32::BLACK, background) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// Keep a preferred opaque text colour when it meets the minimum against an opaque background;
/// otherwise move it toward black or white just far enough to meet the minimum.
pub fn readable_color(preferred: Color32, background: Color32) -> Color32 {
    if contrast(preferred, background) >= MIN_TEXT_CONTRAST {
        return preferred;
    }
    let target = contrasting_text(background);
    let mix = |step: u16| {
        let channel =
            |a: u8, b: u8| ((u16::from(a) * (255 - step) + u16::from(b) * step) / 255) as u8;
        Color32::from_rgb(
            channel(preferred.r(), target.r()),
            channel(preferred.g(), target.g()),
            channel(preferred.b(), target.b()),
        )
    };
    let (mut low, mut high) = (0, 255);
    while high - low > 1 {
        let mid = (low + high) / 2;
        if contrast(mix(mid), background) >= MIN_TEXT_CONTRAST {
            high = mid;
        } else {
            low = mid;
        }
    }
    mix(high)
}

/// A semantic foreground that is readable on every text-bearing HUD surface.
pub(super) fn status_color(preferred: Color32) -> Color32 {
    readable_color(preferred, LIGHTEST_SURFACE)
}

pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(Color32::from_gray(190));
    // Disabled controls keep readable labels; egui still uses noninteractive widget styling.
    v.disabled_alpha = 1.0;
    v.panel_fill = Color32::from_gray(27);
    v.window_fill = v.panel_fill;
    v.extreme_bg_color = Color32::from_gray(16);
    v.text_edit_bg_color = Some(v.extreme_bg_color);
    v.code_bg_color = Color32::from_gray(50);
    v.faint_bg_color = Color32::from_gray(38);
    v.selection.bg_fill = Color32::from_rgb(30, 60, 90);
    v.selection.stroke.color = TEXT;
    v.hyperlink_color = status_color(v.hyperlink_color);
    v.warn_fg_color = status_color(v.warn_fg_color);
    v.error_fg_color = status_color(v.error_fg_color);
    for widget in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.fg_stroke.color = TEXT;
    }
    v.widgets.active.fg_stroke.color = Color32::WHITE;
    v
}

/// Runs before any panel, including boot/menu screens and all modal overlays. The palette is
/// explicit so OS theme changes cannot substitute an unverified set of colours.
pub(super) fn apply_theme(mut contexts: EguiContexts) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals();
        // Opacity fades would temporarily turn readable labels into low-contrast text.
        style.animation_time = 0.0;
    });
    Ok(())
}

/// Text on terrain or data marks always gets its own opaque surface, even on faded projects.
pub(super) fn paint_label(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: egui::Align2,
    text: &str,
    size: f32,
) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(size),
        LABEL_TEXT,
    );
    let rect = align.anchor_size(pos, galley.size());
    painter.rect_filled(rect.expand(1.0), egui::CornerRadius::same(2), LABEL_BACKING);
    painter.galley(rect.min, galley, LABEL_TEXT);
}
