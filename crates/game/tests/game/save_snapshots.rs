//! Converted portable saves load normally; no recovery UI or writes are needed.
use crate::{
    hud_smoke::dispatch_in,
    support::{egui_harness::button, egui_pass::EguiPass, headless_app, shipped_assets, update_n},
};
use application::{InMemoryCommandLog, SaveStore};
use bevy::prelude::*;
use game::{
    hud::UiAction,
    paths::SaveDir,
    sim::{PersistAction, Persistence, Session, SimSpeed},
};
use infrastructure::save::{FileSaveStore, decode_save, replay_compatibility};
use tempfile::TempDir;
fn settle(app: &mut App) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.update();
        if !app.world().resource::<Persistence>().busy() {
            break;
        }
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    }
}
fn setup(
    file: &str,
) -> (
    App,
    TempDir,
    TempDir,
    application::SaveId,
    Vec<u8>,
    application::GameCheckpoint,
) {
    let saves = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../infrastructure/tests/support/portable-saves")
            .join(file),
    )
    .unwrap();
    let (bundle, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
    let id = bundle.metadata.slot;
    std::fs::write(saves.path().join(format!("{}.mgsave", id.as_str())), &bytes).unwrap();
    let mut app = headless_app(&shipped_assets(), logs.path());
    app.insert_resource(SaveDir(saves.path().into()))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ))
        .add_plugins((game::hud::HudPlugin, game::preferences::PreferencesPlugin));
    update_n(&mut app, 3);
    settle(&mut app);
    update_n(&mut app, 3);
    (app, saves, logs, id, bytes, bundle.checkpoint)
}
#[test]
fn portable_resume_click_uses_normal_ui_and_restores_paused_progress() {
    for phase in ["planning", "operating"] {
        let (mut app, saves, _logs, id, bytes, expected) =
            setup(&format!("schema1-migrated-{phase}.mgsave"));
        dispatch_in(
            &mut app,
            vec![UiAction::Persistence(PersistAction::OpenBrowser)],
        );
        settle(&mut app);
        let pass = EguiPass::attach(&mut app);
        let widgets = pass.settled(&mut app, 1280.0, 640.0);
        let resume = button(&widgets, "Resume");
        assert!(
            !widgets
                .iter()
                .any(|w| w.text.contains("upgrade") || w.text.contains("Upgraded"))
        );
        pass.click_at(&mut app, 1280.0, 640.0, resume.rect.center());
        settle(&mut app);
        update_n(&mut app, 5);
        assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
        assert_eq!(*app.world().resource::<SimSpeed>(), SimSpeed::Paused);
        let store = FileSaveStore::new(saves.path().into());
        let entries = store.list().unwrap();
        assert_eq!(entries.len(), 1);
        let entry = entries[0].entry.as_ref().unwrap();
        assert_eq!(entry.metadata.slot, id);
        let (copy, region) = store.read(&entry.metadata.slot).unwrap();
        let resumed = copy
            .restore(
                region,
                &store.compatibility(),
                Box::new(InMemoryCommandLog::default()),
                &|| false,
            )
            .unwrap();
        assert_eq!(resumed.checkpoint(), expected);
        assert_eq!(
            std::fs::read(saves.path().join(format!("{}.mgsave", id.as_str()))).unwrap(),
            bytes
        );
    }
}
#[test]
fn portable_resume_preload_and_activation_preserve_the_saved_bytes() {
    let (mut app, saves, _logs, id, bytes, expected) = setup("schema1-migrated-review.mgsave");
    let pass = EguiPass::attach(&mut app);
    pass.settled(&mut app, 1280.0, 640.0);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.world().resource::<Persistence>().resume_preloaded() {
        app.update();
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    }
    let files = || {
        std::fs::read_dir(saves.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "mgsave")
            })
            .count()
    };
    assert_eq!(files(), 1);
    assert!(!app.world().contains_resource::<Session>());
    let widgets = pass.settled(&mut app, 1280.0, 640.0);
    let resume = button(&widgets, "Resume game");
    pass.click_at(&mut app, 1280.0, 640.0, resume.rect.center());
    settle(&mut app);
    update_n(&mut app, 5);
    assert_eq!(files(), 1);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
    assert_eq!(
        std::fs::read(saves.path().join(format!("{}.mgsave", id.as_str()))).unwrap(),
        bytes
    );
}

#[test]
fn load_library_renders_cached_cards_immediately_while_refreshing_metadata() {
    let (mut app, _saves, _logs, id, _bytes, expected) = setup("schema1-migrated-review.mgsave");
    assert!(app.world().resource::<Persistence>().menu_scanned());
    let pass = EguiPass::attach(&mut app);
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::OpenBrowser)],
    );
    // Paint before polling the new list job: the previous menu index is already useful.
    assert!(app.world().resource::<Persistence>().busy());
    let widgets = pass.run(&mut app, 1280.0, 640.0);
    assert!(
        widgets
            .iter()
            .any(|w| w.text.contains("Legacy 7 western_interconnection review"))
    );
    assert!(
        !widgets
            .iter()
            .any(|w| w.text == "Working…" || w.text == "Loading saved games…")
    );
    settle(&mut app);
    assert!(!app.world().contains_resource::<Session>());
    assert_eq!(
        app.world().resource::<Persistence>().entries[0]
            .entry
            .as_ref()
            .unwrap()
            .summary,
        expected.summary
    );
    dispatch_in(
        &mut app,
        vec![UiAction::Persistence(PersistAction::Load(id))],
    );
    let l10n = crate::support::shipped_l10n();
    let model = game::hud::saves_model(
        app.world().resource::<Persistence>(),
        app.world().resource::<game::hud::SaveForm>(),
        &l10n,
    );
    assert_eq!(
        model.body,
        l10n.text("save-restoring"),
        "selected loading keeps its distinct validation progress"
    );
    settle(&mut app);
    update_n(&mut app, 5);
    assert_eq!(app.world().resource::<Session>().0.checkpoint(), expected);
}
