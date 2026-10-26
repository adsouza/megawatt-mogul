use crate::hud_smoke::dispatch_in;
use crate::support::usd_style;
use crate::support::{hud_app, request, update_n};
use application::Command;
use bevy::prelude::*;
use game::{
    hud::UiAction,
    paths::SaveDir,
    sim::{PersistAction, Persistence, Session, SimSpeed},
};
use tempfile::TempDir;

fn settle(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        app.update();
        if !app.world().resource::<Persistence>().busy() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("persistence worker did not complete");
}
/// Opens the load browser the way a player can: the menu's Load game button is disabled while
/// persistence is frozen (a foreground job such as the menu's save scan), so wait until it would
/// be enabled, then dispatch. A slow machine can still be scanning a few frames after returning
/// to the menu, and an `OpenBrowser` dispatched then is ignored. Callers settle afterwards.
pub(crate) fn open_browser(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.world().resource::<Persistence>().frozen() {
        assert!(
            std::time::Instant::now() < deadline,
            "persistence stayed frozen; Load game would stay disabled"
        );
        app.update();
        std::thread::yield_now();
    }
    action(app, PersistAction::OpenBrowser);
}
#[test]
fn save_resume_mid_day_restores_a_paused_runnable_game_with_all_projections() {
    for &region in crate::support::SHIPPED_REGIONS {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        app.add_plugins(game::world::WorldViewPlugin);
        app.insert_resource(SaveDir(saves.path().into()));
        request(&mut app, region);
        order_across_economic_zones(&mut app.world_mut().resource_mut::<Session>().0);
        dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
        update_n(&mut app, 3);
        app.world_mut()
            .resource_mut::<Session>()
            .0
            .execute(Command::AdvanceTicks { n: 877 })
            .unwrap();
        let before = app.world().resource::<Session>().0.checkpoint();
        let projections = app.world().resource::<Session>().0.projections().clone();
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
                "Afternoon".into(),
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
        let saved_directory = saves.path().join(format!("{}.mgsave", slot.as_str()));
        assert!(
            saved_directory.is_dir(),
            "production Save writes a directory snapshot"
        );
        assert!(saved_directory.join("snapshot.ron.z").is_file());
        assert!(
            std::fs::read_dir(&saved_directory).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".bin.z"))
        );
        let mut uninterrupted = app.world_mut().remove_resource::<Session>().unwrap().0;
        drop(app);
        let mut app = hud_app(logs.path());
        app.add_plugins(game::world::WorldViewPlugin);
        app.insert_resource(SaveDir(saves.path().into()));
        update_n(&mut app, 2);
        open_browser(&mut app);
        settle(&mut app);
        dispatch_in(
            &mut app,
            vec![UiAction::Persistence(PersistAction::Load(slot))],
        );
        settle(&mut app);
        update_n(&mut app, 6);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), before);
        assert_eq!(
            app.world().resource::<Session>().0.projections(),
            &projections
        );
        assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
        assert_eq!(
            app.world().resource::<game::world::ViewSync>().plants.len(),
            app.world()
                .resource::<Session>()
                .0
                .session()
                .plant_views()
                .len()
        );
        assert_eq!(
            app.world()
                .resource::<game::world::WorldScene>()
                .model
                .as_ref()
                .unwrap()
                .phase,
            domain::session::DayPhase::Operating
        );
        uninterrupted
            .execute(Command::AdvanceTicks { n: 1440 })
            .unwrap();
        dispatch_in(
            &mut app,
            vec![UiAction::Command(Command::AdvanceTicks { n: 1440 })],
        );
        settle(&mut app);
        assert_eq!(
            app.world().resource::<Session>().0.checkpoint(),
            uninterrupted.checkpoint()
        );
        assert_eq!(
            app.world().resource::<Session>().0.projections(),
            uninterrupted.projections()
        );
        for command in [
            Command::FinishReview,
            Command::BeginOperating,
            Command::AdvanceTicks { n: 75 },
        ] {
            uninterrupted.execute(command.clone()).unwrap();
            dispatch_in(&mut app, vec![UiAction::Command(command)]);
            app.update();
        }
        assert_eq!(
            app.world().resource::<Session>().0.checkpoint(),
            uninterrupted.checkpoint()
        );
        assert_eq!(
            app.world().resource::<Session>().0.projections(),
            uninterrupted.projections()
        );
    }
}

/// In a region with economic zones (Europe-Maghreb), orders a Wind plant in the nonzero zone
/// nearest home other than home's own, and a line from home to it, so the save carries a
/// cross-zone plant and route in flight. Other regions are left unchanged.
fn order_across_economic_zones(game: &mut application::Game) {
    use domain::{events::DomainEvent, ids::CellCoord, region::PlantKind};
    let region = std::sync::Arc::clone(game.session().region());
    if !region.has_economic_zones() {
        return;
    }
    let home = region.city(region.start().home_city).unwrap().cell;
    let home_zone = region.economic_zone_at(home).unwrap();
    let occupied: Vec<CellCoord> = game
        .session()
        .plant_views()
        .iter()
        .map(|p| p.cell)
        .chain(game.session().city_views().iter().map(|c| c.cell))
        .collect();
    let terrain = region.terrain();
    let cell = (0..terrain.height())
        .flat_map(|y| (0..terrain.width()).map(move |x| CellCoord { x, y }))
        .filter(|c| {
            let zone = region.economic_zone_at(*c).unwrap();
            zone.0 != 0
                && zone != home_zone
                && !occupied.contains(c)
                && region.site_check(PlantKind::Wind, *c).is_ok()
        })
        .min_by_key(|c| {
            (
                u32::from(c.x.abs_diff(home.x)).pow(2) + u32::from(c.y.abs_diff(home.y)).pow(2),
                *c,
            )
        })
        .expect("another zone has a wind site");
    let plant = game
        .execute(Command::BuildPlant {
            kind: PlantKind::Wind,
            cell,
        })
        .unwrap()
        .iter()
        .find_map(|e| match e {
            DomainEvent::PlantConstructionStarted { plant, .. } => Some(*plant),
            _ => None,
        })
        .unwrap();
    let node = game
        .session()
        .plant_views()
        .into_iter()
        .find(|p| p.id == plant)
        .unwrap()
        .node;
    let from = game.session().anchor_node();
    let quote = game.session().line_quote(from, node).unwrap();
    assert!(quote.zones.len() > 1, "the route crosses zones: {quote:?}");
    game.execute(Command::BuildLine { from, to: node }).unwrap();
}

fn action(app: &mut App, action: PersistAction) {
    dispatch_in(app, vec![UiAction::Persistence(action)]);
}
fn operating(logs: &TempDir, saves: &TempDir) -> App {
    let mut app = hud_app(logs.path());
    app.add_plugins(game::world::WorldViewPlugin);
    app.insert_resource(SaveDir(saves.path().into()));
    request(&mut app, "western_interconnection");
    dispatch_in(&mut app, vec![UiAction::Command(Command::BeginOperating)]);
    update_n(&mut app, 3);
    app
}

#[test]
fn save_resume_drains_the_in_flight_tick_once_before_manual_capture() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::X8)]);
    app.world_mut().run_schedule(FixedUpdate);
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Session>().0.session().clock().tick(),
        8
    );
    action(&mut app, PersistAction::SaveNew("Queued tick".into()));
    settle(&mut app);
    let slot = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .unwrap()
        .metadata
        .slot
        .clone();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let (bundle, _) = application::SaveStore::read(&store, &slot).unwrap();
    assert_eq!(bundle.checkpoint.summary.tick, 8);
    assert_eq!(
        bundle
            .checkpoint
            .commands
            .iter()
            .filter(|c| matches!(c, Command::AdvanceTicks { .. }))
            .count(),
        1
    );
    for _ in 0..3 {
        app.world_mut().run_schedule(FixedUpdate);
        app.update();
    }
    assert_eq!(
        app.world().resource::<Session>().0.session().clock().tick(),
        8
    );
}

#[test]
fn save_resume_closing_autosave_cannot_fulfil_a_pending_save_and_menu() {
    use application::SaveStore;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::AdvanceTicks { n: 1439 })
        .unwrap();
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::AdvanceTicks { n: 1 }),
            UiAction::Command(Command::FinishReview),
            UiAction::Persistence(PersistAction::OpenSave { leave: true }),
        ],
    );
    settle(&mut app);
    assert_eq!(
        app.world().resource::<State<game::state::AppState>>().get(),
        &game::state::AppState::InGame
    );
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Save
    );
    assert_eq!(
        app.world().resource::<Session>().0.session().phase(),
        domain::session::DayPhase::Review
    );
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let entries = store.list().unwrap();
    assert_eq!(entries.len(), 1);
    let auto = store.read(entries[0].id.as_ref().unwrap()).unwrap().0;
    assert_eq!(auto.metadata.kind, application::SaveKind::Automatic);
    assert!(!auto.checkpoint.commands.contains(&Command::FinishReview));
    action(&mut app, PersistAction::SaveNew("Completed month".into()));
    settle(&mut app);
    update_n(&mut app, 3);
    assert!(!app.world().contains_resource::<Session>());
    let entries = store.list().unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        let saved = store.read(entry.id.as_ref().unwrap()).unwrap().0;
        assert_eq!(
            saved.checkpoint.summary.phase,
            domain::session::DayPhase::Review
        );
        assert!(!saved.checkpoint.commands.contains(&Command::FinishReview));
    }
}

#[test]
fn save_resume_failed_save_and_menu_keeps_the_game_until_matching_retry_succeeds() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
        move |stage| {
            if stage == infrastructure::save::PublishStage::PartialWrite
                && flag.load(Ordering::Relaxed)
            {
                Err(application::SaveError::Io {
                    stage: "injected",
                    message: "full".into(),
                })
            } else {
                Ok(())
            }
        },
    ));
    let mut app = hud_app(logs.path());
    app.insert_resource(Persistence::with_store(Arc::new(store)));
    request(&mut app, "eastern_interconnection");
    action(&mut app, PersistAction::OpenSave { leave: true });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("Keep me".into()));
    settle(&mut app);
    update_n(&mut app, 3);
    assert!(app.world().contains_resource::<Session>());
    assert!(app.world().resource::<Persistence>().error.is_some());
    assert!(app.world().resource::<Persistence>().last_saved().is_none());
    fail.store(false, Ordering::Relaxed);
    action(&mut app, PersistAction::Retry);
    settle(&mut app);
    update_n(&mut app, 3);
    assert!(!app.world().contains_resource::<Session>());
}

#[test]
fn save_resume_refuses_old_generation_commands_and_load_entry_never_drives_a_tick() {
    use game::sim::{PlayerCommand, SessionGeneration};
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    let gen_before = app.world().resource::<SessionGeneration>().0;
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("Entry".into()));
    settle(&mut app);
    let id = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .unwrap()
        .metadata
        .slot
        .clone();
    dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    update_n(&mut app, 3);
    open_browser(&mut app);
    settle(&mut app);
    action(&mut app, PersistAction::Load(id));
    settle(&mut app);
    for _ in 0..8 {
        app.world_mut()
            .write_message(PlayerCommand(Command::AdvanceTicks { n: 8 }, gen_before));
        app.world_mut().run_schedule(FixedUpdate);
        app.update();
        if let Some(session) = app.world().get_resource::<Session>() {
            assert_eq!(session.0.session().clock().tick(), 0);
        }
    }
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
}

#[test]
fn save_resume_actual_dialog_fits_and_load_browser_shows_corruption_separately() {
    use crate::support::egui_pass::EguiPass;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    let pass = EguiPass::attach(&mut app);
    for (width, height) in [(1024.0, 640.0), (1600.0, 1000.0)] {
        let widgets = pass.settled(&mut app, width, height);
        let saves: Vec<_> = widgets.iter().filter(|w| w.text == "Save game").collect();
        assert!(!saves.is_empty());
        for widget in widgets
            .iter()
            .filter(|w| ["Save new slot", "Refresh", "Cancel"].contains(&w.text.as_str()))
        {
            assert!(
                widget.rect.left() >= 0.0
                    && widget.rect.right() <= width
                    && widget.rect.bottom() <= height,
                "{widget:?}"
            );
        }
        assert!(!widgets.iter().any(|w| w.text == "Month in review"));
        assert!(crate::contrast::check_paint(pass.paint_with(&mut app, width, height, vec![])) > 0);
    }
    action(
        &mut app,
        PersistAction::SaveNew("A full manual slot".into()),
    );
    settle(&mut app);
    std::fs::write(
        saves.path().join(format!("{}.mgsave", "f".repeat(32))),
        "broken",
    )
    .unwrap();
    dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    update_n(&mut app, 3);
    open_browser(&mut app);
    settle(&mut app);
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == "Resume"));
    assert!(
        widgets
            .iter()
            .any(|w| w.text.contains("could not be decoded"))
    );
}

#[test]
fn save_resume_next_autosave_supersedes_an_acknowledged_uncertain_publication() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
        move |stage| {
            if stage == infrastructure::save::PublishStage::SyncDirectory
                && flag.load(Ordering::Relaxed)
            {
                Err(application::SaveError::Io {
                    stage: "injected",
                    message: "sync".into(),
                })
            } else {
                Ok(())
            }
        },
    ));
    let mut app = hud_app(logs.path());
    app.insert_resource(Persistence::with_store(Arc::new(store)));
    request(&mut app, "western_interconnection");
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    settle(&mut app);
    assert!(matches!(
        app.world().resource::<Persistence>().error,
        Some(application::SaveError::DurabilityUnconfirmed { .. })
    ));
    assert!(app.world().resource::<Persistence>().last_saved().is_none());
    action(&mut app, PersistAction::ContinueWithoutSaving);
    assert!(
        app.world()
            .resource::<Persistence>()
            .autosave_failure()
            .is_some(),
        "an unconfirmed autosave warns like a failed one"
    );
    fail.store(false, Ordering::Relaxed);
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::FinishReview),
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    settle(&mut app);
    let persistence = app.world().resource::<Persistence>();
    assert!(persistence.error.is_none(), "{:?}", persistence.error);
    assert!(persistence.last_saved().is_some());
    assert_eq!(persistence.last_summary.as_ref().unwrap().month.month(), 2);
    // The unconfirmed publication warned until this later autosave was confirmed.
    assert_eq!(persistence.autosave_failure(), None);
}

#[test]
fn save_resume_cancelled_late_publication_cannot_leave_or_change_a_later_session() {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let store = infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
        move |at| {
            if at == infrastructure::save::PublishStage::Publish {
                entered_tx.send(()).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            Ok(())
        },
    ));
    let mut app = hud_app(logs.path());
    app.insert_resource(Persistence::with_store(Arc::new(store)));
    request(&mut app, "western_interconnection");
    action(&mut app, PersistAction::OpenSave { leave: true });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("Late completion".into()));
    app.update();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    action(&mut app, PersistAction::Cancel);
    dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    update_n(&mut app, 3);
    request(&mut app, "eastern_interconnection");
    let boundary = app.world().resource::<Session>().0.checkpoint();
    release_tx.send(()).unwrap();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    for _ in 0..2000 {
        app.update();
        if !application::SaveStore::list(&store).unwrap().is_empty() {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(application::SaveStore::list(&store).unwrap().len(), 1);
    update_n(&mut app, 4);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), boundary);
    assert!(app.world().resource::<Persistence>().last_saved().is_none());
    assert_eq!(
        app.world().resource::<State<game::state::AppState>>().get(),
        &game::state::AppState::InGame
    );
}

#[test]
fn save_resume_choosing_another_target_after_failure_requires_fresh_overwrite_confirmation() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let flag = fail.clone();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
        move |stage| {
            if stage == infrastructure::save::PublishStage::PartialWrite
                && flag.load(Ordering::Relaxed)
            {
                Err(application::SaveError::Io {
                    stage: "test",
                    message: "failure".into(),
                })
            } else {
                Ok(())
            }
        },
    ));
    let mut app = hud_app(logs.path());
    app.insert_resource(Persistence::with_store(Arc::new(store)));
    request(&mut app, "western_interconnection");
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("Original slot".into()));
    settle(&mut app);
    let id = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .unwrap()
        .metadata
        .slot
        .clone();
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    fail.store(true, Ordering::Relaxed);
    action(&mut app, PersistAction::SaveNew("Failed new slot".into()));
    settle(&mut app);
    assert!(app.world().resource::<Persistence>().error.is_some());
    action(
        &mut app,
        PersistAction::ChooseOverwrite {
            slot: id,
            name: "Replacement name".into(),
        },
    );
    let p = app.world().resource::<Persistence>();
    assert_eq!(p.screen, game::sim::SaveScreen::ConfirmOverwrite);
    assert!(p.error.is_none());
    let model = game::hud::saves_model(
        p,
        &game::hud::SaveForm::default(),
        &crate::support::shipped_l10n(),
    );
    assert!(model.body.contains("Original slot"));
    assert!(
        !model
            .buttons
            .iter()
            .any(|b| b.action == UiAction::Persistence(PersistAction::Retry))
    );
    assert!(
        model
            .buttons
            .iter()
            .any(|b| b.action == UiAction::Persistence(PersistAction::ConfirmOverwrite))
    );
}

#[test]
fn save_resume_autosave_at_a_queued_month_close_returns_to_the_original_leave_choice() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::X8)]);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::AdvanceTicks { n: 1439 })
        .unwrap();
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::AdvanceTicks { n: 1 }),
            UiAction::Persistence(PersistAction::OpenLeave),
        ],
    );
    settle(&mut app);
    let p = app.world().resource::<Persistence>();
    assert_eq!(p.screen, game::sim::SaveScreen::Leave);
    assert_eq!(
        p.last_saved().unwrap().metadata.kind,
        application::SaveKind::Automatic
    );
    let model = game::hud::saves_model(
        p,
        &game::hud::SaveForm::default(),
        &crate::support::shipped_l10n(),
    );
    assert!(
        model
            .buttons
            .iter()
            .any(|b| b.action == UiAction::ReturnToMenu)
    );
    assert!(app.world().contains_resource::<Session>());
    action(&mut app, PersistAction::Cancel);
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
}

// ---- a failed autosave keeps warning (spec §17.5) ------------------------------------------

/// How an injected fault fails a publication.
#[derive(Clone, Copy)]
enum Injected {
    /// The stage returns an I/O error.
    Error,
    /// The stage panics, so the persistence worker dies and reports `Completion::Failed`.
    Panic,
}

/// A store that fails every publication at its first write while `fail` is set.
fn faulty_store(saves: &TempDir) -> (std::sync::Arc<std::sync::atomic::AtomicBool>, Persistence) {
    faulty_store_at(
        saves,
        infrastructure::save::PublishStage::PartialWrite,
        Injected::Error,
    )
}

/// A store that fails every publication at `at`, as `injected` says, while `fail` is set.
fn faulty_store_at(
    saves: &TempDir,
    at: infrastructure::save::PublishStage,
    injected: Injected,
) -> (std::sync::Arc<std::sync::atomic::AtomicBool>, Persistence) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let fail = Arc::new(AtomicBool::new(false));
    let flag = fail.clone();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
        move |stage| {
            if stage != at || !flag.load(Ordering::Relaxed) {
                return Ok(());
            }
            match injected {
                Injected::Error => Err(application::SaveError::Io {
                    stage: "injected",
                    message: "full".into(),
                }),
                Injected::Panic => panic!("injected persistence worker panic"),
            }
        },
    ));
    (fail, Persistence::with_store(Arc::new(store)))
}

fn set(flag: &std::sync::atomic::AtomicBool, value: bool) {
    flag.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Plays the day from Planning to its month close, which autosaves.
fn play_a_month(app: &mut App) {
    dispatch_in(
        app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    settle(app);
}

/// Moves on from Review and plays the next day to its month close.
fn play_the_next_month(app: &mut App) {
    dispatch_in(app, vec![UiAction::Command(Command::FinishReview)]);
    play_a_month(app);
}

/// The month-close autosave fails; the error dialog is up, and the top bar already warns.
fn fail_the_autosave(app: &mut App, flag: &std::sync::atomic::AtomicBool) {
    set(flag, true);
    play_a_month(app);
    let p = app.world().resource::<Persistence>();
    assert_eq!(p.screen, game::sim::SaveScreen::Automatic);
    assert!(p.automatic_error(), "{:?}", p.error);
    // Recorded when the write failed, not when the player acknowledges it: "(saved)" never
    // shows behind the error dialog (Review Focus 2).
    let model = top_bar(app);
    assert!(
        model.autosave_failed.is_some(),
        "the warning starts with the failure"
    );
    assert_ne!(model.save.label, SAVED);
}

/// The month-close autosave fails, the player acknowledges it, and play is open again.
fn fail_the_autosave_and_continue(app: &mut App, flag: &std::sync::atomic::AtomicBool) {
    fail_the_autosave(app, flag);
    action(app, PersistAction::ContinueWithoutSaving);
    settle(app);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
}

fn top_bar(app: &App) -> game::hud::TopBarModel {
    game::hud::top_bar_model(
        &app.world().resource::<Session>().0,
        app.world().resource::<Persistence>(),
        &crate::support::shipped_l10n(),
        &usd_style(),
    )
}

/// The month the session's latest closed day belongs to, as the warning's tooltip names it.
fn closed_month(app: &App) -> String {
    let session = app.world().resource::<Session>();
    game::l10n::format::month_year(session.0.session().clock().month())
}

/// The same month, abbreviated as the top-bar warning names it: `Sep 2027`.
fn closed_month_short(app: &App) -> String {
    let session = app.world().resource::<Session>();
    game::l10n::format::month_year_short(session.0.session().clock().month())
}

const SAVE: &str = "Save game";
const SAVED: &str = "Save (saved)";

/// The top bar's warning as the player sees it at the smallest window: its painted glyphs. A
/// label egui cuts short keeps its whole string in AccessKit, so the widget text can't show what
/// was cut.
fn shown_warning(app: &mut App, pass: &crate::support::egui_pass::EguiPass) -> String {
    let whole = top_bar(app).warnings.expect("a warning is shown").text;
    pass.settled(app, 1024.0, 640.0);
    let output = pass.paint_with(app, 1024.0, 640.0, vec![]);
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    crate::contrast::painted_glyphs(&painted, &whole)
        .unwrap_or_else(|| panic!("the warning {whole:?} is painted"))
}

/// At 1024×640 the autosave warning fits whole for every month of the year, not only the one
/// this game failed in. The banner sits right-aligned against the buttons, so its slot runs from
/// the last widget on its left in the bar (the strikes stat) to the first button on its right; the
/// widest "Autosave failed: Mmm YYYY" must fit in that slot.
fn the_warning_fits_for_every_month(
    app: &mut App,
    pass: &crate::support::egui_pass::EguiPass,
    what: &str,
) {
    use crate::support::egui_harness::button as find_button;
    let whole = top_bar(app).warnings.expect("a warning is shown").text;
    let widgets = pass.settled(app, 1024.0, 640.0);
    // The label and its hover response are both in the AccessKit tree; either gives its left edge.
    let label = widgets
        .iter()
        .filter(|w| w.text == whole)
        .map(|w| w.rect)
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .unwrap_or_else(|| panic!("{what}: no widget {whole:?}"));
    let buttons = ["Menu", "Milestones", SAVE].map(|name| find_button(&widgets, name).rect.min.x);
    let right = buttons.into_iter().fold(f32::INFINITY, f32::min);
    let left = widgets
        .iter()
        .filter(|w| w.text != whole && (w.rect.center().y - label.center().y).abs() < 1.0)
        .map(|w| w.rect.max.x)
        .filter(|&x| x <= label.min.x + 0.5)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(left.is_finite(), "{what}: a stat sits left of the warning");
    let spacing = pass.ctx().global_style().spacing.item_spacing.x;
    let room = right - left - 2.0 * spacing;
    let output = pass.paint_with(app, 1024.0, 640.0, vec![]);
    let painted = output.shapes.clone();
    output.drop_without_applying_deltas();
    let galley = crate::contrast::painted_galley(&painted, &whole)
        .unwrap_or_else(|| panic!("{what}: the warning {whole:?} is painted"));
    let font = galley.job.sections[0].format.font_id.clone();
    let l10n = app.world().resource::<game::l10n::Localizer>();
    for m in 1..=12 {
        let month = domain::time::SimMonth::new(2027, m).unwrap();
        let text = l10n.args(
            "save-autosave-failed",
            &[("month", game::l10n::format::month_year_short(month).into())],
        );
        let width = pass.ctx().fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(text.clone(), font.clone(), bevy_egui::egui::Color32::WHITE)
                .size()
                .x
        });
        assert!(
            width <= room,
            "{what}: {text:?} is {width:.1} px but the warning has {room:.1} px"
        );
    }
}

/// The warning outlives *Continue without saving* and playing on, and at 1024×640 it still shows
/// the month that failed, in Review and in Operating. Its tooltip gives the whole month and the
/// advice.
#[test]
fn a_failed_autosave_keeps_warning_after_continue_without_saving() {
    use crate::support::egui_harness::button as find_button;
    use crate::support::egui_pass::EguiPass;
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let (fail, persistence) = faulty_store(&saves);
        let mut app = hud_app(logs.path());
        app.insert_resource(persistence);
        request(&mut app, region);
        // The first month's autosave succeeds, so the button has said "(saved)".
        play_a_month(&mut app);
        assert_eq!(top_bar(&app).save.label, SAVED, "{region}");
        dispatch_in(&mut app, vec![UiAction::Command(Command::FinishReview)]);
        update_n(&mut app, 2);

        fail_the_autosave_and_continue(&mut app, &fail);
        let month = closed_month(&app);
        let short = closed_month_short(&app);

        let model = top_bar(&app);
        let warning = model
            .autosave_failed
            .clone()
            .expect("the warning outlives the dialog");
        assert!(
            warning.contains("failed") && warning.contains(&short),
            "{region}: {warning} says the {short} autosave failed"
        );
        let tooltip = model.warnings.expect("the bar shows it").tooltip;
        assert!(
            tooltip.contains(&month) && tooltip.contains("Save to keep your progress"),
            "{region}: the tooltip gives the month and the advice: {tooltip:?}"
        );
        assert_ne!(model.save.label, SAVED);
        assert_eq!(model.save.label, SAVE);
        let pass = EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, 1024.0, 640.0);
        find_button(&widgets, SAVE);
        assert!(!widgets.iter().any(|w| w.text == SAVED));
        let shown = shown_warning(&mut app, &pass);
        assert!(
            shown.contains("failed") && shown.contains(&short) && !shown.contains('…'),
            "{region} review: the painted warning {shown:?} says the {short} autosave failed"
        );
        the_warning_fits_for_every_month(&mut app, &pass, &format!("{region} review"));

        // Playing on does not make it go away.
        dispatch_in(
            &mut app,
            vec![
                UiAction::Command(Command::FinishReview),
                UiAction::Command(Command::BeginOperating),
                UiAction::Command(Command::AdvanceTicks { n: 300 }),
            ],
        );
        settle(&mut app);
        update_n(&mut app, 3);
        let model = top_bar(&app);
        assert!(
            model
                .autosave_failed
                .as_ref()
                .is_some_and(|w| w.contains(&short)),
            "{model:?}"
        );
        assert_eq!(model.save.label, SAVE);
        let widgets = pass.settled(&mut app, 1024.0, 640.0);
        assert!(!widgets.iter().any(|w| w.text == SAVED));
        let shown = shown_warning(&mut app, &pass);
        assert!(
            shown.contains("failed") && shown.contains(&short) && !shown.contains('…'),
            "{region} operating: the painted warning {shown:?} says the {short} autosave failed"
        );
        the_warning_fits_for_every_month(&mut app, &pass, &format!("{region} operating"));
    }
}

#[test]
fn a_persistence_worker_panic_during_an_autosave_warns_at_once_and_after_continue() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let (fail, persistence) = faulty_store_at(
        &saves,
        infrastructure::save::PublishStage::PartialWrite,
        Injected::Panic,
    );
    let mut app = hud_app(logs.path());
    app.insert_resource(persistence);
    request(&mut app, "eastern_interconnection");

    // The worker dies mid-write: `Completion::Failed`, not a `SaveError` receipt.
    fail_the_autosave_and_continue(&mut app, &fail);

    let model = top_bar(&app);
    assert!(model.autosave_failed.is_some(), "{model:?}");
    assert_eq!(model.save.label, SAVE);
    set(&fail, false);
    play_the_next_month(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().autosave_failure(),
        None
    );
    assert_eq!(top_bar(&app).save.label, SAVED);
}

/// Retry re-sends the failed boundary; once it is confirmed, that boundary is on disk.
#[test]
fn a_confirmed_retry_of_a_failed_autosave_clears_the_warning() {
    use infrastructure::save::PublishStage;
    for (at, unconfirmed) in [
        (PublishStage::PartialWrite, false),
        (PublishStage::SyncDirectory, true),
    ] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let (fail, persistence) = faulty_store_at(&saves, at, Injected::Error);
        let mut app = hud_app(logs.path());
        app.insert_resource(persistence);
        request(&mut app, "western_interconnection");
        fail_the_autosave(&mut app, &fail);
        assert_eq!(
            matches!(
                app.world().resource::<Persistence>().error,
                Some(application::SaveError::DurabilityUnconfirmed { .. })
            ),
            unconfirmed,
            "{at:?}"
        );

        set(&fail, false);
        action(&mut app, PersistAction::Retry);
        settle(&mut app);

        let p = app.world().resource::<Persistence>();
        assert!(p.error.is_none(), "{at:?}: {:?}", p.error);
        assert_eq!(p.screen, game::sim::SaveScreen::Closed, "{at:?}");
        assert_eq!(p.autosave_failure(), None, "{at:?}");
        let model = top_bar(&app);
        assert_eq!(model.autosave_failed, None, "{at:?}");
        assert_eq!(model.save.label, SAVED, "{at:?}");
    }
}

#[test]
fn a_later_confirmed_autosave_clears_the_warning() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let (fail, persistence) = faulty_store(&saves);
    let mut app = hud_app(logs.path());
    app.insert_resource(persistence);
    request(&mut app, "eastern_interconnection");
    fail_the_autosave_and_continue(&mut app, &fail);
    assert!(top_bar(&app).autosave_failed.is_some());

    set(&fail, false);
    play_the_next_month(&mut app);

    let p = app.world().resource::<Persistence>();
    assert!(p.error.is_none(), "{:?}", p.error);
    assert_eq!(p.autosave_failure(), None);
    let model = top_bar(&app);
    assert_eq!(model.autosave_failed, None);
    assert_eq!(model.save.label, SAVED);
}

#[test]
fn a_later_confirmed_manual_save_clears_the_warning_and_a_failed_one_does_not() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let (fail, persistence) = faulty_store(&saves);
    let mut app = hud_app(logs.path());
    app.insert_resource(persistence);
    request(&mut app, "eastern_interconnection");
    fail_the_autosave_and_continue(&mut app, &fail);

    // A manual save that fails too leaves the warning.
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("Also fails".into()));
    settle(&mut app);
    assert!(app.world().resource::<Persistence>().error.is_some());
    action(&mut app, PersistAction::Cancel);
    assert!(top_bar(&app).autosave_failed.is_some());
    assert_eq!(top_bar(&app).save.label, SAVE);

    // One that is confirmed, at this boundary, covers the failed autosave.
    set(&fail, false);
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("By hand".into()));
    settle(&mut app);

    let p = app.world().resource::<Persistence>();
    assert!(p.last_saved().is_some());
    assert_eq!(p.autosave_failure(), None);
    let model = top_bar(&app);
    assert_eq!(model.autosave_failed, None);
    assert_eq!(model.save.label, SAVED);
}

/// The warning belongs to its run: leaving for the menu ends the run and clears it, so neither
/// the next new game nor a load can inherit it.
#[test]
fn returning_to_the_menu_clears_the_warning_before_a_new_game_or_load() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let (fail, persistence) = faulty_store(&saves);
    let mut app = hud_app(logs.path());
    app.insert_resource(persistence);
    request(&mut app, "eastern_interconnection");
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    action(&mut app, PersistAction::SaveNew("To load".into()));
    settle(&mut app);
    let slot = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .unwrap()
        .metadata
        .slot
        .clone();

    // A new game.
    fail_the_autosave_and_continue(&mut app, &fail);
    assert!(top_bar(&app).autosave_failed.is_some());
    dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    update_n(&mut app, 3);
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(
        app.world().resource::<Persistence>().autosave_failure(),
        None,
        "cleared on leaving, before any new game"
    );
    request(&mut app, "western_interconnection");
    assert_eq!(
        app.world().resource::<Persistence>().autosave_failure(),
        None
    );
    assert_eq!(top_bar(&app).autosave_failed, None);

    // A load.
    fail_the_autosave_and_continue(&mut app, &fail);
    assert!(top_bar(&app).autosave_failed.is_some());
    set(&fail, false);
    dispatch_in(&mut app, vec![UiAction::ReturnToMenu]);
    update_n(&mut app, 3);
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(
        app.world().resource::<Persistence>().autosave_failure(),
        None,
        "cleared on leaving, before any load"
    );
    open_browser(&mut app);
    settle(&mut app);
    action(&mut app, PersistAction::Load(slot));
    settle(&mut app);
    update_n(&mut app, 6);
    assert!(app.world().contains_resource::<Session>());
    assert_eq!(
        app.world().resource::<Persistence>().autosave_failure(),
        None
    );
    let model = top_bar(&app);
    assert_eq!(model.autosave_failed, None);
}

/// The widest top bar: recording stopped with a full disk's reason and a failed autosave, at the
/// smallest window. The two become one indicator, "⚠ 2 warnings", whose tooltip has both whole
/// messages. Menu, Milestones and Save stay on the screen, clear of it, on one row, and they still
/// work. Drawn by the real systems, in Review (where the warning starts) and in Operating.
#[test]
fn at_the_smallest_window_two_warnings_become_one_indicator_beside_menu_milestones_and_save() {
    use crate::hud_smoke::{NO_SPACE, NoSpaceLeft};
    use crate::support::egui_harness::{Widget, button as find_button};
    use crate::support::egui_pass::EguiPass;
    use bevy_egui::egui;
    let (width, height) = (1024.0, 640.0);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let (fail, persistence) = faulty_store(&saves);
        let mut app = hud_app(logs.path());
        app.insert_resource(persistence);
        request(&mut app, region);
        let catalog = infrastructure::region::RonRegionCatalog::new(
            crate::support::shipped_assets().join("regions"),
        );
        let game = application::Game::start_new(
            &catalog,
            &domain::ids::RegionId::new(region),
            7,
            domain::session::GameSettings::default(),
            Box::new(NoSpaceLeft),
        )
        .unwrap();
        app.insert_resource(Session(game));
        fail_the_autosave_and_continue(&mut app, &fail);
        assert!(
            app.world().resource::<Session>().0.log_failure().is_some(),
            "{region}"
        );
        let month = closed_month(&app);
        let pass = EguiPass::attach(&mut app);
        pass.ctx()
            .all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);

        for phase in ["review", "operating"] {
            if phase == "operating" {
                dispatch_in(
                    &mut app,
                    vec![
                        UiAction::Command(Command::FinishReview),
                        UiAction::Command(Command::BeginOperating),
                    ],
                );
                update_n(&mut app, 3);
            }
            // The pointer leaves, so no tooltip from the phase before is still open.
            pass.run_with(&mut app, width, height, vec![egui::Event::PointerGone]);
            let widgets = pass.settled(&mut app, width, height);
            let find = |widgets: &[Widget], prefix: &str| {
                widgets
                    .iter()
                    .find(|w| w.text.starts_with(prefix))
                    .unwrap_or_else(|| panic!("{region} {phase}: {prefix:?} is shown"))
                    .rect
            };
            let indicator = find(&widgets, "⚠ 2 warnings");
            let autosave = top_bar(&app).autosave_failed.expect("the autosave warning");
            assert!(
                !widgets
                    .iter()
                    .any(|w| w.text.starts_with("Recording stopped") || w.text == autosave),
                "{region} {phase}: no truncated banner beside the indicator"
            );
            assert!(
                !widgets
                    .iter()
                    .any(|w| w.text.starts_with('⚠') && w.text.ends_with('…')),
                "{region} {phase}: the indicator is whole"
            );
            let menu = find_button(&widgets, "Menu").rect;
            let milestones = find_button(&widgets, "Milestones").rect;
            let save = find_button(&widgets, SAVE).rect;
            for (name, rect) in [("Menu", menu), ("Milestones", milestones), ("Save", save)] {
                assert!(
                    screen.contains_rect(rect),
                    "{region} {phase}: {name} {rect:?} is off the screen"
                );
                assert!(
                    !rect.intersects(indicator),
                    "{region} {phase}: {name} {rect:?} overlaps the indicator {indicator:?}"
                );
                assert!(
                    (rect.center().y - indicator.center().y).abs() < 1.0,
                    "{region} {phase}: the bar is one row"
                );
            }

            // The indicator and the rest of the screen are legible (4.5:1).
            assert!(
                crate::contrast::check_paint(pass.paint_with(&mut app, width, height, vec![])) > 0
            );

            // Its tooltip, under the bar, gives each whole message.
            let hover = vec![egui::Event::PointerMoved(indicator.center())];
            pass.run_with(&mut app, width, height, hover.clone());
            let hovered = pass.run_with(&mut app, width, height, hover);
            let tooltip = hovered
                .iter()
                .filter(|w| w.rect.top() >= indicator.bottom())
                .find(|w| w.text.contains(NO_SPACE))
                .unwrap_or_else(|| panic!("{region} {phase}: a tooltip gives the disk error"));
            assert!(
                tooltip.text.contains("Autosave failed") && tooltip.text.contains(&month),
                "{region} {phase}: the tooltip names the failed month: {tooltip:?}"
            );
            pass.run_with(&mut app, width, height, vec![egui::Event::PointerGone]);

            // egui hides a tooltip after a click while the test's clock stands still, so the
            // buttons are clicked once every phase's tooltips have been seen.
            if phase != "operating" {
                continue;
            }
            pass.click_at(&mut app, width, height, milestones.center());
            assert!(
                app.world().resource::<game::hud::MilestonesOpen>().0,
                "{region}: Milestones opens its panel"
            );
            dispatch_in(&mut app, vec![UiAction::ToggleMilestones]);
            update_n(&mut app, 2);
            pass.click_at(&mut app, width, height, save.center());
            assert_eq!(
                app.world().resource::<Persistence>().screen,
                game::sim::SaveScreen::Save,
                "{region}: Save opens the save dialog"
            );
            settle(&mut app);
            action(&mut app, PersistAction::Cancel);
            update_n(&mut app, 2);
            // With saves enabled, Menu opens the save-or-leave choice, which asks first.
            pass.click_at(&mut app, width, height, menu.center());
            assert_eq!(
                app.world().resource::<Persistence>().screen,
                game::sim::SaveScreen::Leave,
                "{region}: Menu opens the leave choice"
            );
            assert!(app.world().contains_resource::<Session>());
            settle(&mut app);
            action(&mut app, PersistAction::Cancel);
            update_n(&mut app, 2);
        }
    }
}

/// A store stamping its writes as a different build on a different platform: what an older
/// game, or the same game on another machine, leaves in the save directory.
fn older_build_store(saves: &TempDir) -> infrastructure::save::FileSaveStore {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    let next = Arc::new(AtomicU64::new(1));
    infrastructure::save::FileSaveStore::with_sources(
        saves.path().into(),
        application::ReplayCompatibility {
            build: "an-older-build".into(),
            platform: "another-target".into(),
        },
        Arc::new(|| 1_800_000_000),
        Arc::new(move || {
            let n = next.fetch_add(1, Ordering::Relaxed);
            application::SaveId::new(format!("{n:032x}")).unwrap()
        }),
    )
}

/// Writes `checkpoint` through `store`, stamped with that store's build, as a manual save.
fn write_manual(
    store: &infrastructure::save::FileSaveStore,
    name: &str,
    checkpoint: application::GameCheckpoint,
) -> application::SaveId {
    use application::SaveStore;
    let bundle = application::SaveBundle {
        compatibility: store.compatibility(),
        metadata: application::SaveMetadata {
            slot: store.allocate_id(),
            run: store.allocate_id(),
            kind: application::SaveKind::Manual,
            name: name.into(),
            saved_at: store.now(),
        },
        checkpoint,
    };
    store
        .write(&bundle, &application::SaveTarget::Create, &|| false)
        .unwrap()
        .metadata
        .slot
}

/// A menu app on `saves` with its load browser open and listed.
fn load_browser(logs: &TempDir, saves: &TempDir) -> App {
    let mut app = hud_app(logs.path());
    app.insert_resource(SaveDir(saves.path().into()));
    update_n(&mut app, 2);
    open_browser(&mut app);
    settle(&mut app);
    app
}

fn browser_model(app: &App) -> game::hud::SavesModel {
    game::hud::saves_model(
        app.world().resource::<Persistence>(),
        &game::hud::SaveForm::default(),
        &crate::support::shipped_l10n(),
    )
}

#[test]
fn load_library_empty_state_keeps_refresh_and_cancel_available() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = load_browser(&logs, &saves);
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0 / 1.25, 640.0 / 1.25);
    assert!(widgets.iter().any(|w| w.text == "No saves found."));
    assert!(!widgets.iter().any(|w| w.text == "Resume"));
    let cancel = widgets.iter().find(|w| w.text == "Cancel").unwrap();
    assert!(widgets.iter().any(|w| w.text == "Refresh" && !w.disabled));
    pass.click_at(&mut app, 1024.0 / 1.25, 640.0 / 1.25, cancel.rect.center());
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
}

#[test]
fn load_library_has_prominent_resume_controls_and_keeps_its_footer_visible() {
    use crate::support::egui_pass::EguiPass;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let source = operating(&logs, &saves);
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    write_manual(
        &store,
        "Northwest power company",
        source.world().resource::<Session>().0.checkpoint(),
    );
    drop(source);
    let mut app = load_browser(&logs, &saves);
    let pass = EguiPass::attach(&mut app);
    for (width, height) in [
        (1024.0, 640.0),
        (1024.0 / 1.25, 640.0 / 1.25),
        (1920.0, 1080.0),
    ] {
        let widgets = pass.settled(&mut app, width, height);
        let resume = widgets.iter().find(|w| w.text == "Resume").unwrap();
        assert!(
            resume.rect.height() >= 36.0,
            "Resume must be a clear primary action"
        );
        assert!(
            resume.rect.right()
                < widgets
                    .iter()
                    .find(|w| w.text == "Automatic snapshots")
                    .unwrap()
                    .rect
                    .left(),
            "named saves belong in the left column"
        );
        for label in ["Resume", "Rename", "Refresh", "Cancel"] {
            let widget = widgets.iter().find(|w| w.text == label).unwrap();
            assert!(
                widget.rect.left() >= 0.0
                    && widget.rect.right() <= width
                    && widget.rect.bottom() <= height,
                "{width}×{height}: {widget:?}"
            );
        }
        assert!(crate::contrast::check_paint(pass.paint_with(&mut app, width, height, vec![])) > 0);
    }
}

/// The build fingerprint no longer gates loading: a save from another build or platform lists by
/// its name and loads, paused, exactly as a same-build save does, when its replay verifies here.
/// The resumed game's own saves carry this build's stamp.
#[test]
fn a_save_from_another_build_lists_by_name_and_resumes_paused_when_its_replay_verifies() {
    use application::SaveStore;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    app.world_mut()
        .resource_mut::<Session>()
        .0
        .execute(Command::AdvanceTicks { n: 600 })
        .unwrap();
    let live = app.world_mut().remove_resource::<Session>().unwrap().0;
    drop(app);
    let older = older_build_store(&saves);
    let slot = write_manual(&older, "From an older build", live.checkpoint());

    let mut app = load_browser(&logs, &saves);
    let model = browser_model(&app);
    assert!(model.error.is_none(), "{:?}", model.error);
    let [row] = model.rows.as_slice() else {
        panic!("{:?}", model.rows)
    };
    assert_eq!(row.name, "From an older build");
    assert!(row.problem.is_none(), "{:?}", row.problem);
    assert!(row.details.contains("Western"), "{}", row.details);
    assert_eq!(
        row.action.as_ref().map(|b| &b.action),
        Some(&UiAction::Persistence(PersistAction::Load(slot.clone())))
    );

    action(&mut app, PersistAction::Load(slot));
    settle(&mut app);
    update_n(&mut app, 6);
    let session = &app.world().resource::<Session>().0;
    assert_eq!(session.checkpoint(), live.checkpoint());
    assert_eq!(session.projections(), live.projections());
    assert_eq!(
        session.session().phase(),
        domain::session::DayPhase::Operating
    );
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
    assert_eq!(
        *app.world().resource::<State<game::state::AppState>>().get(),
        game::state::AppState::InGame
    );

    // The month-close autosave of the resumed game is this build's.
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 840 })],
    );
    settle(&mut app);
    let current = infrastructure::save::FileSaveStore::new(saves.path().into());
    let autosave = current
        .list()
        .unwrap()
        .into_iter()
        .filter_map(|listing| listing.entry.ok())
        .find(|entry| entry.metadata.kind == application::SaveKind::Automatic)
        .expect("the month close autosaved");
    assert_eq!(
        autosave.compatibility,
        infrastructure::save::replay_compatibility()
    );
    assert_ne!(autosave.compatibility, older.compatibility());
}

/// A save whose replay doesn't reproduce its recorded boundary fails to load, installs nothing
/// and leaves the file as it was. From another build, the copy says the version differs.
#[test]
fn a_save_that_does_not_verify_names_the_other_version_and_leaves_the_game_and_file_alone() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let app = operating(&logs, &saves);
    let mut diverged = app.world().resource::<Session>().0.checkpoint();
    drop(app);
    diverged.state_hash ^= 1;
    let older = write_manual(&older_build_store(&saves), "Older", diverged.clone());
    let same = write_manual(
        &infrastructure::save::FileSaveStore::new(saves.path().into()),
        "Same build",
        diverged,
    );
    let file = |slot: &application::SaveId| {
        std::fs::read(saves.path().join(format!("{}.mgsave", slot.as_str()))).unwrap()
    };
    let (older_bytes, same_bytes) = (file(&older), file(&same));
    let l10n = crate::support::shipped_l10n();

    let mut app = load_browser(&logs, &saves);
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    for (slot, key) in [
        (older.clone(), "save-error-other-version"),
        (same.clone(), "save-error-state"),
    ] {
        action(&mut app, PersistAction::Load(slot));
        settle(&mut app);
        update_n(&mut app, 3);
        assert!(!app.world().contains_resource::<Session>(), "{key}");
        assert_eq!(
            *app.world().resource::<State<game::state::AppState>>().get(),
            game::state::AppState::MainMenu,
            "{key}"
        );
        assert_eq!(browser_model(&app).error, Some(l10n.text(key)));
        // The error and the browser's controls stay on screen at the smallest window.
        let (width, height) = (1024.0, 640.0);
        let widgets = pass.settled(&mut app, width, height);
        let on_screen = |w: &&crate::support::egui_harness::Widget| {
            w.rect.left() >= 0.0 && w.rect.right() <= width && w.rect.bottom() <= height
        };
        assert!(
            widgets
                .iter()
                .filter(|w| w.text.starts_with(&l10n.text(key)[..20]))
                .any(|w| on_screen(&w)),
            "{key}"
        );
        for label in ["Retry", "Refresh", "Cancel"] {
            let button = widgets.iter().find(|w| w.text == label);
            assert!(button.is_some_and(|b| on_screen(&b)), "{key}: {label}");
        }
    }
    assert!(matches!(
        app.world().resource::<Persistence>().error,
        Some(application::SaveError::Restore(
            application::RestoreError::StateHashMismatch
        ))
    ));
    assert_eq!(file(&older), older_bytes);
    assert_eq!(file(&same), same_bytes);
    assert!(l10n.problems().is_empty(), "{:?}", l10n.problems());
}

/// A checkpoint of a fresh western game whose embedded region this build can't read, though its
/// fingerprint still matches its bytes: what a region schema change leaves an older save.
fn unreadable_checkpoint(logs: &TempDir, saves: &TempDir) -> application::GameCheckpoint {
    use sha2::{Digest, Sha256};
    let app = operating(logs, saves);
    let mut checkpoint = app.world().resource::<Session>().0.checkpoint();
    checkpoint.region_source = b"(not: a region)".to_vec();
    checkpoint.header.region_fingerprint =
        application::RegionFingerprint::new(Sha256::digest(&checkpoint.region_source).into());
    checkpoint
}

/// The file names in `dir`, or none when it doesn't exist.
fn files_in(dir: &std::path::Path) -> std::collections::BTreeSet<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .map(|entries| entries.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default()
}

/// A save another build wrote that this build can't decode reads as another version's, in the
/// browser and on load, not as a corrupt file. The same build's undecodable save keeps the
/// malformed copy.
#[test]
fn a_save_from_another_build_that_no_longer_decodes_names_its_version_not_corruption() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let checkpoint = unreadable_checkpoint(&logs, &saves);
    let older = write_manual(&older_build_store(&saves), "Older", checkpoint.clone());
    let same = write_manual(
        &infrastructure::save::FileSaveStore::new(saves.path().into()),
        "Same build",
        checkpoint,
    );
    let l10n = crate::support::shipped_l10n();
    let mut app = load_browser(&logs, &saves);
    let model = browser_model(&app);
    for (slot, key) in [
        (&older, "save-error-other-version"),
        (&same, "save-error-malformed"),
    ] {
        let row = model
            .rows
            .iter()
            .find(|row| row.name == slot.as_str())
            .unwrap_or_else(|| panic!("{key}: {:?}", model.rows));
        assert_eq!(row.problem, Some(l10n.text(key)));
    }
    for (slot, key) in [
        (older, "save-error-other-version"),
        (same, "save-error-malformed"),
    ] {
        action(&mut app, PersistAction::Load(slot));
        settle(&mut app);
        update_n(&mut app, 3);
        assert!(!app.world().contains_resource::<Session>(), "{key}");
        assert_eq!(browser_model(&app).error, Some(l10n.text(key)));
    }
    assert!(l10n.problems().is_empty(), "{:?}", l10n.problems());
}

/// A load that fails verification abandons its candidate without leaving a branch log or region
/// archive behind, however often it is tried. A load that succeeds still records its branch.
#[test]
fn a_failed_load_leaves_no_new_files_in_the_log_directory() {
    let setup = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let app = operating(&setup, &saves);
    let good = app.world().resource::<Session>().0.checkpoint();
    drop(app);
    let mut diverged = good.clone();
    diverged.state_hash ^= 1;
    let current = infrastructure::save::FileSaveStore::new(saves.path().into());
    let bad = [
        write_manual(&older_build_store(&saves), "Older", diverged.clone()),
        write_manual(&current, "Same build", diverged),
    ];
    let good = write_manual(&current, "Good", good);

    let logs = TempDir::new().unwrap();
    let mut app = load_browser(&logs, &saves);
    let before = files_in(logs.path());
    for _ in 0..2 {
        for slot in &bad {
            action(&mut app, PersistAction::Load(slot.clone()));
            settle(&mut app);
            update_n(&mut app, 3);
            assert!(app.world().resource::<Persistence>().error.is_some());
            assert_eq!(files_in(logs.path()), before);
        }
    }
    action(&mut app, PersistAction::Load(good));
    settle(&mut app);
    update_n(&mut app, 3);
    assert!(app.world().contains_resource::<Session>());
    assert!(
        files_in(logs.path())
            .difference(&before)
            .any(|name| name.to_string_lossy().starts_with("resume-")),
        "a successful load records its branch"
    );
}

#[test]
fn save_and_rename_name_fields_submit_with_enter_using_the_current_text() {
    use bevy_egui::egui;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    let expected = app.world().resource::<Session>().0.checkpoint();
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let (width, height) = (1280.0, 640.0);
    let enter = |pressed| egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    let widgets = pass.settled(&mut app, width, height);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput)
        .unwrap();
    pass.click_at(&mut app, width, height, field.rect.center());
    // Empty input must not submit, even from the focused field.
    pass.run_with(&mut app, width, height, vec![enter(true), enter(false)]);
    assert!(app.world().resource::<Persistence>().last_saved().is_none());
    let widgets = pass.settled(&mut app, width, height);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput)
        .unwrap();
    pass.click_at(&mut app, width, height, field.rect.center());
    // Text and Enter in the same frame must use the new text and validation.
    pass.run_with(
        &mut app,
        width,
        height,
        vec![
            egui::Event::Text("Keyboard save".into()),
            enter(true),
            enter(false),
        ],
    );
    settle(&mut app);
    let saved = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .expect("Enter creates a save")
        .clone();
    assert_eq!(saved.metadata.name, "Keyboard save");
    open_browser(&mut app);
    settle(&mut app);
    action(
        &mut app,
        PersistAction::BeginRename(saved.metadata.slot.clone()),
    );
    pass.settled(&mut app, width, height);
    app.world_mut()
        .resource_mut::<game::hud::SaveForm>()
        .rename_name
        .clear();
    let widgets = pass.settled(&mut app, width, height);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput)
        .unwrap();
    pass.click_at(&mut app, width, height, field.rect.center());
    pass.run_with(&mut app, width, height, vec![enter(true), enter(false)]);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Rename
    );
    let widgets = pass.settled(&mut app, width, height);
    let field = widgets
        .iter()
        .find(|w| w.role == egui::accesskit::Role::TextInput)
        .unwrap();
    pass.click_at(&mut app, width, height, field.rect.center());
    pass.run_with(
        &mut app,
        width,
        height,
        vec![
            egui::Event::Text("Keyboard rename".into()),
            enter(true),
            enter(false),
        ],
    );
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Browser
    );
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    use application::SaveStore;
    let (renamed, _) = store.read(&saved.metadata.slot).unwrap();
    assert_eq!(renamed.metadata.name, "Keyboard rename");
    assert_eq!(renamed.checkpoint, expected);
    assert_eq!(store.list().unwrap().len(), 1, "Enter submits only once");
}

#[test]
fn save_rename_browser_prefills_name_cancels_and_renames_without_loading_the_game() {
    use crate::support::egui_pass::EguiPass;
    use application::SaveStore;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let older = older_build_store(&saves);
    let game = application::Game::start_new(
        &infrastructure::region::RonRegionCatalog::new(
            crate::support::shipped_assets().join("regions"),
        ),
        &domain::ids::RegionId::new("eastern_interconnection"),
        7,
        domain::session::GameSettings::default(),
        Box::new(application::InMemoryCommandLog::default()),
    )
    .unwrap();
    let slot = write_manual(&older, "Original name", game.checkpoint());
    let before = older.read(&slot).unwrap().0;
    let mut app = load_browser(&logs, &saves);
    let pass = EguiPass::attach(&mut app);
    for (width, height) in [(1024.0, 640.0), (1280.0, 800.0)] {
        let widgets = pass.settled(&mut app, width, height);
        let rename = widgets.iter().find(|w| w.text == "Rename").unwrap();
        pass.click_at(&mut app, width, height, rename.rect.center());
        pass.settled(&mut app, width, height);
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Rename
        );
        assert_eq!(
            app.world().resource::<game::hud::SaveForm>().rename_name,
            "Original name"
        );
        let widgets = pass.settled(&mut app, width, height);
        let field = widgets
            .iter()
            .find(|w| w.role == bevy_egui::egui::accesskit::Role::TextInput)
            .unwrap();
        pass.click_at(&mut app, width, height, field.rect.center());
        pass.run_with(
            &mut app,
            width,
            height,
            vec![bevy_egui::egui::Event::Key {
                key: bevy_egui::egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: bevy_egui::egui::Modifiers::NONE,
            }],
        );
        settle(&mut app);
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Browser
        );
        assert_eq!(older.read(&slot).unwrap().0, before);
    }
    action(&mut app, PersistAction::BeginRename(slot.clone()));
    pass.settled(&mut app, 1024.0, 640.0);
    app.world_mut()
        .resource_mut::<game::hud::SaveForm>()
        .rename_name = "  Renamed game  ".into();
    let widgets = pass.settled(&mut app, 1024.0, 640.0);
    let rename = widgets.iter().find(|w| w.text == "Rename").unwrap();
    pass.click_at(&mut app, 1024.0, 640.0, rename.rect.center());
    settle(&mut app);
    let mut expected = before;
    expected.metadata.name = "Renamed game".into();
    assert_eq!(older.read(&slot).unwrap().0, expected);
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Browser
    );
    assert_eq!(browser_model(&app).rows[0].name, "Renamed game");
    assert!(
        std::fs::read_dir(logs.path()).unwrap().next().is_none(),
        "Rename must not start a replay branch"
    );
}

#[test]
fn save_rename_preserves_the_active_autosave_name_and_revision_at_the_next_month_close() {
    use application::{SaveKind, SaveStore};
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    dispatch_in(
        &mut app,
        vec![UiAction::Command(Command::AdvanceTicks { n: 1440 })],
    );
    settle(&mut app);
    let original = app
        .world()
        .resource::<Persistence>()
        .last_saved()
        .unwrap()
        .clone();
    assert_eq!(original.metadata.kind, SaveKind::Automatic);
    open_browser(&mut app);
    settle(&mut app);
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let summary = app.world().resource::<Persistence>().last_summary.clone();
    action(
        &mut app,
        PersistAction::BeginRename(original.metadata.slot.clone()),
    );
    action(
        &mut app,
        PersistAction::Rename("My monthly checkpoint".into()),
    );
    settle(&mut app);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
    assert_eq!(app.world().resource::<Persistence>().last_summary, summary);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Browser
    );
    assert_eq!(browser_model(&app).rows[0].name, "My monthly checkpoint");
    action(&mut app, PersistAction::Cancel);
    for command in [
        Command::FinishReview,
        Command::BeginOperating,
        Command::AdvanceTicks { n: 1440 },
    ] {
        dispatch_in(&mut app, vec![UiAction::Command(command)]);
        settle(&mut app);
    }
    let p = app.world().resource::<Persistence>();
    assert!(p.error.is_none(), "{:?}", p.error);
    let next = p.last_saved().unwrap();
    assert_eq!(next.metadata.slot, original.metadata.slot);
    assert_eq!(next.metadata.name, "My monthly checkpoint");
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        store.read(&next.metadata.slot).unwrap().0.checkpoint,
        app.world().resource::<Session>().0.checkpoint()
    );
}

#[test]
fn save_rename_retry_recovers_pre_and_post_publication_errors_without_saving_the_current_game() {
    use application::SaveStore;
    use infrastructure::save::{FileSaveStore, PublishStage};
    for stage in [PublishStage::Publish, PublishStage::SyncDirectory] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let (fail, persistence) = faulty_store_at(&saves, stage, Injected::Error);
        let mut app = hud_app(logs.path());
        app.insert_resource(persistence);
        request(&mut app, "western_interconnection");
        action(&mut app, PersistAction::OpenSave { leave: false });
        settle(&mut app);
        action(&mut app, PersistAction::SaveNew("Before".into()));
        settle(&mut app);
        let original = app
            .world()
            .resource::<Persistence>()
            .last_saved()
            .unwrap()
            .clone();
        let checkpoint = app.world().resource::<Session>().0.checkpoint();
        let summary = app.world().resource::<Persistence>().last_summary.clone();
        action(&mut app, PersistAction::OpenSave { leave: false });
        settle(&mut app);
        action(
            &mut app,
            PersistAction::BeginRename(original.metadata.slot.clone()),
        );
        set(&fail, true);
        action(&mut app, PersistAction::Rename("After".into()));
        settle(&mut app);
        assert!(app.world().resource::<Persistence>().error.is_some());
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Rename
        );
        assert_eq!(
            app.world().resource::<Persistence>().last_saved(),
            Some(&original)
        );
        let store = FileSaveStore::new(saves.path().into());
        assert_eq!(
            store.read(&original.metadata.slot).unwrap().0.metadata.name,
            if stage == PublishStage::Publish {
                "Before"
            } else {
                "After"
            }
        );
        set(&fail, false);
        action(&mut app, PersistAction::Retry);
        settle(&mut app);
        assert!(app.world().resource::<Persistence>().error.is_none());
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Save
        );
        assert_eq!(
            store.read(&original.metadata.slot).unwrap().0.metadata.name,
            "After"
        );
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), checkpoint);
        assert_eq!(app.world().resource::<Persistence>().last_summary, summary);
    }
}

#[test]
fn save_rename_conflict_keeps_the_other_windows_change_and_cancel_refreshes_the_list() {
    use application::SaveStore;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let store = older_build_store(&saves);
    let source_logs = TempDir::new().unwrap();
    let mut source = hud_app(source_logs.path());
    request(&mut source, "eastern_interconnection");
    let slot = write_manual(
        &store,
        "Original",
        source.world().resource::<Session>().0.checkpoint(),
    );
    let mut app = load_browser(&logs, &saves);
    let revision = app.world().resource::<Persistence>().entries[0]
        .entry
        .as_ref()
        .unwrap()
        .revision;
    store
        .rename(&slot, revision, "Other window", &|| false)
        .unwrap();
    action(&mut app, PersistAction::BeginRename(slot.clone()));
    action(&mut app, PersistAction::Rename("Stale edit".into()));
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().error,
        Some(application::SaveError::SlotChanged)
    );
    assert_eq!(store.read(&slot).unwrap().0.metadata.name, "Other window");
    action(&mut app, PersistAction::Cancel);
    settle(&mut app);
    assert_eq!(browser_model(&app).rows[0].name, "Other window");
}

fn startup_menu(logs: &TempDir, saves: &TempDir) -> App {
    let mut app = crate::support::headless_app(&crate::support::shipped_assets(), logs.path());
    app.insert_resource(SaveDir(saves.path().into()))
        .add_plugins((game::hud::HudPlugin, game::preferences::PreferencesPlugin));
    settle(&mut app);
    update_n(&mut app, 3);
    app
}

#[test]
fn first_launch_with_no_snapshots_goes_straight_to_new_game_setup() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = startup_menu(&logs, &saves);
    assert_eq!(
        *app.world().resource::<State<game::state::AppState>>().get(),
        game::state::AppState::RegionSelect
    );
    assert!(app.world().contains_resource::<game::hud::RegionListings>());
    assert!(!app.world().contains_resource::<Session>());
    dispatch_in(
        &mut app,
        vec![UiAction::Navigate(game::hud::MenuScreen::MainMenu)],
    );
    update_n(&mut app, 3);
    assert_eq!(
        *app.world().resource::<State<game::state::AppState>>().get(),
        game::state::AppState::MainMenu
    );
}

#[test]
fn corrupt_snapshots_and_scan_errors_do_not_skip_the_opening_screen() {
    for broken_directory in [false, true] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        if broken_directory {
            std::fs::write(saves.path().join("not-a-directory"), "x").unwrap();
            let mut app =
                crate::support::headless_app(&crate::support::shipped_assets(), logs.path());
            app.insert_resource(SaveDir(saves.path().join("not-a-directory")))
                .add_plugins(game::hud::HudPlugin);
            settle(&mut app);
            update_n(&mut app, 3);
            assert_eq!(
                *app.world().resource::<State<game::state::AppState>>().get(),
                game::state::AppState::MainMenu
            );
            assert!(app.world().resource::<Persistence>().error.is_some());
        } else {
            std::fs::write(saves.path().join("broken.mgsave"), "broken").unwrap();
            let app = startup_menu(&logs, &saves);
            assert_eq!(
                *app.world().resource::<State<game::state::AppState>>().get(),
                game::state::AppState::MainMenu
            );
            assert_eq!(app.world().resource::<Persistence>().entries.len(), 1);
        }
    }
}

#[test]
fn opening_resume_defaults_to_the_newest_monthly_autosave_and_restores_it_with_enter() {
    use application::SaveStore;
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    play_a_month(&mut source);
    let older = source.world().resource::<Session>().0.checkpoint();
    play_the_next_month(&mut source);
    let expected = source.world().resource::<Session>().0.checkpoint();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    write_snapshot(
        &store,
        "Older autosave",
        application::SaveKind::Automatic,
        100,
        older,
    );
    let chosen = write_snapshot(
        &store,
        "Latest autosave",
        application::SaveKind::Automatic,
        200,
        expected.clone(),
    );
    let manual = write_snapshot(
        &store,
        "Newer manual save",
        application::SaveKind::Manual,
        999,
        expected.clone(),
    );
    std::fs::write(saves.path().join("broken.mgsave"), "broken").unwrap();
    for keyboard in [true, false] {
        let mut app = startup_menu(&logs, &saves);
        assert_eq!(
            *app.world().resource::<State<game::state::AppState>>().get(),
            game::state::AppState::MainMenu
        );
        let model = game::hud::main_menu_model(
            app.world().resource::<Persistence>(),
            &crate::support::shipped_l10n(),
        );
        assert_eq!(
            model.continue_game.unwrap().action,
            UiAction::Persistence(PersistAction::Continue(chosen.clone()))
        );
        let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, 1024.0 / 1.25, 640.0 / 1.25);
        let resume = crate::support::egui_harness::button(&widgets, "Resume game");
        assert!(!resume.disabled);
        assert!(
            resume.rect.top()
                < crate::support::egui_harness::button(&widgets, "New game")
                    .rect
                    .top()
        );
        if keyboard {
            pass.run_with(
                &mut app,
                1024.0 / 1.25,
                640.0 / 1.25,
                vec![bevy_egui::egui::Event::Key {
                    key: bevy_egui::egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: bevy_egui::egui::Modifiers::NONE,
                }],
            );
        } else {
            pass.click_at(&mut app, 1024.0 / 1.25, 640.0 / 1.25, resume.rect.center());
        }
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Loading
        );
        let progress = browser_model(&app);
        assert!(!progress.library);
        assert!(progress.rows.is_empty());
        assert_eq!(progress.title, "Resuming game…");
        assert!(
            crate::contrast::check_paint(pass.paint_with(
                &mut app,
                1024.0 / 1.25,
                640.0 / 1.25,
                vec![]
            )) > 0
        );
        settle(&mut app);
        update_n(&mut app, 5);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
        assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
        assert_ne!(chosen, manual);
        assert_eq!(store.list().unwrap().len(), 4);
        assert!(
            crate::contrast::check_paint(pass.paint_with(
                &mut app,
                1024.0 / 1.25,
                640.0 / 1.25,
                vec![]
            )) > 0
        );
    }
}

fn write_snapshot(
    store: &infrastructure::save::FileSaveStore,
    name: &str,
    kind: application::SaveKind,
    saved_at: u64,
    checkpoint: application::GameCheckpoint,
) -> application::SaveId {
    use application::SaveStore;
    let bundle = application::SaveBundle {
        compatibility: store.compatibility(),
        metadata: application::SaveMetadata {
            slot: store.allocate_id(),
            run: store.allocate_id(),
            kind,
            name: name.into(),
            saved_at,
        },
        checkpoint,
    };
    store
        .write(&bundle, &application::SaveTarget::Create, &|| false)
        .unwrap()
        .metadata
        .slot
}

#[test]
fn manual_snapshots_keep_the_menu_and_load_game_without_a_resume_shortcut() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    write_manual(
        &store,
        "Manual only",
        source.world().resource::<Session>().0.checkpoint(),
    );
    let mut app = startup_menu(&logs, &saves);
    assert_eq!(
        *app.world().resource::<State<game::state::AppState>>().get(),
        game::state::AppState::MainMenu
    );
    let model = game::hud::main_menu_model(
        app.world().resource::<Persistence>(),
        &crate::support::shipped_l10n(),
    );
    assert!(model.continue_game.is_none());
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1024.0 / 1.25, 640.0 / 1.25);
    assert!(!widgets.iter().any(|w| w.text == "Resume game"));
    let load = crate::support::egui_harness::button(&widgets, "Load game");
    pass.click_at(&mut app, 1024.0 / 1.25, 640.0 / 1.25, load.rect.center());
    settle(&mut app);
    assert_eq!(browser_model(&app).rows[0].name, "Manual only");
}

#[test]
fn opening_resume_reports_a_snapshot_removed_after_the_index_scan() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    play_a_month(&mut source);
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let slot = write_snapshot(
        &store,
        "Removed",
        application::SaveKind::Automatic,
        200,
        source.world().resource::<Session>().0.checkpoint(),
    );
    let mut app = startup_menu(&logs, &saves);
    std::fs::remove_file(saves.path().join(format!("{}.mgsave", slot.as_str()))).unwrap();
    action(&mut app, PersistAction::Continue(slot));
    settle(&mut app);
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Browser
    );
    assert!(browser_model(&app).error.is_some());
    action(&mut app, PersistAction::Cancel);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
}

fn wait_for_preload(app: &mut App) {
    for _ in 0..4000 {
        app.update();
        if app.world().resource::<Persistence>().resume_preloaded() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("resume preload did not finish");
}

#[test]
fn opening_preload_waits_for_render_keeps_the_menu_interactive_and_discards_on_new_game() {
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    play_a_month(&mut source);
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    write_snapshot(
        &store,
        "Latest",
        application::SaveKind::Automatic,
        200,
        source.world().resource::<Session>().0.checkpoint(),
    );
    let before = files_in(logs.path());
    let mut app = startup_menu(&logs, &saves);
    assert!(!app.world().resource::<Persistence>().resume_preloaded());
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    pass.settled(&mut app, 1024.0, 640.0);
    wait_for_preload(&mut app);
    assert!(!app.world().resource::<Persistence>().busy());
    assert!(!app.world().resource::<Persistence>().frozen());
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(
        files_in(logs.path()),
        before,
        "background verification creates no recording files"
    );
    dispatch_in(
        &mut app,
        vec![UiAction::Navigate(game::hud::MenuScreen::RegionSelect)],
    );
    update_n(&mut app, 2);
    assert!(!app.world().resource::<Persistence>().resume_preloaded());
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(files_in(logs.path()), before);
}

struct GatedReadStore {
    inner: infrastructure::save::FileSaveStore,
    reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    release: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl application::SaveStore for GatedReadStore {
    fn compatibility(&self) -> application::ReplayCompatibility {
        self.inner.compatibility()
    }
    fn allocate_id(&self) -> application::SaveId {
        self.inner.allocate_id()
    }
    fn now(&self) -> u64 {
        self.inner.now()
    }
    fn list(&self) -> Result<Vec<application::SaveListing>, application::SaveError> {
        self.inner.list()
    }
    fn revision(
        &self,
        slot: &application::SaveId,
    ) -> Result<application::SaveRevision, application::SaveError> {
        self.inner.revision(slot)
    }
    fn read(
        &self,
        slot: &application::SaveId,
    ) -> Result<(application::SaveBundle, application::LoadedRegion), application::SaveError> {
        use std::sync::atomic::Ordering;
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.started.store(true, Ordering::SeqCst);
        // Bound the test gate even if an assertion fails, so an abandoned thread cannot spin forever.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !self.release.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        self.inner.read(slot)
    }
    fn rename(
        &self,
        slot: &application::SaveId,
        rev: application::SaveRevision,
        name: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<application::SaveReceipt, application::SaveError> {
        self.inner.rename(slot, rev, name, cancelled)
    }
    fn delete(
        &self,
        slot: &application::SaveId,
        revision: application::SaveRevision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), application::SaveError> {
        self.inner.delete(slot, revision, cancelled)
    }
    fn write(
        &self,
        bundle: &application::SaveBundle,
        target: &application::SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<application::SaveReceipt, application::SaveError> {
        self.inner.write(bundle, target, cancelled)
    }
}

#[test]
fn resume_waits_for_one_in_flight_preload_and_other_choices_discard_late_results() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    for resume in [true, false] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let mut source = hud_app(logs.path());
        request(&mut source, "western_interconnection");
        play_a_month(&mut source);
        let expected = source.world().resource::<Session>().0.checkpoint();
        let store = infrastructure::save::FileSaveStore::new(saves.path().into());
        let slot = write_snapshot(
            &store,
            "Latest",
            application::SaveKind::Automatic,
            200,
            expected.clone(),
        );
        let reads = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let mut app = crate::support::headless_app(&crate::support::shipped_assets(), logs.path());
        app.add_plugins(game::hud::HudPlugin)
            .insert_resource(Persistence::with_store(Arc::new(GatedReadStore {
                inner: store,
                reads: reads.clone(),
                started: started.clone(),
                release: release.clone(),
            })));
        settle(&mut app);
        update_n(&mut app, 3);
        let before = files_in(logs.path());
        let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
        pass.settled(&mut app, 1024.0, 640.0);
        for _ in 0..4000 {
            if started.load(Ordering::SeqCst) {
                break;
            }
            std::thread::yield_now();
        }
        assert!(started.load(Ordering::SeqCst));
        assert!(!app.world().resource::<Persistence>().busy());
        assert_eq!(files_in(logs.path()), before);
        if resume {
            action(&mut app, PersistAction::Continue(slot));
            assert!(app.world().resource::<Persistence>().busy());
            assert_eq!(
                app.world().resource::<Persistence>().screen,
                game::sim::SaveScreen::Loading
            );
        } else {
            dispatch_in(
                &mut app,
                vec![UiAction::Navigate(game::hud::MenuScreen::RegionSelect)],
            );
        }
        release.store(true, Ordering::SeqCst);
        settle(&mut app);
        update_n(&mut app, 5);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "Resume reuses the first read/replay job"
        );
        if resume {
            assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
            assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
        } else {
            assert!(!app.world().contains_resource::<Session>());
            assert!(!app.world().resource::<Persistence>().resume_preloaded());
            assert_eq!(files_in(logs.path()), before);
        }
    }
}

#[test]
fn a_preloaded_snapshot_replaced_on_disk_is_rejected_and_load_game_discards_the_cache() {
    use application::SaveStore;
    for replace in [false, true] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let mut source = hud_app(logs.path());
        request(&mut source, "western_interconnection");
        play_a_month(&mut source);
        let store = infrastructure::save::FileSaveStore::new(saves.path().into());
        let slot = write_snapshot(
            &store,
            "Latest",
            application::SaveKind::Automatic,
            200,
            source.world().resource::<Session>().0.checkpoint(),
        );
        let mut app = startup_menu(&logs, &saves);
        let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
        pass.settled(&mut app, 1024.0, 640.0);
        wait_for_preload(&mut app);
        let before = files_in(logs.path());
        if replace {
            let (mut bundle, _) = store.read(&slot).unwrap();
            let revision = store.revision(&slot).unwrap();
            bundle.metadata.saved_at += 1;
            store
                .write(
                    &bundle,
                    &application::SaveTarget::Replace(revision),
                    &|| false,
                )
                .unwrap();
            action(&mut app, PersistAction::Continue(slot));
            settle(&mut app);
            assert_eq!(
                app.world().resource::<Persistence>().error,
                Some(application::SaveError::SlotChanged)
            );
        } else {
            open_browser(&mut app);
            settle(&mut app);
        }
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Browser
        );
        assert!(!app.world().resource::<Persistence>().resume_preloaded());
        assert!(!app.world().contains_resource::<Session>());
        assert_eq!(files_in(logs.path()), before);
    }
}

#[test]
fn load_library_splits_large_named_cards_from_twelve_recent_compact_snapshots() {
    use application::{SaveKind, SaveStore};
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    play_a_month(&mut source);
    let checkpoint = source.world().resource::<Session>().0.checkpoint();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    for n in 0..17 {
        write_snapshot(
            &store,
            &format!("Snapshot {n:02}"),
            SaveKind::Automatic,
            100 + n,
            checkpoint.clone(),
        );
    }
    write_snapshot(
        &store,
        "Named company Alpha",
        SaveKind::Manual,
        1,
        checkpoint.clone(),
    );
    write_snapshot(
        &store,
        "Named company Beta",
        SaveKind::Manual,
        2,
        checkpoint,
    );
    std::fs::write(saves.path().join("broken.mgsave"), "broken").unwrap();
    let mut app = load_browser(&logs, &saves);
    let model = browser_model(&app);
    let automatic: Vec<_> = model
        .rows
        .iter()
        .filter(|row| row.automatic)
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(automatic.len(), 12);
    assert_eq!(automatic[0], "Snapshot 16");
    assert_eq!(automatic[11], "Snapshot 05");
    assert_eq!(
        model
            .rows
            .iter()
            .filter(|row| !row.automatic && row.problem.is_none())
            .count(),
        2
    );
    assert_eq!(
        model
            .rows
            .iter()
            .filter(|row| row.problem.is_some())
            .count(),
        1
    );
    assert_eq!(
        store.list().unwrap().len(),
        20,
        "the display cap never deletes files"
    );
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    for (width, height) in [
        (1024.0, 640.0),
        (1024.0 / 1.25, 640.0 / 1.25),
        (1920.0, 1080.0),
    ] {
        let widgets = pass.settled(&mut app, width, height);
        let named = widgets.iter().find(|w| w.text == "Named games").unwrap();
        let auto = widgets
            .iter()
            .find(|w| w.text == "Automatic snapshots")
            .unwrap();
        assert!(named.rect.right() < auto.rect.left());
        let left_resume = widgets
            .iter()
            .find(|w| w.text == "Resume" && w.rect.left() < auto.rect.left())
            .unwrap();
        let right_resume = widgets
            .iter()
            .find(|w| w.text == "Resume" && w.rect.left() >= auto.rect.left())
            .unwrap();
        assert!(left_resume.rect.height() >= 36.0);
        assert!(right_resume.rect.height() <= 30.0);
        assert!(left_resume.rect.width() > right_resume.rect.width());
        for label in ["Refresh", "Cancel"] {
            let control = widgets.iter().find(|w| w.text == label).unwrap();
            assert!(
                control.rect.left() >= 0.0
                    && control.rect.right() <= width
                    && control.rect.bottom() <= height
            );
        }
        assert!(crate::contrast::check_paint(pass.paint_with(&mut app, width, height, vec![])) > 0);
    }
    let (width, height) = (1024.0 / 1.25, 640.0 / 1.25);
    let before = pass.settled(&mut app, width, height);
    let named = before
        .iter()
        .find(|w| w.text == "Named company Beta")
        .unwrap()
        .rect;
    let heading = before
        .iter()
        .find(|w| w.text == "Automatic snapshots")
        .unwrap()
        .rect;
    pass.run_with(
        &mut app,
        width,
        height,
        vec![
            bevy_egui::egui::Event::PointerMoved(
                heading.center() + bevy_egui::egui::vec2(0.0, 90.0),
            ),
            bevy_egui::egui::Event::MouseWheel {
                unit: bevy_egui::egui::MouseWheelUnit::Point,
                phase: bevy_egui::egui::TouchPhase::Move,
                delta: bevy_egui::egui::vec2(0.0, -3000.0),
                modifiers: bevy_egui::egui::Modifiers::NONE,
            },
        ],
    );
    let after = pass.settled(&mut app, width, height);
    assert_eq!(
        after
            .iter()
            .find(|w| w.text == "Named company Beta")
            .unwrap()
            .rect,
        named,
        "columns scroll independently"
    );
    let oldest = after.iter().find(|w| w.text == "Snapshot 05").unwrap().rect;
    assert!(
        oldest.top() >= heading.bottom() && oldest.bottom() < height - 50.0,
        "{oldest:?}"
    );
    assert!(!app.world().contains_resource::<Session>());
    let visible_action = |widgets: &[crate::support::egui_harness::Widget], label: &str| {
        widgets
            .iter()
            .find(|w| {
                w.text == label
                    && w.rect.left() >= heading.left()
                    && w.rect.top() >= heading.bottom()
                    && w.rect.bottom() < height - 50.0
            })
            .unwrap()
            .rect
            .center()
    };
    pass.click_at(&mut app, width, height, visible_action(&after, "Rename"));
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Rename
    );
    action(&mut app, PersistAction::Cancel);
    settle(&mut app);
    let widgets = pass.settled(&mut app, width, height);
    pass.click_at(&mut app, width, height, visible_action(&widgets, "Resume"));
    settle(&mut app);
    update_n(&mut app, 5);
    assert_eq!(
        app.world().resource::<Session>().0.checkpoint(),
        source.world().resource::<Session>().0.checkpoint()
    );
    assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
}

#[test]
fn save_delete_card_confirms_cancels_and_removes_only_the_selected_save() {
    use application::{SaveKind, SaveStore};
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    let checkpoint = source.world().resource::<Session>().0.checkpoint();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let manual = write_snapshot(
        &store,
        "Delete this company",
        SaveKind::Manual,
        3,
        checkpoint.clone(),
    );
    let auto = write_snapshot(
        &store,
        "Monthly snapshot",
        SaveKind::Automatic,
        2,
        checkpoint,
    );
    let auto_path = saves.path().join(format!("{}.mgsave", auto.as_str()));
    let auto_bytes = std::fs::read(&auto_path).unwrap();
    let mut app = load_browser(&logs, &saves);
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let (width, height) = (1024.0 / 1.25, 640.0 / 1.25);
    let widgets = pass.settled(&mut app, width, height);
    let deletes: Vec<_> = widgets.iter().filter(|w| w.text == "Delete").collect();
    assert_eq!(deletes.len(), 2);
    for button in &deletes {
        assert!(!button.disabled);
        assert!(button.rect.right() <= width && button.rect.bottom() < height - 45.0);
    }
    let manual_delete = deletes
        .iter()
        .min_by(|a, b| a.rect.left().total_cmp(&b.rect.left()))
        .unwrap();
    pass.click_at(&mut app, width, height, manual_delete.rect.center());
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::ConfirmDelete
    );
    let model = browser_model(&app);
    assert!(model.body.contains("Delete this company"));
    assert!(model.body.contains("Western") && model.body.contains("2027"));
    assert!(
        store.read(&manual).is_ok(),
        "choosing Delete never removes a file"
    );
    let widgets = pass.settled(&mut app, width, height);
    let cancel = widgets.iter().find(|w| w.text == "Cancel").unwrap();
    pass.click_at(&mut app, width, height, cancel.rect.center());
    settle(&mut app);
    assert!(store.read(&manual).is_ok());
    action(&mut app, PersistAction::BeginDelete(manual.clone()));
    pass.settled(&mut app, width, height);
    for pressed in [true, false] {
        pass.run_with(
            &mut app,
            width,
            height,
            vec![bevy_egui::egui::Event::Key {
                key: bevy_egui::egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: bevy_egui::egui::Modifiers::NONE,
            }],
        );
    }
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Browser,
        "Enter defaults to Cancel"
    );
    assert!(store.read(&manual).is_ok());
    action(&mut app, PersistAction::BeginDelete(manual.clone()));
    let widgets = pass.settled(&mut app, width, height);
    assert!(crate::contrast::check_paint(pass.paint_with(&mut app, width, height, vec![])) > 0);
    let confirm = widgets
        .iter()
        .find(|w| w.text == "Delete permanently")
        .unwrap();
    pass.click_at(&mut app, width, height, confirm.rect.center());
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Browser
    );
    assert!(
        !saves
            .path()
            .join(format!("{}.mgsave", manual.as_str()))
            .exists()
    );
    assert_eq!(std::fs::read(&auto_path).unwrap(), auto_bytes);
    assert_eq!(browser_model(&app).rows.len(), 1);
    assert!(!app.world().contains_resource::<Session>());
}

#[test]
fn save_delete_refreshes_resume_and_protects_a_changed_confirmation_target() {
    use application::{SaveError, SaveKind, SaveStore};
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut source = hud_app(logs.path());
    request(&mut source, "western_interconnection");
    let checkpoint = source.world().resource::<Session>().0.checkpoint();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let older = write_snapshot(&store, "Older", SaveKind::Automatic, 1, checkpoint.clone());
    let latest = write_snapshot(&store, "Latest", SaveKind::Automatic, 2, checkpoint);
    let mut app = load_browser(&logs, &saves);
    action(&mut app, PersistAction::BeginDelete(latest.clone()));
    store
        .rename(
            &latest,
            store.revision(&latest).unwrap(),
            "Changed elsewhere",
            &|| false,
        )
        .unwrap();
    action(&mut app, PersistAction::ConfirmDelete);
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Persistence>().error,
        Some(SaveError::SlotChanged)
    );
    assert_eq!(
        store.read(&latest).unwrap().0.metadata.name,
        "Changed elsewhere"
    );
    action(&mut app, PersistAction::Cancel);
    settle(&mut app);
    action(&mut app, PersistAction::BeginDelete(latest));
    action(&mut app, PersistAction::ConfirmDelete);
    settle(&mut app);
    assert_eq!(
        app.world()
            .resource::<Persistence>()
            .continue_entry()
            .unwrap()
            .metadata
            .slot,
        older
    );
    action(&mut app, PersistAction::BeginDelete(older));
    action(&mut app, PersistAction::ConfirmDelete);
    settle(&mut app);
    action(&mut app, PersistAction::Cancel);
    assert!(
        app.world()
            .resource::<Persistence>()
            .continue_entry()
            .is_none()
    );
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
}

#[test]
fn save_delete_retries_storage_failures_and_deletes_an_unreadable_snapshot() {
    use application::{SaveError, SaveStore};
    use infrastructure::save::{FileSaveStore, PublishStage};
    for stage in [PublishStage::Publish, PublishStage::SyncDirectory] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let (fail, persistence) = faulty_store_at(&saves, stage, Injected::Error);
        let slot = application::SaveId::new("f".repeat(32)).unwrap();
        let path = saves.path().join(format!("{}.mgsave", slot.as_str()));
        std::fs::write(&path, b"unreadable snapshot").unwrap();
        let mut app = hud_app(logs.path());
        app.insert_resource(persistence);
        open_browser(&mut app);
        settle(&mut app);
        let row = browser_model(&app).rows.remove(0);
        assert!(row.action.is_none() && row.rename.is_none());
        assert!(row.delete.is_some());
        action(&mut app, PersistAction::BeginDelete(slot.clone()));
        assert!(browser_model(&app).body.contains(slot.as_str()));
        set(&fail, true);
        action(&mut app, PersistAction::ConfirmDelete);
        // Cancellation cannot orphan a committed deletion, even before polling its worker.
        action(&mut app, PersistAction::Cancel);
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::ConfirmDelete
        );
        assert!(
            browser_model(&app)
                .buttons
                .iter()
                .any(|b| b.label == "Cancel" && !b.enabled)
        );
        settle(&mut app);
        let p = app.world().resource::<Persistence>();
        assert_eq!(p.screen, game::sim::SaveScreen::ConfirmDelete);
        assert!(p.error.is_some());
        if stage == PublishStage::SyncDirectory {
            assert!(matches!(
                p.error,
                Some(SaveError::DeleteDurabilityUnconfirmed { .. })
            ));
            assert!(!path.exists());
        } else {
            assert_eq!(std::fs::read(&path).unwrap(), b"unreadable snapshot");
        }
        set(&fail, false);
        action(&mut app, PersistAction::Retry);
        settle(&mut app);
        assert_eq!(
            app.world().resource::<Persistence>().screen,
            game::sim::SaveScreen::Browser
        );
        assert!(browser_model(&app).rows.is_empty());
        assert!(
            FileSaveStore::new(saves.path().into())
                .list()
                .unwrap()
                .is_empty()
        );
        assert!(!app.world().contains_resource::<Session>());
    }
}

#[test]
fn monthly_autosave_stays_in_background_and_serializes_later_boundaries() {
    use application::SaveStore;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let _release = Release(released.clone());
    let flag = entered.clone();
    let gate = released.clone();
    let store = Arc::new(
        infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
            move |stage| {
                if stage == infrastructure::save::PublishStage::PartialWrite {
                    flag.store(true, Ordering::Release);
                    while !gate.load(Ordering::Acquire) {
                        std::thread::park_timeout(std::time::Duration::from_millis(1));
                    }
                }
                Ok(())
            },
        )),
    );
    let mut app = hud_app(logs.path());
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ))
    .insert_resource(Persistence::with_store(store.clone()));
    request(&mut app, "western_interconnection");
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !entered.load(Ordering::Acquire) {
        app.update();
        assert!(std::time::Instant::now() < deadline);
    }
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
    assert!(!app.world().resource::<Persistence>().frozen());
    update_n(&mut app, 3);
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1280.0, 640.0);
    let retrospective =
        crate::support::egui_harness::button(&widgets, "Supply demand retrospective");
    pass.click_at(&mut app, 1280.0, 640.0, retrospective.rect.center());
    let widgets = pass.settled(&mut app, 1280.0, 640.0);
    assert!(widgets.iter().any(|w| w.text == "Review summary"));
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::FinishReview),
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    update_n(&mut app, 3);
    let latest = app.world().resource::<Session>().0.checkpoint();
    assert_eq!(latest.summary.month.month(), 2);
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::FinishReview),
            UiAction::Command(Command::BeginOperating),
        ],
    );
    update_n(&mut app, 3);
    dispatch_in(&mut app, vec![UiAction::SetSpeed(SimSpeed::X4)]);
    released.store(true, Ordering::Release);
    settle(&mut app);
    assert_eq!(
        *app.world().resource::<SimSpeed>(),
        SimSpeed::X4,
        "late autosave completion must not pause the next day"
    );
    let rows = store.list().unwrap();
    assert_eq!(rows.len(), 1);
    let (saved, _) = store.read(rows[0].id.as_ref().unwrap()).unwrap();
    assert_eq!(
        saved.checkpoint, latest,
        "latest month-close snapshot wins; later live commands stay out"
    );
    assert_eq!(
        app.world()
            .resource::<Session>()
            .0
            .session()
            .clock()
            .month()
            .month(),
        3
    );
}

struct DelayedAutosave {
    entered: std::sync::Arc<std::sync::atomic::AtomicBool>,
    released: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for DelayedAutosave {
    fn drop(&mut self) {
        self.released
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
fn delayed_autosave(
    fail_once: bool,
) -> (
    App,
    TempDir,
    TempDir,
    std::sync::Arc<infrastructure::save::FileSaveStore>,
    DelayedAutosave,
) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let gate = DelayedAutosave {
        entered: Arc::new(AtomicBool::new(false)),
        released: Arc::new(AtomicBool::new(false)),
    };
    let entered = gate.entered.clone();
    let released = gate.released.clone();
    let fail = AtomicBool::new(fail_once);
    let store = Arc::new(
        infrastructure::save::FileSaveStore::new(saves.path().into()).with_fault(Arc::new(
            move |stage| {
                if stage == infrastructure::save::PublishStage::PartialWrite {
                    entered.store(true, Ordering::Release);
                    while !released.load(Ordering::Acquire) {
                        std::thread::park_timeout(std::time::Duration::from_millis(1));
                    }
                    if fail.swap(false, Ordering::Relaxed) {
                        return Err(application::SaveError::Io {
                            stage: "test",
                            message: "full".into(),
                        });
                    }
                }
                Ok(())
            },
        )),
    );
    let mut app = hud_app(logs.path());
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ))
    .insert_resource(Persistence::with_store(store.clone()));
    request(&mut app, "western_interconnection");
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !gate.entered.load(Ordering::Acquire) {
        app.update();
        assert!(std::time::Instant::now() < deadline);
    }
    (app, logs, saves, store, gate)
}
#[test]
fn queued_background_autosaves_can_retry_a_failure_without_losing_the_newer_boundary() {
    use application::SaveStore;
    use std::sync::atomic::Ordering;
    let (mut app, _logs, _saves, store, gate) = delayed_autosave(true);
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::FinishReview),
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 1440 }),
        ],
    );
    update_n(&mut app, 3);
    let latest = app.world().resource::<Session>().0.checkpoint();
    assert_eq!(latest.summary.month.month(), 2);
    gate.released.store(true, Ordering::Release);
    settle(&mut app);
    let p = app.world().resource::<Persistence>();
    assert!(p.automatic_error());
    assert!(!p.busy());
    assert_eq!(p.autosave_failure().unwrap().month.month(), 1);
    action(&mut app, PersistAction::Retry);
    settle(&mut app);
    let p = app.world().resource::<Persistence>();
    assert!(p.error.is_none());
    assert!(p.autosave_failure().is_none());
    assert_eq!(p.screen, game::sim::SaveScreen::Closed);
    let rows = store.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        store
            .read(rows[0].id.as_ref().unwrap())
            .unwrap()
            .0
            .checkpoint,
        latest
    );
}
#[test]
fn cancelling_a_manual_dialog_does_not_cancel_the_background_month_close_save() {
    use application::SaveStore;
    use std::sync::atomic::Ordering;
    let (mut app, _logs, _saves, store, gate) = delayed_autosave(false);
    let closed = app.world().resource::<Session>().0.checkpoint();
    dispatch_in(
        &mut app,
        vec![
            UiAction::Command(Command::FinishReview),
            UiAction::Command(Command::BeginOperating),
            UiAction::Command(Command::AdvanceTicks { n: 20 }),
        ],
    );
    update_n(&mut app, 3);
    let live = app.world().resource::<Session>().0.checkpoint();
    action(&mut app, PersistAction::OpenSave { leave: false });
    update_n(&mut app, 2);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Save
    );
    action(&mut app, PersistAction::Cancel);
    assert_eq!(
        app.world().resource::<Persistence>().screen,
        game::sim::SaveScreen::Closed
    );
    gate.released.store(true, Ordering::Release);
    settle(&mut app);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), live);
    let rows = store.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        store
            .read(rows[0].id.as_ref().unwrap())
            .unwrap()
            .0
            .checkpoint,
        closed
    );
}

#[test]
fn save_game_shows_only_manual_games_while_load_keeps_monthly_snapshots() {
    use application::{SaveKind, SaveStore};
    let logs = TempDir::new().unwrap();
    let saves = TempDir::new().unwrap();
    let mut app = operating(&logs, &saves);
    let checkpoint = app.world().resource::<Session>().0.checkpoint();
    let store = infrastructure::save::FileSaveStore::new(saves.path().into());
    let automatic = write_snapshot(
        &store,
        "Renamed monthly checkpoint",
        SaveKind::Automatic,
        2,
        checkpoint.clone(),
    );
    let manual = write_snapshot(&store, "My saved company", SaveKind::Manual, 1, checkpoint);
    std::fs::write(saves.path().join("broken.mgsave"), "broken").unwrap();
    let original_automatic = store.read(&automatic).unwrap().0;

    action(&mut app, PersistAction::OpenSave { leave: false });
    settle(&mut app);
    let model = browser_model(&app);
    assert_eq!(model.rows.len(), 1);
    assert_eq!(model.rows[0].name, "My saved company");
    assert_eq!(
        model.rows[0].action.as_ref().unwrap().action,
        UiAction::Persistence(PersistAction::ChooseOverwrite {
            slot: manual,
            name: "My saved company".into()
        })
    );
    let pass = crate::support::egui_pass::EguiPass::attach(&mut app);
    let widgets = pass.settled(&mut app, 1280.0, 800.0);
    assert!(
        widgets
            .iter()
            .any(|widget| widget.text == "My saved company")
    );
    assert!(widgets.iter().any(|widget| widget.text == "Overwrite"));
    assert!(
        !widgets
            .iter()
            .any(|widget| widget.text == "Renamed monthly checkpoint")
    );
    assert!(!widgets.iter().any(|widget| widget.text == "broken.mgsave"));

    action(&mut app, PersistAction::Cancel);
    open_browser(&mut app);
    settle(&mut app);
    let model = browser_model(&app);
    assert_eq!(model.rows.len(), 3);
    assert!(
        model
            .rows
            .iter()
            .any(|row| row.automatic && row.name == "Renamed monthly checkpoint")
    );
    assert!(model.rows.iter().any(|row| row.problem.is_some()));
    assert_eq!(store.read(&automatic).unwrap().0, original_automatic);
    assert_eq!(store.list().unwrap().len(), 3);
}
