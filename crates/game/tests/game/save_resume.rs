use crate::hud_smoke::dispatch_in;
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
    for _ in 0..2000 {
        app.update();
        if !app.world().resource::<Persistence>().busy() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("persistence worker did not complete");
}
#[test]
fn save_resume_mid_day_restores_a_paused_runnable_game_with_all_projections() {
    for region in ["eastern_interconnection", "western_interconnection"] {
        let logs = TempDir::new().unwrap();
        let saves = TempDir::new().unwrap();
        let mut app = hud_app(logs.path());
        app.insert_resource(SaveDir(saves.path().into()));
        request(&mut app, region);
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
        let mut uninterrupted = app.world_mut().remove_resource::<Session>().unwrap().0;
        drop(app);
        let mut app = hud_app(logs.path());
        app.insert_resource(SaveDir(saves.path().into()));
        update_n(&mut app, 2);
        dispatch_in(
            &mut app,
            vec![UiAction::Persistence(PersistAction::OpenBrowser)],
        );
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

fn action(app: &mut App, action: PersistAction) {
    dispatch_in(app, vec![UiAction::Persistence(action)]);
}
fn operating(logs: &TempDir, saves: &TempDir) -> App {
    let mut app = hud_app(logs.path());
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
    action(&mut app, PersistAction::OpenBrowser);
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
    action(&mut app, PersistAction::OpenBrowser);
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
