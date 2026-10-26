//! Shared helpers for the game integration tests.

pub mod egui_harness;
pub mod egui_pass;

use std::path::{Path, PathBuf};

use application::{Command, Game};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use domain::ids::RegionId;
use game::CorePlugins;
use game::hud::{ActionSink, HudPlugin, UiAction, dispatch, is_commitable};
use game::l10n::{LOCALE, Localizer};
use game::paths::{AssetRoot, LogDir};
use game::sim::{NewGameRequest, Session};
use game::state::AppState;

/// A windowless app with the core (headless-safe) plugins, reading assets from `assets` and
/// writing logs under `logs`.
pub fn headless_app(assets: &Path, logs: &Path) -> App {
    let mut app = App::new();
    app.insert_resource(AssetRoot(assets.to_path_buf()))
        .insert_resource(LogDir(logs.to_path_buf()))
        .add_plugins((MinimalPlugins, StatesPlugin, CorePlugins));
    app
}

/// Runs `n` frames.
pub fn update_n(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// Every region shipped in `assets/regions`, sorted like `RonRegionCatalog::list`. Each region
/// task inserts its ID here; the shipped-region name-key test checks it against the folder.
pub const SHIPPED_REGIONS: &[&str] = &[
    "brazil_sin",
    "continental_europe_maghreb",
    "eastern_interconnection",
    "india_nepal_bhutan",
    "ips_ups",
    "sgcc",
    "southern_china",
    "western_interconnection",
];

/// The workspace's shipped `assets/` directory. Tests only read from it.
pub fn shipped_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// The shipped en-US translations.
pub fn usd_style() -> game::l10n::format::MoneyStyle {
    game::l10n::format::MoneyStyle::resolve(domain::units::Currency::Usd, &shipped_l10n(), |_| true)
}

pub fn shipped_l10n() -> Localizer {
    Localizer::load(&shipped_assets().join("locales").join(LOCALE))
        .unwrap_or_else(|error| panic!("the shipped translations must load: {error}"))
}

/// A headless app with the core plugins and the HUD plugin, past boot, in the main menu, writing
/// logs under `logs`. Fixed time stands still, so no ticks run while a test is in game.
pub fn hud_app(logs: &Path) -> App {
    let mut app = headless_app(&shipped_assets(), logs);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ))
    .add_plugins(HudPlugin);
    update_n(&mut app, 3);
    assert_eq!(
        *app.world().resource::<State<AppState>>().get(),
        AppState::MainMenu
    );
    app
}

/// Asks for a new game in `region`, seed 7, and runs the frames it takes to start.
pub fn request(app: &mut App, region: &str) {
    // A player can only ask for a new game from the menus. With a save directory the app stays in
    // Boot until its first save scan finishes (spec §2.6), and a request written then is never
    // read: Bevy drops unread messages after two updates. A slow runner can still be scanning a
    // few frames in, so wait for the menus first. A boot error never leaves Boot; don't wait then.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot
        && !app.world().contains_resource::<game::state::BootError>()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the app never left Boot for the menus"
        );
        app.update();
        std::thread::yield_now();
    }
    app.world_mut().write_message(NewGameRequest {
        region: RegionId::new(region),
        seed: 7,
    });
    update_n(app, 2);
}

/// The commands that undo the inherited schedule (spec §16.3) as a player would in Planning:
/// `StopPlant` (the all-off pattern) and AGC off on every plant the grid can commit.
pub fn empty_start_commands(game: &Game) -> Vec<Command> {
    game.session()
        .plant_views()
        .into_iter()
        .filter(is_commitable)
        .flat_map(|plant| {
            [
                Command::StopPlant { plant: plant.id },
                Command::SetAutoFollow {
                    plant: plant.id,
                    on: false,
                },
            ]
        })
        .collect()
}

/// Recreates the empty start, nothing committed and nothing on AGC, on a bare `game`: for the
/// tests written for a fleet left offline.
pub fn empty_start(game: &mut Game) {
    for command in empty_start_commands(game) {
        game.execute(command).expect("an empty start in Planning");
    }
}

/// Recreates the empty start in the app's game, through the real `dispatch` and the frame that
/// applies the commands.
pub fn empty_start_in(app: &mut App) {
    let actions: Vec<UiAction> = empty_start_commands(&app.world().resource::<Session>().0)
        .into_iter()
        .map(UiAction::Command)
        .collect();
    app.world_mut()
        .run_system_once(move |mut sink: ActionSink| dispatch(actions.clone(), &mut sink))
        .unwrap();
    update_n(app, 1);
}
