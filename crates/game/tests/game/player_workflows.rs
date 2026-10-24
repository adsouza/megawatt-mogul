//! Player tasks through actual widgets, dispatch, simulation and retained history.
use application::Command;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_egui::egui;
use domain::{region::PlantKind, session::DayPhase, units::Megawatts};
use game::{
    hud::{self, CommitmentForm, OpenPrompt, PlanningTab, UiState},
    paths::SaveDir,
    sim::{CommandFailed, Persistence, PlayerCommand, Session, SimSpeed},
};
use tempfile::TempDir;

use crate::hud_smoke::sent;
use crate::support::egui_harness::{Widget, button};
use crate::support::egui_pass::EguiPass;
use crate::support::{empty_start_in, hud_app, request, shipped_l10n, update_n};

pub(super) fn click(pass: &EguiPass, app: &mut App, size: (f32, f32), label: &str) {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0, size.1));
    for _ in 0..12 {
        let widgets = pass.settled(app, size.0, size.1);
        let target = button(&widgets, label);
        if screen.contains_rect(target.rect) {
            assert!(!target.disabled, "{label} must be enabled");
            pass.click_at(app, size.0, size.1, target.rect.center());
            return;
        }
        // Rows can be below the commitment grid's viewport. Reach them as a
        // player does, without editing UI resources or dispatching their actions.
        let direction = if target.rect.bottom() > screen.bottom() {
            -1.0
        } else {
            1.0
        };
        pass.run_with(
            app,
            size.0,
            size.1,
            vec![
                egui::Event::PointerMoved(egui::pos2(target.rect.center().x, size.1 - 80.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0.0, direction * 120.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    panic!("{label} must be reachable after scrolling");
}

fn key(pass: &EguiPass, app: &mut App, size: (f32, f32), key: egui::Key) {
    let event = |pressed| egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    pass.run_with(app, size.0, size.1, vec![event(true), event(false)]);
}

pub(super) fn settle_save(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        if !app.world().resource::<Persistence>().busy() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "autosave must finish");
        std::thread::yield_now();
    }
    update_n(app, 2);
}

#[test]
fn edit_a_valid_schedule_check_forecast_operate_review_and_revisit_history() {
    for (region, zoom) in [
        ("eastern_interconnection", 1.0),
        ("western_interconnection", 1.25),
    ] {
        let dir = TempDir::new().unwrap();
        let mut app = hud_app(&dir.path().join("logs"));
        app.insert_resource(SaveDir(dir.path().join("saves")));
        request(&mut app, region);
        empty_start_in(&mut app);
        update_n(&mut app, 2);
        let l10n = shipped_l10n();
        let plant = app
            .world()
            .resource::<Session>()
            .0
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.kind == PlantKind::Peaker)
            .unwrap();
        let capacity = app
            .world()
            .resource::<Session>()
            .0
            .session()
            .region()
            .spec(plant.kind)
            .capacity;
        let pass = EguiPass::attach(&mut app);
        pass.ctx().set_zoom_factor(zoom);
        let size = (
            game::window::MIN_WINDOW_SIZE.x / zoom,
            game::window::MIN_WINDOW_SIZE.y / zoom,
        );
        let row = hud::commitment_model(
            &app.world().resource::<Session>().0,
            &l10n,
            app.world().resource::<UiState>(),
            &CommitmentForm::default(),
        )
        .groups
        .into_iter()
        .flat_map(|g| g.rows)
        .find(|r| r.plant == plant.id)
        .unwrap();
        let before = app.world().resource::<Session>().0.checkpoint();
        click(&pass, &mut app, size, &row.cells[18].label);
        assert_eq!(
            app.world().resource::<Session>().0.checkpoint(),
            before,
            "a draft has no simulation effect"
        );
        let widgets = pass.settled(&mut app, size.0, size.1);
        assert!(button(&widgets, "Start the day").disabled);
        let commands_before = sent::<PlayerCommand>(&app).len();
        click(&pass, &mut app, size, "Apply");
        let mut hours = [false; 24];
        hours[18] = true;
        assert_eq!(
            sent::<PlayerCommand>(&app)[commands_before..]
                .iter()
                .map(|c| &c.0)
                .collect::<Vec<_>>(),
            [&Command::CommitPlant {
                plant: plant.id,
                hours
            }]
        );
        update_n(&mut app, 2);
        assert_eq!(
            app.world()
                .resource::<Session>()
                .0
                .session()
                .plant_views()
                .into_iter()
                .find(|p| p.id == plant.id)
                .unwrap()
                .commitment,
            hours
        );

        click(&pass, &mut app, size, &l10n.text("forecast-title"));
        let model = hud::forecast_model(
            &app.world().resource::<Session>().0,
            &l10n,
            app.world().resource::<UiState>(),
        );
        let peaker = model
            .stacks
            .iter()
            .find(|s| s.kind == PlantKind::Peaker)
            .unwrap();
        for (hour, mw) in peaker.mw.iter().enumerate() {
            // Manual initial target is what was committed; nameplate capacity is an upper bound.
            let target = plant.initial_target.value();
            assert_eq!(*mw, if hour == 18 { target } else { 0.0 });
            assert!(*mw <= capacity.value());
        }
        let warning = app
            .world()
            .resource::<Session>()
            .0
            .session()
            .capacity_shortfall_check()
            .unwrap();
        assert!(
            model
                .demand_minutes
                .iter()
                .zip(&model.net_supply)
                .any(|(d, s)| (d[1] - s[1] - warning.shortfall.value()).abs() < 1e-6)
        );
        click(&pass, &mut app, size, "Start the day");
        let unchanged = app.world().resource::<Session>().0.checkpoint();
        key(&pass, &mut app, size, egui::Key::Escape);
        assert_eq!(*app.world().resource::<OpenPrompt>(), OpenPrompt(None));
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), unchanged);
        click(&pass, &mut app, size, "Start the day");
        click(&pass, &mut app, size, "Start anyway");
        assert_eq!(
            sent::<PlayerCommand>(&app)
                .iter()
                .filter(|c| c.0 == Command::BeginOperating)
                .count(),
            1
        );
        update_n(&mut app, 2);
        click(&pass, &mut app, size, "32×");
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        for _ in 0..60 {
            app.update();
            assert!(sent::<CommandFailed>(&app).is_empty());
            if app.world().resource::<Session>().0.session().phase() == DayPhase::Review {
                break;
            }
        }
        assert_eq!(
            app.world().resource::<Session>().0.session().phase(),
            DayPhase::Review
        );
        assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
        settle_save(&mut app);
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        let report = app
            .world()
            .resource::<Session>()
            .0
            .projections()
            .monthly_reports
            .latest()
            .unwrap()
            .clone();
        assert!(
            report.unserved.value() > 0.0,
            "the deliberately short plan really has a shortfall"
        );
        click(&pass, &mut app, size, "Supply demand retrospective");
        assert!(app.world().resource::<UiState>().review_supply);
        let history = hud::supply_history_model(&app.world().resource::<Session>().0, None, &l10n)
            .chart
            .unwrap();
        assert_eq!(history.demand.len(), 1440);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
        click(&pass, &mut app, size, "Review summary");
        click(&pass, &mut app, size, "Continue");
        update_n(&mut app, 2);
        assert_eq!(
            app.world().resource::<Session>().0.session().phase(),
            DayPhase::Planning
        );
        click(&pass, &mut app, size, &l10n.text("supply-history-title"));
        assert_eq!(app.world().resource::<UiState>().tab, PlanningTab::History);
        assert_eq!(
            hud::supply_history_model(&app.world().resource::<Session>().0, None, &l10n)
                .chart
                .unwrap(),
            history
        );
        assert_eq!(
            app.world()
                .resource::<Session>()
                .0
                .projections()
                .monthly_reports
                .latest(),
            Some(&report)
        );
        assert_eq!(l10n.problems(), vec![]);
    }
}

#[test]
fn infeasible_schedule_refusal_returns_to_an_editable_usable_plan() {
    let dir = TempDir::new().unwrap();
    let mut app = hud_app(&dir.path().join("logs"));
    app.insert_resource(SaveDir(dir.path().join("saves")));
    request(&mut app, "eastern_interconnection");
    let l10n = shipped_l10n();
    let grid = hud::commitment_model(
        &app.world().resource::<Session>().0,
        &l10n,
        app.world().resource::<UiState>(),
        &CommitmentForm::default(),
    );
    let row = grid.groups.iter().flat_map(|g| &g.rows).next().unwrap();
    let plant = row.plant;
    let label = row.cells[0].label.clone();
    let pass = EguiPass::attach(&mut app);
    let size = (
        game::window::MIN_WINDOW_SIZE.x,
        game::window::MIN_WINDOW_SIZE.y,
    );
    let before = app.world().resource::<Session>().0.session().state_hash();
    click(&pass, &mut app, size, &label);
    click(&pass, &mut app, size, "Apply");
    update_n(&mut app, 1);
    assert_eq!(
        sent::<CommandFailed>(&app).len(),
        1,
        "one short off-gap must be refused"
    );
    update_n(&mut app, 2);
    assert_eq!(
        app.world().resource::<Session>().0.session().state_hash(),
        before
    );
    assert!(app.world().resource::<UiState>().draft.is_none());
    let widgets: Vec<Widget> = pass.settled(&mut app, size.0, size.1);
    assert!(!button(&widgets, "Start the day").disabled);
    let actual = app
        .world()
        .resource::<Session>()
        .0
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.id == plant)
        .unwrap();
    assert_eq!(actual.commitment, [true; 24]);
    let forecast = hud::forecast_model(
        &app.world().resource::<Session>().0,
        &l10n,
        app.world().resource::<UiState>(),
    );
    assert!(
        forecast
            .net_supply
            .iter()
            .any(|p| p[1] > Megawatts::ZERO.value())
    );
    click(&pass, &mut app, size, "Start the day");
    update_n(&mut app, 2);
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        DayPhase::Operating
    );
}
