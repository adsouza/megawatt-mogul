//! Automatic commitments (spec 2026-10-31 §5): the checkbox, its preference, the planner's
//! commands and the tab Planning opens on.

use crate::hud_smoke::{dispatch_in, sent};
use crate::support::{headless_app, shipped_assets, update_n};
use application::Command;
use bevy::prelude::*;
use game::hud::{
    GameOverView, LastSpeed, LegendChoice, MapLayer, MilestonesOpen, OpenPrompt, OperatingTab,
    PlanningTab, Selection, UiAction, UiState,
};
use game::sim::{CommandFailed, PlannerCommand, PlayerCommand, SessionGeneration};
use tempfile::TempDir;

/// A headless game in Planning with every resource `dispatch` writes.
fn planning_app(logs: &TempDir) -> App {
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.init_resource::<game::world::WorldControls>()
        .init_resource::<Selection>()
        .init_resource::<MapLayer>()
        .init_resource::<LegendChoice>()
        .init_resource::<OpenPrompt>()
        .init_resource::<MilestonesOpen>()
        .init_resource::<GameOverView>()
        .init_resource::<UiState>()
        .init_resource::<LastSpeed>()
        .init_resource::<OperatingTab>()
        .init_resource::<game::progress::ClientProgressStore>()
        .init_resource::<game::hud::tutorial::TutorialRun>();
    update_n(&mut app, 3);
    dispatch_in(
        &mut app,
        vec![UiAction::NewGame {
            region: domain::ids::RegionId::new("eastern_interconnection"),
            seed: 7,
        }],
    );
    update_n(&mut app, 3);
    app
}

fn planner_rounds(app: &App) -> Vec<PlannerCommand> {
    sent::<PlannerCommand>(app)
}

#[test]
fn set_auto_commitments_on_in_planning_sends_planner_commands() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    let expected =
        application::automatic_commitment_commands(&app.world().resource::<game::sim::Session>().0);
    assert!(
        !expected.is_empty(),
        "Eastern's start fleet has something to trim"
    );
    let generation = app.world().resource::<SessionGeneration>().0;
    dispatch_in(&mut app, vec![UiAction::SetAutoCommitments(true)]);
    assert!(app.world().resource::<UiState>().auto_commitments.0);
    assert_eq!(
        planner_rounds(&app),
        expected
            .into_iter()
            .map(|command| PlannerCommand(command, generation))
            .collect::<Vec<_>>()
    );
    assert!(sent::<PlayerCommand>(&app).is_empty());
    app.update();
    let diagnostic = std::fs::read_dir(logs.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".log.diagnostics.log"))
        .expect("the UI's planner run persists its reasoning beside the recording");
    let diagnostic = std::fs::read_to_string(diagnostic).unwrap();
    assert!(diagnostic.contains("automatic commitment month="));
    assert!(diagnostic.contains("equal estimated cost"));
}

#[test]
fn toggling_on_replans_once_and_off_changes_nothing() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    dispatch_in(&mut app, vec![UiAction::SetAutoCommitments(false)]);
    assert!(!app.world().resource::<UiState>().auto_commitments.0);
    assert!(planner_rounds(&app).is_empty());
    let round =
        application::automatic_commitment_commands(&app.world().resource::<game::sim::Session>().0)
            .len();
    dispatch_in(
        &mut app,
        vec![
            UiAction::SetAutoCommitments(true),
            UiAction::SetAutoCommitments(false),
            UiAction::SetAutoCommitments(true),
        ],
    );
    assert_eq!(planner_rounds(&app).len(), 2 * round);
}

#[test]
fn open_planning_tab_sets_only_the_tab() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    let before = app.world().resource::<UiState>().clone();
    dispatch_in(
        &mut app,
        vec![UiAction::OpenPlanningTab(PlanningTab::Commitments)],
    );
    let after = app.world().resource::<UiState>().clone();
    assert_eq!(after.tab, PlanningTab::Commitments);
    assert_eq!(
        UiState {
            tab: before.tab,
            ..after
        },
        before
    );
}

#[test]
fn planner_discards_an_open_draft() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    let plant = app
        .world()
        .resource::<game::sim::Session>()
        .0
        .session()
        .plant_views()[0]
        .id;
    dispatch_in(
        &mut app,
        vec![UiAction::EditCommitment {
            plant,
            hours: [false; 24],
        }],
    );
    assert!(app.world().resource::<UiState>().draft.is_some());
    dispatch_in(&mut app, vec![UiAction::RunAutoCommitments]);
    assert!(app.world().resource::<UiState>().draft.is_none());
}

#[test]
fn planner_refusal_logs_without_a_toast() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    let generation = app.world().resource::<SessionGeneration>().0;
    let before = app.world().resource::<game::sim::Session>().0.checkpoint();
    // Planning refuses AdvanceTicks: a planner command the domain turns down.
    app.world_mut()
        .write_message(PlannerCommand(Command::AdvanceTicks { n: 1 }, generation));
    app.update();
    assert!(sent::<CommandFailed>(&app).is_empty());
    assert_eq!(
        app.world().resource::<game::sim::Session>().0.checkpoint(),
        before
    );
}

#[test]
fn automatic_commitment_diagnostics_ignore_a_previous_session() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    let generation = app.world().resource::<SessionGeneration>().0;
    app.world_mut().write_message(game::sim::PlannerDiagnostic(
        "stale-planner-evidence".into(),
        generation.wrapping_sub(1),
    ));
    dispatch_in(&mut app, vec![UiAction::RunAutoCommitments]);
    app.update();
    let diagnostic = std::fs::read_dir(logs.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".log.diagnostics.log"))
        .unwrap();
    let diagnostic = std::fs::read_to_string(diagnostic).unwrap();
    assert!(!diagnostic.contains("stale-planner-evidence"));
    assert_eq!(diagnostic.matches("automatic commitment month=").count(), 1);
}

mod planning_begins {
    use super::*;
    use crate::save_resume::{open_browser, settle};
    use crate::support::{hud_app, request};
    use game::paths::SaveDir;
    use game::sim::{PersistAction, Persistence, Session};

    const EAST: &str = "eastern_interconnection";

    fn game(app: &App) -> &application::Game {
        &app.world().resource::<Session>().0
    }

    fn patterns(app: &App) -> Vec<[bool; 24]> {
        game(app)
            .session()
            .plant_views()
            .iter()
            .map(|plant| plant.commitment)
            .collect()
    }

    fn tab(app: &App) -> PlanningTab {
        app.world().resource::<UiState>().tab
    }

    /// A new game in the HUD app, given a few frames for the planner's commands to land.
    fn started(logs: &TempDir, auto: bool) -> App {
        let mut app = hud_app(logs.path());
        update_n(&mut app, 2);
        if auto {
            crate::support::auto_commitments_on(&mut app);
        }
        request(&mut app, EAST);
        update_n(&mut app, 3);
        app
    }

    #[test]
    fn new_game_opens_on_forecast_with_trimmed_patterns() {
        let logs = TempDir::new().unwrap();
        let app = started(&logs, true);
        assert_eq!(tab(&app), PlanningTab::Forecast);
        assert!(application::automatic_commitment_commands(game(&app)).is_empty());
        assert!(
            patterns(&app).iter().any(|hours| hours != &[true; 24]),
            "the inherited all-day schedule was trimmed"
        );
    }

    #[test]
    fn with_the_preference_off_nothing_is_replanned_and_the_tab_is_commitments() {
        let logs = TempDir::new().unwrap();
        let app = started(&logs, false);
        assert_eq!(tab(&app), PlanningTab::Commitments);
        assert!(!application::automatic_commitment_commands(game(&app)).is_empty());
    }

    #[test]
    fn finish_review_replans_and_opens_on_forecast() {
        let logs = TempDir::new().unwrap();
        let mut app = started(&logs, true);
        // A hand edit for this month: every plant all day.
        let plants: Vec<_> = game(&app)
            .session()
            .plant_views()
            .iter()
            .filter(|plant| plant.kind.is_dispatchable())
            .map(|plant| plant.id)
            .collect();
        dispatch_in(
            &mut app,
            plants
                .into_iter()
                .map(|plant| {
                    UiAction::Command(Command::CommitPlant {
                        plant,
                        hours: [true; 24],
                    })
                })
                .chain([
                    UiAction::SetPlanningTab(PlanningTab::Tariff),
                    UiAction::Command(Command::BeginOperating),
                    UiAction::Command(Command::AdvanceTicks { n: 1440 }),
                ])
                .collect(),
        );
        settle(&mut app);
        update_n(&mut app, 2);
        assert!(patterns(&app).iter().all(|hours| hours == &[true; 24]));
        dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
        update_n(&mut app, 4);
        assert_eq!(
            game(&app).session().phase(),
            domain::session::DayPhase::Planning
        );
        assert_eq!(tab(&app), PlanningTab::Forecast);
        crate::support::settle_automatic_commitments(&mut app);
        assert!(application::automatic_commitment_commands(game(&app)).is_empty());
        assert!(patterns(&app).iter().any(|hours| hours != &[true; 24]));
    }

    #[test]
    fn review_precomputes_without_mutating_the_game_and_continue_reuses_it() {
        let logs = TempDir::new().unwrap();
        let mut app = started(&logs, true);
        dispatch_in(
            &mut app,
            vec![
                UiAction::Command(Command::BeginOperating),
                UiAction::Command(Command::AdvanceTicks { n: 1440 }),
            ],
        );
        settle(&mut app);
        update_n(&mut app, 2);
        let before = game(&app).checkpoint();
        let diagnostics_path = std::fs::read_dir(logs.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.to_string_lossy().ends_with(".log.diagnostics.log"))
            .unwrap();
        let diagnostics_before = std::fs::read_to_string(&diagnostics_path).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !app
            .world()
            .resource::<game::sim::ReviewCommitmentCache>()
            .is_ready()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "review precomputation finished"
            );
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(game(&app).checkpoint(), before);
        assert_eq!(
            std::fs::read_to_string(&diagnostics_path).unwrap(),
            diagnostics_before
        );
        dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
        update_n(&mut app, 3);
        assert_eq!(
            game(&app).session().phase(),
            domain::session::DayPhase::Planning
        );
        assert_eq!(tab(&app), PlanningTab::Forecast);
        assert!(
            !app.world()
                .resource::<game::sim::ReviewCommitmentCache>()
                .is_applying()
        );
        assert!(application::automatic_commitment_commands(game(&app)).is_empty());
        let diagnostics_after = std::fs::read_to_string(diagnostics_path).unwrap();
        assert_eq!(
            diagnostics_after
                .matches("automatic commitment month=")
                .count(),
            diagnostics_before
                .matches("automatic commitment month=")
                .count()
                + 1
        );
    }

    #[test]
    fn loading_a_planning_manual_save_keeps_its_patterns() {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        app.insert_resource(SaveDir(saves.path().into()));
        update_n(&mut app, 2);
        crate::support::auto_commitments_on(&mut app);
        request(&mut app, EAST);
        update_n(&mut app, 3);
        // Undo the plan by hand, then save mid-Planning.
        let plants: Vec<_> = game(&app)
            .session()
            .plant_views()
            .iter()
            .filter(|plant| plant.kind.is_dispatchable())
            .map(|plant| plant.id)
            .collect();
        dispatch_in(
            &mut app,
            plants
                .into_iter()
                .map(|plant| {
                    UiAction::Command(Command::CommitPlant {
                        plant,
                        hours: [true; 24],
                    })
                })
                .collect(),
        );
        update_n(&mut app, 2);
        let edited = patterns(&app);
        assert!(edited.iter().all(|hours| hours == &[true; 24]));
        dispatch_in(
            &mut app,
            vec![UiAction::Persistence(PersistAction::OpenSave {
                leave: false,
            })],
        );
        settle(&mut app);
        dispatch_in(
            &mut app,
            vec![UiAction::Persistence(PersistAction::SaveNew(
                "Planning".into(),
            ))],
        );
        settle(&mut app);
        let slot = app
            .world()
            .resource::<Persistence>()
            .last_saved()
            .unwrap()
            .metadata
            .slot
            .clone();
        drop(app);
        let mut app = hud_app(logs.path());
        app.insert_resource(SaveDir(saves.path().into()));
        update_n(&mut app, 2);
        crate::support::auto_commitments_on(&mut app);
        open_browser(&mut app);
        settle(&mut app);
        dispatch_in(
            &mut app,
            vec![UiAction::Persistence(PersistAction::Load(slot))],
        );
        settle(&mut app);
        update_n(&mut app, 6);
        assert_eq!(patterns(&app), edited);
        assert_eq!(tab(&app), PlanningTab::Forecast);
    }
}

#[test]
fn ticking_the_checkbox_on_discards_an_open_draft() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(&logs);
    dispatch_in(&mut app, vec![UiAction::SetAutoCommitments(false)]);
    let plant = app
        .world()
        .resource::<game::sim::Session>()
        .0
        .session()
        .plant_views()[0]
        .id;
    dispatch_in(
        &mut app,
        vec![UiAction::EditCommitment {
            plant,
            hours: [false; 24],
        }],
    );
    assert!(app.world().resource::<UiState>().draft.is_some());
    dispatch_in(&mut app, vec![UiAction::SetAutoCommitments(true)]);
    assert!(app.world().resource::<UiState>().draft.is_none());
}
