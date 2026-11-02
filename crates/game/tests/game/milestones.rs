//! The Milestones panel's hidden fourth row and the toasts for milestones earned and revealed.
//!
//! The games here are real: new games, a real month closed through `dispatch`, a save loaded
//! through the persistence worker. Only the milestone projection is placed by hand (a hidden
//! bonus needs twelve fossil-free months, which no short test can play), through a real
//! checkpoint restore, so the app's own systems see a game like any other.

use application::projections::{MilestoneKind, MilestoneProgress, MilestoneStatus, Projections};
use application::{Command, Game, InMemoryCommandLog, RegionCatalog};
use bevy::prelude::*;
use bevy_egui::egui;
use domain::ids::RegionId;
use domain::session::GameSettings;
use domain::state::{CURRENT_STATE_SCHEMA, State as _};
use domain::time::SimMonth;
use domain::units::Megawatts;
use game::hud::{MilestoneNotice, MilestoneToast, MilestonesOpen, UiAction};
use game::l10n::Localize;
use game::paths::SaveDir;
use game::sim::{PersistAction, Session, SessionGeneration};
use game::state::AppState;
use infrastructure::region::RonRegionCatalog;
use tempfile::TempDir;

use crate::hud_smoke::dispatch_in;
use crate::support::egui_pass::EguiPass;
use crate::support::{
    assert_runs_before, hud_app, request, shipped_assets, shipped_l10n, update_n,
};

const WESTERN: &str = "western_interconnection";

fn catalog() -> RonRegionCatalog {
    RonRegionCatalog::new(shipped_assets().join("regions"))
}

fn new_game(region: &str) -> Game {
    Game::start_new(
        &catalog(),
        &RegionId::new(region),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
}

/// What to put in a hand-placed milestone projection.
struct Placed {
    /// Which of the three regional goals (by index) are reached, in the order they were.
    goals: &'static [usize],
    /// The bonus too, as reached.
    bonus_reached: bool,
    /// The best monthly peak already meets the peak goal, which the next month close then awards.
    peak_met: bool,
    clean_run: u32,
    revealed: bool,
}

/// The megawatts of a peak goal.
fn peak_goal(kind: MilestoneKind) -> f64 {
    match kind {
        MilestoneKind::ServePeak { gigawatts } => f64::from(gigawatts) * 1_000.0,
        MilestoneKind::ServePeakMegawatts { megawatts } => f64::from(megawatts),
        other => panic!("{other:?} is not a peak goal"),
    }
}

/// A copy of `game` whose milestone projection is `placed`, restored from a real checkpoint with
/// its state hash recomputed.
fn with_milestones(game: &Game, placed: &Placed) -> Game {
    let goals = *game.projections().milestones.targets();
    let month = game.session().clock().month();
    let mut reached: Vec<(MilestoneKind, SimMonth)> = placed
        .goals
        .iter()
        .map(|index| (goals[*index], month))
        .collect();
    if placed.bonus_reached {
        reached.push((MilestoneKind::FossilFreeYear, month));
    }
    let peak = if placed.peak_met || placed.goals.contains(&0) {
        peak_goal(goals[0])
    } else {
        0.0
    };
    let connected = game
        .session()
        .city_views()
        .iter()
        .filter(|city| city.connected)
        .count();
    let mut bytes = Vec::new();
    reached.write(&mut bytes);
    Vec::<MilestoneKind>::new().write(&mut bytes);
    Megawatts::new(peak).write(&mut bytes);
    u32::try_from(connected).unwrap().write(&mut bytes);
    false.write(&mut bytes);
    Option::<u8>::None.write(&mut bytes);
    goals.write(&mut bytes);
    placed.clean_run.write(&mut bytes);
    placed.revealed.then_some(month).write(&mut bytes);
    let milestones = application::projections::MilestoneProjection::from_state_bytes_schema(
        &bytes,
        CURRENT_STATE_SCHEMA,
    )
    .expect("hand-written milestone state decodes");

    let mut checkpoint = game.checkpoint();
    let snapshot = checkpoint.snapshot.as_mut().unwrap();
    let mut projections =
        Projections::from_state_bytes_schema(&snapshot.projections, snapshot.schema).unwrap();
    projections.milestones = milestones;
    snapshot.projections = projections.state_bytes_schema(snapshot.schema);
    checkpoint.state_hash = snapshot.state_hash();
    let region = catalog().load(game.session().region().id()).unwrap();
    Game::restore(
        region,
        checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .expect("the placed checkpoint restores")
}

/// Every `MilestoneToast` the app has sent, across frames.
#[derive(Resource, Default)]
struct Sent(Vec<MilestoneToast>);

fn record(mut sent: ResMut<Sent>, mut toasts: MessageReader<MilestoneToast>) {
    sent.0.extend(toasts.read().cloned());
}

/// A game in `region` with `placed` milestones, loaded from a save the way a player resumes one,
/// in an app that records milestone toasts.
fn app_with(logs: &TempDir, saves: &TempDir, region: &str, placed: &Placed) -> App {
    let game = with_milestones(&new_game(region), placed);
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let slot = crate::save_resume::write_manual(&store, "Placed", game.checkpoint());
    let mut app = app_on_saves(logs, saves);
    crate::save_resume::open_browser(&mut app);
    crate::save_resume::settle(&mut app);
    load(&mut app, slot);
    assert_eq!(state(&app), AppState::InGame);
    app
}

fn sent(app: &App) -> Vec<MilestoneToast> {
    app.world().resource::<Sent>().0.clone()
}

fn shown(app: &App) -> Vec<String> {
    let world = app.world();
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    game::hud::toasts_model(
        world.resource::<game::hud::ToastQueue>(),
        world.resource::<game::l10n::Localizer>(),
        now,
    )
    .toasts
}

fn play_a_day(app: &mut App) {
    dispatch_in(
        app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1_440 }),
        ],
    );
    update_n(app, 3);
}

fn goals(app: &App) -> [MilestoneKind; 3] {
    *app.world()
        .resource::<Session>()
        .0
        .projections()
        .milestones
        .targets()
}

// ---- the panel -----------------------------------------------------------------------------

#[test]
fn bonus_row_appears_only_after_reveal() {
    let l10n = shipped_l10n();
    let game = new_game(WESTERN);
    assert_eq!(game.projections().milestones.bonus(), None);
    assert_eq!(game::hud::milestones_model(&game, &l10n).rows.len(), 3);

    let revealed = with_milestones(
        &game,
        &Placed {
            goals: &[0, 1, 2],
            bonus_reached: false,
            peak_met: false,
            clean_run: 5,
            revealed: true,
        },
    );
    assert_eq!(
        revealed.projections().milestones.bonus(),
        Some(MilestoneKind::FossilFreeYear)
    );
    let model = game::hud::milestones_model(&revealed, &l10n);
    assert_eq!(model.rows.len(), 4);
    let bonus = &model.rows[3];
    assert_eq!(
        bonus.name,
        "Run a full year on clean power, without a strike"
    );
    assert_eq!(bonus.status, "In progress");
    assert_eq!(bonus.detail, "5 of 12 clean, strike-free months");
    assert!(!bonus.reached);
    assert!(model.rows[..3].iter().all(|row| row.reached));
    assert_eq!(l10n.problems(), vec![]);

    let earned = with_milestones(
        &game,
        &Placed {
            goals: &[0, 1, 2],
            bonus_reached: true,
            peak_met: false,
            clean_run: 12,
            revealed: true,
        },
    );
    let model = game::hud::milestones_model(&earned, &l10n);
    assert_eq!(model.rows.len(), 4);
    assert!(model.rows[3].reached);
    assert_eq!(model.rows[3].status, "Reached");
    assert!(matches!(
        earned.projections().milestones.status(
            MilestoneKind::FossilFreeYear,
            earned.session().clock().month(),
            earned.session().region().calendar()
        ),
        MilestoneStatus::Reached { .. }
    ));
}

#[test]
fn the_bonus_row_reads_zero_while_a_strike_is_pending() {
    // The projection says so; the panel only shows it (a strike is issued by a real close).
    let game = new_game(WESTERN);
    let status = game.projections().milestones.status(
        MilestoneKind::FossilFreeYear,
        game.session().clock().month(),
        game.session().region().calendar(),
    );
    assert_eq!(
        status,
        MilestoneStatus::InProgress(MilestoneProgress::CleanMonths {
            months: 0,
            needed: 12
        })
    );
}

/// At the smallest window and the largest UI zoom the four rows and Close are all on screen.
#[test]
fn milestones_panel_fits_four_rows_at_min_window_and_max_zoom() {
    const MAX_ZOOM: f32 = 1.5;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = app_with(
        &logs,
        &saves,
        WESTERN,
        &Placed {
            goals: &[0],
            bonus_reached: false,
            peak_met: false,
            clean_run: 11,
            revealed: true,
        },
    );
    let pass = EguiPass::attach(&mut app);
    pass.ctx().set_zoom_factor(MAX_ZOOM);
    dispatch_in(&mut app, vec![UiAction::ToggleMilestones]);
    assert!(app.world().resource::<MilestonesOpen>().0);
    let (width, height) = (
        game::window::MIN_WINDOW_SIZE.x / MAX_ZOOM,
        game::window::MIN_WINDOW_SIZE.y / MAX_ZOOM,
    );
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    let widgets = pass.settled(&mut app, width, height);

    let l10n = shipped_l10n();
    let model = game::hud::milestones_model(&app.world().resource::<Session>().0, &l10n);
    assert_eq!(model.rows.len(), 4);
    for row in &model.rows {
        let name = widgets
            .iter()
            .find(|w| w.text == row.name && w.role == egui::accesskit::Role::Label)
            .unwrap_or_else(|| panic!("{} is drawn", row.name));
        assert!(
            screen.contains_rect(name.rect),
            "{}: {:?} is off the screen {screen:?}",
            row.name,
            name.rect
        );
    }
    let detail = widgets
        .iter()
        .find(|w| w.text == "11 of 12 clean, strike-free months")
        .expect("the bonus row's progress is drawn");
    assert!(screen.contains_rect(detail.rect), "{:?}", detail.rect);
    let close = crate::support::egui_harness::button(&widgets, "Close").rect;
    assert!(
        screen.contains_rect(close),
        "Close {close:?} is off the screen"
    );
    let window = pass
        .ctx()
        .memory(|memory| memory.area_rect(egui::Id::new("milestones")))
        .expect("the Milestones window");
    assert!(
        screen.expand(1.0).contains_rect(window),
        "{window:?} is off the screen"
    );
}

// ---- the toasts ----------------------------------------------------------------------------

#[test]
fn announcing_runs_after_the_commands_and_a_load_and_before_the_toasts_are_collected() {
    let logs = TempDir::new().unwrap();
    let app = hud_app(logs.path());
    assert_runs_before(&app, "::apply_player_commands", "::announce_milestones");
    assert_runs_before(&app, "::poll_persistence", "::announce_milestones");
    assert_runs_before(&app, "::announce_milestones", "::collect_toasts");
}

#[test]
fn milestone_reached_toasts_once_through_the_real_dispatch() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = app_with(
        &logs,
        &saves,
        WESTERN,
        &Placed {
            goals: &[],
            bonus_reached: false,
            peak_met: true,
            clean_run: 0,
            revealed: false,
        },
    );
    // A new game's own frames, and the placed game's, raise nothing.
    assert_eq!(sent(&app), vec![]);
    assert_eq!(shown(&app), Vec::<String>::new());

    play_a_day(&mut app);

    let peak = goals(&app)[0];
    assert_eq!(
        sent(&app),
        vec![MilestoneToast(MilestoneNotice::Reached(peak))]
    );
    let name = peak.localize(&shipped_l10n());
    assert_eq!(shown(&app), vec![format!("Milestone reached: {name}")]);
    // Later frames and days neither repeat it nor add another.
    update_n(&mut app, 5);
    assert_eq!(sent(&app).len(), 1);
    assert_eq!(shown(&app), vec![format!("Milestone reached: {name}")]);
}

#[test]
fn bonus_reveal_and_award_toast_once() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    // The peak is the last goal: the real close reaches it, completing the three and revealing
    // the bonus together.
    let mut app = app_with(
        &logs,
        &saves,
        WESTERN,
        &Placed {
            goals: &[1, 2],
            bonus_reached: false,
            peak_met: true,
            clean_run: 3,
            revealed: false,
        },
    );
    assert_eq!(sent(&app), vec![]);
    let peak = goals(&app)[0];

    play_a_day(&mut app);

    let l10n = shipped_l10n();
    let bonus = MilestoneKind::FossilFreeYear;
    assert_eq!(
        sent(&app),
        vec![
            MilestoneToast(MilestoneNotice::Reached(peak)),
            MilestoneToast(MilestoneNotice::Revealed(bonus)),
        ]
    );
    let mut texts = shown(&app);
    texts.reverse(); // oldest first
    assert_eq!(
        texts,
        vec![
            format!("Milestone reached: {}", peak.localize(&l10n)),
            format!("New milestone: {}", bonus.localize(&l10n)),
        ]
    );
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .projections()
            .milestones
            .bonus(),
        Some(bonus)
    );
    update_n(&mut app, 5);
    assert_eq!(sent(&app).len(), 2, "a reveal is announced once");

    // The twelfth clean month awards it: the game as that close leaves it.
    let earned = with_milestones(
        &app.world().resource::<Session>().0,
        &Placed {
            goals: &[1, 2, 0],
            bonus_reached: true,
            peak_met: false,
            clean_run: 12,
            revealed: true,
        },
    );
    app.world_mut().resource_mut::<Session>().0 = earned;
    update_n(&mut app, 5);
    assert_eq!(
        sent(&app)[2..],
        [MilestoneToast(MilestoneNotice::Reached(bonus))]
    );
    update_n(&mut app, 5);
    assert_eq!(sent(&app).len(), 3, "an award is announced once");
    assert_eq!(
        shown(&app)[0],
        format!("Milestone reached: {}", bonus.localize(&l10n))
    );
}

#[test]
fn a_reveal_and_award_in_one_close_announce_the_reveal_first() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = app_with(
        &logs,
        &saves,
        WESTERN,
        &Placed {
            goals: &[1, 2],
            bonus_reached: false,
            peak_met: true,
            clean_run: 0,
            revealed: false,
        },
    );
    let peak = goals(&app)[0];
    let bonus = MilestoneKind::FossilFreeYear;
    // What that close would leave: the third goal, the reveal and the award at once.
    let game = with_milestones(
        &app.world().resource::<Session>().0,
        &Placed {
            goals: &[1, 2, 0],
            bonus_reached: true,
            peak_met: false,
            clean_run: 12,
            revealed: true,
        },
    );
    app.world_mut().resource_mut::<Session>().0 = game;
    update_n(&mut app, 3);

    assert_eq!(
        sent(&app),
        vec![
            MilestoneToast(MilestoneNotice::Reached(peak)),
            MilestoneToast(MilestoneNotice::Revealed(bonus)),
            MilestoneToast(MilestoneNotice::Reached(bonus)),
        ]
    );
}

// ---- loading -------------------------------------------------------------------------------

/// The schema 7 save with an extra connected city: its restore credits the city milestone.
const OLD_SAVE: &str = "../infrastructure/tests/support/pre-expansion-schema7/expanded.mgsave";

fn write_old_save(saves: &TempDir) -> application::SaveId {
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(OLD_SAVE))
        .expect("the old save fixture");
    let (bundle, _) =
        infrastructure::save::decode_save(&bytes, &infrastructure::save::replay_compatibility())
            .unwrap();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    crate::save_resume::write_manual(&store, "Old save", bundle.checkpoint)
}

fn load(app: &mut App, slot: application::SaveId) {
    dispatch_in(app, vec![UiAction::Persistence(PersistAction::Load(slot))]);
    crate::save_resume::settle(app);
    update_n(app, 6);
}

fn app_on_saves(logs: &TempDir, saves: &TempDir) -> App {
    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(saves.path().into()));
    app.init_resource::<Sent>().add_systems(Update, record);
    update_n(&mut app, 2);
    app
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

fn back_to_the_menu(app: &mut App) {
    dispatch_in(app, vec![UiAction::ReturnToMenu]);
    update_n(app, 5);
    assert_eq!(state(app), AppState::MainMenu);
}

/// A save awarded a milestone as it restored raises no toast, even after a game whose cursor held
/// fewer milestones: the load adopts the loaded game's counts.
#[test]
fn loading_a_save_raises_no_milestone_toasts() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let slot = write_old_save(&saves);
    let mut app = app_on_saves(&logs, &saves);
    request(&mut app, WESTERN);
    update_n(&mut app, 3);
    assert_eq!(state(&app), AppState::InGame);
    assert_eq!(sent(&app), vec![]);
    let generation = app.world().resource::<SessionGeneration>().0;
    back_to_the_menu(&mut app);
    crate::save_resume::open_browser(&mut app);
    crate::save_resume::settle(&mut app);

    load(&mut app, slot);

    assert_eq!(state(&app), AppState::InGame);
    assert_ne!(app.world().resource::<SessionGeneration>().0, generation);
    let loaded = &app.world().resource::<Session>().0;
    assert!(
        !loaded.projections().milestones.reached().is_empty(),
        "the old save was awarded a milestone as it restored"
    );
    assert_eq!(sent(&app), vec![]);
    assert_eq!(shown(&app), Vec::<String>::new());
    // The cursor holds what the load adopted: later frames stay quiet too.
    update_n(&mut app, 5);
    assert_eq!(sent(&app), vec![]);
}

/// The cursor of a game with four milestones doesn't carry into the next game: its first
/// milestone is announced.
#[test]
fn a_new_game_after_a_game_with_more_milestones_announces_its_first() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = app_with(
        &logs,
        &saves,
        WESTERN,
        &Placed {
            goals: &[0, 1, 2],
            bonus_reached: true,
            peak_met: false,
            clean_run: 12,
            revealed: true,
        },
    );
    back_to_the_menu(&mut app);
    request(&mut app, WESTERN);
    update_n(&mut app, 3);
    assert_eq!(state(&app), AppState::InGame);
    assert_eq!(sent(&app), vec![], "a new game raises nothing");

    let first = with_milestones(
        &app.world().resource::<Session>().0,
        &Placed {
            goals: &[0],
            bonus_reached: false,
            peak_met: false,
            clean_run: 0,
            revealed: false,
        },
    );
    app.world_mut().resource_mut::<Session>().0 = first;
    update_n(&mut app, 3);
    assert_eq!(
        sent(&app),
        vec![MilestoneToast(MilestoneNotice::Reached(goals(&app)[0]))]
    );
}
