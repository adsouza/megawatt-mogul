//! Storage through the real command funnel, egui pass, charts and illustrative network routes.
use crate::hud_smoke::dispatch_in;
use crate::support::egui_harness::{button, buttons};
use crate::support::egui_pass::EguiPass;
use crate::support::usd_style;
use crate::support::{headless_app, request, shipped_assets, shipped_l10n, update_n};
use application::Command;
use bevy::prelude::*;
use domain::{ids::PlantId, region::PlantKind, session::DayPhase, units::MegawattHours};
use game::{
    hud::{self, Selection, UiAction},
    paths::SaveDir,
    sim::Session,
    world::{FlowDirection, WorldModel},
};
use tempfile::TempDir;

const WIDTH: f32 = game::window::MIN_WINDOW_SIZE.x;
const HEIGHT: f32 = game::window::MIN_WINDOW_SIZE.y;

fn app_with_battery() -> (App, TempDir, PlantId) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("regions")).unwrap();
    std::fs::create_dir_all(dir.path().join("locales/en-US")).unwrap();
    for entry in std::fs::read_dir(shipped_assets().join("locales/en-US")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            dir.path().join("locales/en-US").join(entry.file_name()),
        )
        .unwrap();
    }
    let source =
        std::fs::read_to_string(shipped_assets().join("regions/western_interconnection.ron"))
            .unwrap();
    let source = source.replacen("fleet: [", "fleet: [(Battery, -117.0, 34.5),", 1);
    std::fs::write(
        dir.path().join("regions/western_interconnection.ron"),
        source,
    )
    .unwrap();
    // Its sidecars (spec section 28.11.1) come along unedited.
    let sidecars = dir.path().join("regions/western_interconnection");
    std::fs::create_dir(&sidecars).unwrap();
    for entry in
        std::fs::read_dir(shipped_assets().join("regions/western_interconnection")).unwrap()
    {
        let path = entry.unwrap().path();
        std::fs::copy(&path, sidecars.join(path.file_name().unwrap())).unwrap();
    }
    let mut app = headless_app(dir.path(), &dir.path().join("logs"));
    app.insert_resource(SaveDir(dir.path().join("saves")))
        .add_plugins((hud::HudPlugin, game::world::WorldViewPlugin));
    update_n(&mut app, 3);
    request(&mut app, "western_interconnection");
    let id = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .iter()
        .find(|p| p.kind == PlantKind::Battery)
        .unwrap()
        .id;
    (app, dir, id)
}
fn command(app: &mut App, command: Command) {
    dispatch_in(app, vec![UiAction::Command(command)]);
    update_n(app, 2);
}

#[test]
fn battery_card_uses_real_enabled_control_without_thermal_controls_at_minimum_size() {
    let (mut app, _dir, id) = app_with_battery();
    let l10n = shipped_l10n();
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Plant(id))]);
    update_n(&mut app, 2);
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, WIDTH, HEIGHT);
    let disable = button(&widgets, &l10n.text("battery-disable"));
    assert!(egui_rect().contains_rect(disable.rect));
    assert!(buttons(&widgets, &l10n.text("plant-start")).is_empty());
    assert!(buttons(&widgets, &l10n.text("plant-stop")).is_empty());
    pass.click_at(&mut app, WIDTH, HEIGHT, disable.rect.center());
    update_n(&mut app, 2);
    assert!(
        !app.world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .battery
            .as_ref()
            .unwrap()
            .enabled
    );
    command(&mut app, Command::BeginOperating);
    let widgets = pass.settled(&mut app, WIDTH, HEIGHT);
    let enable = button(&widgets, &l10n.text("battery-enable"));
    assert!(egui_rect().contains_rect(enable.rect));
    pass.click_at(&mut app, WIDTH, HEIGHT, enable.rect.center());
    update_n(&mut app, 2);
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .battery
            .as_ref()
            .unwrap()
            .enabled
    );
    assert!(
        app.world()
            .resource::<game::l10n::Localizer>()
            .problems()
            .is_empty()
    );
}
fn egui_rect() -> bevy_egui::egui::Rect {
    bevy_egui::egui::Rect::from_min_max(
        bevy_egui::egui::Pos2::ZERO,
        bevy_egui::egui::pos2(WIDTH, HEIGHT),
    )
}

#[test]
fn charged_battery_toggle_save_with_enter_leave_load_and_resume_is_a_complete_player_task() {
    use crate::player_workflows::{click, settle_save};
    use bevy_egui::egui;
    let (mut app, dir, id) = app_with_battery();
    // Leave a real evening supply gap: reserve mode must not discharge just because it is 16:00.
    let thermal: Vec<_> = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .filter(|p| p.kind.is_dispatchable() && p.kind != PlantKind::Hydro)
        .map(|p| p.id)
        .collect();
    let mut hours = [false; 24];
    hours[..18].fill(true);
    for plant in thermal {
        command(&mut app, Command::CommitPlant { plant, hours });
    }
    command(&mut app, Command::BeginOperating);
    command(&mut app, Command::AdvanceTicks { n: 900 });
    let battery = |app: &App| {
        app.world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .battery
            .unwrap()
    };
    let stored = battery(&app).stored;
    assert!(
        stored > MegawattHours::ZERO,
        "this task must save actual stored energy"
    );
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Plant(id))]);
    let pass = EguiPass::attach(&mut app);
    pass.ctx().set_zoom_factor(1.25);
    let size = (WIDTH / 1.25, HEIGHT / 1.25);
    let l10n = shipped_l10n();
    click(&pass, &mut app, size, &l10n.text("battery-disable"));
    update_n(&mut app, 2);
    assert!(!battery(&app).enabled);
    assert_eq!(battery(&app).stored, stored);
    let expected = app.world().resource::<Session>().0.checkpoint();
    click(&pass, &mut app, size, &l10n.text("save-title"));
    settle_save(&mut app);
    let widgets = pass.settled(&mut app, size.0, size.1);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput)
        .unwrap();
    pass.click_at(&mut app, size.0, size.1, field.rect.center());
    let enter = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    pass.run_with(
        &mut app,
        size.0,
        size.1,
        vec![
            egui::Event::Text("Charged but disabled".into()),
            enter(true),
            enter(false),
        ],
    );
    settle_save(&mut app);
    let entry = app
        .world()
        .resource::<game::sim::Persistence>()
        .last_saved()
        .unwrap()
        .clone();
    assert_eq!(entry.metadata.name, "Charged but disabled");
    assert!(
        dir.path()
            .join("saves")
            .join(format!("{}.mgsave", entry.metadata.slot.as_str()))
            .join("snapshot.ron.z")
            .is_file()
    );
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
    click(&pass, &mut app, size, &l10n.text("topbar-menu"));
    settle_save(&mut app);
    click(&pass, &mut app, size, &l10n.text("save-leave-unsaved"));
    update_n(&mut app, 3);
    settle_save(&mut app);
    assert!(!app.world().contains_resource::<Session>());
    click(&pass, &mut app, size, &l10n.text("save-load"));
    settle_save(&mut app);
    let widgets = pass.settled(&mut app, size.0, size.1);
    assert!(widgets.iter().any(|w| w.text == "Charged but disabled"));
    click(&pass, &mut app, size, &l10n.text("save-resume"));
    settle_save(&mut app);
    update_n(&mut app, 4);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
    assert_eq!(
        *app.world().resource::<game::sim::SimSpeed>(),
        game::sim::SimSpeed::Paused
    );
    assert!(!battery(&app).enabled);
    assert_eq!(battery(&app).stored, stored);
    dispatch_in(&mut app, vec![UiAction::Select(Selection::Plant(id))]);
    click(&pass, &mut app, size, &l10n.text("battery-enable"));
    update_n(&mut app, 2);
    assert!(battery(&app).enabled);
    assert_eq!(battery(&app).stored, stored);
    command(&mut app, Command::AdvanceTicks { n: 240 });
    assert!(
        battery(&app).stored < stored,
        "the restored battery supplies a genuine evening generation gap"
    );
    assert!(
        app.world()
            .resource::<game::l10n::Localizer>()
            .problems()
            .is_empty()
    );
}

#[test]
fn battery_day_flows_change_line_direction_and_survive_history_review_and_restore() {
    let (mut app, _dir, id) = app_with_battery();
    let l10n = shipped_l10n();
    let forecast = hud::forecast_model(
        &app.world().resource::<Session>().0,
        &l10n,
        &Default::default(),
    );
    assert!(forecast.storage_charge.iter().any(|[_, mw]| *mw > 0.0));
    // The committed fleet can cover expected demand. Storage holds its reserve;
    // live ramp gaps and weather/demand noise can still call on it.
    assert!(forecast.storage_discharge.iter().all(|[_, mw]| *mw == 0.0));
    assert!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .battery_schedule()
            .plants
            .values()
            .all(|p| p
                .discharge
                .iter()
                .all(|mw| *mw == domain::units::Megawatts::ZERO))
    );
    command(&mut app, Command::BeginOperating);
    let mut saw_charge = false;
    let mut saw_discharge = false;
    for _ in 0..45 {
        command(&mut app, Command::AdvanceTicks { n: 32 });
        let game = &app.world().resource::<Session>().0;
        let view = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap();
        let battery = view.battery.unwrap();
        let world = WorldModel::of(game);
        let line = game
            .session()
            .line_views()
            .into_iter()
            .find(|l| l.to == view.node || l.from == view.node)
            .unwrap();
        if battery.flow.charge.value() > 1.0 {
            saw_charge = true;
            assert_eq!(
                world.lines[&line.id].flow,
                Some(if line.to == view.node {
                    FlowDirection::Forward
                } else {
                    FlowDirection::Reverse
                })
            );
        }
        if battery.flow.discharge.value() > 1.0 {
            saw_discharge = true;
            assert_eq!(
                world.lines[&line.id].flow,
                Some(if line.from == view.node {
                    FlowDirection::Forward
                } else {
                    FlowDirection::Reverse
                })
            );
        }
    }
    assert!(saw_charge && saw_discharge);
    let game = &app.world().resource::<Session>().0;
    assert_eq!(game.session().phase(), DayPhase::Review);
    let report = game.projections().monthly_reports.latest().unwrap();
    assert!(report.storage[&id].input.value() > 0.0);
    assert_eq!(
        game.session()
            .plant_views()
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .battery
            .as_ref()
            .unwrap()
            .stored,
        MegawattHours::ZERO
    );
    let review = hud::review_model(game, hud::GameOverView::Summary, &l10n, &usd_style()).unwrap();
    assert!(!review.storage.is_empty());
    assert!(
        review
            .merit
            .iter()
            .all(|r| r.kind != l10n.text("plant-kind-battery"))
    );
    let graph = hud::supply_history_model(game, None, &l10n, &usd_style())
        .chart
        .unwrap();
    assert!(graph.has_storage);
    assert_eq!(graph.stored.len(), 1440);
    assert!(
        (graph.stored.last().unwrap()
            - report.storage[&id].unused.value() / f64::from(report.days))
        .abs()
            < 1e-8
    );
    let identity = infrastructure::save::replay_compatibility();
    let bundle = application::SaveBundle {
        compatibility: identity.clone(),
        metadata: application::SaveMetadata {
            slot: application::SaveId::new("a".repeat(32)).unwrap(),
            run: application::SaveId::new("b".repeat(32)).unwrap(),
            kind: application::SaveKind::Manual,
            name: "Battery day".into(),
            saved_at: 1,
        },
        checkpoint: game.checkpoint(),
    };
    let (decoded, region) = infrastructure::save::decode_save(
        &infrastructure::save::encode_save(&bundle).unwrap(),
        &identity,
    )
    .unwrap();
    let restored = application::Game::restore(
        region,
        decoded.checkpoint,
        Box::new(application::InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.projections(), game.projections());
    assert_eq!(restored.session().state_hash(), game.session().state_hash());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.world().resource::<game::sim::Persistence>().busy() {
        assert!(std::time::Instant::now() < deadline);
        app.update();
        std::thread::yield_now();
    }
    update_n(&mut app, 3);
    let pass = EguiPass::attach(&mut app);
    for zoom in [1.0, 1.25] {
        pass.ctx().set_zoom_factor(zoom);
        for (width, height) in [(WIDTH, HEIGHT), (1920.0, 1080.0)] {
            let (width, height) = (width / zoom, height / zoom);
            let widgets = pass.settled(&mut app, width, height);
            let at = |key: &str| {
                widgets
                    .iter()
                    .find(|w| {
                        w.text == l10n.text(key)
                            && w.role == bevy_egui::egui::accesskit::Role::Label
                    })
                    .expect(key)
                    .rect
            };
            assert!(at("review-pnl").left() < at("review-merit-order").left());
            assert!(at("review-merit-order").left() < at("battery-review-heading").left());
            let screen = bevy_egui::egui::Rect::from_min_size(
                bevy_egui::egui::Pos2::ZERO,
                bevy_egui::egui::vec2(width, height),
            );
            assert!(screen.contains_rect(button(&widgets, &l10n.text("ui-continue")).rect));
        }
    }
    assert_eq!(l10n.problems(), vec![]);
}

#[test]
fn chart_layout_click_is_shared_by_history_and_review_and_survives_new_game_and_restart() {
    use game::{
        hud::{PlanningTab, SupplyChartLayout, UiState},
        preferences::PreferencesPlugin,
    };
    let (mut app, dir, _) = app_with_battery();
    app.add_plugins(PreferencesPlugin);
    update_n(&mut app, 2);
    command(&mut app, Command::BeginOperating);
    command(&mut app, Command::AdvanceTicks { n: 60 });
    let pass = EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, WIDTH, HEIGHT);
    let l10n = shipped_l10n();
    let toggle = widgets
        .iter()
        .find(|w| w.text == l10n.text("supply-chart-combine"))
        .expect("combine toggle");
    assert!(egui_rect().contains_rect(toggle.rect));
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    pass.click_at(&mut app, WIDTH, HEIGHT, toggle.rect.center());
    assert_eq!(
        app.world().resource::<UiState>().supply_chart_layout,
        SupplyChartLayout::Combined
    );
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    // Continue to a completed month; the Review retrospective uses the same choice.
    command(&mut app, Command::AdvanceTicks { n: 1_380 });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.world().resource::<game::sim::Persistence>().busy() {
        assert!(std::time::Instant::now() < deadline);
        app.update();
        std::thread::yield_now();
    }
    update_n(&mut app, 3);
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Review
    );
    dispatch_in(&mut app, vec![UiAction::ShowReviewSupply(true)]);
    let widgets = pass.settled(&mut app, WIDTH, HEIGHT);
    let toggle = widgets
        .iter()
        .find(|w| w.text == l10n.text("supply-chart-combine"))
        .unwrap();
    assert!(toggle.selected);
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    pass.click_at(&mut app, WIDTH, HEIGHT, toggle.rect.center());
    assert_eq!(
        app.world().resource::<UiState>().supply_chart_layout,
        SupplyChartLayout::Separate
    );
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    dispatch_in(&mut app, vec![UiAction::ShowReviewSupply(false)]);
    command(&mut app, Command::FinishReview);
    dispatch_in(
        &mut app,
        vec![UiAction::SetPlanningTab(PlanningTab::History)],
    );
    let widgets = pass.settled(&mut app, WIDTH, HEIGHT);
    let toggle = widgets
        .iter()
        .find(|w| w.text == l10n.text("supply-chart-combine"))
        .unwrap();
    assert!(!toggle.selected);
    pass.click_at(&mut app, WIDTH, HEIGHT, toggle.rect.center());
    assert_eq!(
        app.world().resource::<UiState>().supply_chart_layout,
        SupplyChartLayout::Combined
    );
    dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    update_n(&mut app, 3);
    request(&mut app, "western_interconnection");
    assert_eq!(
        app.world().resource::<UiState>().supply_chart_layout,
        SupplyChartLayout::Combined
    );
    pass.settled(&mut app, WIDTH, HEIGHT);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    drop(app);
    assert!(
        infrastructure::client_preferences::read_preferences(&dir.path().join("saves"))
            .unwrap()
            .combined_supply_chart
    );
    let mut fresh = headless_app(dir.path(), &dir.path().join("fresh-logs"));
    fresh
        .insert_resource(SaveDir(dir.path().join("saves")))
        .add_plugins((hud::HudPlugin, PreferencesPlugin));
    update_n(&mut fresh, 3);
    assert_eq!(
        fresh.world().resource::<UiState>().supply_chart_layout,
        SupplyChartLayout::Combined
    );
}
