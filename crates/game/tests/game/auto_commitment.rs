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
