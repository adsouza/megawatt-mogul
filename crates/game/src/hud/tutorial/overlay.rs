//! The tutorial's overlay (first-game tutorial spec §4.1): the crawl, spotlights and instructions,
//! drawn over every other HUD layer in [`HudSet::Tutorial`](crate::hud::HudSet::Tutorial).
//!
//! - [`tutorial_view`] is the pure presenter: what the active step shows, `shown_for` seconds after
//!   it first appeared.
//! - [`draw_spotlight`] and [`draw_callout`] (and [`super::crawl::draw_crawl`]) lay it out;
//!   the spotlight and the crawl return what was asked for.
//! - [`tutorial_overlay`] picks the active step each pass, keeps its clock, and hands the actions
//!   to [`dispatch`].
//!
//! A spotlight or the crawl waits while a prompt is open (prompts come first) or, for a
//! spotlight, while its anchor isn't drawn; its clock counts only the passes it is drawn on, so
//! one hidden part-way (or under the save screen, when the overlay doesn't run) pauses and picks
//! up where it left off. A spotlight whose anchor sits in a
//! scroll area (the battery card, the line section, the Review's Construction row) first asks for
//! it to be scrolled into view, so it never waits on the player to find it, and asks once more
//! (for each selection) if it is scrolled away part-way. A reveal no panel finished goes with its
//! step: when the active step changes the overlay drops it, so a later step's panel never jumps.
//!
//! While the crawl or a spotlight is up it is the active layer: it takes every click, Escape
//! goes to it rather than to a window under it (it is egui's top modal layer), and the keyboard
//! shortcuts wait (each pass it tells `dispatch` whether it covers the screen,
//! [`UiState::tutorial_cover`](crate::hud::UiState::tutorial_cover), which Space and the map's R
//! and O read through the `ActionSink`).
//!
//! An instruction is an arrow callout: its text on a plain backing, with an arrow from the text at
//! the control it asks for (or, for the map, down into the map from inside its top edge). It never
//! waits and never blocks input, not even on its own text; while a prompt is open it isn't drawn,
//! so it never covers the prompt. If its anchor isn't in view it asks once (for each selection)
//! for the anchor to be scrolled in, and until then it is the same text without an arrow at the
//! map's top.

use application::Game;
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::crawl::draw_crawl;
use super::{
    StepDef, StepKind, TutorialAnchor, TutorialState, TutorialStep, active_step, anchor_rect,
    construction_key,
};
use crate::hud::Selection;
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
    /// An arrow callout pointing at `anchor` that stays until its step is done.
    Instruction {
        anchor: TutorialAnchor,
        text: String,
    },
}

/// What `step` shows `shown_for` seconds after it first appeared.
pub fn tutorial_view(
    step: &StepDef,
    state: &TutorialState,
    game: &Game,
    selection: Selection,
    l10n: &Localizer,
    shown_for: f32,
) -> TutorialView {
    let text = || {
        let key = step
            .text_key
            .unwrap_or_else(|| construction_key(state, game));
        if matches!(
            step.step,
            TutorialStep::CcgtSpotlight | TutorialStep::BuildCcgt
        ) {
            let region = game.session().region();
            let kind = domain::region::PlantKind::Ccgt;
            let months = match selection {
                Selection::Cell(cell) => region
                    .plant_spec_at(kind, cell)
                    .map(|spec| spec.build_months)
                    .unwrap_or(region.spec(kind).build_months),
                _ => region.spec(kind).build_months,
            };
            let time = l10n.args("build-months-value", &[("months", months.into())]);
            l10n.args(key, &[("time", time.into())])
        } else {
            l10n.text(key)
        }
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

/// How long the active step's crawl or spotlight has been shown, by the real clock: only the time
/// between two passes it was drawn on, one frame apart, counts. So while it waits (for its anchor
/// or a prompt) its clock stands, and so it does while the overlay doesn't run at all (the save
/// screen is open over it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shown {
    step: TutorialStep,
    /// Seconds shown so far.
    shown_for: f64,
    /// When, and on which frame, it was last drawn, if it was on the pass before.
    last: Option<(f64, u32)>,
    /// The selection under which the step's anchor was last asked to scroll into view, if it was,
    /// and whether the step had been shown by then (a spotlight asks again once paused).
    revealed: Option<(Selection, bool)>,
}

impl Shown {
    fn new(step: TutorialStep) -> Self {
        Self {
            step,
            shown_for: 0.0,
            last: None,
            revealed: None,
        }
    }

    /// Moves the clock to `now`, a pass in `frame` on which the step is `showing` or not: the time
    /// since the pass before counts only if both showed it and that pass was in this frame or the
    /// one before (frames the overlay was skipped in don't count).
    fn tick(&mut self, now: f64, frame: u32, showing: bool) {
        if !showing {
            self.last = None;
            return;
        }
        if let Some((last, last_frame)) = self.last
            && frame.wrapping_sub(last_frame) <= 1
        {
            self.shown_for += (now - last).max(0.0);
        }
        self.last = Some((now, frame));
    }

    /// Whether it has been shown at all.
    fn started(&self) -> bool {
        self.last.is_some() || self.shown_for > 0.0
    }
}

/// Draws the active tutorial step over the HUD and hands what it asks for to [`dispatch`], with
/// whether the crawl or a spotlight now covers the screen.
pub(crate) fn tutorial_overlay(
    mut contexts: EguiContexts,
    time: Res<Time<Real>>,
    frame: Res<FrameCount>,
    l10n: Res<Localizer>,
    mut shown: Local<Option<Shown>>,
    mut active: Local<Option<TutorialStep>>,
    mut sink: ActionSink,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let now = time.elapsed_secs_f64();
    let uncover = UiAction::Tutorial(TutorialAction::Cover(false));
    let (Some(state), Some(game)) = (sink.tutorial(), sink.game()) else {
        // Ending the run dropped its reveals.
        *shown = None;
        *active = None;
        dispatch([uncover], &mut sink);
        return Ok(());
    };
    let def = active_step(state, game, sink.selection());
    let mut actions = Vec::new();
    // A reveal the step before asked for and no panel finished goes with it: the next step's
    // own asks come after.
    let step = def.map(|def| def.step);
    if *active != step {
        actions.push(UiAction::Tutorial(TutorialAction::StepChanged));
        *active = step;
    }
    let Some(def) = def else {
        *shown = None;
        actions.push(uncover);
        dispatch(actions, &mut sink);
        return Ok(());
    };
    let mut clock = match *shown {
        Some(clock) if clock.step == def.step => clock,
        _ => Shown::new(def.step),
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
    // An anchor in a scroll area is brought into view once for each selection (which changes what
    // the build menu shows): before its spotlight starts, once more when it is paused part-way with
    // its anchor out of view, or when an instruction's anchor isn't in view.
    let reveal = match def.kind {
        StepKind::Spotlight => !clock.started() || anchor.is_none(),
        StepKind::Instruction => anchor.is_none(),
        StepKind::Move | StepKind::Crawl => false,
    };
    let asked = (sink.selection(), clock.started());
    if reveal && clock.revealed != Some(asked) {
        if let Some(anchor) = def.anchor.filter(|anchor| scrolls(*anchor)) {
            actions.push(UiAction::Tutorial(TutorialAction::Reveal(anchor)));
        }
        clock.revealed = Some(asked);
    }
    clock.tick(now, frame.0, ready);
    let shown_for = clock.shown_for as f32;
    let view = tutorial_view(def, state, game, sink.selection(), &l10n, shown_for);
    let escape = !prompt_open && !egui::Popup::is_any_open(&ctx);
    // Whether the crawl or a spotlight is drawn and stays up: it takes every click, so the
    // keyboard shortcuts wait while it does.
    let mut cover = false;
    match view {
        TutorialView::Move(step) => actions.push(UiAction::Tutorial(TutorialAction::Play(step))),
        TutorialView::Crawl { .. } | TutorialView::Spotlight { .. } if !ready => {}
        TutorialView::Crawl { text, progress } => {
            if progress >= 1.0 {
                actions.push(UiAction::Tutorial(TutorialAction::Dismiss(def.step)));
            } else {
                let ended = draw_crawl(&ctx, def.step, &text, progress, escape);
                cover = ended.is_empty();
                actions.extend(ended);
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
                let ended = draw_spotlight(&ctx, step, anchor, &text, opacity, escape);
                cover = ended.is_empty();
                actions.extend(ended);
            }
        }
        // Prompts come first: an instruction steps aside while one is open.
        TutorialView::Instruction { .. } if prompt_open => {}
        TutorialView::Instruction { anchor: to, text } => {
            let map = anchor_rect(&ctx, TutorialAnchor::Map);
            let place = match (to, anchor) {
                // No one control: inside the map's top, pointing down into it.
                (TutorialAnchor::Map, _) => CalloutPlace::TopOf {
                    area: map,
                    arrow: map.is_some(),
                },
                (_, Some(target)) => CalloutPlace::Beside(target),
                // Its control isn't in view: the text alone, where the player is looking.
                (_, None) => CalloutPlace::TopOf {
                    area: map,
                    arrow: false,
                },
            };
            draw_callout(&ctx, place, &text);
        }
    }
    *shown = Some(clock);
    actions.push(UiAction::Tutorial(TutorialAction::Cover(cover)));
    dispatch(actions, &mut sink);
    Ok(())
}

/// Whether `anchor` is drawn inside a scroll area, so a reveal can bring it into view.
fn scrolls(anchor: TutorialAnchor) -> bool {
    matches!(
        anchor,
        TutorialAnchor::PlantCard
            | TutorialAnchor::PlantBuildButton
            | TutorialAnchor::LineSection
            | TutorialAnchor::PlantLineButton
            | TutorialAnchor::ReviewConstruction
    )
}

/// Where a spotlight's card of `size` goes on `screen`: beside `target` (below, above, right or
/// left, whichever fits first), always inside the screen.
fn card_rect(screen: egui::Rect, target: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let bounds = screen.shrink(SCREEN_MARGIN);
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
    clamp_into(egui::Rect::from_min_size(at, size), bounds)
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

/// A spotlight card's frame: the window fill of the shared palette, opaque, outlined in the accent.
fn card_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .stroke(egui::Stroke::new(1.5, ACCENT_YELLOW))
        .shadow(egui::epaint::Shadow::NONE)
}

/// A spotlight card's text, laid out to wrap within the card on this screen.
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
    let card = card_rect(screen, cutout, galley.size() + frame.total_margin().sum());
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
    claim_top(ctx, area.response.layer_id);
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

/// Makes `layer`, a spotlight's or the crawl's, the top layer and the active one: egui's top modal
/// layer, so a window drawn under it (Milestones, Links) leaves Escape to it (`action::cancelled`
/// defers to the top modal layer) and nothing under it takes input. egui reports the modal layer
/// from the pass before, so this holds from the pass after the overlay first shows: a key pressed
/// on that first pass was pressed before the player saw it.
pub(super) fn claim_top(ctx: &egui::Context, layer: egui::LayerId) {
    ctx.move_to_top(layer);
    ctx.memory_mut(|memory| memory.set_modal_layer(layer));
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

/// Where a callout goes: its text's backing, and its arrow, if it has one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CalloutLayout {
    backing: egui::Rect,
    /// The width its text wraps at.
    wrap: f32,
    arrow: Option<Arrow>,
}

/// An arrow from `tail`, on a callout's backing, to `tip`, on (or in) what it points at.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Arrow {
    tail: egui::Pos2,
    tip: egui::Pos2,
}

/// Where a callout is placed.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CalloutPlace {
    /// Beside this target, pointing at it ([`callout_layout`]).
    Beside(egui::Rect),
    /// Inside the top of this area (the map's; the screen's if there is none), pointing down
    /// into it when `arrow` ([`top_callout_layout`]).
    TopOf {
        area: Option<egui::Rect>,
        arrow: bool,
    },
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
                arrow: Some(Arrow {
                    tail: egui::pos2(backing.left(), y),
                    tip: egui::pos2(target.right(), y),
                }),
            };
        }
    }
    vertical_callout_layout(bounds, target, measure)
}

/// An arrow callout below `target` (above, if below doesn't fit in `bounds`), centred on it, its
/// arrow pointing up (down) at the target's edge. Inside `bounds`, never over the target.
fn vertical_callout_layout(
    bounds: egui::Rect,
    target: egui::Rect,
    measure: impl Fn(f32) -> egui::Vec2,
) -> CalloutLayout {
    let padding = egui::Vec2::splat(2.0 * f32::from(CALLOUT_PADDING));
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
        arrow: Some(Arrow {
            tail: egui::pos2(x, tail),
            tip: egui::pos2(x, tip),
        }),
    }
}

/// Where a callout goes inside the top of `area` (the screen, if `None`) on `screen`, given
/// `measure` as for [`callout_layout`]: centred on the area, its backing a card's gap below the
/// area's top edge, so it keeps clear of the area's middle. With `arrow`, an arrow runs from the
/// backing's bottom straight down into the area; an area too short to hold the text and the arrow
/// (the map at the smallest window and largest UI zoom) gets the callout just below it instead,
/// its arrow pointing up at the area's edge, as [`callout_layout`] does when there is no room on
/// the right. Always inside the screen.
fn top_callout_layout(
    screen: egui::Rect,
    area: Option<egui::Rect>,
    measure: impl Fn(f32) -> egui::Vec2,
    arrow: bool,
) -> CalloutLayout {
    let bounds = screen.shrink(SCREEN_MARGIN);
    let area = area.unwrap_or(screen);
    let padding = egui::Vec2::splat(2.0 * f32::from(CALLOUT_PADDING));
    let room = area.width().min(bounds.width()) - 2.0 * CARD_GAP - padding.x;
    let wrap = room.clamp(40.0, CARD_TEXT_WIDTH);
    let size = measure(wrap) + padding;
    let at = egui::pos2(area.center().x - size.x / 2.0, area.top() + CARD_GAP);
    let backing = clamp_into(egui::Rect::from_min_size(at, size), bounds);
    if !arrow {
        return CalloutLayout {
            backing,
            wrap,
            arrow: None,
        };
    }
    let tip = backing.bottom() + ARROW_GAP;
    if tip > area.bottom() {
        return vertical_callout_layout(bounds, area, measure);
    }
    let x = area.center().x.max(backing.left()).min(backing.right());
    CalloutLayout {
        backing,
        wrap,
        arrow: Some(Arrow {
            tail: egui::pos2(x, backing.bottom()),
            tip: egui::pos2(x, tip),
        }),
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

/// An instruction as a callout: its text placed by `place`, with an arrow, in the accent, from the
/// text at what it points at. It takes no clicks.
fn draw_callout(ctx: &egui::Context, place: CalloutPlace, text: &str) {
    let screen = ctx.content_rect();
    let style = ctx.global_style();
    let font = egui::TextStyle::Body.resolve(&style);
    let color = style.visuals.text_color();
    let galley =
        |wrap: f32| ctx.fonts_mut(|fonts| fonts.layout(text.to_owned(), font.clone(), color, wrap));
    let measure = |wrap| galley(wrap).size();
    let layout = match place {
        CalloutPlace::Beside(target) => callout_layout(screen, target, measure),
        CalloutPlace::TopOf { area, arrow } => top_callout_layout(screen, area, measure, arrow),
    };
    let galley = galley(layout.wrap);
    let frame = callout_frame(&style);
    egui::Area::new(egui::Id::new("tutorial-callout"))
        .order(egui::Order::Tooltip)
        .fixed_pos(layout.backing.min)
        .constrain_to(screen.shrink(SCREEN_MARGIN))
        .interactable(false)
        .show(ctx, |ui| {
            if let Some(Arrow { tail, tip }) = layout.arrow {
                paint_arrow(&ui.painter().with_clip_rect(screen), tail, tip);
            }
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

    /// The clock counts the time between passes that both showed the step, so a step hidden
    /// part-way (its anchor gone, a prompt open) stands still and picks up where it left off.
    #[test]
    fn the_clock_counts_only_passes_that_show_the_step() {
        let mut clock = Shown::new(TutorialStep::Cash);
        // Waiting to be shown: no time passes, however long.
        clock.tick(0.0, 0, false);
        clock.tick(5.0, 1, false);
        assert!(!clock.started());
        assert_eq!(clock.shown_for, 0.0);
        clock.tick(10.0, 2, true);
        assert!(clock.started());
        assert_eq!(clock.shown_for, 0.0);
        clock.tick(12.0, 3, true);
        assert_eq!(clock.shown_for, 2.0);
        // Hidden for ten seconds of passes.
        for second in 13..=22 {
            clock.tick(f64::from(second), second - 9, false);
        }
        assert_eq!(clock.shown_for, 2.0);
        assert!(clock.started());
        // Shown again: the gap doesn't count, the passes after it do.
        clock.tick(23.0, 14, true);
        assert_eq!(clock.shown_for, 2.0);
        clock.tick(25.5, 15, true);
        assert_eq!(clock.shown_for, 4.5);
        assert!(spotlight_opacity(clock.shown_for as f32) > 0.0);
        clock.tick(25.7, 16, true);
        assert!(clock.shown_for as f32 >= SPOTLIGHT_SECONDS);
    }

    /// Frames the overlay didn't run in (the save screen open over it) don't count either: only
    /// the time from a pass one frame after the last shown one does, across the frame counter's
    /// wrap too; and two passes in one frame count the time between them.
    #[test]
    fn the_clock_skips_frames_the_overlay_did_not_run_in() {
        let mut clock = Shown::new(TutorialStep::Cash);
        clock.tick(0.0, 10, true);
        clock.tick(1.0, 11, true);
        assert_eq!(clock.shown_for, 1.0);
        // Skipped for frames 12..=20 and ten seconds.
        clock.tick(11.0, 21, true);
        assert_eq!(clock.shown_for, 1.0);
        assert!(clock.started());
        clock.tick(11.5, 22, true);
        assert_eq!(clock.shown_for, 1.5);
        clock.tick(11.75, 22, true);
        assert_eq!(clock.shown_for, 1.75);
        let mut clock = Shown::new(TutorialStep::Cash);
        clock.tick(0.0, u32::MAX, true);
        clock.tick(0.5, 0, true);
        assert_eq!(clock.shown_for, 0.5);
    }

    #[test]
    fn a_card_goes_below_its_target_when_it_fits() {
        let target = egui::Rect::from_min_size(egui::pos2(300.0, 10.0), egui::vec2(80.0, 20.0));
        let card = card_rect(screen(), target, egui::vec2(200.0, 60.0));
        assert_eq!(card.top(), target.bottom() + CARD_GAP);
        assert_eq!(card.center().x, target.center().x);
        assert!(!card.intersects(target));
    }

    #[test]
    fn a_card_goes_above_then_beside_when_below_is_full() {
        let low = egui::Rect::from_min_size(egui::pos2(300.0, 380.0), egui::vec2(80.0, 20.0));
        let card = card_rect(screen(), low, egui::vec2(200.0, 60.0));
        assert_eq!(card.bottom(), low.top() - CARD_GAP);
        // A target as tall as the screen: the card goes to its right, or its left at the edge.
        let tall = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(200.0, 427.0));
        let card = card_rect(screen(), tall, egui::vec2(200.0, 60.0));
        assert_eq!(card.left(), tall.right() + CARD_GAP);
        let edge = egui::Rect::from_min_max(egui::pos2(700.0, 0.0), egui::pos2(840.0, 427.0));
        let card = card_rect(screen(), edge, egui::vec2(200.0, 60.0));
        assert_eq!(card.right(), edge.left() - CARD_GAP);
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
            let card = card_rect(screen(), target, size);
            assert!(bounds.contains_rect(card), "{target:?}: {card:?}");
        }
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
        let arrow = layout.arrow.expect("an arrow");
        assert_eq!(arrow.tip, egui::pos2(target.right(), y));
        assert_eq!(arrow.tail, egui::pos2(layout.backing.left(), y));
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
        let arrow = layout.arrow.expect("an arrow");
        assert_eq!(
            arrow.tail,
            egui::pos2(target.center().x, layout.backing.top())
        );
        assert_eq!(arrow.tip, egui::pos2(target.center().x, target.bottom()));
        assert!(bounds.contains_rect(layout.backing), "{layout:?}");
        // Near the bottom, it goes above instead.
        let low = target.translate(egui::vec2(0.0, 380.0 - target.top()));
        let layout = callout_layout(screen(), low, sentence);
        assert_eq!(layout.backing.bottom(), low.top() - ARROW_GAP);
        let arrow = layout.arrow.expect("an arrow");
        assert_eq!(arrow.tip, egui::pos2(low.center().x, low.top()));
        assert_eq!(
            arrow.tail,
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
            let arrow = layout.arrow.expect("an arrow");
            assert!(
                layout.backing.expand(0.01).contains(arrow.tail),
                "{layout:?}"
            );
            assert!(
                target.expand(0.01).contains(arrow.tip),
                "{target:?}: {layout:?}"
            );
        }
    }

    /// Picking a site: centred inside the map's top, the arrow running down from the backing's
    /// bottom into the map. Without the arrow, or without a map, it is the same text alone.
    #[test]
    fn a_top_callout_sits_inside_the_map_top_and_points_down_into_it() {
        let map = egui::Rect::from_min_max(egui::pos2(268.0, 73.0), egui::pos2(700.0, 392.0));
        let layout = top_callout_layout(screen(), Some(map), sentence, true);
        assert_eq!(layout.backing.top(), map.top() + CARD_GAP);
        assert_eq!(layout.backing.center().x, map.center().x);
        assert!(map.contains_rect(layout.backing), "{layout:?}");
        assert_eq!(
            layout.backing.size(),
            sentence(layout.wrap) + egui::Vec2::splat(2.0 * padding())
        );
        let arrow = layout.arrow.expect("an arrow");
        assert_eq!(
            arrow.tail,
            egui::pos2(map.center().x, layout.backing.bottom())
        );
        assert_eq!(
            arrow.tip,
            egui::pos2(map.center().x, layout.backing.bottom() + ARROW_GAP)
        );
        // It stays near the top: well clear of the map's centre.
        assert!(arrow.tip.y < map.center().y, "{layout:?}");
        // The same place without an arrow.
        let plain = top_callout_layout(screen(), Some(map), sentence, false);
        assert_eq!(plain.backing, layout.backing);
        assert_eq!(plain.arrow, None);
        // A narrow map wraps the text within it.
        let narrow = egui::Rect::from_min_max(egui::pos2(268.0, 73.0), egui::pos2(400.0, 392.0));
        let layout = top_callout_layout(screen(), Some(narrow), sentence, true);
        assert!(narrow.contains_rect(layout.backing), "{layout:?}");
        // No map: the screen's top centre, inside its margin.
        let layout = top_callout_layout(screen(), None, sentence, false);
        assert_eq!(layout.backing.top(), CARD_GAP);
        assert_eq!(layout.backing.center().x, screen().center().x);
    }

    /// A map too short to hold the text and the arrow below it (the smallest window at the largest
    /// UI zoom leaves a strip) gets the callout just below the map, pointing up at its edge.
    #[test]
    fn a_top_callout_on_a_short_map_points_up_at_it_from_below() {
        let bounds = screen().shrink(SCREEN_MARGIN);
        let short = egui::Rect::from_min_size(egui::pos2(268.0, 136.0), egui::vec2(277.0, 43.0));
        let layout = top_callout_layout(screen(), Some(short), sentence, true);
        assert!(bounds.contains_rect(layout.backing), "{layout:?}");
        assert!(!layout.backing.intersects(short), "{layout:?}");
        assert_eq!(layout.backing.top(), short.bottom() + ARROW_GAP);
        assert_eq!(layout.backing.center().x, short.center().x);
        let arrow = layout.arrow.expect("an arrow");
        assert_eq!(arrow.tip, egui::pos2(short.center().x, short.bottom()));
        assert_eq!(
            arrow.tail,
            egui::pos2(short.center().x, layout.backing.top())
        );
        // Just tall enough: the arrow's tip on the map's bottom edge, from inside.
        let text = sentence(top_callout_layout(screen(), Some(short), sentence, false).wrap);
        let height = CARD_GAP + text.y + 2.0 * padding() + ARROW_GAP;
        let fits = egui::Rect::from_min_size(short.min, egui::vec2(277.0, height));
        let layout = top_callout_layout(screen(), Some(fits), sentence, true);
        assert!(fits.contains_rect(layout.backing), "{layout:?}");
        assert_eq!(layout.arrow.expect("an arrow").tip.y, fits.bottom());
        // Without an arrow, the text stays inside the top, however short the map.
        let plain = top_callout_layout(screen(), Some(short), sentence, false);
        assert_eq!(plain.backing.top(), short.top() + CARD_GAP);
        assert_eq!(plain.arrow, None);
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
                |ui| {
                    draw_callout(
                        ui.ctx(),
                        CalloutPlace::Beside(target),
                        "Build a battery here.",
                    )
                },
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
