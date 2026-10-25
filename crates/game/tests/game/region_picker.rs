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

// Same geographic extent as the overview; assert the rendered image is the click target.
fn point(rect: Rect, lon: f32, lat: f32) -> Pos2 {
    // Independent f64 equations at the public rendered-image boundary.
    let project = |lon: f64, lat: f64| {
        let t = (3.0_f64.sqrt() * 0.5 * lat.to_radians().sin()).asin();
        let t2 = t * t;
        (
            2.0 * lon.to_radians() * t.cos()
                / (3.0_f64.sqrt()
                    * (1.340264 - 3.0 * 0.081106 * t2
                        + t2.powi(3) * (7.0 * 0.000893 + 9.0 * 0.003796 * t2))),
            t * (1.340264 - 0.081106 * t2 + t2.powi(3) * (0.000893 + 0.003796 * t2)),
        )
    };
    let (x, y) = project(f64::from(lon), f64::from(lat));
    let north = project(0.0, 75.0).1;
    let south = project(0.0, -60.0).1;
    rect.min
        + egui::vec2(
            ((x + 2.7066299836960748) / 5.4132599673921496) as f32 * rect.width(),
            ((north - y) / (north - south)) as f32 * rect.height(),
        )
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
    let form = RefCell::new(MenuForm::new(42));
    for (width, height) in [(1024.0, 640.0), (1280.0, 800.0), (3072.0, 1728.0)] {
        let harness = Harness::with_screen(width, height);
        let widgets = harness.draw(|ui| {
            central(ui, |ui| {
                draw_region_select(ui, &model, &mut form.borrow_mut())
            })
        });
        for name in ["Eastern Interconnection", "Western Interconnection", "Back"] {
            assert!(
                harness
                    .screen_rect()
                    .contains_rect(widget(&widgets, name).rect),
                "{width}x{height}: {name}"
            );
        }
        let map = widget(&widgets, "World power grids").rect;
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

// Detects dropping raw atlas metadata or failing to paint its identifier/colour.
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
fn atlas_letters_and_colours_come_from_region_data() {
    let model = metadata_model();
    let mut form = MenuForm::new(42);
    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        central(ui, |ui| draw_region_select(ui, &model, &mut form));
    });
    let shapes = output.shapes.clone();
    output.drop_without_applying_deltas();
    let texts = crate::contrast::painted_texts(&shapes);
    assert!(texts.iter().any(|text| text.text == "B"));
    assert!(shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(18,52,86))));
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
fn every_region_is_startable_from_its_card_at_minimum_window_size() {
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
    for (width, height) in [(minimum.x, minimum.y), (1024.0, 640.0)] {
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
    let model = metadata_model();
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
            events: vec![egui::Event::PointerMoved(point(map, 6.5, 3.5))],
            ..Default::default()
        },
        |ui| {
            central(ui, |ui| draw_region_select(ui, &model, &mut form));
        },
    );
    let shapes = output.shapes.clone();
    assert!(crate::contrast::check_paint(output) > 0);
    let texts = crate::contrast::painted_texts(&shapes);
    for expected in ["B", "region-minimal", "World power grids"] {
        let label = texts
            .iter()
            .find(|text| text.text == expected && map.contains(text.at))
            .expect(expected);
        assert_eq!(label.background, game::hud::theme::LABEL_BACKING);
        assert_eq!(label.background.a(), 255);
    }
}
