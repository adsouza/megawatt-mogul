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
    let last_fill = shapes
        .iter()
        .rposition(|shape| {
            matches!(&shape.shape,
                egui::Shape::Mesh(mesh) if !mesh.vertices.is_empty()
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

// Detects dropping authored atlas colours or leaking retired illustration labels into the UI.
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
    for model in [model(Some(42)), metadata_model_from(&legacy)] {
        for width in [1024.0, 1280.0] {
            let mut form = MenuForm::new(42);
            let ctx = egui::Context::default();
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 720.0))),
                    ..Default::default()
                },
                |ui| {
                    central(ui, |ui| draw_region_select(ui, &model, &mut form));
                },
            );
            let texts = crate::contrast::painted_texts(&output.shapes);
            assert!(!texts.iter().any(|text| {
                text.text == "World power grids"
                    || matches!(
                        text.text.as_str(),
                        "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H"
                    )
            }));
            for choice in &model.regions {
                assert!(texts.iter().any(|text| text.text == choice.button.label));
            }
            output.drop_without_applying_deltas();
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
