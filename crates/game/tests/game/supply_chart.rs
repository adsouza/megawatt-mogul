//! The supply key must have its own space, with the daily curve unobstructed.
use crate::support::usd_style;
use application::Command;
use bevy_egui::egui::{self, Id, Rect};
use domain::region::PlantKind;
use egui_plot::PlotMemory;
use game::hud::{SupplyModel, SupplyStack, draw_supply_chart, kind_color, supply_model};
use game::l10n::Localize;
use game::sim::Session;
use tempfile::TempDir;

use crate::hud_smoke::{WESTERN, operating_hour_app};
use crate::support::{egui_harness::widgets_of, shipped_l10n};

fn full_key() -> SupplyModel {
    let logs = TempDir::new().unwrap();
    let mut app = operating_hour_app(&logs, WESTERN);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::AdvanceTicks { n: 1_320 })
        .unwrap();
    let l10n = shipped_l10n();
    let mut model = supply_model(&app.world().resource::<Session>().0, &l10n, &usd_style());
    // Exercise the largest key even when a starter fleet does not use every kind today.
    model.stacks = PlantKind::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| SupplyStack {
            kind,
            name: kind.localize(&l10n),
            color: kind_color(kind),
            low: vec![index as f64 * 100.0; model.x.len()],
            high: vec![(index + 1) as f64 * 100.0; model.x.len()],
        })
        .collect();
    model
}

fn paint(
    ctx: &egui::Context,
    model: &SupplyModel,
    width: f32,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Id) {
    let mut id = None;
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 152.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                id = Some(ui.make_persistent_id(Id::new("operating_supply")));
                assert!(draw_supply_chart(ui, model).is_empty());
            });
        },
    );
    (output, id.unwrap())
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.set_visuals(game::hud::theme::visuals());
    ctx
}

fn hour_ticks(shapes: &[egui::epaint::ClippedShape], frame: Rect) -> Vec<(u32, Rect)> {
    fn collect(shape: &egui::Shape, frame: Rect, ticks: &mut Vec<(u32, Rect)>) {
        match shape {
            egui::Shape::Vec(children) => {
                for child in children {
                    collect(child, frame, ticks);
                }
            }
            egui::Shape::Text(text) => {
                let rect = text.visual_bounding_rect();
                if rect.top() >= frame.bottom()
                    && rect.center().x >= frame.left() - 1.0
                    && rect.center().x <= frame.right() + 1.0
                    && let Ok(hour) = text.galley.text().parse::<u32>()
                {
                    assert!(hour <= 24, "unexpected hour {hour}");
                    ticks.push((hour, rect));
                }
            }
            _ => {}
        }
    }
    let mut ticks = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, frame, &mut ticks);
    }
    ticks.sort_by_key(|(hour, _)| *hour);
    ticks
}

#[test]
fn daily_chart_labels_every_hour_when_wide_and_keeps_narrow_ticks_apart() {
    let model = full_key();
    for width in [420.0, 1280.0] {
        let ctx = context();
        for _ in 0..3 {
            paint(&ctx, &model, width, vec![])
                .0
                .drop_without_applying_deltas();
        }
        let (output, id) = paint(&ctx, &model, width, vec![]);
        let frame = *PlotMemory::load(&ctx, id).unwrap().transform().frame();
        let ticks = hour_ticks(&output.shapes, frame);
        output.drop_without_applying_deltas();
        if width == 1280.0 {
            assert_eq!(
                ticks.iter().map(|(hour, _)| *hour).collect::<Vec<_>>(),
                (0..=24).collect::<Vec<_>>()
            );
        } else {
            assert!(ticks.len() >= 3);
        }
        for pair in ticks.windows(2) {
            assert!(
                pair[0].1.right() + 4.0 < pair[1].1.left(),
                "{width}: overlapping ticks {pair:?}"
            );
        }
    }
}

#[test]
fn operating_supply_labels_every_hour_beside_the_incident_column_and_battery_plot() {
    use game::hud::{OperatingContent, OperatingTab, draw_operating_panel, operating_panel_model};

    let logs = TempDir::new().unwrap();
    let app = operating_hour_app(&logs, WESTERN);
    let l10n = shipped_l10n();
    let mut model = operating_panel_model(
        &app.world().resource::<Session>().0,
        &l10n,
        OperatingTab::Supply,
        &usd_style(),
    )
    .unwrap();
    for has_storage in [false, true] {
        let OperatingContent::Supply(chart) = &mut model.content else {
            unreachable!()
        };
        chart.has_storage = has_storage;
        chart.stored = vec![400.0; chart.x.len()];
        let ctx = context();
        let mut id = None;
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        id = Some(ui.make_persistent_id(Id::new("operating_supply")));
                        assert!(draw_operating_panel(ui, &model).is_empty());
                    });
                },
            );
            shapes = output.shapes.clone();
            output.drop_without_applying_deltas();
        }
        let frame = *PlotMemory::load(&ctx, id.unwrap())
            .unwrap()
            .transform()
            .frame();
        let ticks = hour_ticks(&shapes, frame);
        assert_eq!(
            ticks.iter().map(|(hour, _)| *hour).collect::<Vec<_>>(),
            (0..=24).collect::<Vec<_>>(),
            "has_storage={has_storage}, frame={frame:?}"
        );
        for pair in ticks.windows(2) {
            assert!(
                pair[0].1.right() < pair[1].1.left(),
                "overlapping hour labels: {pair:?}"
            );
        }
    }
}

fn assert_key_fits(model: &SupplyModel, width: f32) {
    let ctx = context();
    for _ in 0..3 {
        paint(&ctx, model, width, vec![])
            .0
            .drop_without_applying_deltas();
    }
    let (output, id) = paint(&ctx, model, width, vec![]);
    let plot = PlotMemory::load(&ctx, id).unwrap();
    let frame = *plot.transform().frame();
    assert!(
        frame.height() >= 40.0,
        "the curve still needs height: {frame:?}"
    );
    assert!(plot.bounds().min()[0] <= 0.0 && plot.bounds().max()[0] >= 24.0);
    let widgets = widgets_of(&output);
    let names = model.stacks.iter().map(|stack| stack.name.as_str()).chain([
        model.demand_label.as_str(),
        model.served_label.as_str(),
        model.forecast_label.as_str(),
        model.cost_label.as_str(),
    ]);
    for name in names {
        let key = widgets
            .iter()
            .find(|widget| widget.text == name && widget.role == egui::accesskit::Role::CheckBox)
            .expect(name);
        assert!(
            !frame.intersects(key.rect),
            "{width}: {name} at {:?} overlaps curve {frame:?}",
            key.rect
        );
        assert!(
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 152.0)).contains_rect(key.rect),
            "{width}: {name} runs outside the chart"
        );
    }
    output.drop_without_applying_deltas();
}

#[test]
fn the_full_supply_key_fits_outside_the_daily_curve_at_narrow_and_wide_sizes() {
    let model = full_key();
    for width in [420.0, 528.0, 1280.0] {
        assert_key_fits(&model, width);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn supply_key_never_covers_the_curve_at_any_usable_width(width in 420.0_f32..2560.0) {
        assert_key_fits(&full_key(), width);
    }
}

#[test]
fn clicking_an_external_key_still_hides_and_restores_its_series() {
    let model = full_key();
    let ctx = context();
    paint(&ctx, &model, 528.0, vec![])
        .0
        .drop_without_applying_deltas();
    let (output, id) = paint(&ctx, &model, 528.0, vec![]);
    let widgets = widgets_of(&output);
    let at = widgets
        .iter()
        .find(|widget| widget.text == model.cost_label)
        .unwrap()
        .rect
        .center();
    output.drop_without_applying_deltas();
    for hidden in [true, false] {
        for pressed in [true, false] {
            let events = vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            paint(&ctx, &model, 528.0, events)
                .0
                .drop_without_applying_deltas();
        }
        assert_eq!(
            PlotMemory::load(&ctx, id)
                .unwrap()
                .hidden_items
                .contains(&Id::new(&model.cost_label)),
            hidden
        );
    }
}

#[test]
fn supply_key_visibility_can_be_changed_from_the_keyboard() {
    let model = full_key();
    let ctx = context();
    let (output, id) = paint(&ctx, &model, 528.0, vec![]);
    output.drop_without_applying_deltas();
    for (key, hidden) in [
        (egui::Key::Tab, false),
        (egui::Key::Space, true),
        (egui::Key::Space, false),
    ] {
        for pressed in [true, false] {
            paint(
                &ctx,
                &model,
                528.0,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
            .0
            .drop_without_applying_deltas();
        }
        assert_eq!(
            PlotMemory::load(&ctx, id)
                .unwrap()
                .hidden_items
                .contains(&Id::new(&model.stacks[0].name)),
            hidden
        );
    }
}

#[test]
fn storage_inventory_keeps_a_readable_plot_beside_the_supply_curve_in_a_short_panel() {
    let mut model = full_key();
    model.has_storage = true;
    model.stored = vec![400.0; model.x.len()];
    for width in [528.0, 1280.0] {
        let ctx = context();
        for _ in 0..3 {
            paint(&ctx, &model, width, vec![])
                .0
                .drop_without_applying_deltas();
        }
        let (output, supply_id) = paint(&ctx, &model, width, vec![]);
        let supply = *PlotMemory::load(&ctx, supply_id)
            .unwrap()
            .transform()
            .frame();
        let energy_id = supply_id.with("inventory");
        let inventory = *PlotMemory::load(&ctx, energy_id)
            .expect("inventory plot")
            .transform()
            .frame();
        assert!(supply.height() >= 40.0, "{supply:?}");
        assert!(inventory.height() >= 40.0, "{inventory:?}");
        assert!(!supply.intersects(inventory));
        assert!(supply.right() < inventory.left());
        assert!(
            // The two supply-axis gutters consume more of its 2:1 allocation.
            supply.width() > inventory.width() * 1.7,
            "supply {supply:?}, inventory {inventory:?}"
        );
        output.drop_without_applying_deltas();
    }
}

#[test]
fn combined_storage_uses_one_full_width_plot_and_a_labelled_energy_axis() {
    use game::hud::SupplyChartLayout;
    let mut model = full_key().with_layout(SupplyChartLayout::Combined);
    model.has_storage = true;
    model.stored = vec![400.0; model.x.len()];
    for width in [528.0, 1280.0] {
        let ctx = context();
        for _ in 0..3 {
            paint(&ctx, &model, width, vec![])
                .0
                .drop_without_applying_deltas();
        }
        let (output, id) = paint(&ctx, &model, width, vec![]);
        let frame = *PlotMemory::load(&ctx, id).unwrap().transform().frame();
        assert!(frame.width() > width * 0.6, "{frame:?}");
        assert!(frame.height() >= 40.0, "{frame:?}");
        assert!(PlotMemory::load(&ctx, id.with("inventory")).is_none());
        let widgets = widgets_of(&output);
        assert!(widgets.iter().any(|w| w.text == model.layout_label));
        // Axis text is painted by egui_plot, not exposed as a widget.
        fn contains(shapes: &[egui::epaint::ClippedShape], text: &str) -> bool {
            shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == text))
        }
        assert!(contains(&output.shapes, &model.energy_unit));
        assert!(contains(&output.shapes, &model.cost_unit));
        assert!(!contains(&output.shapes, &model.storage_energy_label));
        assert!(!contains(&output.shapes, &model.cost_axis));
        output.drop_without_applying_deltas();
    }
}

#[test]
fn storage_visibility_survives_switching_between_separate_and_combined_views() {
    use game::hud::SupplyChartLayout;
    let mut model = full_key();
    model.has_storage = true;
    model.stored = vec![400.0; model.x.len()];
    let ctx = context();
    for _ in 0..3 {
        paint(&ctx, &model, 1280.0, vec![])
            .0
            .drop_without_applying_deltas();
    }
    let (output, id) = paint(&ctx, &model, 1280.0, vec![]);
    let at = widgets_of(&output)
        .iter()
        .find(|w| w.text == model.storage_energy_legend)
        .unwrap()
        .rect
        .center();
    output.drop_without_applying_deltas();
    for layout in [SupplyChartLayout::Separate, SupplyChartLayout::Combined] {
        model.layout = layout;
        for pressed in [true, false] {
            paint(
                &ctx,
                &model,
                1280.0,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
            .0
            .drop_without_applying_deltas();
        }
        assert_eq!(
            PlotMemory::load(&ctx, id)
                .unwrap()
                .hidden_items
                .contains(&Id::new(&model.storage_energy_legend)),
            layout == SupplyChartLayout::Separate
        );
    }
}

#[test]
fn combined_inventory_hover_reports_mwh_instead_of_scaled_power() {
    use game::hud::SupplyChartLayout;
    let mut model = full_key().with_layout(SupplyChartLayout::Combined);
    model.has_storage = true;
    model.stored = vec![400.0; model.x.len()];
    let ctx = context();
    for _ in 0..3 {
        paint(&ctx, &model, 1280.0, vec![])
            .0
            .drop_without_applying_deltas();
    }
    let (output, id) = paint(&ctx, &model, 1280.0, vec![]);
    output.drop_without_applying_deltas();
    // The inventory peak reaches the top of the physical power data, but reads as 400 MWh.
    let power_peak = model
        .demand
        .iter()
        .chain(&model.storage_load)
        .chain(model.stacks.iter().flat_map(|s| &s.high))
        .chain(model.forecast.iter().map(|[_, y]| y))
        .copied()
        .fold(1.0_f64, f64::max);
    let at = PlotMemory::load(&ctx, id)
        .unwrap()
        .transform()
        .position_from_point(&egui_plot::PlotPoint::new(12.0, power_peak));
    let mut found = false;
    for _ in 0..3 {
        let (output, _) = paint(&ctx, &model, 1280.0, vec![egui::Event::PointerMoved(at)]);
        fn has_label(shape: &egui::Shape) -> bool {
            match shape {
                egui::Shape::Text(t) => t.galley.text().contains("400 MWh"),
                egui::Shape::Vec(shapes) => shapes.iter().any(has_label),
                _ => false,
            }
        }
        found |= output.shapes.iter().any(|s| has_label(&s.shape));
        output.drop_without_applying_deltas();
    }
    assert!(found, "hover must report the actual stored MWh");
}

#[test]
fn games_without_storage_have_no_layout_toggle_or_energy_key() {
    let model = full_key();
    assert!(!model.has_storage);
    let (output, _) = paint(&context(), &model, 528.0, vec![]);
    assert!(
        widgets_of(&output)
            .iter()
            .all(|w| w.text != model.layout_label && w.text != model.storage_energy_legend)
    );
    output.drop_without_applying_deltas();
}
