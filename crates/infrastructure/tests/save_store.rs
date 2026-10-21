use application::*;
use domain::{ids::RegionId, session::GameSettings};
use infrastructure::{
    region::RonRegionCatalog,
    save::{FileSaveStore, PublishStage},
};
use std::{path::Path, sync::Arc};
use tempfile::TempDir;

fn bundle(store: &FileSaveStore, name: &str) -> SaveBundle {
    let catalog =
        RonRegionCatalog::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    let game = Game::start_new(
        &catalog,
        &RegionId::new("eastern_interconnection"),
        3,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    SaveBundle {
        compatibility: store.compatibility(),
        metadata: SaveMetadata {
            slot: store.allocate_id(),
            run: store.allocate_id(),
            kind: SaveKind::Manual,
            name: name.into(),
            saved_at: store.now(),
        },
        checkpoint: game.checkpoint(),
    }
}

#[test]
fn save_store_atomic_round_trip_revision_and_independent_slots() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().join("new/deep"));
    assert!(store.list().unwrap().is_empty());
    let mut b = bundle(&store, "First");
    let first = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    b.metadata.name = "Second".into();
    assert!(matches!(
        store.write(&b, &SaveTarget::Create, &|| false),
        Err(SaveError::SlotChanged)
    ));
    let second = store
        .write(&b, &SaveTarget::Replace(first.revision), &|| false)
        .unwrap();
    assert_ne!(first.revision, second.revision);
    let other_store = FileSaveStore::new(dir.path().join("new/deep"));
    assert!(matches!(
        other_store.write(&b, &SaveTarget::Replace(first.revision), &|| false),
        Err(SaveError::SlotChanged)
    ));
    assert_eq!(store.read(&b.metadata.slot).unwrap().0, b);
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn save_store_publication_faults_preserve_old_bytes_except_post_publish_sync() {
    for stage in PublishStage::ALL {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let mut b = bundle(&store, "Before");
        let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
        let path = dir
            .path()
            .join(format!("{}.mgsave", b.metadata.slot.as_str()));
        let original = std::fs::read(&path).unwrap();
        b.metadata.name = "After".into();
        let failing = FileSaveStore::new(dir.path().into()).with_fault(Arc::new(move |at| {
            if at == stage {
                Err(SaveError::Io {
                    stage: "injected",
                    message: "fault".into(),
                })
            } else {
                Ok(())
            }
        }));
        let result = failing.write(&b, &SaveTarget::Replace(receipt.revision), &|| false);
        if stage == PublishStage::SyncDirectory {
            let Err(SaveError::DurabilityUnconfirmed { revision, .. }) = result else {
                panic!("{result:?}")
            };
            assert_eq!(
                store.read(&b.metadata.slot).unwrap().0.metadata.name,
                "After"
            );
            store
                .write(&b, &SaveTarget::ConfirmDurability(revision), &|| false)
                .unwrap();
        } else {
            assert!(result.is_err(), "{stage:?}");
            assert_eq!(std::fs::read(path).unwrap(), original, "{stage:?}");
        }
    }
}

#[test]
fn save_store_bad_slots_do_not_hide_good_ones_and_cancel_does_not_publish() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let b = bundle(&store, "Good");
    assert!(matches!(
        store.write(&b, &SaveTarget::Create, &|| true),
        Err(SaveError::Cancelled)
    ));
    assert!(store.list().unwrap().is_empty());
    store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    std::fs::write(
        dir.path().join(format!("{}.mgsave", "f".repeat(32))),
        "broken",
    )
    .unwrap();
    let entries = store.list().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries.iter().filter(|e| e.entry.is_ok()).count(), 1);
}

#[test]
fn save_store_os_lock_and_revision_allow_only_one_writer_of_a_confirmed_slot() {
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let mut b = bundle(&store, "Initial");
    let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    b.metadata.name = "Winner".into();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let waiting = FileSaveStore::new(dir.path().into()).with_fault(Arc::new(move |at| {
        if at == PublishStage::Publish {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        Ok(())
    }));
    let first = b.clone();
    let worker = std::thread::spawn(move || {
        waiting.write(&first, &SaveTarget::Replace(receipt.revision), &|| false)
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let result = store.write(&b, &SaveTarget::Replace(receipt.revision), &|| false);
    release_tx.send(()).unwrap();
    assert!(matches!(result, Err(SaveError::SlotBusy)));
    worker.join().unwrap().unwrap();
    assert!(matches!(
        store.write(&b, &SaveTarget::Replace(receipt.revision), &|| false),
        Err(SaveError::SlotChanged)
    ));
    assert_eq!(store.read(&b.metadata.slot).unwrap().0, b);
}
#[test]
fn save_store_lists_invalid_filenames_and_ignores_abandoned_temporary_files() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    std::fs::write(dir.path().join("broken.mgsave"), "bad").unwrap();
    std::fs::write(dir.path().join(".tmp-abandoned"), "partial").unwrap();
    std::fs::write(dir.path().join("a.lock"), "").unwrap();
    let entries = store.list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_label, "broken");
    assert!(entries[0].id.is_none());
    assert!(matches!(entries[0].entry, Err(SaveError::InvalidId)));
}
#[test]
fn save_store_failed_new_publications_leave_no_final_slot() {
    for stage in PublishStage::ALL
        .into_iter()
        .filter(|s| *s != PublishStage::SyncDirectory)
    {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let b = bundle(&store, "New");
        let failing = store.with_fault(Arc::new(move |at| {
            if at == stage {
                Err(SaveError::Cancelled)
            } else {
                Ok(())
            }
        }));
        assert!(failing.write(&b, &SaveTarget::Create, &|| false).is_err());
        assert!(failing.list().unwrap().is_empty());
    }
}

#[test]
fn save_store_a_second_resume_needs_neither_parent_save_nor_logs_nor_installed_region() {
    use infrastructure::command_log::{FileCommandLog, read_command_log};
    let root = TempDir::new().unwrap();
    let assets = root.path().join("regions");
    std::fs::create_dir(&assets).unwrap();
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/regions/western_interconnection.ron");
    std::fs::copy(shipped, assets.join("western_interconnection.ron")).unwrap();
    let catalog = RonRegionCatalog::new(assets.clone());
    let log = root.path().join("initial.log");
    let mut live = Game::start_new(
        &catalog,
        &RegionId::new("western_interconnection"),
        7,
        GameSettings::default(),
        Box::new(FileCommandLog::create(&log).unwrap()),
    )
    .unwrap();
    live.execute(Command::BeginOperating).unwrap();
    live.execute(Command::AdvanceTicks { n: 877 }).unwrap();
    let store = FileSaveStore::new(root.path().join("saves"));
    let first = SaveBundle {
        compatibility: store.compatibility(),
        metadata: SaveMetadata {
            slot: store.allocate_id(),
            run: store.allocate_id(),
            kind: SaveKind::Manual,
            name: "First".into(),
            saved_at: store.now(),
        },
        checkpoint: live.checkpoint(),
    };
    store.write(&first, &SaveTarget::Create, &|| false).unwrap();
    drop(live);
    std::fs::remove_dir_all(assets).unwrap();
    std::fs::remove_file(&log).unwrap();
    std::fs::remove_file(root.path().join("initial.log.region.ron")).unwrap();
    let (b, region) = store.read(&first.metadata.slot).unwrap();
    let branch_log = root.path().join("branch.log");
    let mut branch = Game::restore(
        region,
        b.checkpoint,
        Box::new(FileCommandLog::create(&branch_log).unwrap()),
        &|| false,
    )
    .unwrap();
    branch.execute(Command::AdvanceTicks { n: 1439 }).unwrap();
    assert_eq!(
        read_command_log(&branch_log)
            .unwrap()
            .iter()
            .filter(|r| matches!(r, LogRecord::Command(_)))
            .count(),
        branch.checkpoint().commands.len()
    );
    let second = SaveBundle {
        compatibility: store.compatibility(),
        metadata: SaveMetadata {
            slot: store.allocate_id(),
            run: store.allocate_id(),
            kind: SaveKind::Manual,
            name: "Second".into(),
            saved_at: store.now(),
        },
        checkpoint: branch.checkpoint(),
    };
    store
        .write(&second, &SaveTarget::Create, &|| false)
        .unwrap();
    std::fs::remove_file(store_path(&root.path().join("saves"), &first.metadata.slot)).unwrap();
    drop(branch);
    std::fs::remove_file(&branch_log).unwrap();
    std::fs::remove_file(root.path().join("branch.log.region.ron")).unwrap();
    let (b, region) = store.read(&second.metadata.slot).unwrap();
    let restored = Game::restore(
        region,
        b.checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.checkpoint(), second.checkpoint);
    assert_eq!(restored.projections().monthly_reports.history().len(), 1);
}
fn store_path(dir: &Path, id: &SaveId) -> std::path::PathBuf {
    dir.join(format!("{}.mgsave", id.as_str()))
}
