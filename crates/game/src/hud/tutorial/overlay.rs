//! The tutorial's overlay (first-game tutorial spec §4.1): the crawl, spotlights and instructions,
//! drawn over every other HUD layer in [`HudSet::Tutorial`](crate::hud::HudSet::Tutorial).
//!
//! - [`tutorial_view`] is the pure presenter: what the active step shows, `shown_for` seconds after
//!   it first appeared.
//! - [`draw_spotlight`] and [`draw_instruction`] (and [`super::crawl::draw_crawl`]) lay it out and
//!   return what was asked for.
//! - [`tutorial_overlay`] picks the active step each pass, keeps its clock, and hands the actions
//!   to [`dispatch`].
//!
//! A spotlight or the crawl waits, its clock not started, while a prompt is open (prompts come
//! first) or, for a spotlight, while its anchor isn't drawn. A spotlight whose anchor sits in a
//! scroll area (the battery card, the line section, the Review's Construction row) first asks for
//! it to be scrolled into view, so it never waits on the player to find it. An instruction never
//! waits and never blocks input outside its own card; while a prompt is open it lets every click
//! through.

use application::Game;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::crawl::draw_crawl;
use super::{
    StepDef, StepKind, TutorialAnchor, TutorialState, TutorialStep, active_step, anchor_rect,
    construction_key,
};
use crate::hud::action::{ActionSink, ButtonModel, TutorialAction, UiAction, dispatch};
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
    /// A callout that stays until its step is done, with a link that ends the tutorial.
    Instruction {
        anchor: TutorialAnchor,
        text: String,
        end: ButtonModel,
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
            end: ButtonModel::new(
                l10n.text("tutorial-end"),
                UiAction::Tutorial(TutorialAction::End),
            ),
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
    if def.kind == StepKind::Spotlight && clock.since.is_none() && !clock.revealed {
        // An anchor in a scroll area is brought into view before its spotlight starts.
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
        TutorialView::Instruction { anchor, text, end } => {
            let target = anchor_rect(&ctx, anchor);
            let map = anchor_rect(&ctx, TutorialAnchor::Map);
            let placement = match (anchor, target) {
                (TutorialAnchor::Map, _) | (_, None) => Placement::TopOf(map),
                (_, Some(target)) => Placement::Beside(target),
            };
            actions.extend(draw_instruction(
                &ctx,
                placement,
                &text,
                (!prompt_open).then_some(&end),
            ));
        }
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

/// The size of an instruction's card: its text and, below, the End link if there is one.
fn instruction_size(
    ctx: &egui::Context,
    galley: &egui::Galley,
    end: Option<&ButtonModel>,
) -> egui::Vec2 {
    let style = ctx.global_style();
    let frame = card_frame(&style);
    let mut size = galley.size() + frame.total_margin().sum();
    if let Some(end) = end {
        // The link is a frameless button: its text, padded, at least the interaction height.
        let font = egui::TextStyle::Body.resolve(&style);
        let text = ctx.fonts_mut(|fonts| {
            fonts.layout_no_wrap(end.label.clone(), font, style.visuals.hyperlink_color)
        });
        let link = (text.size() + 2.0 * style.spacing.button_padding)
            .max(egui::vec2(0.0, style.spacing.interact_size.y));
        size.y += style.spacing.item_spacing.y + link.y;
        size.x = size.x.max(link.x + frame.total_margin().sum().x);
    }
    size
}

/// An instruction: its text and, when `end` is given, the End tutorial link, on a card placed by
/// `placement`. The card takes only clicks on itself; without `end` it takes none.
fn draw_instruction(
    ctx: &egui::Context,
    placement: Placement,
    text: &str,
    end: Option<&ButtonModel>,
) -> Vec<UiAction> {
    let screen = ctx.content_rect();
    let galley = card_galley(ctx, screen, text);
    let frame = card_frame(&ctx.global_style());
    let size = instruction_size(ctx, &galley, end);
    let card = card_rect(screen, placement, size);
    egui::Area::new(egui::Id::new("tutorial-instruction"))
        .order(egui::Order::Tooltip)
        .fixed_pos(card.min)
        // Should the measure above ever fall short, egui keeps the card on screen.
        .constrain_to(screen.shrink(SCREEN_MARGIN))
        .interactable(end.is_some())
        .show(ctx, |ui| {
            let mut actions = Vec::new();
            frame.show(ui, |ui| {
                ui.set_width(size.x - frame.total_margin().sum().x);
                ui.add(egui::Label::new(galley).selectable(false));
                if let Some(end) = end {
                    // A link's look, but a button to assistive technology (egui's own link
                    // reports itself as a label).
                    let text = egui::RichText::new(end.label.as_str())
                        .color(ui.visuals().hyperlink_color)
                        .underline();
                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        actions.push(end.action.clone());
                    }
                }
            });
            actions
        })
        .inner
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
        let end = ButtonModel::new(
            "End tutorial".into(),
            UiAction::Tutorial(TutorialAction::End),
        );
        for link in [Some(&end), None] {
            let mut measured = None;
            for _ in 0..2 {
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen()),
                        ..Default::default()
                    },
                    |ui| {
                        let galley = card_galley(ui.ctx(), screen(), text);
                        measured = Some(instruction_size(ui.ctx(), &galley, link));
                        draw_instruction(ui.ctx(), Placement::TopOf(None), text, link);
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
                "{:?}: drawn {drawn:?}, measured {measured:?}",
                link.map(|_| "with the link")
            );
        }
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
}
