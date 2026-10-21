use application::RegionCatalog;
use bevy_egui::egui::{self, Pos2, Rect};
use domain::ids::RegionId;
use game::hud::{RegionAtlas, RegionListings, UiAction, draw_region_select, region_select_model};
use infrastructure::region::RonRegionCatalog;

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

// Same geographic extent as the overview; assert the rendered image is the click target.
fn point(rect: Rect, lon: f32, lat: f32) -> Pos2 {
    rect.min
        + egui::vec2(
            (lon + 168.0) / 116.0 * rect.width(),
            (74.0 - lat) / 62.0 * rect.height(),
        )
}

#[test]
fn overview_clicks_start_the_right_region_with_the_entered_seed() {
    let model = model(Some(42));
    for (lon, lat, region) in [
        (-122.33, 47.61, "western_interconnection"),
        (-83.0, 40.0, "eastern_interconnection"),
    ] {
        let harness = Harness::with_screen(1024.0, 640.0);
        let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_region_select(ui, &model));
        let widgets = harness.draw(draw);
        let map = widget(&widgets, "North America").rect;
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
        let harness = Harness::new();
        let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_region_select(ui, &model));
        let map = widget(&harness.draw(draw), "North America").rect;
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
    let harness = Harness::new();
    let draw = |ui: &mut egui::Ui| central(ui, |ui| draw_region_select(ui, &model));
    harness.draw(draw);
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
    let harness = Harness::new();
    let rect = widget(
        &harness.draw(|ui| central(ui, |ui| draw_region_select(ui, &model))),
        "North America",
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
            |ui| actions.extend(central(ui, |ui| draw_region_select(ui, &model))),
        );
        assert!(actions.is_empty());
        output
    };
    let initial = paint(vec![]);
    let highlighted = egui::Color32::from_rgb(69, 178, 179);
    assert!(!initial.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.iter().any(|v| v.color == highlighted))));
    initial.drop_without_applying_deltas();
    let output = paint(vec![egui::Event::PointerMoved(point(rect, -83.0, 40.0))]);
    assert_eq!(
        output.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
    let highlighted = egui::Color32::from_rgb(69, 178, 179);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.iter().any(|v| v.color == highlighted))));
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.width == 2.0 && rect.stroke.color == highlighted)));
    // This point is halfway along an overlapping land ring's old interior seam. Inspect the
    // actual tessellated paint: the visible colour must be the region fill, not a white stroke.
    let at = point(rect, -96.025, 46.5);
    let mut visible = None;
    for primitive in ctx.tessellate(output.shapes.clone(), output.pixels_per_point) {
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
    output.drop_without_applying_deltas();
}

#[test]
fn map_cards_and_descriptions_fit_the_smallest_and_large_windows() {
    let model = model(Some(42));
    for (width, height) in [(1024.0, 640.0), (1280.0, 800.0), (3072.0, 1728.0)] {
        let harness = Harness::with_screen(width, height);
        let widgets = harness.draw(|ui| central(ui, |ui| draw_region_select(ui, &model)));
        for name in ["Eastern Interconnection", "Western Interconnection", "Back"] {
            assert!(
                harness
                    .screen_rect()
                    .contains_rect(widget(&widgets, name).rect),
                "{width}x{height}: {name}"
            );
        }
        let map = widget(&widgets, "North America").rect;
        for choice in &model.regions {
            for text in [
                choice.description.as_ref().unwrap(),
                choice.detail.as_ref().unwrap(),
            ] {
                // AccessKit exposes a wrapped label as several nodes with the same value.
                let labels: Vec<_> = widgets
                    .iter()
                    .filter(|widget| &widget.text == text)
                    .collect();
                assert!(!labels.is_empty());
                for label in labels {
                    assert!(harness.screen_rect().contains_rect(label.rect));
                    assert!(label.rect.left() >= map.right());
                }
            }
        }
    }
}
