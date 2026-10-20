//! One whole month through the app, for each shipped region: a new game, the hands-off plan
//! clicked in through the presenters' own buttons and the real `dispatch`, the day run at 8× by
//! the fixed-step driver, and the review continued into the next Planning.
//!
//! The app is headless (the core plugins and the HUD's non-egui systems, no `EguiPlugin`), so the
//! panels never draw: each step takes the action of the button the player would click from the
//! panel's model. Time is never real: fixed time stands still until the day starts, and then each
//! `app.update()` runs exactly one fixed step. The outcome on seed 7 is the simulation's business
//! (the acceptance runs in `infrastructure` pin it); this test asserts only that the app carries a
//! month from end to end.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use application::{Command, CommandError, Game};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use domain::session::DayPhase;
use game::hud::{
    ActionSink, CommitmentForm, ToastQueue, UiAction, UiState, commitment_model, dispatch,
    is_commitable, planning_model, review_model, speed_model, toasts_model,
};
use game::l10n::{Localizer, Names, Toast};
use game::paths::LogDir;
use game::sim::{CommandFailed, DomainEventMessage, PlayerCommand, Session, SimSpeed};
use game::state::PhaseState;
use tempfile::TempDir;

use crate::support::{hud_app, request, update_n};

/// The month's day has 1,440 ticks; 8× runs 8 per fixed step. A margin for the frames around it.
const MAX_FIXED_STEPS: usize = 1_440 / 8 + 20;

/// Everything the app did, as seen at the end of each frame.
#[derive(Resource, Default)]
struct Record {
    /// Every command that reached the game, from the HUD or the driver.
    commands: Vec<Command>,
    failures: Vec<CommandError>,
    /// The phase after each change, starting with the first one seen in a game.
    phases: Vec<PhaseState>,
    /// Every toast the overlay would have shown.
    toasts: BTreeSet<String>,
    /// The toast text of every domain event that has one.
    event_toasts: BTreeSet<String>,
    /// `(commands so far, lines in the log)`, at the end of each frame in a game.
    log_lines: Vec<(usize, usize)>,
}

/// Runs in `Last`, after the commands were applied and the toasts collected.
#[allow(clippy::too_many_arguments)]
fn record(
    mut commands: MessageReader<PlayerCommand>,
    mut failures: MessageReader<CommandFailed>,
    mut events: MessageReader<DomainEventMessage>,
    session: Option<Res<Session>>,
    phase: Option<Res<State<PhaseState>>>,
    l10n: Res<Localizer>,
    queue: Res<ToastQueue>,
    time: Res<Time<Real>>,
    log_dir: Res<LogDir>,
    mut record: ResMut<Record>,
) {
    record
        .commands
        .extend(commands.read().map(|command| command.0.clone()));
    record
        .failures
        .extend(failures.read().map(|failed| failed.0.clone()));
    let Some(session) = session else {
        events.clear();
        return;
    };
    let names = Names::new(&l10n, session.0.session());
    let event_toasts: Vec<String> = events
        .read()
        .filter_map(|event| event.0.toast(&l10n, &names))
        .collect();
    record.event_toasts.extend(event_toasts);
    let shown = toasts_model(&queue, &l10n, time.elapsed_secs_f64()).toasts;
    record.toasts.extend(shown);
    if let Some(phase) = phase.map(|phase| *phase.get())
        && record.phases.last() != Some(&phase)
    {
        record.phases.push(phase);
    }
    let lines = line_count(&the_log(&log_dir.0));
    let so_far = record.commands.len();
    record.log_lines.push((so_far, lines));
}

/// The one command log a game wrote under `dir` (beside it sits the region archive).
fn the_log(dir: &Path) -> PathBuf {
    let logs: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "log"))
        .collect();
    assert_eq!(logs.len(), 1, "{logs:?}");
    logs.into_iter().next().unwrap()
}

fn line_count(path: &Path) -> usize {
    fs::read_to_string(path).unwrap().lines().count()
}

fn game(app: &App) -> &Game {
    &app.world().resource::<Session>().0
}

fn phase(app: &App) -> PhaseState {
    *app.world().resource::<State<PhaseState>>().get()
}

/// Runs `actions` through the real `dispatch`, as a panel system does after drawing.
fn click(app: &mut App, actions: Vec<UiAction>) {
    app.world_mut()
        .run_system_once(move |mut sink: ActionSink| dispatch(actions.clone(), &mut sink))
        .unwrap();
}

/// The actions of the hands-off plan, from the commitment grid: *All day* on every row not yet
/// committed all day, and the AGC switch of every row where it is off.
fn hands_off_clicks(app: &App) -> Vec<UiAction> {
    let world = app.world();
    let grid = commitment_model(
        game(app),
        world.resource::<Localizer>(),
        world.resource::<UiState>(),
        world.resource::<CommitmentForm>(),
    );
    let rows = grid.groups.iter().flat_map(|group| &group.rows);
    let mut actions = Vec::new();
    for row in rows {
        if row.all_day.enabled {
            actions.push(row.all_day.action.clone());
        }
        if !row.agc.on {
            actions.push(row.agc.button.action.clone());
        }
    }
    actions
}

fn begin_the_day(app: &App) -> UiAction {
    let world = app.world();
    let planning = planning_model(
        game(app),
        world.resource::<Localizer>(),
        world.resource::<UiState>(),
        world.resource::<CommitmentForm>(),
    )
    .expect("the Planning panel is up");
    assert!(planning.begin.enabled, "{:?}", planning.begin_hint);
    planning.begin.action
}

fn eight_times(app: &App) -> UiAction {
    let world = app.world();
    let speeds = speed_model(
        game(app),
        world.resource::<Localizer>(),
        *world.resource::<SimSpeed>(),
    );
    speeds
        .speeds
        .into_iter()
        .map(|toggle| toggle.button.action)
        .find(|action| *action == UiAction::SetSpeed(SimSpeed::X8))
        .expect("the speed bar has 8×")
}

fn continue_from_review(app: &App) -> UiAction {
    let review = review_model(game(app), app.world().resource::<Localizer>())
        .expect("the month's review is up");
    assert!(review.continue_button.enabled);
    review.continue_button.action
}

fn play_a_hands_off_month(region: &str) {
    let started = Instant::now();
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    app.init_resource::<Record>().add_systems(Last, record);

    // 1. A new game, seed 7.
    request(&mut app, region);
    assert_eq!(phase(&app), PhaseState::Planning);
    let month = game(&app).session().clock().month();

    // 2. The hands-off plan: every dispatchable plant committed all day, with AGC on.
    let plan = hands_off_clicks(&app);
    assert!(!plan.is_empty());
    click(&mut app, plan);
    update_n(&mut app, 1);
    assert_eq!(hands_off_clicks(&app), [], "every row is all day with AGC");
    let plants = game(&app).session().plant_views();
    let commitable: Vec<_> = plants.iter().filter(|plant| is_commitable(plant)).collect();
    assert!(!commitable.is_empty());
    for plant in commitable {
        assert_eq!(plant.commitment, [true; 24], "{:?}", plant.id);
        assert!(plant.auto_follow, "{:?}", plant.id);
    }

    // 3. Start the day.
    let begin = begin_the_day(&app);
    click(&mut app, vec![begin]);
    update_n(&mut app, 2);
    assert_eq!(phase(&app), PhaseState::Operating);

    // 4. 8×, and from now on one fixed step per frame.
    let fast = eight_times(&app);
    click(&mut app, vec![fast]);
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));

    // 5. The driver runs the day to the month close.
    let mut steps = 0;
    while phase(&app) != PhaseState::Review {
        assert!(
            steps < MAX_FIXED_STEPS,
            "{region}: no review after {steps} fixed steps (tick {})",
            game(&app).session().clock().tick()
        );
        app.update();
        steps += 1;
    }
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
    let next = continue_from_review(&app);
    click(&mut app, vec![next]);
    update_n(&mut app, 2);

    // The phases, in order.
    assert_eq!(phase(&app), PhaseState::Planning);
    assert_eq!(game(&app).session().phase(), DayPhase::Planning);
    let record = app.world().resource::<Record>();
    assert_eq!(
        record.phases,
        [
            PhaseState::Planning,
            PhaseState::Operating,
            PhaseState::Review,
            PhaseState::Planning,
        ]
    );

    // Nothing was refused, and every toast came from a domain event, none from a refusal.
    assert_eq!(record.failures, []);
    for toast in &record.toasts {
        assert!(
            record
                .event_toasts
                .iter()
                .any(|event| toast.contains(event.as_str())),
            "{region}: a toast no event explains: {toast}"
        );
    }

    // The driver sent the day in batches of eight, and the log grew by one line per command.
    let advances: Vec<u32> = record
        .commands
        .iter()
        .filter_map(|command| match command {
            Command::AdvanceTicks { n } => Some(*n),
            _ => None,
        })
        .collect();
    assert_eq!(advances, [8; 1_440 / 8]);
    assert!(record.commands.contains(&Command::BeginOperating));
    assert_eq!(record.commands.last(), Some(&Command::FinishReview));
    let (first_commands, first_lines) = record.log_lines[0];
    let header = first_lines - first_commands;
    assert!(header >= 1, "the log starts with its header");
    for &(commands, lines) in &record.log_lines {
        assert_eq!(lines, header + commands, "after {commands} commands");
    }
    assert_eq!(
        record.log_lines.last().map(|&(commands, _)| commands),
        Some(record.commands.len())
    );

    // The month's report is on record.
    let reports = game(&app).projections().monthly_reports.history();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].month, month);

    eprintln!(
        "end_to_end {region}: {steps} fixed steps, {:.2} s",
        started.elapsed().as_secs_f64()
    );
}

#[test]
fn a_hands_off_eastern_month_runs_through_the_app() {
    play_a_hands_off_month("eastern_interconnection");
}

#[test]
fn a_hands_off_western_month_runs_through_the_app() {
    play_a_hands_off_month("western_interconnection");
}
