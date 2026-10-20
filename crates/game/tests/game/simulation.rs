//! The simulation bridge: new games, the command funnel, phase mirroring and the fixed-step driver.
//!
//! Time is never real here. Every app starts on `TimeUpdateStrategy::ManualDuration(ZERO)`, so no
//! fixed step runs until a test switches to `FixedTimesteps(1)`: then each `app.update()` runs
//! `FixedUpdate` exactly once.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use application::{
    Command, CommandError, CommandLogError, CommandLogSink, Game, LogHeader, LogRecord,
    RegionCatalog,
};
use bevy::app::FixedMain;
use bevy::ecs::message::Message;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use domain::error::DomainError;
use domain::events::DomainEvent;
use domain::ids::RegionId;
use domain::session::{DayPhase, GameSettings, SessionStatus};
use domain::units::Megawatts;
use game::sim::{
    Catalog, CommandFailed, DomainEventMessage, NewGameRequest, PlayerCommand, ReturnToMenu,
    Session, SimSpeed,
};
use game::state::{AppState, PhaseState};
use infrastructure::region::RonRegionCatalog;
use tempfile::TempDir;

use crate::support::{empty_start_commands, headless_app, shipped_assets, update_n};

const EASTERN: &str = "eastern_interconnection";

/// Every message of type `M` seen so far, collected in `Last` so none expires unread.
#[derive(Resource)]
struct Seen<M>(Vec<M>);

impl<M> Default for Seen<M> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

fn collect<M: Message + Clone>(mut reader: MessageReader<M>, mut seen: ResMut<Seen<M>>) {
    seen.0.extend(reader.read().cloned());
}

fn watch<M: Message + Clone>(app: &mut App) {
    app.init_resource::<Seen<M>>()
        .add_systems(Last, collect::<M>);
}

/// A headless app past boot, in the main menu, with no fixed steps until a test asks for them.
fn sim_app(assets: &Path, logs: &Path) -> App {
    let mut app = headless_app(assets, logs);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    watch::<CommandFailed>(&mut app);
    watch::<DomainEventMessage>(&mut app);
    watch::<PlayerCommand>(&mut app);
    update_n(&mut app, 3);
    assert_eq!(app_state(&app), AppState::MainMenu);
    app
}

/// A shipped-assets app with a new eastern game in Planning.
fn planning_app(logs: &Path) -> App {
    let mut app = sim_app(&shipped_assets(), logs);
    start_game(&mut app, EASTERN, 7);
    app
}

fn start_game(app: &mut App, region: &str, seed: u64) {
    app.world_mut().write_message(NewGameRequest {
        region: RegionId::new(region),
        seed,
    });
    update_n(app, 2);
}

fn send(app: &mut App, commands: impl IntoIterator<Item = Command>) {
    for command in commands {
        app.world_mut().write_message(PlayerCommand(command));
    }
}

/// Starts the day and lets the phase mirror catch up.
fn begin_operating(app: &mut App) {
    send(app, [Command::BeginOperating]);
    update_n(app, 2);
    assert_eq!(phase_state(app), Some(PhaseState::Operating));
}

/// From now on each `app.update()` runs exactly one fixed step (at the 4 Hz timestep).
fn run_fixed_steps(app: &mut App) {
    app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
}

fn set_speed(app: &mut App, speed: SimSpeed) {
    app.insert_resource(speed);
}

fn game(app: &App) -> &Game {
    &app.world().resource::<Session>().0
}

fn tick(app: &App) -> u32 {
    game(app).session().clock().tick()
}

fn app_state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

fn phase_state(app: &App) -> Option<PhaseState> {
    app.world()
        .get_resource::<State<PhaseState>>()
        .map(|state| *state.get())
}

fn failures(app: &App) -> Vec<CommandError> {
    let seen = app.world().resource::<Seen<CommandFailed>>();
    seen.0.iter().map(|failed| failed.0.clone()).collect()
}

fn events(app: &App) -> Vec<DomainEvent> {
    let seen = app.world().resource::<Seen<DomainEventMessage>>();
    seen.0.iter().map(|message| message.0.clone()).collect()
}

/// The `n` of every `AdvanceTicks` sent so far, by anyone.
fn advance_requests(app: &App) -> Vec<u32> {
    let seen = app.world().resource::<Seen<PlayerCommand>>();
    seen.0
        .iter()
        .filter_map(|command| match command.0 {
            Command::AdvanceTicks { n } => Some(n),
            _ => None,
        })
        .collect()
}

fn wrong_phase(actual: DayPhase) -> CommandError {
    CommandError::Domain(DomainError::WrongPhase { actual })
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Copies `assets/regions` and `assets/locales` under `root`, for a test that edits them.
fn copy_assets(root: &Path) -> PathBuf {
    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let path = entry.unwrap().path();
            let target = to.join(path.file_name().unwrap());
            if path.is_dir() {
                copy_dir(&path, &target);
            } else {
                fs::copy(&path, &target).unwrap();
            }
        }
    }
    let assets = root.join("assets");
    for sub in ["regions", "locales"] {
        copy_dir(&shipped_assets().join(sub), &assets.join(sub));
    }
    assets
}

#[test]
fn boot_inserts_the_region_catalog() {
    let logs = TempDir::new().unwrap();
    let app = sim_app(&shipped_assets(), logs.path());

    let catalog = &app.world().resource::<Catalog>().0;
    let listing = catalog.list().unwrap();
    assert!(listing.iter().any(|entry| entry.id.as_str() == EASTERN));
}

#[test]
fn a_new_game_request_enters_planning_and_writes_a_log() {
    let root = TempDir::new().unwrap();
    // The log directory doesn't exist yet: starting a game creates it.
    let logs = root.path().join("logs").join("nested");

    let app = planning_app(&logs);

    assert_eq!(app_state(&app), AppState::InGame);
    assert_eq!(phase_state(&app), Some(PhaseState::Planning));
    assert_eq!(game(&app).session().phase(), DayPhase::Planning);
    assert_eq!(game(&app).session().region().id(), &RegionId::new(EASTERN));

    let names = files_in(&logs);
    assert_eq!(names.len(), 2, "{names:?}");
    let log = &names[0];
    assert_eq!(names[1], format!("{log}.region.ron"));
    // <YYYYMMDD-HHMMSS>-<region>-<seed>.log
    let stamp = log
        .strip_suffix("-eastern_interconnection-7.log")
        .unwrap_or_else(|| panic!("unexpected log name {log}"));
    let (date, time) = stamp.split_once('-').unwrap();
    assert_eq!((date.len(), time.len()), (8, 6), "{stamp}");
    assert!(stamp.chars().all(|c| c.is_ascii_digit() || c == '-'));

    let archived = fs::read(logs.join(&names[1])).unwrap();
    let shipped = fs::read(shipped_assets().join("regions/eastern_interconnection.ron")).unwrap();
    assert_eq!(archived, shipped);
    assert!(
        fs::read_to_string(logs.join(log))
            .unwrap()
            .contains("Header")
    );
}

#[test]
fn a_failed_new_game_stays_in_the_menu_and_leaves_no_log() {
    let logs = TempDir::new().unwrap();
    let mut app = sim_app(&shipped_assets(), logs.path());

    start_game(&mut app, "atlantis", 7);

    assert_eq!(app_state(&app), AppState::MainMenu);
    assert!(!app.world().contains_resource::<Session>());
    let failed = failures(&app);
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(
        matches!(failed[0], CommandError::Region(_)),
        "{:?}",
        failed[0]
    );
    assert_eq!(files_in(logs.path()), Vec::<String>::new());
}

#[test]
fn commands_apply_in_order_and_re_emit_events() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());

    // Out of order, AdvanceTicks would be rejected in Planning.
    send(
        &mut app,
        [Command::BeginOperating, Command::AdvanceTicks { n: 3 }],
    );
    app.update();

    assert_eq!(failures(&app), []);
    assert_eq!(tick(&app), 3);
    let events = events(&app);
    assert!(events.contains(&DomainEvent::PhaseChanged {
        from: DayPhase::Planning,
        to: DayPhase::Operating,
    }));
    // The HUD reads ticks from the projections; three ticks re-emit nothing per tick.
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, DomainEvent::TickCompleted(_)))
    );
}

#[test]
fn a_rejected_command_becomes_one_command_failed() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    let before = game(&app).session().state_hash();

    send(&mut app, [Command::AdvanceTicks { n: 1 }]);
    update_n(&mut app, 3);

    assert_eq!(failures(&app), [wrong_phase(DayPhase::Planning)]);
    assert_eq!(game(&app).session().state_hash(), before);
    assert_eq!(phase_state(&app), Some(PhaseState::Planning));
}

#[test]
fn commands_queued_across_a_phase_change_are_rejected_cleanly() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    let plant = game(&app).session().plant_views()[0].id;
    begin_operating(&mut app);

    // The day ends inside the first command; the target change arrives in Review.
    send(
        &mut app,
        [
            Command::AdvanceTicks { n: 1440 },
            Command::SetDispatchTarget {
                plant,
                mw: Megawatts::new(0.0),
            },
        ],
    );
    update_n(&mut app, 2);

    assert_eq!(game(&app).session().phase(), DayPhase::Review);
    assert_eq!(phase_state(&app), Some(PhaseState::Review));
    assert_eq!(failures(&app), [wrong_phase(DayPhase::Review)]);
}

#[test]
fn operating_advances_speed_ticks_per_fixed_step() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    begin_operating(&mut app);
    assert_eq!(
        app.world().resource::<Time<Fixed>>().timestep(),
        Duration::from_millis(250),
        "4 Hz fixed steps"
    );
    set_speed(&mut app, SimSpeed::X4);
    run_fixed_steps(&mut app);
    let start = tick(&app);

    update_n(&mut app, 5);

    assert_eq!(tick(&app), start + 4 * 5);
    assert_eq!(advance_requests(&app), [4; 5]);
    assert_eq!(failures(&app), []);
}

#[test]
fn a_slow_frame_still_sends_one_batch() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    begin_operating(&mut app);
    set_speed(&mut app, SimSpeed::X2);
    let start = tick(&app);

    // A long frame catches up with several fixed steps before `Update` applies anything.
    for _ in 0..3 {
        app.world_mut().run_schedule(FixedMain);
    }
    app.update();

    assert_eq!(advance_requests(&app), [2]);
    assert_eq!(tick(&app), start + 2);
}

#[test]
fn pause_stops_ticks() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    begin_operating(&mut app);
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::X1);
    run_fixed_steps(&mut app);

    update_n(&mut app, 2);
    assert_eq!(tick(&app), 2);

    set_speed(&mut app, SimSpeed::Paused);
    update_n(&mut app, 5);

    assert_eq!(tick(&app), 2);
    assert_eq!(advance_requests(&app), [1, 1]);
}

#[test]
fn the_month_close_mirrors_review_and_pauses() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    begin_operating(&mut app);
    send(&mut app, [Command::AdvanceTicks { n: 1436 }]);
    app.update();
    set_speed(&mut app, SimSpeed::X8);
    run_fixed_steps(&mut app);

    // Eight ticks asked for, four left: the batch stops at the month close.
    update_n(&mut app, 6);

    assert_eq!(game(&app).session().phase(), DayPhase::Review);
    assert_eq!(phase_state(&app), Some(PhaseState::Review));
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
    assert_eq!(advance_requests(&app), [1436, 8]);
    assert_eq!(failures(&app), []);
    assert!(
        events(&app)
            .iter()
            .any(|event| matches!(event, DomainEvent::MonthClosed(_)))
    );
}

#[test]
fn game_over_stops_the_driver() {
    // Two routes to game over at the third month close, so the test doesn't hang on balance
    // tuning. Undoing the inherited schedule leaves the start fleet offline, and the regulator's
    // strikes revoke the licence by the third close. And this copy of the eastern region starts deep in
    // debt, so the third negative close is also bankruptcy (revocation, checked first, wins today).
    let root = TempDir::new().unwrap();
    let assets = copy_assets(root.path());
    let region = assets.join("regions").join(format!("{EASTERN}.ron"));
    let source = fs::read_to_string(&region).unwrap();
    let solvent = "cash_usd: 5000000000,";
    assert!(source.contains(solvent), "the shipped start cash changed");
    fs::write(
        &region,
        source.replace(solvent, "cash_usd: -1000000000000,"),
    )
    .unwrap();
    let logs = root.path().join("logs");
    let mut app = sim_app(&assets, &logs);
    start_game(&mut app, EASTERN, 7);
    let empty = empty_start_commands(game(&app));
    send(&mut app, empty);

    let day = [
        Command::BeginOperating,
        Command::AdvanceTicks { n: 1440 },
        Command::FinishReview,
    ];
    send(&mut app, day.iter().cloned().chain(day.iter().cloned()));
    send(
        &mut app,
        [Command::BeginOperating, Command::AdvanceTicks { n: 1432 }],
    );
    update_n(&mut app, 2);
    assert_eq!(game(&app).session().status(), SessionStatus::Active);
    assert_eq!(phase_state(&app), Some(PhaseState::Operating));

    // The driver runs the last eight minutes into the month close, and the game ends there.
    set_speed(&mut app, SimSpeed::X8);
    run_fixed_steps(&mut app);
    update_n(&mut app, 3);
    assert!(
        matches!(game(&app).session().status(), SessionStatus::GameOver(_)),
        "{:?}",
        game(&app).session().status()
    );
    assert!(
        events(&app)
            .iter()
            .any(|event| matches!(event, DomainEvent::LicenseRevoked | DomainEvent::Bankrupt))
    );
    let sent = advance_requests(&app).len();

    // Even with the speed forced back up, nothing more is asked of a finished game.
    set_speed(&mut app, SimSpeed::X8);
    update_n(&mut app, 20);

    assert_eq!(advance_requests(&app).len(), sent);
    assert_eq!(failures(&app), []);
}

/// Accepts the header, then fails every command: a disk that filled up after the game started.
struct FullDisk;

impl CommandLogSink for FullDisk {
    fn initialize(&mut self, _: &LogHeader, _: &[u8]) -> Result<(), CommandLogError> {
        Ok(())
    }

    fn append(&mut self, _: &LogRecord) -> Result<(), CommandLogError> {
        Err(CommandLogError {
            message: "disk full".to_owned(),
        })
    }
}

#[test]
fn a_failing_log_keeps_the_game_running() {
    let logs = TempDir::new().unwrap();
    let mut app = sim_app(&shipped_assets(), logs.path());
    let catalog = RonRegionCatalog::new(shipped_assets().join("regions"));
    let game_with_full_disk = Game::start_new(
        &catalog,
        &RegionId::new(EASTERN),
        7,
        GameSettings::default(),
        Box::new(FullDisk),
    )
    .unwrap();
    app.insert_resource(Session(game_with_full_disk));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();

    begin_operating(&mut app);
    run_fixed_steps(&mut app);
    update_n(&mut app, 3);

    assert!(game(&app).log_failure().is_some());
    assert_eq!(tick(&app), 3);
    assert_eq!(failures(&app), []);
}

#[test]
fn return_to_menu_drops_the_session() {
    let logs = TempDir::new().unwrap();
    let mut app = planning_app(logs.path());
    begin_operating(&mut app);

    app.world_mut().write_message(ReturnToMenu);
    update_n(&mut app, 2);

    assert_eq!(app_state(&app), AppState::MainMenu);
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(phase_state(&app), None);
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
}
