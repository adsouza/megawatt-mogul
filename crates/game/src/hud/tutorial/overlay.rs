//! The tutorial's overlay (first-game tutorial spec §4.1): the crawl, spotlights and instructions,
//! drawn over every other HUD layer in [`HudSet::Tutorial`](crate::hud::HudSet::Tutorial).
//!
//! - [`tutorial_view`] is the pure presenter: what the active step shows, `shown_for` seconds after
//!   it first appeared.
//! - [`draw_spotlight`] and [`draw_instruction`] (and [`super::crawl::draw_crawl`]) lay it out;
//!   the spotlight and the crawl return what was asked for.
//! - [`tutorial_overlay`] picks the active step each pass, keeps its clock, and hands the actions
//!   to [`dispatch`].
//!
//! A spotlight or the crawl waits, its clock not started, while a prompt is open (prompts come
//! first) or, for a spotlight, while its anchor isn't drawn. A spotlight whose anchor sits in a
//! scroll area (the battery card, the line section, the Review's Construction row) first asks for
//! it to be scrolled into view, so it never waits on the player to find it. An instruction never
//! waits and never blocks input, not even on its own text; while a prompt is open it isn't drawn,
//! so it never covers the prompt. Most instructions sit on a card; one drawn as an arrow callout
//! (building the battery) points an arrow from its text at its anchor, and asks once for that
//! anchor to be scrolled into view if it isn't.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::crawl::draw_crawl;
use super::{
    InstructionStyle, StepDef, StepKind, TutorialAnchor, TutorialState, TutorialStep, active_step,
    anchor_rect, construction_key,
};
use crate::hud::action::{ActionSink, TutorialAction, UiAction, dispatch};
use crate::hud::theme::ACCENT_YELLOW;
use crate::l10n::Localizer;

/// How long a spotlight fades in, and out.
pub const SPOTLIGHT_FADE: f32 = 0.3;
/// When a spotlight's fade out begins: after 4.0 s held at full strength.
pub const SPOTLIGHT_HOLD_END: f32 = 4.3;
/// When a spotlight has run its course.
pub const SPOTLIGHT_SECONDS: f32 = SPOTLIGHT_HOLD_END + SPOTLIGHT_FADE;
/// How long the crawl takes to pass, unless the player ends it first.
pub const CRAWL_SECONDS: f32 = 45.0;

/// The strength of a spotlight's dimming at full opacity.
const DIM_ALPHA: f32 = 0.65;
/// How far a spotlight's cut-out reaches beyond its anchor, and how round its corners are.
const CUTOUT_MARGIN: f32 = 6.0;
const CUTOUT_RADIUS: f32 = 8.0;
/// A card's widest text, the space between it and what it points at, and its distance from the
/// screen's edges.
const CARD_TEXT_WIDTH: f32 = 300.0;
const CARD_GAP: f32 = 10.0;
const SCREEN_MARGIN: f32 = 8.0;

/// What the active tutorial step shows.
#[derive(Debug, Clone, PartialEq)]
pub enum TutorialView {
    /// An automatic UI move, made by playing the step.
    Move(TutorialStep),
    /// The opening crawl, `progress` of the way through (1 when it has passed).
    Crawl { text: String, progress: f32 },
    /// A self-ending highlight of `anchor`, with a note, at `opacity` of full strength.
    Spotlight {
        step: TutorialStep,
        anchor: TutorialAnchor,
        text: String,
        opacity: f32,
    },
    /// A callout that stays until its step is done, drawn in `style`.
    Instruction {
        anchor: TutorialAnchor,
        text: String,
        style: InstructionStyle,
    },
}

/// What `step` shows `shown_for` seconds after it first appeared.
pub fn tutorial_view(
    step: &StepDef,
    state: &TutorialState,
    game: &Game,
    l10n: &Localizer,
    shown_for: f32,
) -> TutorialView {
    let text = || {
        l10n.text(
            step.text_key
                .unwrap_or_else(|| construction_key(state, game)),
        )
    };
    let anchor = step.anchor.unwrap_or(TutorialAnchor::Map);
    match step.kind {
        StepKind::Move => TutorialView::Move(step.step),
        StepKind::Crawl => TutorialView::Crawl {
            text: text(),
            progress: shown_for / CRAWL_SECONDS,
        },
        StepKind::Spotlight => TutorialView::Spotlight {
            step: step.step,
            anchor,
            text: text(),
            opacity: spotlight_opacity(shown_for),
        },
        StepKind::Instruction => TutorialView::Instruction {
            anchor,
            text: text(),
            style: step.style,
        },
    }
}

/// A spotlight's strength `shown_for` seconds in: it fades in over 0.3 s, holds until 4.3 s and
/// fades out by 4.6 s.
pub fn spotlight_opacity(shown_for: f32) -> f32 {
    if shown_for <= SPOTLIGHT_HOLD_END {
        (shown_for / SPOTLIGHT_FADE).clamp(0.0, 1.0)
    } else {
        ((SPOTLIGHT_SECONDS - shown_for) / SPOTLIGHT_FADE).clamp(0.0, 1.0)
    }
}

/// When a crawl or spotlight that is showing first appeared, by the real clock. A step that isn't
/// showing yet (waiting for its anchor or a prompt) has no clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    step: TutorialStep,
    since: Option<f64>,
    /// Whether the step's anchor has been asked to scroll into view.
    revealed: bool,
}

/// Draws the active tutorial step over the HUD and hands what it asks for to [`dispatch`].
pub(crate) fn tutorial_overlay(
    mut contexts: EguiContexts,
    time: Res<Time<Real>>,
    l10n: Res<Localizer>,
    mut shown: Local<Option<Shown>>,
    mut sink: ActionSink,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let now = time.elapsed_secs_f64();
    let (Some(state), Some(game)) = (sink.tutorial(), sink.game()) else {
        *shown = None;
        return Ok(());
    };
    let Some(def) = active_step(state, game, sink.selection()) else {
        *shown = None;
        return Ok(());
    };
    let mut clock = match *shown {
        Some(clock) if clock.step == def.step => clock,
        _ => Shown {
            step: def.step,
            since: None,
            revealed: false,
        },
    };
    let prompt_open = sink.open_prompt().0.is_some();
    // Prompts come first: a timed step waits for one to close.
    let anchor = def.anchor.and_then(|anchor| anchor_rect(&ctx, anchor));
    let ready = !prompt_open
        && match def.kind {
            StepKind::Crawl => true,
            StepKind::Spotlight => anchor.is_some(),
            StepKind::Move | StepKind::Instruction => false,
        };
    let mut actions = Vec::new();
    // An anchor in a scroll area is brought into view once: before its spotlight starts, or when
    // an arrow callout's anchor isn't in view (a card needs none: it falls back to the map's top).
    let reveal = match def.kind {
        StepKind::Spotlight => clock.since.is_none(),
        StepKind::Instruction => def.style == InstructionStyle::Arrow && anchor.is_none(),
        StepKind::Move | StepKind::Crawl => false,
    };
    if reveal && !clock.revealed {
        if let Some(anchor) = def.anchor.filter(|anchor| scrolls(*anchor)) {
            actions.push(UiAction::Tutorial(TutorialAction::Reveal(anchor)));
        }
        clock.revealed = true;
    }
    if ready && clock.since.is_none() {
        clock.since = Some(now);
    }
    let shown_for = clock.since.map_or(0.0, |since| (now - since) as f32);
    let view = tutorial_view(def, state, game, &l10n, shown_for);
    let escape = !prompt_open && !egui::Popup::is_any_open(&ctx);
    match view {
        TutorialView::Move(step) => actions.push(UiAction::Tutorial(TutorialAction::Play(step))),
        TutorialView::Crawl { .. } | TutorialView::Spotlight { .. } if !ready => {}
        TutorialView::Crawl { text, progress } => {
            if progress >= 1.0 {
                actions.push(UiAction::Tutorial(TutorialAction::Dismiss(def.step)));
            } else {
                actions.extend(draw_crawl(&ctx, def.step, &text, progress, escape));
            }
        }
        TutorialView::Spotlight {
            step,
            text,
            opacity,
            ..
        } => {
            if shown_for >= SPOTLIGHT_SECONDS {
                actions.push(UiAction::Tutorial(TutorialAction::Dismiss(step)));
            } else if let Some(anchor) = anchor {
                actions.extend(draw_spotlight(&ctx, step, anchor, &text, opacity, escape));
            }
        }
        // Prompts come first: an instruction steps aside while one is open.
        TutorialView::Instruction { .. } if prompt_open => {}
        TutorialView::Instruction {
            anchor,
            text,
            style,
        } => match (style, anchor_rect(&ctx, anchor)) {
            (InstructionStyle::Arrow, Some(target)) => draw_callout(&ctx, target, &text),
            // A card; or, when an arrow's anchor isn't drawn, the card at the map's top.
            (_, target) => {
                let map = anchor_rect(&ctx, TutorialAnchor::Map);
                let placement = match (anchor, target) {
                    (TutorialAnchor::Map, _) | (_, None) => Placement::TopOf(map),
                    (_, Some(target)) => Placement::Beside(target),
                };
                draw_instruction(&ctx, placement, &text);
            }
        },
    }
    *shown = Some(clock);
    dispatch(actions, &mut sink);
    Ok(())
}

/// Whether `anchor` is drawn inside a scroll area, so a reveal can bring it into view.
fn scrolls(anchor: TutorialAnchor) -> bool {
    matches!(
        anchor,
        TutorialAnchor::BatteryCard
            | TutorialAnchor::BatteryBuildButton
            | TutorialAnchor::LineSection
            | TutorialAnchor::ReviewConstruction
    )
}

/// Where a card goes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Placement {
    /// Beside this rect, outside it.
    Beside(egui::Rect),
    /// Near the top centre of this rect (the map's), or of the screen if there is none.
    TopOf(Option<egui::Rect>),
}

/// Where a card of `size` goes on `screen`: beside its target (below, above, right or left,
/// whichever fits first), or near the top centre of its area; always inside the screen.
fn card_rect(screen: egui::Rect, placement: Placement, size: egui::Vec2) -> egui::Rect {
    let bounds = screen.shrink(SCREEN_MARGIN);
    let rect = match placement {
        Placement::TopOf(area) => {
            let area = area.unwrap_or(screen);
            egui::Rect::from_min_size(
                egui::pos2(area.center().x - size.x / 2.0, area.top() + CARD_GAP),
                size,
            )
        }
        Placement::Beside(target) => {
            let centred_x = target.center().x - size.x / 2.0;
            let centred_y = target.center().y - size.y / 2.0;
            let below = egui::pos2(centred_x, target.bottom() + CARD_GAP);
            let above = egui::pos2(centred_x, target.top() - CARD_GAP - size.y);
            let right = egui::pos2(target.right() + CARD_GAP, centred_y);
            let left = egui::pos2(target.left() - CARD_GAP - size.x, centred_y);
            let fits_y = |at: egui::Pos2| at.y >= bounds.top() && at.y + size.y <= bounds.bottom();
            let fits_x = |at: egui::Pos2| at.x >= bounds.left() && at.x + size.x <= bounds.right();
            let at = if fits_y(below) {
                below
            } else if fits_y(above) {
                above
            } else if fits_x(right) {
                right
            } else if fits_x(left) {
                left
            } else {
                below
            };
            egui::Rect::from_min_size(at, size)
        }
    };
    clamp_into(rect, bounds)
}

/// `rect` moved the least distance that puts it inside `bounds` (its top left, if it is bigger).
fn clamp_into(rect: egui::Rect, bounds: egui::Rect) -> egui::Rect {
    let x = rect
        .left()
        .min(bounds.right() - rect.width())
        .max(bounds.left());
    let y = rect
        .top()
        .min(bounds.bottom() - rect.height())
        .max(bounds.top());
    egui::Rect::from_min_size(egui::pos2(x, y), rect.size())
}

/// The card's frame: the window fill of the shared palette, opaque, outlined in the accent.
fn card_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .stroke(egui::Stroke::new(1.5, ACCENT_YELLOW))
        .shadow(egui::epaint::Shadow::NONE)
}

/// A card's text, laid out to wrap within the card on this screen.
fn card_galley(
    ctx: &egui::Context,
    screen: egui::Rect,
    text: &str,
) -> std::sync::Arc<egui::Galley> {
    let style = ctx.global_style();
    let frame = card_frame(&style);
    let width =
        CARD_TEXT_WIDTH.min(screen.width() - 2.0 * SCREEN_MARGIN - frame.total_margin().sum().x);
    let font = egui::TextStyle::Body.resolve(&style);
    let color = style.visuals.text_color();
    ctx.fonts_mut(|fonts| fonts.layout(text.to_owned(), font, color, width.max(40.0)))
}

/// A spotlight: everything but `anchor` dimmed, the note on a card beside it. Any click, which the
/// spotlight takes, or Escape (when `escape`) ends it.
fn draw_spotlight(
    ctx: &egui::Context,
    step: TutorialStep,
    anchor: egui::Rect,
    text: &str,
    opacity: f32,
    escape: bool,
) -> Vec<UiAction> {
    let screen = ctx.content_rect();
    let cutout = anchor.expand(CUTOUT_MARGIN).intersect(screen);
    let galley = card_galley(ctx, screen, text);
    let frame = card_frame(&ctx.global_style());
    let card = card_rect(
        screen,
        Placement::Beside(cutout),
        galley.size() + frame.total_margin().sum(),
    );
    let area = egui::Area::new(egui::Id::new("tutorial-spotlight"))
        .order(egui::Order::Tooltip)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let response = ui.allocate_rect(screen, egui::Sense::click());
            paint_dim(ui.painter(), screen, cutout, opacity);
            ui.painter().rect_stroke(
                cutout,
                CUTOUT_RADIUS,
                egui::Stroke::new(2.0, ACCENT_YELLOW.gamma_multiply(opacity)),
                egui::StrokeKind::Outside,
            );
            let mut card_ui = ui.new_child(egui::UiBuilder::new().max_rect(card));
            card_ui.multiply_opacity(opacity);
            frame.show(&mut card_ui, |ui| {
                ui.add(egui::Label::new(galley).selectable(false));
            });
            response.clicked()
        });
    ctx.move_to_top(area.response.layer_id);
    // Planning and Review sleep between events: keep the fades smooth and the clock running.
    ctx.request_repaint();
    let escaped = escape
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if area.inner || escaped {
        vec![UiAction::Tutorial(TutorialAction::Dismiss(step))]
    } else {
        Vec::new()
    }
}

/// Black at the spotlight's strength over all of `screen` but the rounded `cutout`: four bands
/// around it, and a fan filling each corner outside its rounding.
fn paint_dim(painter: &egui::Painter, screen: egui::Rect, cutout: egui::Rect, opacity: f32) {
    let color = egui::Color32::from_black_alpha((DIM_ALPHA * opacity * 255.0).round() as u8);
    let bands = [
        egui::Rect::from_min_max(screen.min, egui::pos2(screen.right(), cutout.top())),
        egui::Rect::from_min_max(egui::pos2(screen.left(), cutout.bottom()), screen.max),
        egui::Rect::from_min_max(
            egui::pos2(screen.left(), cutout.top()),
            egui::pos2(cutout.left(), cutout.bottom()),
        ),
        egui::Rect::from_min_max(
            egui::pos2(cutout.right(), cutout.top()),
            egui::pos2(screen.right(), cutout.bottom()),
        ),
    ];
    for band in bands.into_iter().filter(|band| band.is_positive()) {
        painter.rect_filled(band, 0.0, color);
    }
    let radius = CUTOUT_RADIUS
        .min(cutout.width() / 2.0)
        .min(cutout.height() / 2.0);
    if radius <= 0.0 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    // Each corner: the cut-out's sharp corner, and the centre of its rounding.
    let corners = [
        (cutout.left_top(), egui::vec2(1.0, 1.0)),
        (cutout.right_top(), egui::vec2(-1.0, 1.0)),
        (cutout.right_bottom(), egui::vec2(-1.0, -1.0)),
        (cutout.left_bottom(), egui::vec2(1.0, -1.0)),
    ];
    const STEPS: u32 = 6;
    for (corner, inward) in corners {
        let centre = corner + inward * radius;
        let tip = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, color);
        for i in 0..=STEPS {
            let angle = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
            let offset = egui::vec2(-inward.x * angle.cos(), -inward.y * angle.sin()) * radius;
            mesh.colored_vertex(centre + offset, color);
        }
        for i in 0..STEPS {
            mesh.add_triangle(tip, tip + 1 + i, tip + 2 + i);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The size of an instruction's card: its text in the card's frame.
fn instruction_size(ctx: &egui::Context, galley: &egui::Galley) -> egui::Vec2 {
    galley.size() + card_frame(&ctx.global_style()).total_margin().sum()
}

/// An instruction: its text on a card placed by `placement`. The card takes no clicks.
fn draw_instruction(ctx: &egui::Context, placement: Placement, text: &str) {
    let screen = ctx.content_rect();
    let galley = card_galley(ctx, screen, text);
    let frame = card_frame(&ctx.global_style());
    let size = instruction_size(ctx, &galley);
    let card = card_rect(screen, placement, size);
    egui::Area::new(egui::Id::new("tutorial-instruction"))
        .order(egui::Order::Tooltip)
        .fixed_pos(card.min)
        // Should the measure above ever fall short, egui keeps the card on screen.
        .constrain_to(screen.shrink(SCREEN_MARGIN))
        .interactable(false)
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(size.x - frame.total_margin().sum().x);
                ui.add(egui::Label::new(galley).selectable(false));
            });
        });
}

/// The space between an arrow callout's text and what it points at: the arrow's length.
const ARROW_GAP: f32 = 28.0;
/// An arrow's line width, and its head's length and half its width.
const ARROW_WIDTH: f32 = 2.0;
const ARROW_HEAD: f32 = 8.0;
const ARROW_HEAD_HALF_WIDTH: f32 = 5.0;
/// The narrowest line an arrow callout beside its target wraps to; with less room it goes below.
const CALLOUT_MIN_TEXT_WIDTH: f32 = 96.0;
/// The margin of an arrow callout's backing around its text.
const CALLOUT_PADDING: i8 = 4;

/// Where an arrow callout goes: its text's backing, and its arrow from `tail`, on the backing's
/// edge, to `tip`, on its target's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CalloutLayout {
    backing: egui::Rect,
    /// The width its text wraps at.
    wrap: f32,
    tail: egui::Pos2,
    tip: egui::Pos2,
}

/// Where an arrow callout pointing at `target` goes on `screen`, given `measure`, its text's size
/// wrapped at a width: to the target's right, past the arrow and centred on it, wrapping to the
/// room left before the screen's margin. With less room than a readable line there, it goes below
/// the target (above, if below doesn't fit), the arrow pointing up (down) at it. Always inside the
/// screen, never over the target.
fn callout_layout(
    screen: egui::Rect,
    target: egui::Rect,
    measure: impl Fn(f32) -> egui::Vec2,
) -> CalloutLayout {
    let bounds = screen.shrink(SCREEN_MARGIN);
    let padding = egui::Vec2::splat(2.0 * f32::from(CALLOUT_PADDING));
    let left = target.right() + ARROW_GAP;
    let room = bounds.right() - left - padding.x;
    if room >= CALLOUT_MIN_TEXT_WIDTH {
        let wrap = room.min(CARD_TEXT_WIDTH);
        let size = measure(wrap) + padding;
        // A word too long to wrap could still overflow; then it goes below.
        if left + size.x <= bounds.right() {
            let at = egui::pos2(left, target.center().y - size.y / 2.0);
            let backing = clamp_into(egui::Rect::from_min_size(at, size), bounds);
            // On the backing's edge and the target's (min/max, which can't panic as clamp can).
            let y = target
                .center()
                .y
                .max(backing.top())
                .min(backing.bottom())
                .max(target.top())
                .min(target.bottom());
            return CalloutLayout {
                backing,
                wrap,
                tail: egui::pos2(backing.left(), y),
                tip: egui::pos2(target.right(), y),
            };
        }
    }
    let wrap = (bounds.width() - padding.x).clamp(40.0, CARD_TEXT_WIDTH);
    let size = measure(wrap) + padding;
    let x = target.center().x - size.x / 2.0;
    let below = target.bottom() + ARROW_GAP;
    let above = below + size.y > bounds.bottom();
    let y = if above {
        target.top() - ARROW_GAP - size.y
    } else {
        below
    };
    let backing = clamp_into(egui::Rect::from_min_size(egui::pos2(x, y), size), bounds);
    let x = target
        .center()
        .x
        .max(backing.left())
        .min(backing.right())
        .max(target.left())
        .min(target.right());
    let (tail, tip) = if above {
        (backing.bottom(), target.top())
    } else {
        (backing.top(), target.bottom())
    };
    CalloutLayout {
        backing,
        wrap,
        tail: egui::pos2(x, tail),
        tip: egui::pos2(x, tip),
    }
}

/// An arrow callout's backing: the panels' own fill, opaque so its text meets the contrast floor
/// over anything, with no outline and no hover look, so it doesn't read as a control.
fn callout_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::NONE
        .fill(style.visuals.panel_fill)
        .inner_margin(CALLOUT_PADDING)
        .corner_radius(4)
}

/// An instruction as an arrow callout: its text beside `target` with an arrow, in the accent,
/// pointing at the target. Like the card, it takes no clicks.
fn draw_callout(ctx: &egui::Context, target: egui::Rect, text: &str) {
    let screen = ctx.content_rect();
    let style = ctx.global_style();
    let font = egui::TextStyle::Body.resolve(&style);
    let color = style.visuals.text_color();
    let galley =
        |wrap: f32| ctx.fonts_mut(|fonts| fonts.layout(text.to_owned(), font.clone(), color, wrap));
    let layout = callout_layout(screen, target, |wrap| galley(wrap).size());
    let galley = galley(layout.wrap);
    let frame = callout_frame(&style);
    egui::Area::new(egui::Id::new("tutorial-callout"))
        .order(egui::Order::Tooltip)
        .fixed_pos(layout.backing.min)
        .constrain_to(screen.shrink(SCREEN_MARGIN))
        .interactable(false)
        .show(ctx, |ui| {
            paint_arrow(
                &ui.painter().with_clip_rect(screen),
                layout.tail,
                layout.tip,
            );
            frame.show(ui, |ui| {
                ui.add(egui::Label::new(galley).selectable(false));
            });
        });
}

/// An arrow from `tail` to `tip`: a line, and a filled head whose point is `tip`.
fn paint_arrow(painter: &egui::Painter, tail: egui::Pos2, tip: egui::Pos2) {
    let along = (tip - tail).normalized();
    let base = tip - along * ARROW_HEAD;
    let across = along.rot90() * ARROW_HEAD_HALF_WIDTH;
    painter.line_segment([tail, base], egui::Stroke::new(ARROW_WIDTH, ACCENT_YELLOW));
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base + across, base - across],
        ACCENT_YELLOW,
        egui::Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(853.0, 427.0))
    }

    #[test]
    fn opacity_ramps_in_holds_and_ramps_out() {
        assert_eq!(spotlight_opacity(-1.0), 0.0);
        assert_eq!(spotlight_opacity(0.0), 0.0);
        assert!((spotlight_opacity(0.15) - 0.5).abs() < 1e-6);
        assert_eq!(spotlight_opacity(0.3), 1.0);
        assert_eq!(spotlight_opacity(4.3), 1.0);
        assert!((spotlight_opacity(4.45) - 0.5).abs() < 1e-4);
        assert_eq!(spotlight_opacity(SPOTLIGHT_SECONDS), 0.0);
        assert_eq!(spotlight_opacity(10.0), 0.0);
    }

    /// Planning sleeps between events, so a spotlight, whose clock runs while it is drawn, asks
    /// for the next frame itself: fading or holding, every pass, not only while egui is settling
    /// a new area.
    #[test]
    fn a_spotlight_keeps_asking_for_frames() {
        let anchor = egui::Rect::from_min_size(egui::pos2(300.0, 10.0), egui::vec2(80.0, 20.0));
        for opacity in [0.5, 1.0] {
            let ctx = egui::Context::default();
            let mut delay = None;
            // A second a pass, so egui's own fade-in of a new area finishes.
            for second in 0..5 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen()),
                        time: Some(f64::from(second)),
                        ..Default::default()
                    },
                    |ui| {
                        draw_spotlight(
                            ui.ctx(),
                            TutorialStep::Cash,
                            anchor,
                            "Your money.",
                            opacity,
                            true,
                        );
                    },
                );
                delay = Some(output.viewport_output[&egui::ViewportId::ROOT].repaint_delay);
                output.drop_without_applying_deltas();
            }
            assert_eq!(delay, Some(std::time::Duration::ZERO), "at {opacity}");
        }
    }

    #[test]
    fn a_card_goes_below_its_target_when_it_fits() {
        let target = egui::Rect::from_min_size(egui::pos2(300.0, 10.0), egui::vec2(80.0, 20.0));
        let card = card_rect(screen(), Placement::Beside(target), egui::vec2(200.0, 60.0));
        assert_eq!(card.top(), target.bottom() + CARD_GAP);
        assert_eq!(card.center().x, target.center().x);
        assert!(!card.intersects(target));
    }

    #[test]
    fn a_card_goes_above_then_beside_when_below_is_full() {
        let low = egui::Rect::from_min_size(egui::pos2(300.0, 380.0), egui::vec2(80.0, 20.0));
        let card = card_rect(screen(), Placement::Beside(low), egui::vec2(200.0, 60.0));
        assert_eq!(card.bottom(), low.top() - CARD_GAP);
        // A target as tall as the screen: the card goes to its right, or its left at the edge.
        let tall = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(200.0, 427.0));
        let card = card_rect(screen(), Placement::Beside(tall), egui::vec2(200.0, 60.0));
        assert_eq!(card.left(), tall.right() + CARD_GAP);
        let edge = egui::Rect::from_min_max(egui::pos2(700.0, 0.0), egui::pos2(840.0, 427.0));
        let card = card_rect(screen(), Placement::Beside(edge), egui::vec2(200.0, 60.0));
        assert_eq!(card.right(), edge.left() - CARD_GAP);
    }

    /// The card is drawn at the size it was measured at, so placing it by that size keeps it on
    /// screen.
    #[test]
    fn an_instruction_card_is_drawn_at_its_measured_size() {
        let ctx = egui::Context::default();
        ctx.set_visuals(crate::hud::theme::visuals());
        let text = "Click a square on the map to see what you can build there.";
        let mut measured = None;
        for _ in 0..2 {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen()),
                    ..Default::default()
                },
                |ui| {
                    let galley = card_galley(ui.ctx(), screen(), text);
                    measured = Some(instruction_size(ui.ctx(), &galley));
                    draw_instruction(ui.ctx(), Placement::TopOf(None), text);
                },
            )
            .drop_without_applying_deltas();
        }
        let drawn = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("tutorial-instruction")))
            .expect("the card was drawn");
        let measured = measured.unwrap();
        assert!(
            (drawn.size() - measured).length() < 0.5,
            "drawn {drawn:?}, measured {measured:?}"
        );
    }

    #[test]
    fn a_card_is_always_inside_the_screen() {
        let bounds = screen().shrink(SCREEN_MARGIN);
        let size = egui::vec2(300.0, 120.0);
        for target in [
            egui::Rect::from_min_size(egui::pos2(-50.0, -50.0), egui::vec2(10.0, 10.0)),
            egui::Rect::from_min_size(egui::pos2(840.0, 420.0), egui::vec2(30.0, 30.0)),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(853.0, 427.0)),
        ] {
            let card = card_rect(screen(), Placement::Beside(target), size);
            assert!(bounds.contains_rect(card), "{target:?}: {card:?}");
        }
        let map = egui::Rect::from_min_size(egui::pos2(268.0, 136.0), egui::vec2(277.0, 43.0));
        let card = card_rect(screen(), Placement::TopOf(Some(map)), size);
        assert!(bounds.contains_rect(card), "{card:?}");
        assert_eq!(card.center().x, map.center().x);
        let card = card_rect(screen(), Placement::TopOf(None), size);
        assert_eq!(card.top(), CARD_GAP.max(SCREEN_MARGIN));
        assert_eq!(card.center().x, screen().center().x);
    }

    /// A 150-point sentence, 18 points a line, wrapped at `width`.
    fn sentence(width: f32) -> egui::Vec2 {
        egui::vec2(150.0_f32.min(width), 18.0 * (150.0 / width).ceil())
    }

    fn padding() -> f32 {
        f32::from(CALLOUT_PADDING)
    }

    #[test]
    fn a_callout_goes_right_of_its_target_centred_on_it() {
        let target = egui::Rect::from_min_size(egui::pos2(600.0, 200.0), egui::vec2(80.0, 20.0));
        let layout = callout_layout(screen(), target, sentence);
        let y = target.center().y;
        assert_eq!(layout.backing.left(), target.right() + ARROW_GAP);
        assert_eq!(layout.backing.center().y, y);
        assert_eq!(layout.tip, egui::pos2(target.right(), y));
        assert_eq!(layout.tail, egui::pos2(layout.backing.left(), y));
        // It wraps to the room between the arrow and the screen's margin.
        let bounds = screen().shrink(SCREEN_MARGIN);
        assert!(layout.wrap >= CALLOUT_MIN_TEXT_WIDTH, "{layout:?}");
        assert_eq!(
            layout.wrap,
            bounds.right() - layout.backing.left() - 2.0 * padding()
        );
        assert_eq!(
            layout.backing.size(),
            sentence(layout.wrap) + egui::Vec2::splat(2.0 * padding())
        );
        assert!(bounds.contains_rect(layout.backing), "{layout:?}");
        // With room to spare it keeps to a card's width.
        let left = egui::Rect::from_min_size(egui::pos2(20.0, 200.0), egui::vec2(80.0, 20.0));
        assert_eq!(
            callout_layout(screen(), left, sentence).wrap,
            CARD_TEXT_WIDTH
        );
    }

    #[test]
    fn a_callout_without_room_on_the_right_goes_below_or_above() {
        let bounds = screen().shrink(SCREEN_MARGIN);
        // Less than the narrowest line between the arrow and the margin.
        let right = bounds.right() - ARROW_GAP - 2.0 * padding() - CALLOUT_MIN_TEXT_WIDTH + 1.0;
        let target =
            egui::Rect::from_min_max(egui::pos2(right - 80.0, 200.0), egui::pos2(right, 220.0));
        let layout = callout_layout(screen(), target, sentence);
        assert_eq!(layout.backing.top(), target.bottom() + ARROW_GAP);
        assert_eq!(
            layout.tail,
            egui::pos2(target.center().x, layout.backing.top())
        );
        assert_eq!(layout.tip, egui::pos2(target.center().x, target.bottom()));
        assert!(bounds.contains_rect(layout.backing), "{layout:?}");
        // Near the bottom, it goes above instead.
        let low = target.translate(egui::vec2(0.0, 380.0 - target.top()));
        let layout = callout_layout(screen(), low, sentence);
        assert_eq!(layout.backing.bottom(), low.top() - ARROW_GAP);
        assert_eq!(layout.tip, egui::pos2(low.center().x, low.top()));
        assert_eq!(
            layout.tail,
            egui::pos2(low.center().x, layout.backing.bottom())
        );
        assert!(bounds.contains_rect(layout.backing), "{layout:?}");
    }

    #[test]
    fn a_callout_is_on_screen_and_never_covers_its_target() {
        let bounds = screen().shrink(SCREEN_MARGIN);
        for target in [
            egui::Rect::from_min_size(egui::pos2(600.0, 0.0), egui::vec2(80.0, 20.0)),
            egui::Rect::from_min_size(egui::pos2(600.0, 410.0), egui::vec2(80.0, 17.0)),
            egui::Rect::from_min_size(egui::pos2(770.0, 10.0), egui::vec2(80.0, 20.0)),
            egui::Rect::from_min_size(egui::pos2(0.0, 200.0), egui::vec2(840.0, 20.0)),
            egui::Rect::from_min_size(egui::pos2(-40.0, 200.0), egui::vec2(80.0, 20.0)),
        ] {
            let layout = callout_layout(screen(), target, sentence);
            assert!(
                bounds.contains_rect(layout.backing),
                "{target:?}: {layout:?}"
            );
            assert!(!layout.backing.intersects(target), "{target:?}: {layout:?}");
            assert!(
                layout.backing.expand(0.01).contains(layout.tail),
                "{layout:?}"
            );
            assert!(
                target.expand(0.01).contains(layout.tip),
                "{target:?}: {layout:?}"
            );
        }
    }

    /// What one pass of `draw_callout` paints, after egui has settled the new area.
    fn paint_callout(target: egui::Rect) -> (egui::Context, Vec<egui::epaint::ClippedShape>) {
        let ctx = egui::Context::default();
        ctx.set_visuals(crate::hud::theme::visuals());
        let mut shapes = Vec::new();
        for second in 0..4 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen()),
                    time: Some(f64::from(second)),
                    ..Default::default()
                },
                |ui| draw_callout(ui.ctx(), target, "Build a battery here."),
            );
            shapes = output.shapes.clone();
            output.drop_without_applying_deltas();
        }
        (ctx, shapes)
    }

    /// The arrowhead's points, and the shaft's ends, in the accent.
    fn arrow(shapes: &[egui::epaint::ClippedShape]) -> (Vec<egui::Pos2>, [egui::Pos2; 2]) {
        let head = shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Path(path) if path.closed && path.fill == ACCENT_YELLOW => {
                    Some(path.points.clone())
                }
                _ => None,
            })
            .expect("an arrowhead");
        let shaft = shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { points, stroke } if stroke.color == ACCENT_YELLOW => {
                    assert_eq!(stroke.width, 2.0);
                    Some(*points)
                }
                _ => None,
            })
            .expect("an arrow's shaft");
        (head, shaft)
    }

    #[test]
    fn a_callout_points_its_arrow_at_its_target_and_takes_no_clicks() {
        let target = egui::Rect::from_min_size(egui::pos2(600.0, 200.0), egui::vec2(80.0, 20.0));
        let (ctx, shapes) = paint_callout(target);
        let (head, shaft) = arrow(&shapes);
        assert_eq!(head.len(), 3);
        // The head's tip touches the target's right edge, level with its centre, and points left.
        let tip = head
            .iter()
            .copied()
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        assert_eq!(tip, egui::pos2(target.right(), target.center().y));
        assert!(
            head.iter()
                .filter(|p| **p != tip)
                .all(|p| p.x - tip.x >= 7.5)
        );
        let note = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("tutorial-callout")))
            .expect("the callout was drawn");
        // The shaft runs from the text's backing to the head.
        let ends = [shaft[0].x.min(shaft[1].x), shaft[0].x.max(shaft[1].x)];
        assert_eq!(ends[1], note.left(), "{shaft:?} / {note:?}");
        assert!(ends[0] > tip.x && ends[0] < tip.x + 8.5, "{shaft:?}");
        // No outline: the backing is the panel's fill, with no stroke.
        let fill = crate::hud::theme::visuals().panel_fill;
        let backing: Vec<_> = shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) => Some(rect),
                _ => None,
            })
            .collect();
        assert_eq!(backing.len(), 1, "{backing:?}");
        assert_eq!(backing[0].rect, note);
        assert_eq!(backing[0].fill, fill);
        assert_eq!(backing[0].stroke.width, 0.0);
        // A click on it goes through to whatever is under it.
        let callout = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tutorial-callout"));
        assert_ne!(ctx.layer_id_at(note.center()), Some(callout));
        assert!(
            ctx.layer_id_at(note.center()).is_some(),
            "the layer under it"
        );
    }

    #[test]
    fn a_callout_below_its_target_points_up_at_it() {
        let target = egui::Rect::from_min_size(egui::pos2(760.0, 200.0), egui::vec2(80.0, 20.0));
        let (ctx, shapes) = paint_callout(target);
        let note = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("tutorial-callout")))
            .expect("the callout was drawn");
        assert_eq!(note.top(), target.bottom() + ARROW_GAP);
        let (head, _) = arrow(&shapes);
        let tip = head
            .iter()
            .copied()
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .unwrap();
        assert_eq!(tip, egui::pos2(target.center().x, target.bottom()));
        assert!(
            head.iter()
                .filter(|p| **p != tip)
                .all(|p| p.y - tip.y >= 7.5)
        );
    }
}
