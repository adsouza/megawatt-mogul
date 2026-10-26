use application::RegionCatalog;
use bevy_egui::egui::{self, Pos2, Rect};
use domain::ids::RegionId;
use game::hud::{
    MenuForm, RegionAtlas, RegionListings, UiAction, draw_region_select, region_select_model,
};
use infrastructure::region::RonRegionCatalog;
use std::cell::RefCell;

use super::support::{
    egui_harness::{Harness, central, widget},
    shipped_assets, shipped_l10n,
};

fn model(seed: Option<u64>) -> game::hud::RegionSelectModel {
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
    let listings = RegionListings(catalog.list());
    region_select_model(&listings, seed, &shipped_l10n())
        .with_atlas(&RegionAtlas::load(&catalog, &listings), &shipped_l10n())
}

// Independent orthographic equations at the rendered-image boundary. Initial view faces 100°W.
fn point(rect: Rect, lon: f32, lat: f32) -> Pos2 {
    point_at(rect, lon, lat, -100.0)
}
fn point_at(rect: Rect, lon: f32, lat: f32, centre: f32) -> Pos2 {
    let longitude = (lon - centre).to_radians();
    let latitude = lat.to_radians();
    let tilt = 22.0f32.to_radians();
    let x = latitude.cos() * longitude.sin();
    let y = latitude.sin() * tilt.cos() - latitude.cos() * longitude.cos() * tilt.sin();
    rect.center() + egui::vec2(x, -y) * rect.width() * 0.5
}

#[test]
fn invalid_seed_edits_disable_cards_and_map_without_using_the_previous_value() {
    let model = model(Some(42));
    let form = RefCell::new(MenuForm {
        seed: "invalid".into(),
    });
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        central(ui, |ui| {
            draw_region_select(ui, &model, &mut form.borrow_mut())
        })
    };
    let widgets = harness.draw(draw);
    assert!(
        widgets
            .iter()
            .any(|w| w.text.contains("seed must be a whole number"))
    );
    for name in ["Eastern Interconnection", "Western Interconnection"] {
        assert!(widget(&widgets, name).disabled);
        assert!(harness.click(name, draw).is_empty());
    }
    let map = widget(&widgets, "World power grids").rect;
    assert!(harness.click_at(point(map, -83.0, 40.0), draw).is_empty());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn region_selection_uses_the_edited_seed_instead_of_the_presenters_old_value(seed in proptest::prelude::any::<u64>()) {
        let model = model(Some(42));
        let form = RefCell::new(MenuForm::new(seed));
        let harness = Harness::new();
        let actions = harness.click("Eastern Interconnection", |ui| central(ui, |ui| draw_region_select(ui, &model, &mut form.borrow_mut())));
        proptest::prop_assert_eq!(actions, vec![UiAction::NewGame { region: RegionId::new("eastern_interconnection"), seed }]);
    }
}

#[test]
fn overview_clicks_start_the_right_region_with_the_entered_seed() {
    let model = model(Some(42));
    let form = RefCell::new(MenuForm::new(42));
    for (lon, lat, region) in [
        (-122.33, 47.61, "western_interconnection"),
        (-83.0, 40.0, "eastern_interconnection"),
    ] {
        let harness = Harness::with_screen(1024.0, 640.0);
        let draw = |ui: &mut egui::Ui| {
            central(ui, |ui| {
                draw_region_select(ui, &model, &mut form.borrow_mut())
            })
        };
        let widgets = harness.draw(draw);
        let map = widget(&widgets, "World power grids").rect;
        assert_eq!(
            harness.click_at(point(map, lon, lat), draw),
            vec![UiAction::NewGame {
                region: RegionId::new(region),
                seed: 42
            }]
        );
        assert!(
            harness
                .screen_rect()
                .contains_rect(widget(&widgets, "Back").rect)
        );
    }
}

#[test]
fn overview_does_not_select_ocean_ercot_quebec_or_disabled_regions() {
    for seed in [Some(42), None] {
        let model = model(seed);
        let form = RefCell::new(MenuForm {
            seed: seed.map(|s| s.to_string()).unwrap_or_default(),
        });
        let harness = Harness::new();
        let draw = |ui: &mut egui::Ui| {
            central(ui, |ui| {
                draw_region_select(ui, &model, &mut form.borrow_mut())
            })
        };
        let map = widget(&harness.draw(draw), "World power grids").rect;
        for (lon, lat) in [(-140.0, 35.0), (-97.74, 30.27), (-71.2, 46.8)] {
            assert!(harness.click_at(point(map, lon, lat), draw).is_empty());
        }
        if seed.is_none() {
            assert!(harness.click_at(point(map, -83.0, 40.0), draw).is_empty());
        }
    }
}

#[test]
fn region_cards_remain_keyboard_accessible() {
    let model = model(Some(7));
    let form = RefCell::new(MenuForm::new(7));
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        central(ui, |ui| {
            draw_region_select(ui, &model, &mut form.borrow_mut())
        })
    };
    harness.draw(draw);
    harness.press_key(egui::Key::Tab, draw);
    harness.press_key(egui::Key::Tab, draw);
    let actions = harness.press_key(egui::Key::Enter, draw);
    assert!(matches!(
        actions.as_slice(),
        [UiAction::NewGame { seed: 7, .. }]
    ));
}

#[test]
fn hovering_the_map_highlights_a_region_without_starting_a_game() {
    let model = model(Some(42));
    let form = RefCell::new(MenuForm::new(42));
    let harness = Harness::new();
    let rect = widget(
        &harness.draw(|ui| {
            central(ui, |ui| {
                draw_region_select(ui, &model, &mut form.borrow_mut())
            })
        }),
        "World power grids",
    )
    .rect;
    let ctx = egui::Context::default();
    let paint = |events| {
        let mut actions = Vec::new();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(harness.screen_rect()),
                events,
                ..Default::default()
            },
            |ui| {
                actions.extend(central(ui, |ui| {
                    draw_region_select(ui, &model, &mut form.borrow_mut())
                }))
            },
        );
        assert!(actions.is_empty());
        output
    };
    let initial = paint(vec![]);
    let highlighted = egui::Color32::from_rgb(228, 131, 47);
    // Authored atlas colours stay saturated; hover adds the active card outline.
    let initially_active = initial.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.width == 2.0 && rect.stroke.color == highlighted));
    initial.drop_without_applying_deltas();
    assert!(!initially_active);
    let output = paint(vec![egui::Event::PointerMoved(point(rect, -83.0, 40.0))]);
    let cursor = output.platform_output.cursor_icon;
    let shapes = output.shapes.clone();
    let pixels_per_point = output.pixels_per_point;
    output.drop_without_applying_deltas();
    assert_eq!(cursor, egui::CursorIcon::PointingHand);
    let title = shapes.iter().rposition(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == "Eastern Interconnection" && rect.contains(text.pos)
    )).expect("the hovered region has a globe title");
    // A mesh whose every vertex is fully transparent paints nothing, so it cannot cover the
    // title: egui's ScrollArea paints its edge fade that way when its content overflows and the
    // panel behind it has no fill. Every visible mesh still counts.
    let last_fill = shapes
        .iter()
        .rposition(|shape| {
            matches!(&shape.shape,
                egui::Shape::Mesh(mesh) if mesh.vertices.iter().any(|v| v.color.a() > 0)
            )
        })
        .expect("globe geography is painted");
    assert!(
        title > last_fill,
        "no later region fill may cover the hover title"
    );
    let highlighted = egui::Color32::from_rgb(228, 131, 47);
    assert!(shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.iter().any(|v| v.color == highlighted))));
    assert!(shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.width == 2.0 && rect.stroke.color == highlighted)));
    // This point is halfway along an overlapping land ring's old interior seam. Inspect the
    // actual tessellated paint: the visible colour must be the region fill, not a white stroke.
    let at = point(rect, -96.025, 46.5);
    let mut visible = None;
    for primitive in ctx.tessellate(shapes, pixels_per_point) {
        if !primitive.clip_rect.contains(at) {
            continue;
        }
        if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
            for triangle in mesh.triangles() {
                let vertices = triangle.map(|index| mesh.vertices[index as usize]);
                let cross =
                    |a: Pos2, b: Pos2| (b.x - a.x) * (at.y - a.y) - (b.y - a.y) * (at.x - a.x);
                let sides = [
                    cross(vertices[0].pos, vertices[1].pos),
                    cross(vertices[1].pos, vertices[2].pos),
                    cross(vertices[2].pos, vertices[0].pos),
                ];
                if sides.iter().all(|side| *side >= 0.0) || sides.iter().all(|side| *side <= 0.0) {
                    visible = Some(vertices[0].color);
                }
            }
        }
    }
    assert_eq!(
        visible,
        Some(highlighted),
        "interior polygon seam must stay filled"
    );
}

/// One picker screen driven frame by frame, with the clock advancing, so the mouse wheel can
/// scroll the card panel. The output's textures are dropped inside each frame, before any test
/// assertion can unwind past them.
struct Picker<'a> {
    ctx: egui::Context,
    screen: Rect,
    time: f64,
    model: &'a game::hud::RegionSelectModel,
    form: MenuForm,
}

impl<'a> Picker<'a> {
    fn new(model: &'a game::hud::RegionSelectModel, width: f32, height: f32) -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self {
            ctx,
            screen: Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height)),
            time: 0.0,
            model,
            form: MenuForm::new(42),
        }
    }

    /// One pass with `events`: what it painted, its widgets and its actions.
    fn frame(
        &mut self,
        events: Vec<egui::Event>,
    ) -> (
        Vec<egui::epaint::ClippedShape>,
        Vec<super::support::egui_harness::Widget>,
        Vec<UiAction>,
    ) {
        self.time += 0.5;
        let mut actions = Vec::new();
        let (model, form) = (self.model, &mut self.form);
        let output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(self.screen),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| actions = central(ui, |ui| draw_region_select(ui, model, form)),
        );
        let widgets = super::support::egui_harness::widgets_of(&output);
        let shapes = output.shapes.clone();
        output.drop_without_applying_deltas();
        (shapes, widgets, actions)
    }

    /// Wheels the card panel down by `points` with the pointer at `at`.
    fn wheel(
        &mut self,
        at: Pos2,
        points: f32,
    ) -> (
        Vec<egui::epaint::ClippedShape>,
        Vec<super::support::egui_harness::Widget>,
        Vec<UiAction>,
    ) {
        self.frame(vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -points),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ])
    }

    /// Wheels the card panel, with the pointer over it, until `label`'s title is
    /// painted on screen, then lets the wheel settle: that frame's paint and widgets. Panics if
    /// 60 steps of 120 points never reach it.
    fn scroll_to(
        &mut self,
        label: &str,
    ) -> (
        Vec<egui::epaint::ClippedShape>,
        Vec<super::support::egui_harness::Widget>,
    ) {
        let (_, widgets, _) = self.frame(vec![]);
        // Over the panel, at the screen's mid-height: the first card may be scrolled away.
        let at = egui::pos2(
            widget(&widgets, &self.model.regions[0].button.label)
                .rect
                .center()
                .x,
            self.screen.center().y,
        );
        for _ in 0..60 {
            let (shapes, widgets, actions) = self.frame(vec![]);
            assert!(actions.is_empty(), "scrolling must not start a game");
            let shown = |shapes: &[egui::epaint::ClippedShape], widgets: &[_]| {
                let title: &super::support::egui_harness::Widget = widget(widgets, label);
                self.screen.contains_rect(title.rect)
                    && crate::contrast::painted_texts(shapes)
                        .iter()
                        .any(|text| text.text == label && title.rect.contains(text.at))
            };
            if shown(&shapes, &widgets) {
                return (shapes, widgets);
            }
            let (_, _, actions) = self.wheel(at, 120.0);
            assert!(actions.is_empty(), "scrolling must not start a game");
        }
        panic!("{label} is never scrolled into view");
    }

    /// Wheels the card panel down in `step`-point steps, with the pointer over it,
    /// until [`card_shown`] holds for `label`'s card or 60 steps have passed: the last frame's
    /// paint and widgets, for the caller to assert on.
    fn scroll_until(
        &mut self,
        label: &str,
        lines: &[&str],
        step: f32,
    ) -> (
        Vec<egui::epaint::ClippedShape>,
        Vec<super::support::egui_harness::Widget>,
    ) {
        let (_, widgets, _) = self.frame(vec![]);
        // Over the panel, at the screen's mid-height: the first card may be scrolled away.
        let at = egui::pos2(
            widget(&widgets, &self.model.regions[0].button.label)
                .rect
                .center()
                .x,
            self.screen.center().y,
        );
        let mut last = None;
        for _ in 0..60 {
            let (shapes, widgets, actions) = self.frame(vec![]);
            assert!(actions.is_empty(), "scrolling must not start a game");
            if card_shown(&shapes, self.screen, label, lines) {
                return (shapes, widgets);
            }
            last = Some((shapes, widgets));
            let (_, _, actions) = self.wheel(at, step);
            assert!(actions.is_empty(), "scrolling must not start a game");
        }
        last.expect("at least one frame")
    }
}

/// The painted text shapes whose whole string is `text`, as (glyph bounds, clip rect).
fn painted_text_rects(shapes: &[egui::epaint::ClippedShape], text: &str) -> Vec<(Rect, Rect)> {
    fn walk(shape: &egui::Shape, clip: Rect, text: &str, found: &mut Vec<(Rect, Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, clip, text, found);
                }
            }
            egui::Shape::Text(painted) if painted.galley.text() == text => {
                found.push((painted.visual_bounding_rect(), clip));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, clipped.clip_rect, text, &mut found);
    }
    found
}

/// Whether `label`'s card is wholly in view: its title, and then each of its `lines` (the first
/// painted copy below the line before, since shared lines repeat on other cards), is painted
/// inside both the screen and the clip rect it is painted with, which is the card panel's
/// viewport.
fn card_shown(
    shapes: &[egui::epaint::ClippedShape],
    screen: Rect,
    label: &str,
    lines: &[&str],
) -> bool {
    let whole =
        |(glyphs, clip): (Rect, Rect)| screen.contains_rect(glyphs) && clip.contains_rect(glyphs);
    let Some(title) = painted_text_rects(shapes, label).into_iter().next() else {
        return false;
    };
    if !whole(title) {
        return false;
    }
    let mut above = title.0.bottom();
    for line in lines {
        let Some(next) = painted_text_rects(shapes, line)
            .into_iter()
            .filter(|(glyphs, _)| glyphs.top() >= above - 1.0)
            .min_by(|a, b| a.0.top().total_cmp(&b.0.top()))
        else {
            return false;
        };
        if !whole(next) {
            return false;
        }
        above = next.0.bottom();
    }
    true
}

/// The card panel scrolls vertically when the cards are taller than the picker (user decision,
/// 2026-10-25): at the smallest window and the larger ones, every card can be scrolled into
/// view, and each card's title and description fit its column without being cut sideways.
#[test]
fn map_cards_and_descriptions_fit_the_smallest_and_large_windows() {
    let model = model(Some(42));
    for (width, height) in [(1024.0, 640.0), (1280.0, 800.0), (3072.0, 1728.0)] {
        let mut picker = Picker::new(&model, width, height);
        let (_, widgets, _) = picker.frame(vec![]);
        let screen = picker.screen;
        for name in ["World power grids", "Back"] {
            assert!(
                screen.contains_rect(widget(&widgets, name).rect),
                "{width}x{height}: {name}"
            );
        }
        let globe = widget(&widgets, "World power grids").rect;
        let first = widget(&widgets, &model.regions[0].button.label).rect;
        for choice in &model.regions {
            let label = &choice.button.label;
            // Every shipped card has its description, its "home city · N cities" detail line
            // and its metadata line, in that order; the metadata line is the card's last.
            let lines = [
                ("description", &choice.description),
                ("detail", &choice.detail),
                ("metadata", &choice.metadata),
            ]
            .map(|(name, line)| {
                line.as_deref()
                    .unwrap_or_else(|| panic!("{label} has no {name} line"))
            });
            picker.scroll_to(label);
            // Keep wheeling in short steps until the whole card, title to last line, is painted
            // unclipped inside the screen and the card panel's viewport.
            let (shapes, widgets) = picker.scroll_until(label, &lines, 40.0);
            let title = widget(&widgets, label).rect;
            assert!(
                card_shown(&shapes, picker.screen, label, &lines),
                "{width}x{height}: {label}'s card is not wholly in view at any scroll offset"
            );
            // Every card shares one column grid to the right of the globe, inside the screen.
            assert!(
                title.left() > globe.right() && title.right() <= screen.right(),
                "{width}x{height}: {label} spills sideways: {title:?}"
            );
            assert!(
                (title.width() - first.width()).abs() < 1.0,
                "{width}x{height}: {label} is not a full card column"
            );
            // The title button fills the card's content width after the colour stripe, so the
            // card's text must stay between the panel's left edge and the title's right edge,
            // and below the title.
            for text in lines {
                // The card's own copy: the first one below its title (shared metadata lines
                // repeat on other cards).
                let rect = widgets
                    .iter()
                    .filter(|w| w.text == text && w.rect.top() >= title.bottom())
                    .map(|w| w.rect)
                    .min_by(|a, b| a.top().total_cmp(&b.top()))
                    .unwrap_or_else(|| panic!("{text:?} is below its title"));
                assert!(
                    rect.left() > globe.right() && rect.right() <= title.right() + 1.0,
                    "{width}x{height}: {label}'s {text:?} is clipped sideways: {rect:?} beside {title:?}"
                );
                assert!(
                    screen.contains_rect(rect),
                    "{width}x{height}: {label}'s {text:?} is off screen: {rect:?}"
                );
            }
        }
        // Where the cards overflow the panel, the panel itself scrolls: wheeling to the last
        // card moves the first one up while the globe stays put.
        let mut fresh = Picker::new(&model, width, height);
        let (shapes, widgets, _) = fresh.frame(vec![]);
        let last = &model.regions.last().unwrap().button.label;
        if !crate::contrast::painted_texts(&shapes)
            .iter()
            .any(|text| &text.text == last)
        {
            let top = widget(&widgets, &model.regions[0].button.label).rect;
            let (_, scrolled) = fresh.scroll_to(last);
            assert!(
                widget(&scrolled, &model.regions[0].button.label).rect.top() < top.top() - 1.0,
                "{width}x{height}: the card panel scrolled"
            );
            assert_eq!(widget(&scrolled, "World power grids").rect, globe);
        }
    }
}

/// Eight listings from a fixture catalog: at the smallest window the wheel reaches the last
/// card, and its title then starts that region.
#[test]
fn scrolling_reaches_the_last_of_eight_cards_at_the_smallest_window() {
    let source = include_str!("../../../infrastructure/tests/support/minimal_region_v4.ron");
    let dir = tempfile::TempDir::new().unwrap();
    for index in 0..8 {
        let file = source
            .replace(r#"id: "minimal""#, &format!(r#"id: "grid_{index}""#))
            .replace(
                r#"name_key: "region-minimal""#,
                &format!(r#"name_key: "region-grid-{index}""#),
            );
        std::fs::write(dir.path().join(format!("grid_{index}.ron")), file).unwrap();
    }
    let catalog = RonRegionCatalog::new(dir.path());
    let listings = RegionListings(catalog.list());
    let model = region_select_model(&listings, Some(42), &shipped_l10n())
        .with_atlas(&RegionAtlas::load(&catalog, &listings), &shipped_l10n());
    assert_eq!(model.regions.len(), 8);
    assert!(model.regions.iter().all(|choice| choice.button.enabled));
    let mut picker = Picker::new(&model, 1024.0, 640.0);
    let (shapes, widgets, _) = picker.frame(vec![]);
    assert!(
        !crate::contrast::painted_texts(&shapes)
            .iter()
            .any(|text| text.text == "region-grid-7"),
        "eight cards overflow the smallest window"
    );
    let before = widget(&widgets, "region-grid-0").rect;
    let globe = widget(&widgets, "World power grids").rect;
    let (_, widgets) = picker.scroll_to("region-grid-7");
    assert!(
        widget(&widgets, "region-grid-0").rect.top() < before.top() - 1.0,
        "the card panel scrolled"
    );
    assert_eq!(
        widget(&widgets, "World power grids").rect,
        globe,
        "the globe stays in view while the cards scroll"
    );
    let at = widget(&widgets, "region-grid-7").rect.center();
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let (_, _, mut actions) = picker.frame(vec![egui::Event::PointerMoved(at), press(true)]);
    actions.extend(picker.frame(vec![press(false)]).2);
    assert_eq!(
        actions,
        vec![UiAction::NewGame {
            region: RegionId::new("grid_7"),
            seed: 42
        }]
    );
}

fn metadata_model() -> game::hud::RegionSelectModel {
    metadata_model_from(include_str!(
        "../../../infrastructure/tests/support/minimal_region_v4.ron"
    ))
}

fn metadata_model_from(source: &str) -> game::hud::RegionSelectModel {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("minimal.ron"), source).unwrap();
    let catalog = RonRegionCatalog::new(dir.path());
    let listings = RegionListings(catalog.list());
    region_select_model(&listings, Some(42), &shipped_l10n())
        .with_atlas(&RegionAtlas::load(&catalog, &listings), &shipped_l10n())
}

#[test]
fn atlas_colours_and_region_names_are_shown_without_illustration_labels() {
    let model = metadata_model();
    let mut form = MenuForm::new(42);
    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        central(ui, |ui| draw_region_select(ui, &model, &mut form));
    });
    let shapes = output.shapes.clone();
    output.drop_without_applying_deltas();
    let texts = crate::contrast::painted_texts(&shapes);
    assert!(texts.iter().any(|text| text.text == "region-minimal"));
    assert!(!texts.iter().any(|text| text.text == "B"));
    assert!(!texts.iter().any(|text| text.text == "World power grids"));
    assert!(shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(18,52,86))));
}

#[test]
fn picker_hides_legacy_letters_and_map_heading_at_both_layouts() {
    let legacy = include_str!("../../../infrastructure/tests/support/minimal_region_v4.ron")
        .replace("atlas: (", r#"atlas: (letter: "B", "#);
    let no_labels = |shapes: &[egui::epaint::ClippedShape]| {
        !crate::contrast::painted_texts(shapes).iter().any(|text| {
            text.text == "World power grids"
                || matches!(
                    text.text.as_str(),
                    "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H"
                )
        })
    };
    for model in [model(Some(42)), metadata_model_from(&legacy)] {
        for width in [1024.0, 1280.0] {
            let mut picker = Picker::new(&model, width, 720.0);
            let (shapes, _, _) = picker.frame(vec![]);
            assert!(
                no_labels(&shapes),
                "{width}: a letter or heading is painted"
            );
            // Each card's own name is the label, painted once its card is scrolled into view.
            for choice in &model.regions {
                let (shapes, _) = picker.scroll_to(&choice.button.label);
                assert!(
                    no_labels(&shapes),
                    "{width}: a letter or heading is painted"
                );
            }
        }
    }
}

#[test]
fn cards_show_frequency_currency_zones_and_hemisphere() {
    let model = metadata_model();
    let mut form = MenuForm::new(42);
    let widgets =
        Harness::new().draw(|ui| central(ui, |ui| draw_region_select(ui, &model, &mut form)));
    for expected in [
        "60 Hz",
        "EUR",
        "UTC−03:00",
        "UTC+06:45",
        "Northern hemisphere",
    ] {
        assert!(
            widgets.iter().any(|w| w.text.contains(expected)),
            "missing {expected}: {widgets:?}"
        );
    }
}

#[test]
fn every_region_is_startable_from_its_card_in_one_and_two_columns() {
    let mut model = model(Some(42));
    let prototype = model.regions[0].clone();
    model.regions = (0..8)
        .map(|index| {
            let mut choice = prototype.clone();
            choice.button.label = format!("Grid {index}");
            choice.button.action = UiAction::NewGame {
                region: RegionId::new(&format!("grid-{index}")),
                seed: 42,
            };
            choice
        })
        .collect();
    let minimum = game::window::MIN_WINDOW_SIZE;
    for (width, height) in [(minimum.x, minimum.y), (1920.0, 1080.0)] {
        for index in 0..8 {
            let harness = Harness::with_screen(width, height);
            let mut form = MenuForm::new(42);
            let mut draw =
                |ui: &mut egui::Ui| central(ui, |ui| draw_region_select(ui, &model, &mut form));
            harness.draw(&mut draw);
            // Tab order: seed, then cards. Focus scrolls each card into view.
            for _ in 0..index + 2 {
                harness.press_key(egui::Key::Tab, &mut draw);
            }
            let widgets = harness.draw(&mut draw);
            assert!(
                harness
                    .screen_rect()
                    .contains_rect(widget(&widgets, &format!("Grid {index}")).rect),
                "Grid {index} is offscreen: {:?}",
                widget(&widgets, &format!("Grid {index}")).rect
            );
            let actions = harness.press_key(egui::Key::Enter, &mut draw);
            assert_eq!(
                actions,
                vec![UiAction::NewGame {
                    region: RegionId::new(&format!("grid-{index}")),
                    seed: 42
                }],
                "Grid {index} cannot be started from its card"
            );
        }
    }
}

#[test]
fn atlas_labels_meet_contrast() {
    let model = model(Some(42));
    let mut form = MenuForm::new(42);
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1024.0, 640.0));
    let widgets = Harness::with_screen(1024.0, 640.0)
        .draw(|ui| central(ui, |ui| draw_region_select(ui, &model, &mut form)));
    let map = widget(&widgets, "World power grids").rect;
    let ctx = egui::Context::default();
    ctx.set_visuals(game::hud::theme::visuals());
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            central(ui, |ui| draw_region_select(ui, &model, &mut form));
        },
    )
    .drop_without_applying_deltas();
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::PointerMoved(point(map, -83.0, 40.0))],
            ..Default::default()
        },
        |ui| {
            central(ui, |ui| draw_region_select(ui, &model, &mut form));
        },
    );
    let shapes = output.shapes.clone();
    assert!(crate::contrast::check_paint(output) > 0);
    let texts = crate::contrast::painted_texts(&shapes);
    for expected in ["Eastern Interconnection", model.map_note.as_str()] {
        let label = texts
            .iter()
            .find(|text| text.text == expected && map.contains(text.at))
            .expect(expected);
        assert_eq!(label.background, game::hud::theme::LABEL_BACKING);
        assert_eq!(label.background.a(), 255);
    }
}

#[test]
fn dragging_either_direction_never_starts_a_region() {
    let model = model(Some(42));
    let form = RefCell::new(MenuForm::new(42));
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| {
        central(ui, |ui| {
            draw_region_select(ui, &model, &mut form.borrow_mut())
        })
    };
    let globe = widget(&harness.draw(draw), "World power grids").rect;
    let start = point(globe, -83.0, 40.0);
    for direction in [-1.0, 1.0] {
        let frames = harness.drag(start, &[start + egui::vec2(direction * 140.0, 0.0)], draw);
        assert!(frames.into_iter().flatten().next().is_none());
    }
}

#[test]
fn actual_globe_drag_pauses_then_resumes_and_rotated_clicks_use_visible_geography() {
    use super::support::egui_harness::widgets_of;
    let model = model(Some(42));
    let mut form = MenuForm::new(42);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut frame = |time, events| {
        let mut actions = Vec::new();
        let output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                actions = central(ui, |ui| draw_region_select(ui, &model, &mut form));
            },
        );
        let globe = widget(&widgets_of(&output), "World power grids").rect;
        let vertices = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.vertices.len() > 500 => {
                    Some(mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>())
                }
                _ => None,
            })
            .expect("projected context land");
        output.drop_without_applying_deltas();
        (actions, globe, vertices)
    };
    let (_, globe, _) = frame(0.0, vec![]);
    let start = point(globe, -83.0, 40.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    assert!(
        frame(
            0.1,
            vec![egui::Event::PointerMoved(start), button(start, true)]
        )
        .0
        .is_empty()
    );
    let end = start + egui::vec2(120.0, 0.0);
    assert!(
        frame(0.2, vec![egui::Event::PointerMoved(end)])
            .0
            .is_empty()
    );
    let released = frame(0.3, vec![button(end, false)]);
    assert!(released.0.is_empty());
    assert_eq!(released.2, frame(2.2, vec![]).2, "two-second idle hold");
    assert_ne!(
        released.2,
        frame(2.5, vec![]).2,
        "automatic rotation resumes"
    );

    // A fresh context advances exactly a quarter turn in six seconds. Pick the western grid
    // at its new visible location, rather than its old screen coordinates.
    let model = model.clone();
    let mut form = MenuForm::new(42);
    let harness_ctx = egui::Context::default();
    let mut at_time = |time, events| {
        let mut actions = Vec::new();
        harness_ctx
            .run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    actions = central(ui, |ui| draw_region_select(ui, &model, &mut form));
                },
            )
            .drop_without_applying_deltas();
        actions
    };
    at_time(0.0, vec![]);
    at_time(6.0, vec![]);
    let target = point_at(globe, -122.33, 47.61, 170.0);
    assert!(
        at_time(
            6.0,
            vec![egui::Event::PointerMoved(target), button(target, true)]
        )
        .is_empty()
    );
    assert_eq!(
        at_time(6.0, vec![button(target, false)]),
        vec![UiAction::NewGame {
            region: RegionId::new("western_interconnection"),
            seed: 42
        }]
    );
}

#[test]
fn outside_the_globe_and_back_hemisphere_cannot_start_a_region() {
    let model = model(Some(42));
    let mut form = MenuForm::new(42);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut frame = |time, events| {
        let mut actions = Vec::new();
        let output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                actions = central(ui, |ui| draw_region_select(ui, &model, &mut form));
            },
        );
        let rect = widget(
            &super::support::egui_harness::widgets_of(&output),
            "World power grids",
        )
        .rect;
        output.drop_without_applying_deltas();
        (actions, rect)
    };
    frame(0.0, vec![]);
    let globe = frame(12.0, vec![]).1;
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    };
    // At 12 seconds the Eastern grid is behind the globe. Its projected back-face coordinate
    // lies inside the disc, but the visible geographic point there belongs to another continent.
    for target in [
        globe.left_top() + egui::vec2(2.0, 2.0),
        point_at(globe, -83.0, 40.0, 80.0),
    ] {
        assert!(
            frame(
                12.0,
                vec![egui::Event::PointerMoved(target), button(target, true)]
            )
            .0
            .is_empty()
        );
        assert!(frame(12.0, vec![button(target, false)]).0.is_empty());
    }
}

#[test]
fn cards_use_two_columns_when_the_right_panel_has_room_and_keep_row_order() {
    let mut model = model(Some(42));
    let prototype = model.regions[0].clone();
    model.regions = (0..4)
        .map(|index| {
            let mut choice = prototype.clone();
            choice.button.label = format!("Grid {index}");
            choice.button.action = UiAction::NewGame {
                region: RegionId::new(&format!("grid-{index}")),
                seed: 42,
            };
            choice
        })
        .collect();
    for (width, height, two_columns) in [
        (1024.0, 640.0, false),
        (1920.0, 1080.0, true),
        (3072.0, 1728.0, true),
    ] {
        let harness = Harness::with_screen(width, height);
        let form = RefCell::new(MenuForm::new(42));
        let draw = |ui: &mut egui::Ui| {
            central(ui, |ui| {
                draw_region_select(ui, &model, &mut form.borrow_mut())
            })
        };
        let widgets = harness.draw(draw);
        let a = widget(&widgets, "Grid 0").rect;
        let b = widget(&widgets, "Grid 1").rect;
        let c = widget(&widgets, "Grid 2").rect;
        if two_columns {
            assert!(
                (a.top() - b.top()).abs() < 1.0,
                "first row must share a top edge"
            );
            assert!(b.left() > a.right(), "cards must occupy separate columns");
            assert!(
                c.top() > a.bottom() && (c.left() - a.left()).abs() < 1.0,
                "next row retains catalog order"
            );
            assert_eq!(
                harness.click("Grid 1", draw),
                vec![UiAction::NewGame {
                    region: RegionId::new("grid-1"),
                    seed: 42
                }]
            );
        } else {
            assert!(b.top() > a.bottom(), "narrow panel keeps one column");
        }
        let labels: &[&str] = if two_columns {
            &["Grid 0", "Grid 1", "Grid 2", "Grid 3", "Back"]
        } else {
            &["Grid 0", "Back"]
        };
        for label in labels {
            assert!(
                harness
                    .screen_rect()
                    .contains_rect(widget(&widgets, label).rect)
            );
        }
    }
}

#[test]
fn card_body_brings_a_hidden_region_forward_then_resumes_after_two_seconds() {
    use super::support::egui_harness::widgets_of;
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
    let loaded = catalog
        .load(&RegionId::new("eastern_interconnection"))
        .unwrap();
    let outline = infrastructure::region::region_outline(
        &loaded.source_bytes,
        &RegionId::new("eastern_interconnection"),
    )
    .unwrap();
    let midpoint = ((outline.bounds.west + outline.bounds.east) * 0.5) as f32;
    let (_, home_lon, home_lat) = outline.home.unwrap();
    for enabled in [false, true] {
        let model = model(enabled.then_some(42));
        let mut form = if enabled {
            MenuForm::new(42)
        } else {
            MenuForm {
                seed: "invalid".into(),
            }
        };
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0));
        let mut frame = |time, events| {
            let mut actions = Vec::new();
            let output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    actions = central(ui, |ui| draw_region_select(ui, &model, &mut form));
                },
            );
            let widgets = widgets_of(&output);
            let land = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.vertices.len() > 500 => {
                        Some(mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>())
                    }
                    _ => None,
                })
                .expect("context mesh");
            let globe = widget(&widgets, "World power grids").rect;
            let home = point_at(globe, home_lon as f32, home_lat as f32, midpoint);
            let centered = output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Circle(circle) if circle.radius == 3.5 && circle.center.distance(home) < 0.05));
            output.drop_without_applying_deltas();
            assert!(actions.is_empty(), "body selection must not start a game");
            (widgets, land, centered)
        };
        frame(0.0, vec![]);
        let (widgets, hidden, _) = frame(12.0, vec![]); // North America now faces away.
        let title = widget(&widgets, "Eastern Interconnection").rect;
        let body = title.center_bottom() + egui::vec2(0.0, 22.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: body,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        frame(12.0, vec![egui::Event::PointerMoved(body), button(true)]);
        frame(12.0, vec![button(false)]);
        let (_, arrived, centered) = frame(12.6, vec![egui::Event::PointerGone]);
        if enabled {
            assert!(
                centered,
                "the geographic midpoint must face the viewer on arrival"
            );
            assert!(hidden != arrived);
            assert!(
                arrived == frame(14.5, vec![]).1,
                "hold for two seconds after arrival"
            );
            assert!(arrived != frame(14.9, vec![]).1, "resume gradual spin");
        } else {
            assert!(
                arrived != frame(14.5, vec![]).1,
                "disabled cards cannot interrupt the normal rotation"
            );
        }
    }
}
