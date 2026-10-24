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

/// Semantic text colors shared by the HUD. Presenters choose meaning before localization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTone {
    Neutral,
    Information,
    Positive,
    Caution,
    Negative,
}

/// A categorical tone or a continuous scale from favorable (0) to unfavorable (1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueColor {
    Tone(TextTone),
    Scale(f32),
}

impl ValueColor {
    pub(crate) fn color(self) -> Color32 {
        match self {
            Self::Tone(tone) => tone.color(),
            Self::Scale(fraction) => severity_color(fraction),
        }
    }
}

pub(crate) fn severity_color(fraction: f32) -> Color32 {
    let green = TextTone::Positive.color();
    let amber = TextTone::Caution.color();
    let red = TextTone::Negative.color();
    let fraction = fraction.clamp(0.0, 1.0);
    let color = if fraction <= 0.5 {
        green.lerp_to_gamma(amber, fraction * 2.0)
    } else {
        amber.lerp_to_gamma(red, (fraction - 0.5) * 2.0)
    };
    status_color(color)
}

pub(crate) fn marginal_cost_fraction(
    region: &domain::region::RegionDefinition,
    cost: domain::units::Price,
) -> f32 {
    let ceiling = region
        .catalog()
        .iter()
        .map(|spec| spec.marginal_cost.value())
        .fold(0.0, f64::max);
    if ceiling > 0.0 {
        (cost.value() / ceiling).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

impl TextTone {
    pub(crate) fn color(self) -> Color32 {
        let preferred = match self {
            Self::Neutral => Color32::from_rgb(178, 190, 207),
            Self::Information => Color32::from_rgb(116, 216, 210),
            Self::Positive => Color32::from_rgb(141, 222, 164),
            Self::Caution => Color32::from_rgb(244, 201, 120),
            Self::Negative => Color32::from_rgb(255, 159, 150),
        };
        status_color(preferred)
    }

    pub(crate) fn cash(cents: i64) -> Self {
        match cents.cmp(&0) {
            std::cmp::Ordering::Less => Self::Negative,
            std::cmp::Ordering::Equal => Self::Neutral,
            std::cmp::Ordering::Greater => Self::Positive,
        }
    }
}

pub(crate) fn heading_color() -> Color32 {
    status_color(Color32::from_rgb(145, 195, 255))
}

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
    // egui's whole disabled look is this fade, applied to the text and to whatever it sits on, so
    // a faded label's contrast would depend on the surface. The fade stays off and the HUD draws
    // disabled controls with `disabled_visuals` instead (`action::add_enabled`).
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

/// The look of a control that can't be used now, derived from the enabled `visuals`: a button has
/// no fill of its own, so it shows the surface it sits on, and a thin outline in the enabled fill's
/// colour; a selected toggle's fill is dimmed; a slider's rail and handle, a checkbox's box, take
/// the faint fill; text is muted. In egui 0.36 a disabled button still senses clicks, so without
/// this it paints with the enabled idle look. Every state gets the same look, as a disabled
/// control has none of its own. The muted text meets the minimum on every HUD surface
/// (`contrast.rs`).
pub fn disabled_visuals(visuals: &egui::Visuals) -> egui::Visuals {
    let mut v = visuals.clone();
    let text = v.weak_text_color();
    v.override_text_color = Some(text);
    v.selection.stroke.color = text;
    // A selected toggle is filled with the selection colour in every state, so dim it, halfway
    // to the panel: it still reads as on, but not as usable. The muted text meets the minimum on
    // it (`contrast.rs`).
    v.selection.bg_fill = visuals
        .selection
        .bg_fill
        .lerp_to_gamma(visuals.panel_fill, 0.5);
    let enabled = visuals.widgets.inactive;
    let look = egui::style::WidgetVisuals {
        weak_bg_fill: Color32::TRANSPARENT,
        bg_fill: visuals.faint_bg_color,
        bg_stroke: egui::Stroke::new(1.0, enabled.weak_bg_fill),
        fg_stroke: egui::Stroke::new(enabled.fg_stroke.width, text),
        ..enabled
    };
    v.widgets.inactive = look;
    v.widgets.hovered = look;
    v.widgets.active = look;
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
pub fn paint_label(
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
