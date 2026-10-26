//! Selecting a built order and cancelling it through the real HUD and command pipeline.
use crate::support::usd_style;
use application::Command;
use bevy::prelude::*;
use bevy_egui::egui;
use domain::grid::LineStatus;
use domain::ids::LineId;
use game::hud::{
    InspectorModel, MapLayer, OpenPrompt, Project, Prompt, Selection, UiAction, construction_model,
    draw_prompt, inspector_model, prompt_model,
};
use game::sim::{PlayerCommand, Session};
use game::world::{WorldModel, pick_world, region_terrain};
use tempfile::TempDir;

use crate::hud_smoke::{EASTERN, WESTERN, dispatch_in, sent};
use crate::support::egui_harness::{Harness, button, has};
use crate::support::egui_pass::EguiPass;
use crate::support::{hud_app, request, shipped_l10n, update_n};

const SIZE: (f32, f32) = (1024.0, 640.0);

fn order(app: &mut App) -> LineId {
    let session = app.world().resource::<Session>().0.session();
    let to = session
        .city_views()
        .into_iter()
        .find(|city| !city.connected)
        .unwrap()
        .node;
    let from = session.anchor_node();
    dispatch_in(
        app,
        vec![UiAction::Command(Command::BuildLine { from, to })],
    );
    update_n(app, 1);
    app.world()
        .resource::<Session>()
        .0
        .session()
        .line_views()
        .into_iter()
        .find(|line| matches!(line.status, LineStatus::UnderConstruction { .. }))
        .unwrap()
        .id
}

#[test]
fn selecting_an_unfinished_line_can_cancel_the_actual_order() {
    for region in [EASTERN, WESTERN] {
        let logs = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        request(&mut app, region);
        let line = order(&mut app);
        let game = &app.world().resource::<Session>().0;
        let world = WorldModel::of(game);
        let geometry = region_terrain(&world.region, MapLayer::Terrain).unwrap();
        let selected = world.lines[&line]
            .parts
            .iter()
            .find_map(|part| {
                let ray = Ray3d::new(part.center + Vec3::Y * 100.0, Dir3::NEG_Y);
                let hit = pick_world(ray, &world, &geometry, 1.0);
                (hit == Selection::Line(line)).then_some(hit)
            })
            .expect("clicking the line selects its order, rather than drawing another line");
        let completed = game
            .session()
            .line_views()
            .into_iter()
            .find(|line| line.status == LineStatus::InService)
            .unwrap()
            .id;
        let Some(InspectorModel::Line(card)) = inspector_model(
            game,
            &shipped_l10n(),
            Selection::Line(completed),
            &usd_style(),
        ) else {
            panic!("a completed line has a card");
        };
        assert!(
            card.cancel.is_none(),
            "in-service lines cannot be cancelled"
        );
        assert!(
            prompt_model(
                OpenPrompt(Some(Prompt::CancelLineConstruction { line: completed })),
                Some(game),
                &shipped_l10n()
            )
            .is_none()
        );

        dispatch_in(&mut app, vec![UiAction::Select(selected)]);
        let pass = EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, SIZE.0, SIZE.1);
        let before = sent::<PlayerCommand>(&app).len();
        pass.click_at(
            &mut app,
            SIZE.0,
            SIZE.1,
            button(&widgets, "Cancel construction").rect.center(),
        );
        let widgets = pass.settled(&mut app, SIZE.0, SIZE.1);
        assert!(has(&widgets, "Keep building"));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.0, SIZE.1));
        for label in ["Keep building", "Cancel this line"] {
            assert!(screen.contains_rect(button(&widgets, label).rect));
        }
        assert!(sent::<PlayerCommand>(&app)[before..].is_empty());
        pass.click_at(
            &mut app,
            SIZE.0,
            SIZE.1,
            button(&widgets, "Keep building").rect.center(),
        );
        assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
        assert!(
            app.world()
                .resource::<Session>()
                .0
                .session()
                .line_views()
                .iter()
                .any(|view| view.id == line)
        );

        let widgets = pass.settled(&mut app, SIZE.0, SIZE.1);
        pass.click_at(
            &mut app,
            SIZE.0,
            SIZE.1,
            button(&widgets, "Cancel construction").rect.center(),
        );
        let widgets = pass.settled(&mut app, SIZE.0, SIZE.1);
        pass.click_at(
            &mut app,
            SIZE.0,
            SIZE.1,
            button(&widgets, "Cancel this line").rect.center(),
        );
        assert_eq!(
            sent::<PlayerCommand>(&app)[before..]
                .iter()
                .map(|sent| sent.0.clone())
                .collect::<Vec<_>>(),
            [Command::CancelLineConstruction { line }]
        );
        update_n(&mut app, 2);
        assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
        let game = &app.world().resource::<Session>().0;
        assert!(!WorldModel::of(game).lines.contains_key(&line));
        assert!(
            !construction_model(game, &shipped_l10n())
                .rows
                .iter()
                .any(|row| row.project == Project::Line(line))
        );
        let widgets = pass.settled(&mut app, SIZE.0, SIZE.1);
        assert!(!has(&widgets, "Cancel construction"));
    }
}

#[test]
fn line_cancellation_defaults_to_keep_building_and_escape_backs_out() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    request(&mut app, EASTERN);
    let line = order(&mut app);
    let prompt = prompt_model(
        OpenPrompt(Some(Prompt::CancelLineConstruction { line })),
        Some(&app.world().resource::<Session>().0),
        &shipped_l10n(),
    )
    .unwrap();
    let draw = |ui: &mut egui::Ui| {
        egui::Modal::new(egui::Id::new("prompt"))
            .show(ui.ctx(), |ui| draw_prompt(ui, &prompt))
            .inner
    };
    let harness = Harness::new();
    harness.draw(draw);
    assert_eq!(
        harness.press_key(egui::Key::Enter, draw),
        vec![UiAction::Prompt(None)]
    );
    assert_eq!(
        harness.press_key(egui::Key::Escape, draw),
        vec![UiAction::Prompt(None)]
    );
    let harness = Harness::new();
    harness.draw(draw);
    assert!(harness.press_key(egui::Key::Tab, draw).is_empty());
    assert_eq!(
        harness.press_key(egui::Key::Enter, draw),
        vec![UiAction::Command(Command::CancelLineConstruction { line })]
    );
}
