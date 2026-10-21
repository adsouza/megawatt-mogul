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
    action(&mut app, PersistAction::OpenBrowser);
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
    action(&mut app, PersistAction::OpenBrowser);
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
