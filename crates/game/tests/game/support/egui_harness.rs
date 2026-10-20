//! A headless egui harness for the HUD's draw functions: no window, no renderer.
//!
//! Each frame is one `Context::run_ui` pass. Widgets are read back from egui's AccessKit tree
//! (role, text, rect, disabled), and a click is a real pointer press and release at a button's
//! centre, so a test only passes if the button is really there and really clickable.
//!
//! Wrap the draw function in the container its system uses ([`central`], `egui::Panel::top`,
//! `egui::Modal`, ...) so the test sees the real layout.

use bevy_egui::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, accesskit};
use game::hud::UiAction;

/// A widget as AccessKit describes it.
#[derive(Debug)]
pub struct Widget {
    pub role: accesskit::Role,
    /// The label of a button, or the value of a label.
    pub text: String,
    pub rect: Rect,
    pub disabled: bool,
}

pub struct Harness {
    ctx: egui::Context,
}

impl Harness {
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self { ctx }
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            events,
            ..RawInput::default()
        }
    }

    /// Runs one egui pass with `events`: the actions drawn, and every widget on screen.
    fn frame(
        &self,
        events: Vec<Event>,
        draw: &mut dyn FnMut(&mut egui::Ui) -> Vec<UiAction>,
    ) -> (Vec<UiAction>, Vec<Widget>) {
        let mut actions = Vec::new();
        let output = self
            .ctx
            .run_ui(Self::input(events), |ui| actions.extend(draw(ui)));
        let widgets = output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let bounds = node.bounds()?;
                        let text = node.label().or(node.value())?.to_owned();
                        Some(Widget {
                            role: node.role(),
                            text,
                            rect: Rect::from_min_max(
                                Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                                Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                            ),
                            disabled: node.is_disabled(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        // There is no renderer to take the font texture.
        output.drop_without_applying_deltas();
        (actions, widgets)
    }

    /// Draws once, with no input: the widgets on screen. Nothing may be asked for unprompted.
    pub fn draw(&self, mut draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>) -> Vec<Widget> {
        let (actions, widgets) = self.frame(Vec::new(), &mut draw);
        assert_eq!(actions, vec![], "an unclicked screen asked for something");
        widgets
    }

    /// Clicks the only button labelled `label`: the actions that produced. Panics if there are
    /// several (use [`Self::click_nth`]) or none.
    pub fn click(
        &self,
        label: &str,
        draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>,
    ) -> Vec<UiAction> {
        self.click_where(label, None, draw)
    }

    /// Clicks the `n`th (from 0, in drawing order) of the buttons labelled `label`.
    pub fn click_nth(
        &self,
        label: &str,
        n: usize,
        draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>,
    ) -> Vec<UiAction> {
        self.click_where(label, Some(n), draw)
    }

    /// Draws once to find the button, then presses and releases the primary pointer button on
    /// it: the actions of those two frames.
    fn click_where(
        &self,
        label: &str,
        n: Option<usize>,
        mut draw: impl FnMut(&mut egui::Ui) -> Vec<UiAction>,
    ) -> Vec<UiAction> {
        let widgets = self.draw(&mut draw);
        let target = match n {
            None => button(&widgets, label),
            Some(n) => nth_button(&widgets, label, n),
        };
        let at = target.rect.center();
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut actions = self
            .frame(vec![Event::PointerMoved(at), press(true)], &mut draw)
            .0;
        actions.extend(self.frame(vec![press(false)], &mut draw).0);
        actions
    }
}

/// The buttons labelled `label`, in drawing order.
pub fn buttons<'a>(widgets: &'a [Widget], label: &str) -> Vec<&'a Widget> {
    let mut found: Vec<&Widget> = widgets
        .iter()
        .filter(|widget| widget.role == accesskit::Role::Button && widget.text == label)
        .collect();
    // AccessKit lists nodes in no promised order; drawing order is top to bottom, left to right.
    found.sort_by(|a, b| {
        (a.rect.top(), a.rect.left())
            .partial_cmp(&(b.rect.top(), b.rect.left()))
            .expect("widget rects are finite")
    });
    found
}

/// The one button labelled `label`.
pub fn button<'a>(widgets: &'a [Widget], label: &str) -> &'a Widget {
    match buttons(widgets, label).as_slice() {
        [only] => only,
        [] => panic!("no button {label:?} among {widgets:#?}"),
        several => panic!(
            "{} buttons are labelled {label:?}; use click_nth",
            several.len()
        ),
    }
}

/// The `n`th (from 0, in drawing order) button labelled `label`.
pub fn nth_button<'a>(widgets: &'a [Widget], label: &str, n: usize) -> &'a Widget {
    buttons(widgets, label)
        .get(n)
        .copied()
        .unwrap_or_else(|| panic!("no button {label:?} number {n} among {widgets:#?}"))
}

/// Whether any widget's text contains `text`.
pub fn shows(widgets: &[Widget], text: &str) -> bool {
    widgets.iter().any(|widget| widget.text.contains(text))
}

/// `draw` inside an `egui::CentralPanel`, as the whole-screen systems draw it.
pub fn central(
    ui: &mut egui::Ui,
    draw: impl FnOnce(&mut egui::Ui) -> Vec<UiAction>,
) -> Vec<UiAction> {
    egui::CentralPanel::default().show(ui, draw).inner
}
