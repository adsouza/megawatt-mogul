use application::*;
use domain::{ids::RegionId, session::GameSettings};
use infrastructure::{
    region::RonRegionCatalog,
    save::{FileSaveStore, PublishStage},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
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

/// A store whose writes are stamped by `build` on `platform`, as an older or foreign game's are.
fn store_built_as(root: &Path, build: &str, platform: &str) -> FileSaveStore {
    let next = Arc::new(std::sync::atomic::AtomicU64::new(1));
    FileSaveStore::with_sources(
        root.into(),
        ReplayCompatibility {
            build: build.into(),
            platform: platform.into(),
        },
        Arc::new(|| 1_800_000_000),
        Arc::new(move || {
            let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            SaveId::new(format!("{n:032x}")).unwrap()
        }),
    )
}

#[test]
fn save_store_lists_and_reads_a_save_from_another_build_with_its_metadata() {
    let dir = TempDir::new().unwrap();
    let older = store_built_as(dir.path(), "older", "other-target");
    let b = bundle(&older, "From an older build");
    older.write(&b, &SaveTarget::Create, &|| false).unwrap();
    let store = FileSaveStore::new(dir.path().into());
    assert_ne!(store.compatibility(), older.compatibility());
    let listing = store.list().unwrap();
    assert_eq!(listing.len(), 1);
    let entry = listing[0].entry.as_ref().unwrap();
    assert_eq!(entry.metadata.name, "From an older build");
    assert_eq!(entry.region, RegionId::new("eastern_interconnection"));
    assert_eq!(entry.summary.month, b.checkpoint.summary.month);
    assert_eq!(entry.compatibility, older.compatibility());
    let (read, _) = store.read(&b.metadata.slot).unwrap();
    assert_eq!(read.compatibility, older.compatibility());
    assert_eq!(read, b);
}

/// A writer stamps only its own build. A bundle stamped elsewhere is a caller bug, caught in
/// debug builds; release builds write the store's own stamp regardless.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "stamps only its own build")]
fn save_store_debug_builds_catch_a_bundle_stamped_by_another_build() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let mut b = bundle(&store, "Foreign stamp");
    b.compatibility.build = "older".into();
    let _ = store.write(&b, &SaveTarget::Create, &|| false);
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

/// Which directories a write synchronised, in order. `fails` picks the ones whose sync errors.
#[derive(Clone, Default)]
struct SyncRecord {
    synced: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    fails: Arc<std::sync::Mutex<BTreeSet<PathBuf>>>,
}
impl SyncRecord {
    fn seam(&self) -> infrastructure::save::DirectorySync {
        let record = self.clone();
        Arc::new(move |dir: &Path| {
            record.synced.lock().unwrap().push(dir.to_path_buf());
            if record.fails.lock().unwrap().contains(dir) {
                return Err(std::io::Error::other("injected directory sync failure"));
            }
            Ok(())
        })
    }
    /// The directories synchronised since the last call, as a set.
    fn take(&self) -> BTreeSet<PathBuf> {
        std::mem::take(&mut *self.synced.lock().unwrap())
            .into_iter()
            .collect()
    }
    fn fail(&self, dir: &Path) {
        self.fails.lock().unwrap().insert(dir.to_path_buf());
    }
    fn recover(&self) {
        self.fails.lock().unwrap().clear();
    }
}
fn set<const N: usize>(dirs: [PathBuf; N]) -> BTreeSet<PathBuf> {
    dirs.into_iter().collect()
}

#[test]
fn a_save_into_an_existing_root_syncs_only_the_root() {
    let dir = TempDir::new().unwrap();
    let record = SyncRecord::default();
    let store = FileSaveStore::new(dir.path().into()).with_directory_sync(record.seam());
    let mut b = bundle(&store, "First");
    let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    assert_eq!(record.take(), set([dir.path().into()]));
    b.metadata.name = "Second".into();
    store
        .write(&b, &SaveTarget::Replace(receipt.revision), &|| false)
        .unwrap();
    assert_eq!(record.take(), set([dir.path().into()]));
}

#[test]
fn a_save_that_creates_nested_directories_syncs_each_new_directorys_parent() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("a/b/c");
    let record = SyncRecord::default();
    let store = FileSaveStore::new(root.clone()).with_directory_sync(record.seam());
    let mut b = bundle(&store, "First");
    let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    // c is the destination's parent; b, a and the TempDir each gained an entry. Nothing above.
    assert_eq!(
        record.take(),
        set([
            root.clone(),
            dir.path().join("a/b"),
            dir.path().join("a"),
            dir.path().into(),
        ])
    );
    b.metadata.name = "Second".into();
    store
        .write(&b, &SaveTarget::Replace(receipt.revision), &|| false)
        .unwrap();
    assert_eq!(
        record.take(),
        set([root]),
        "the new directories are durable"
    );
}

#[test]
fn a_retry_after_an_uncertain_publish_syncs_the_directories_the_first_attempt_created() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("new/deep");
    let record = SyncRecord::default();
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let store = FileSaveStore::new(root.clone())
        .with_directory_sync(record.seam())
        .with_fault(Arc::new(move |at| {
            if at == PublishStage::SyncDirectory && flag.load(Ordering::Relaxed) {
                Err(SaveError::Io {
                    stage: "injected",
                    message: "sync".into(),
                })
            } else {
                Ok(())
            }
        }));
    let b = bundle(&store, "Uncertain");
    let Err(SaveError::DurabilityUnconfirmed { revision, .. }) =
        store.write(&b, &SaveTarget::Create, &|| false)
    else {
        panic!("the first attempt's directory sync was injected to fail")
    };
    assert!(record.take().is_empty(), "the fault fires before any sync");
    fail.store(false, Ordering::Relaxed);
    let receipt = store
        .write(&b, &SaveTarget::ConfirmDurability(revision), &|| false)
        .unwrap();
    assert_eq!(receipt.revision, revision);
    // The directories exist by now, but Retry still syncs the entries the first attempt added.
    assert_eq!(
        record.take(),
        set([root.clone(), dir.path().join("new"), dir.path().into()])
    );
    store
        .write(&b, &SaveTarget::ConfirmDurability(revision), &|| false)
        .unwrap();
    assert_eq!(
        record.take(),
        set([root]),
        "confirmed once, then only the root"
    );
}

#[test]
fn a_retry_after_a_partial_directory_sync_syncs_what_is_still_unconfirmed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("new/deep");
    let record = SyncRecord::default();
    record.fail(&dir.path().join("new"));
    let store = FileSaveStore::new(root.clone()).with_directory_sync(record.seam());
    let b = bundle(&store, "Partial");
    let Err(SaveError::DurabilityUnconfirmed { revision, .. }) =
        store.write(&b, &SaveTarget::Create, &|| false)
    else {
        panic!("the sync of new/ was injected to fail")
    };
    record.take();
    record.recover();
    store
        .write(&b, &SaveTarget::ConfirmDurability(revision), &|| false)
        .unwrap();
    assert!(
        record.take().contains(&dir.path().join("new")),
        "the failed directory is synced on Retry"
    );
}

#[test]
fn a_save_after_one_that_failed_before_publication_syncs_the_directories_it_created() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("new/deep");
    let record = SyncRecord::default();
    let failing = FileSaveStore::new(root.clone())
        .with_directory_sync(record.seam())
        .with_fault(Arc::new(|at| {
            if at == PublishStage::PartialWrite {
                Err(SaveError::Cancelled)
            } else {
                Ok(())
            }
        }));
    let b = bundle(&failing, "Second try");
    assert!(failing.write(&b, &SaveTarget::Create, &|| false).is_err());
    assert!(record.take().is_empty(), "nothing was published");
    let store = failing.with_fault(Arc::new(|_| Ok(())));
    store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    assert_eq!(
        record.take(),
        set([root, dir.path().join("new"), dir.path().into()])
    );
}

#[test]
fn a_sync_failure_on_the_root_is_still_durability_unconfirmed() {
    let dir = TempDir::new().unwrap();
    let record = SyncRecord::default();
    record.fail(dir.path());
    let store = FileSaveStore::new(dir.path().into()).with_directory_sync(record.seam());
    let b = bundle(&store, "Visible");
    let result = store.write(&b, &SaveTarget::Create, &|| false);
    let Err(SaveError::DurabilityUnconfirmed { slot, revision }) = result else {
        panic!("{result:?}")
    };
    assert_eq!(slot, b.metadata.slot);
    assert_eq!(
        store.read(&b.metadata.slot).unwrap().0,
        b,
        "already visible"
    );
    assert!(matches!(
        store.write(&b, &SaveTarget::ConfirmDurability(revision), &|| false),
        Err(SaveError::DurabilityUnconfirmed { .. })
    ));
    record.recover();
    store
        .write(&b, &SaveTarget::ConfirmDurability(revision), &|| false)
        .unwrap();
}
