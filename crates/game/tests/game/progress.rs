//! Player progress in the game (first-game tutorial spec §5, §6): `progress.ron` loads once,
//! writes when it changes, is never overwritten when it exists but can't be read, and records a
//! region once its game has been played.

use crate::support::{hud_app, request, update_n};
use application::Command;
use application::Game;
use bevy::prelude::*;
use domain::{ids::NodeId, time::TICKS_PER_DAY};
use game::{
    hud::is_commitable,
    paths::SaveDir,
    progress::{ClientProgressStore, ProgressPlugin},
    sim::Session,
};
use infrastructure::client_progress::{ClientProgress, read_progress, write_progress};
use std::collections::BTreeSet;
use tempfile::TempDir;

fn progress_app(root: &std::path::Path) -> App {
    let mut app = App::new();
    app.insert_resource(SaveDir(root.into()))
        .add_plugins((MinimalPlugins, ProgressPlugin));
    update_n(&mut app, 2);
    app
}

#[test]
fn progress_loads_and_writes_in_tempdir() {
    // A missing file loads as the default and is not written until something changes.
    let empty = TempDir::new().unwrap();
    let mut app = progress_app(empty.path());
    let store = app.world().resource::<ClientProgressStore>();
    assert!(store.loaded);
    assert_eq!(store.value, ClientProgress::default());
    update_n(&mut app, 2);
    assert!(!empty.path().join("progress.ron").exists());

    let root = TempDir::new().unwrap();
    let saved = ClientProgress {
        tutorial_pending: false,
        played_regions: BTreeSet::from(["eastern_interconnection".to_owned()]),
        ..Default::default()
    };
    write_progress(root.path(), &saved).unwrap();
    let mut app = progress_app(root.path());
    let store = app.world().resource::<ClientProgressStore>();
    assert!(store.loaded);
    assert_eq!(store.value, saved);

    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .played_regions
        .insert("western_interconnection".into());
    update_n(&mut app, 1);
    let expected = app.world().resource::<ClientProgressStore>().value.clone();
    assert_eq!(expected.played_regions.len(), 2);
    assert_eq!(read_progress(root.path()).unwrap(), expected);

    // A fresh app reads back what the last one wrote.
    let fresh = progress_app(root.path());
    assert_eq!(
        fresh.world().resource::<ClientProgressStore>().value,
        expected
    );
}

#[test]
fn unreadable_progress_is_never_overwritten() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("progress.ron");
    let garbage = b"(version: 1, tutorial_pending: maybe".to_vec();
    std::fs::write(&path, &garbage).unwrap();
    let mut app = progress_app(root.path());
    assert!(app.world().resource::<ClientProgressStore>().loaded);
    app.world_mut()
        .resource_mut::<ClientProgressStore>()
        .value
        .played_regions
        .insert("western_interconnection".into());
    update_n(&mut app, 3);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert_eq!(std::fs::read(&path).unwrap(), garbage);
    assert!(
        app.world()
            .resource::<ClientProgressStore>()
            .value
            .tutorial_pending,
        "the game runs on in-memory defaults"
    );
}

/// A line that connects the smallest city the start leaves unconnected, so the start fleet can
/// still serve it, by its quickest route.
fn smallest_extra_line(game: &Game) -> Command {
    let session = game.session();
    let connected: Vec<NodeId> = session
        .city_views()
        .iter()
        .filter(|city| city.connected)
        .map(|city| city.node)
        .collect();
    let (_, _, from, to) = session
        .city_views()
        .into_iter()
        .filter(|city| !city.connected)
        .flat_map(|city| connected.iter().map(move |to| (city.clone(), *to)))
        .filter_map(|(city, to)| {
            let quote = session.line_quote(city.node, to).ok()?;
            let load = session.region().city(city.id)?.base_load.value();
            Some((load, quote.months, city.node, to))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .expect("an unconnected city can be reached");
    Command::BuildLine { from, to }
}

/// The hands-off plan: every commitable plant committed all day with AGC on, which keeps a
/// shipped start fleet out of regulatory strikes for a year.
fn hands_off_plan(game: &Game) -> Vec<Command> {
    game.session()
        .plant_views()
        .into_iter()
        .filter(is_commitable)
        .flat_map(|plant| {
            [
                Command::CommitPlant {
                    plant: plant.id,
                    hours: [true; 24],
                },
                Command::SetAutoFollow {
                    plant: plant.id,
                    on: true,
                },
            ]
        })
        .collect()
}

fn execute(app: &mut App, command: Command) {
    let mut session = app.world_mut().resource_mut::<Session>();
    let month = session.0.session().clock().month();
    if let Err(error) = session.0.execute(command) {
        panic!("{error:?} in {month:?}");
    }
}

/// Closes one whole month on the app's game directly, then runs the frames that observe it.
fn close_month(app: &mut App) {
    let plan = hands_off_plan(&app.world().resource::<Session>().0);
    for command in plan {
        execute(app, command);
    }
    for command in [
        Command::BeginOperating,
        Command::AdvanceTicks { n: TICKS_PER_DAY },
        Command::FinishReview,
    ] {
        execute(app, command);
    }
    update_n(app, 2);
}

#[test]
fn played_region_is_recorded_once() {
    let logs = TempDir::new().unwrap();
    let mut app = hud_app(logs.path());
    let store = app.world().resource::<ClientProgressStore>();
    assert!(!store.loaded, "no I/O happens without ProgressPlugin");
    request(&mut app, "western_interconnection");
    let game = &app.world().resource::<Session>().0;
    let mut commands = hands_off_plan(game);
    commands.push(smallest_extra_line(game));
    for command in commands {
        execute(&mut app, command);
    }
    let played = |app: &App| {
        let game = &app.world().resource::<Session>().0;
        game.projections().expansion.played()
    };
    let recorded = |app: &App| {
        let store = app.world().resource::<ClientProgressStore>();
        store
            .value
            .played_regions
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut closes = 0;
    while !played(&app) {
        assert!(recorded(&app).is_empty(), "recorded before it was played");
        close_month(&mut app);
        closes += 1;
        assert!(closes < 40, "the region never counted as played");
    }
    assert_eq!(recorded(&app), ["western_interconnection"]);
    // Later months, and frames, never add it again or anything else.
    close_month(&mut app);
    update_n(&mut app, 3);
    assert_eq!(recorded(&app), ["western_interconnection"]);
}

/// A new game in every shipped region starts unexpanded: every city it starts out serving is one
/// of its region's `start.connected_cities`, so no region counts towards "played" (spec §6)
/// before the player has connected a city of their own.
#[test]
fn every_shipped_region_starts_unexpanded() {
    use application::{InMemoryCommandLog, projections::ExpansionProjection};
    use domain::{ids::RegionId, session::GameSettings};
    use infrastructure::region::RonRegionCatalog;

    let catalog = RonRegionCatalog::new(crate::support::shipped_assets().join("regions"));
    for &region in crate::support::SHIPPED_REGIONS {
        let game = Game::start_new(
            &catalog,
            &RegionId::new(region),
            7,
            GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap_or_else(|error| panic!("{region}: {error:?}"));
        let session = game.session();
        let starters = &session.region().start().connected_cities;
        let extra: Vec<_> = session
            .city_views()
            .into_iter()
            .filter(|city| city.connected && !starters.contains(&city.id))
            .map(|city| city.id)
            .collect();
        assert_eq!(
            extra,
            [],
            "{region}: connected at the start but not a starter city"
        );
        let expansion = &game.projections().expansion;
        assert!(!expansion.played(), "{region}");
        assert_eq!(
            *expansion,
            ExpansionProjection::default(),
            "{region}: no count"
        );
    }
}
