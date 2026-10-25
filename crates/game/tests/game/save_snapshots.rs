//! Converted portable saves load normally; no recovery UI or writes are needed.
use crate::{
    hud_smoke::dispatch_in,
    save_resume::open_browser,
    support::{egui_harness::button, egui_pass::EguiPass, headless_app, shipped_assets, update_n},
};
use application::{Game, GameCheckpoint, InMemoryCommandLog, SaveStore};
use bevy::prelude::*;
use domain::state::State;
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
    let expected = bundle.checkpoint;
    assert_eq!(
        expected.header.sim_rules_version, 8,
        "immutable released fixture"
    );
    (app, saves, logs, id, bytes, expected)
}
/// Compare preserved fields using the source schema. A new capture has schema 2 and its
/// own hash, while elapsed state/history, RNG, source region and provenance remain unchanged.
fn assert_upgraded_without_rewriting_history(game: &Game, source: &GameCheckpoint) {
    let actual = game.checkpoint();
    let old = source.snapshot.as_ref().unwrap();
    let new = actual.snapshot.as_ref().unwrap();
    assert_eq!(old.schema, 1);
    assert_eq!(new.schema, 2);
    let mut header = source.header.clone();
    header.sim_rules_version = domain::SIM_RULES_VERSION;
    assert_eq!(actual.header, header);
    assert!(game.session().snapshot_state_schema(1) == old.session);
    assert!(game.projections().state_bytes_schema(1) == old.projections);
    assert_eq!(actual.commands, source.commands);
    assert_eq!(actual.command_count, source.command_count);
    assert_eq!(actual.summary, source.summary);
    assert_eq!(actual.region_source, source.region_source);
    assert_eq!(new.origin, old.origin);
    assert_eq!(new.journal_hash, old.journal_hash);
    assert_ne!(actual.state_hash, source.state_hash);
    for plant in game.session().power().plants().values() {
        if let Some(battery) = plant.spec().battery {
            assert_eq!(battery.charge_end_hour, 18);
        }
    }
}

#[test]
fn portable_resume_click_uses_normal_ui_and_restores_paused_progress() {
    for file in [
        "schema1-migrated-planning.mgsave",
        "schema1-migrated-operating.mgsave",
        "schema1-charged-battery.mgsave",
    ] {
        let (mut app, saves, _logs, id, bytes, expected) = setup(file);
        open_browser(&mut app);
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
        assert_upgraded_without_rewriting_history(&app.world().resource::<Session>().0, &expected);
        if file == "schema1-charged-battery.mgsave" {
            assert!(
                app.world()
                    .resource::<Session>()
                    .0
                    .session()
                    .plant_views()
                    .iter()
                    .any(|plant| plant
                        .battery
                        .as_ref()
                        .is_some_and(|battery| battery.stored.value() > 0.0))
            );
        }
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
        assert_upgraded_without_rewriting_history(&resumed, &expected);
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
    assert_upgraded_without_rewriting_history(&app.world().resource::<Session>().0, &expected);
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
    open_browser(&mut app);
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
    assert_upgraded_without_rewriting_history(&app.world().resource::<Session>().0, &expected);
}
