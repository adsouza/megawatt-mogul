//! The supply key must have its own space, with the daily curve unobstructed.
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
    let mut model = supply_model(&app.world().resource::<Session>().0, &l10n);
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
