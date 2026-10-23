//! Frozen released snapshots remain compatible without historical simulation engines.
use application::*;
use domain::session::DayPhase;
use infrastructure::save::{decode_save, encode_save, replay_compatibility};
use std::path::Path;
use tempfile::TempDir;
fn restored(bytes: &[u8]) -> Game {
    let (bundle, region) = decode_save(bytes, &replay_compatibility()).unwrap();
    bundle
        .restore(
            region,
            &replay_compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap()
}
#[test]
fn released_portable_schema1_golden_saves_restore_and_continue_without_replaying_history() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/portable-saves");
    let mut paths = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "mgsave"))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 4);
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.starts_with(b"(version:2,"),
            "released format-2 fixture"
        );
        let mut game = restored(&bytes);
        let before = game.checkpoint();
        let (mut bundle, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
        assert_eq!(bundle.checkpoint.snapshot.as_ref().unwrap().schema, 1);
        assert_eq!(
            bundle.checkpoint.header.sim_rules_version, 8,
            "released rules-8 bytes"
        );
        let mut expected = bundle.checkpoint.clone();
        expected.header.sim_rules_version = domain::SIM_RULES_VERSION;
        assert_eq!(
            before, expected,
            "rule upgrade preserves all canonical state, RNG, history and audit commands"
        );
        if path.file_name().unwrap() == "schema1-charged-battery.mgsave" {
            assert_eq!(before.summary.tick, 900);
            assert!(
                game.session()
                    .plant_views()
                    .iter()
                    .any(|p| p.battery.as_ref().is_some_and(|b| b.stored.value() > 0.0))
            );
        }
        match game.session().phase() {
            DayPhase::Planning => {
                game.execute(Command::BeginOperating).unwrap();
            }
            DayPhase::Operating => {
                game.execute(Command::AdvanceTicks { n: 10 }).unwrap();
            }
            DayPhase::Review => {
                game.execute(Command::FinishReview).unwrap();
            }
        }
        assert_eq!(game.checkpoint().command_count, before.command_count + 1);
        bundle.compatibility = replay_compatibility();
        bundle.checkpoint = game.checkpoint();
        let rewritten = encode_save(&bundle).unwrap();
        assert!(rewritten.starts_with(b"(version:3,"));
        let resumed = restored(&rewritten);
        assert_eq!(resumed.checkpoint(), game.checkpoint());
        assert_eq!(
            resumed.session().plant_views(),
            game.session().plant_views()
        );
        assert_eq!(resumed.projections(), game.projections());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn resumed_recording_keeps_its_portable_base_and_replays_new_commands_only() {
    let dir = TempDir::new().unwrap();
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/portable-saves/schema1-migrated-operating.mgsave"),
    )
    .unwrap();
    let path = dir.path().join("branch.log");
    let mut game = restored(&bytes)
        .with_recording(
            Box::new(infrastructure::command_log::FileCommandLog::create(&path).unwrap()),
            &|| false,
        )
        .unwrap();
    if game.session().phase() == DayPhase::Operating {
        game.execute(Command::AdvanceTicks { n: 10 }).unwrap();
    }
    let input = infrastructure::command_log::load_replay(&path).unwrap();
    assert!(input.checkpoint.is_some());
    assert_eq!(
        input.replay().unwrap().state_hash(),
        game.session().state_hash()
    );
    // The old command-only API fails explicitly instead of ignoring the snapshot base.
    let input = infrastructure::command_log::load_replay(&path).unwrap();
    assert!(application::replay(input.region, &input.header, &input.commands).is_err());
}

#[test]
fn pre_battery_rules_fail_without_an_in_game_upgrade_or_any_source_write() {
    use application::{ReplayError, RestoreError, SaveError, SaveStore};
    use infrastructure::save::FileSaveStore;
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/portable-saves/schema1-migrated-operating.mgsave"),
    )
    .unwrap();
    for rules in [6, 7] {
        let (mut old, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
        old.checkpoint.header.sim_rules_version = rules;
        let path = dir
            .path()
            .join(format!("{}.mgsave", old.metadata.slot.as_str()));
        let original = encode_save(&old).unwrap();
        std::fs::write(&path, &original).unwrap();
        assert!(
            matches!(store.read(&old.metadata.slot),Err(SaveError::Restore(RestoreError::Replay(ReplayError::RulesVersionMismatch{recorded,..}))) if recorded==rules)
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[test]
fn converted_snapshot_provenance_hides_only_the_exact_original_and_protects_it() {
    use application::{SaveError, SaveRevision, SaveStore, SaveTarget};
    use infrastructure::save::FileSaveStore;
    use sha2::{Digest, Sha256};
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/portable-saves/schema1-migrated-operating.mgsave"),
    )
    .unwrap();
    let (mut bundle, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
    let original = b"original legacy bytes retained for archived recovery";
    let origin = bundle
        .checkpoint
        .snapshot
        .as_mut()
        .unwrap()
        .origin
        .as_mut()
        .unwrap();
    origin.revision = SaveRevision(Sha256::digest(original).into());
    let id = origin.slot.clone();
    let original_path = dir.path().join(format!("{}.mgsave", id.as_str()));
    std::fs::write(&original_path, original).unwrap();
    bundle.compatibility = store.compatibility();
    bundle.metadata.slot = store.allocate_id();
    store
        .write(&bundle, &SaveTarget::Create, &|| false)
        .unwrap();
    let rows = store.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].entry.is_ok());
    let mut overwrite = bundle.clone();
    overwrite.metadata.slot = id.clone();
    assert!(matches!(
        store.write(
            &overwrite,
            &SaveTarget::Replace(SaveRevision(Sha256::digest(original).into())),
            &|| false
        ),
        Err(SaveError::SlotChanged)
    ));
    // An unsupported future copy is visible as unavailable and must not suppress its original.
    bundle.checkpoint.snapshot.as_mut().unwrap().schema = 99;
    let path = dir
        .path()
        .join(format!("{}.mgsave", bundle.metadata.slot.as_str()));
    std::fs::write(&path, encode_save(&bundle).unwrap()).unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    assert_eq!(std::fs::read(&original_path).unwrap(), original);
}

#[test]
fn prepared_recording_is_memory_only_bound_to_the_exact_game_and_replayable() {
    use infrastructure::command_log::{FileCommandLog, PreparedRecording, load_replay};
    let dir = TempDir::new().unwrap();
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/portable-saves/schema1-migrated-operating.mgsave"),
    )
    .unwrap();
    let game = restored(&bytes);
    let checkpoint = game.checkpoint();
    let prepared = PreparedRecording::new(checkpoint.clone(), &|| false).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let path = dir.path().join("prepared.log");
    let mut game = game
        .with_recording(
            Box::new(
                FileCommandLog::create(&path)
                    .unwrap()
                    .with_prepared_recording(prepared),
            ),
            &|| false,
        )
        .unwrap();
    game.execute(Command::AdvanceTicks { n: 1 }).unwrap();
    assert_eq!(
        load_replay(&path).unwrap().replay().unwrap().state_hash(),
        game.session().state_hash()
    );
    let prepared = PreparedRecording::new(checkpoint, &|| false).unwrap();
    let bad = dir.path().join("wrong.log");
    assert!(
        game.with_recording(
            Box::new(
                FileCommandLog::create(&bad)
                    .unwrap()
                    .with_prepared_recording(prepared)
            ),
            &|| false
        )
        .is_err()
    );
    assert!(
        !infrastructure::command_log::checkpoint_archive_path(&bad)
            .unwrap()
            .exists()
    );
    assert!(PreparedRecording::new(restored(&bytes).checkpoint(), &|| true).is_err());
}
