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
    assert!(matches!(
        store.rename(
            &b.metadata.slot,
            receipt.revision,
            "Rename while writing",
            &|| false
        ),
        Err(SaveError::SlotBusy)
    ));
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
    std::fs::copy(&shipped, assets.join("western_interconnection.ron")).unwrap();
    // Its sidecars (spec section 28.11.1) come along.
    std::fs::create_dir(assets.join("western_interconnection")).unwrap();
    for entry in std::fs::read_dir(shipped.with_extension("")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(
            &path,
            assets
                .join("western_interconnection")
                .join(path.file_name().unwrap()),
        )
        .unwrap();
    }
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

#[test]
fn save_store_rename_preserves_the_game_stamp_date_and_slot_and_rejects_stale_writers() {
    let dir = TempDir::new().unwrap();
    let older = store_built_as(dir.path(), "older", "other-target");
    for kind in [SaveKind::Manual, SaveKind::Automatic] {
        let mut b = bundle(&older, "Before");
        b.metadata.kind = kind;
        let original = older.write(&b, &SaveTarget::Create, &|| false).unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let renamed = store
            .rename(&b.metadata.slot, original.revision, "After", &|| false)
            .unwrap();
        b.metadata.name = "After".into();
        assert_eq!(store.read(&b.metadata.slot).unwrap().0, b);
        assert_eq!(renamed.metadata, b.metadata);
        assert_ne!(renamed.revision, original.revision);
        assert_eq!(
            store
                .list()
                .unwrap()
                .iter()
                .find(|row| row.id.as_ref() == Some(&b.metadata.slot))
                .unwrap()
                .entry
                .as_ref()
                .unwrap()
                .metadata
                .name,
            "After"
        );
        assert!(matches!(
            store.rename(&b.metadata.slot, original.revision, "Stale", &|| false),
            Err(SaveError::SlotChanged)
        ));
        assert!(matches!(
            store.rename(&b.metadata.slot, renamed.revision, "", &|| false),
            Err(SaveError::InvalidSaveName)
        ));
        assert!(matches!(
            store.rename(&b.metadata.slot, renamed.revision, "Cancelled", &|| true),
            Err(SaveError::Cancelled)
        ));
        assert_eq!(store.read(&b.metadata.slot).unwrap().0, b);
    }
}

#[test]
fn save_store_rename_publication_failures_preserve_the_old_save_and_retry_durability() {
    for stage in PublishStage::ALL {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let b = bundle(&store, "Before");
        let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
        let path = dir
            .path()
            .join(format!("{}.mgsave", b.metadata.slot.as_str()));
        let original = std::fs::read(&path).unwrap();
        let failing = FileSaveStore::new(dir.path().into()).with_fault(Arc::new(move |at| {
            if at == stage {
                Err(SaveError::Io {
                    stage: "rename test",
                    message: "fault".into(),
                })
            } else {
                Ok(())
            }
        }));
        let result = failing.rename(&b.metadata.slot, receipt.revision, "After", &|| false);
        if stage == PublishStage::SyncDirectory {
            let Err(SaveError::DurabilityUnconfirmed { revision, .. }) = result else {
                panic!("{result:?}")
            };
            store
                .rename(&b.metadata.slot, revision, "After", &|| false)
                .unwrap();
            assert_eq!(
                store.read(&b.metadata.slot).unwrap().0.metadata.name,
                "After"
            );
        } else {
            assert!(result.is_err(), "{stage:?}");
            assert_eq!(std::fs::read(path).unwrap(), original, "{stage:?}");
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn save_store_rename_changes_only_the_name_across_unicode_names(
        names in proptest::collection::vec("[a-zA-Z0-9é💡]{1,32}", 1..5)
    ) {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let mut expected = bundle(&store, "Initial");
        let mut receipt = store.write(&expected, &SaveTarget::Create, &|| false).unwrap();
        for name in names {
            receipt = store.rename(&expected.metadata.slot, receipt.revision, &name, &|| false).unwrap();
            expected.metadata.name = name;
            proptest::prop_assert_eq!(store.read(&expected.metadata.slot).unwrap().0, expected.clone());
        }
    }
}

#[test]
fn save_store_rename_leaves_corrupt_files_and_invalid_names_untouched() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let b = bundle(&store, "Original");
    let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
    let path = dir
        .path()
        .join(format!("{}.mgsave", b.metadata.slot.as_str()));
    let bytes = std::fs::read(&path).unwrap();
    for invalid in [" ".into(), "Bad\nname".into(), "x".repeat(81)] {
        assert!(matches!(
            store.rename(&b.metadata.slot, receipt.revision, &invalid, &|| false),
            Err(SaveError::InvalidSaveName)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    std::fs::write(&path, b"corrupt").unwrap();
    // Even if the caller has the exact current revision, corrupted contents cannot be renamed.
    use sha2::{Digest, Sha256};
    let revision = SaveRevision(Sha256::digest(b"corrupt").into());
    assert!(
        store
            .rename(&b.metadata.slot, revision, "New name", &|| false)
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
}

#[test]
fn save_delete_removes_only_the_confirmed_revision_and_can_remove_corrupt_snapshots() {
    let root = TempDir::new().unwrap();
    let store = FileSaveStore::new(root.path().into());
    let chosen = bundle(&store, "Chosen");
    let keep = bundle(&store, "Keep");
    let receipt = store
        .write(&chosen, &SaveTarget::Create, &|| false)
        .unwrap();
    store.write(&keep, &SaveTarget::Create, &|| false).unwrap();
    let keep_bytes = std::fs::read(
        root.path()
            .join(format!("{}.mgsave", keep.metadata.slot.as_str())),
    )
    .unwrap();
    assert_eq!(
        store.delete(&chosen.metadata.slot, receipt.revision, &|| true),
        Err(SaveError::Cancelled)
    );
    assert_eq!(store.list().unwrap().len(), 2);
    let renamed = store
        .rename(
            &chosen.metadata.slot,
            receipt.revision,
            "Other window",
            &|| false,
        )
        .unwrap();
    assert_eq!(
        store.delete(&chosen.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::SlotChanged)
    );
    store
        .delete(&chosen.metadata.slot, renamed.revision, &|| false)
        .unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(
            root.path()
                .join(format!("{}.mgsave", keep.metadata.slot.as_str()))
        )
        .unwrap(),
        keep_bytes
    );
    let corrupt = SaveId::new("f".repeat(32)).unwrap();
    std::fs::write(
        root.path().join(format!("{}.mgsave", corrupt.as_str())),
        b"broken save",
    )
    .unwrap();
    let listing = store
        .list()
        .unwrap()
        .into_iter()
        .find(|entry| entry.id.as_ref() == Some(&corrupt))
        .unwrap();
    assert!(listing.entry.is_err());
    store
        .delete(&corrupt, listing.revision.unwrap(), &|| false)
        .unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn save_delete_uses_the_slot_lock_and_retries_uncertain_directory_durability() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = TempDir::new().unwrap();
    let store = FileSaveStore::new(root.path().into());
    let chosen = bundle(&store, "Chosen");
    let receipt = store
        .write(&chosen, &SaveTarget::Create, &|| false)
        .unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            root.path()
                .join(format!("{}.lock", chosen.metadata.slot.as_str())),
        )
        .unwrap();
    lock.try_lock().unwrap();
    assert_eq!(
        store.delete(&chosen.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::SlotBusy)
    );
    drop(lock);
    let failing = Arc::new(AtomicBool::new(true));
    let fail = failing.clone();
    let store = store.with_directory_sync(Arc::new(move |_| {
        if fail.load(Ordering::Relaxed) {
            Err(std::io::Error::other("directory unavailable"))
        } else {
            Ok(())
        }
    }));
    assert_eq!(
        store.delete(&chosen.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::DeleteDurabilityUnconfirmed {
            slot: chosen.metadata.slot.clone()
        })
    );
    assert!(store.list().unwrap().is_empty());
    failing.store(false, Ordering::Relaxed);
    store
        .delete(&chosen.metadata.slot, receipt.revision, &|| false)
        .unwrap();
    assert!(store.list().unwrap().is_empty());
    let mut replacement = chosen;
    replacement.metadata.name = "Recreated".into();
    store
        .write(&replacement, &SaveTarget::Create, &|| false)
        .unwrap();
    assert_eq!(
        store.delete(&replacement.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::SlotChanged)
    );
    assert_eq!(
        store
            .read(&replacement.metadata.slot)
            .unwrap()
            .0
            .metadata
            .name,
        "Recreated"
    );
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn save_delete_preserves_every_unselected_snapshot(
        names in proptest::collection::vec("[a-z]{1,12}", 1..6),
        chosen in proptest::num::usize::ANY,
    ) {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let mut b = bundle(&store, "Template");
        let mut originals = Vec::new();
        for name in names {
            b.metadata.slot = store.allocate_id();
            b.metadata.name = name;
            let receipt = store.write(&b, &SaveTarget::Create, &|| false).unwrap();
            let path = dir.path().join(format!("{}.mgsave", b.metadata.slot.as_str()));
            originals.push((receipt, path.clone(), std::fs::read(path).unwrap()));
        }
        let selected = chosen % originals.len();
        let receipt = &originals[selected].0;
        store.delete(&receipt.metadata.slot, receipt.revision, &|| false).unwrap();
        for (index, (_, path, bytes)) in originals.iter().enumerate() {
            if index == selected {
                proptest::prop_assert!(!path.exists());
            } else {
                proptest::prop_assert_eq!(std::fs::read(path).unwrap(), bytes.clone());
            }
        }
        proptest::prop_assert_eq!(store.list().unwrap().len(), originals.len() - 1);
    }
}

#[test]
fn save_listing_reads_only_card_metadata_and_selected_loading_still_validates_everything() {
    use sha2::{Digest, Sha256};
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let mut saved = bundle(&store, "Metadata only");
    // Sufficient card metadata, deliberately insufficient region content for a playable game.
    saved.checkpoint.region_source =
        br#"(id:"eastern_interconnection",name_key:"region-archived-label")"#.to_vec();
    saved.checkpoint.header.region_fingerprint =
        RegionFingerprint::new(Sha256::digest(&saved.checkpoint.region_source).into());
    let bytes = infrastructure::save::encode_save(&saved).unwrap();
    let path = dir
        .path()
        .join(format!("{}.mgsave", saved.metadata.slot.as_str()));
    std::fs::write(&path, &bytes).unwrap();
    let rows = store.list().unwrap();
    let row = rows[0]
        .entry
        .as_ref()
        .expect("listing must not construct the playable region");
    assert_eq!(row.metadata, saved.metadata);
    assert_eq!(row.region_name_key, "region-archived-label");
    assert!(
        store.read(&saved.metadata.slot).is_err(),
        "selected loading must validate the full archived region"
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

fn counted_listing_store(root: &Path) -> (FileSaveStore, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    (
        FileSaveStore::new(root.into()).with_listing_reader(Arc::new(move |path| {
            counter.fetch_add(1, Ordering::Relaxed);
            std::fs::read(path).map_err(|e| SaveError::Io {
                stage: "test-read",
                message: e.to_string(),
            })
        })),
        reads,
    )
}
#[test]
fn listing_cache_skips_unchanged_files_and_refreshes_external_add_rename_replace_and_delete() {
    use std::sync::atomic::Ordering;
    let dir = TempDir::new().unwrap();
    let (store, reads) = counted_listing_store(dir.path());
    let first = bundle(&store, "First");
    let receipt = store.write(&first, &SaveTarget::Create, &|| false).unwrap();
    let cold = store.list().unwrap();
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert_eq!(store.list().unwrap(), cold);
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    let external = FileSaveStore::new(dir.path().into());
    let renamed = external
        .rename(
            &first.metadata.slot,
            receipt.revision,
            "Renamed elsewhere",
            &|| false,
        )
        .unwrap();
    assert_eq!(
        store.list().unwrap()[0]
            .entry
            .as_ref()
            .unwrap()
            .metadata
            .name,
        "Renamed elsewhere"
    );
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    let mut other = first.clone();
    other.metadata.slot = store.allocate_id();
    other.metadata.name = "Other".into();
    let other_receipt = external
        .write(&other, &SaveTarget::Create, &|| false)
        .unwrap();
    assert_eq!(store.list().unwrap().len(), 2);
    assert_eq!(reads.load(Ordering::Relaxed), 3);
    assert_eq!(store.list().unwrap().len(), 2);
    assert_eq!(reads.load(Ordering::Relaxed), 3);
    external
        .delete(&first.metadata.slot, renamed.revision, &|| false)
        .unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(reads.load(Ordering::Relaxed), 3);
    external
        .delete(&other.metadata.slot, other_receipt.revision, &|| false)
        .unwrap();
    assert!(store.list().unwrap().is_empty());
    // Recreating a removed path must not resurrect a stale entry.
    external
        .write(&first, &SaveTarget::Create, &|| false)
        .unwrap();
    assert_eq!(
        store.list().unwrap()[0]
            .entry
            .as_ref()
            .unwrap()
            .metadata
            .name,
        "First"
    );
    assert_eq!(reads.load(Ordering::Relaxed), 4);
}
#[cfg(unix)]
#[test]
fn cache_detects_same_size_edits_with_restored_mtime_and_selected_reads_always_validate() {
    use std::sync::atomic::Ordering;
    let dir = TempDir::new().unwrap();
    let (store, reads) = counted_listing_store(dir.path());
    let saved = bundle(&store, "Original");
    store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    assert!(store.list().unwrap()[0].entry.is_ok());
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    // Recreate the store so this also exercises persisted stamps, including Unix change time.
    let (store, reads) = counted_listing_store(dir.path());
    assert!(store.list().unwrap()[0].entry.is_ok());
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    let path = dir
        .path()
        .join(format!("{}.mgsave", saved.metadata.slot.as_str()));
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut corrupt = std::fs::read(&path).unwrap();
    let at = corrupt.len() / 2;
    corrupt[at] ^= 1;
    std::fs::write(&path, &corrupt).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(
        store.read(&saved.metadata.slot).is_err(),
        "selected loads never trust cached cards"
    );
    assert!(store.list().unwrap()[0].entry.is_err());
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert!(store.list().unwrap()[0].entry.is_err());
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[test]
fn listing_cache_does_not_remember_a_read_raced_by_atomic_replacement() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = TempDir::new().unwrap();
    let writer = FileSaveStore::new(dir.path().into());
    let first = bundle(&writer, "First");
    writer
        .write(&first, &SaveTarget::Create, &|| false)
        .unwrap();
    let mut next = first.clone();
    next.metadata.name = "Replacement".into();
    let replacement = infrastructure::save::encode_save(&next).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    let store = FileSaveStore::new(dir.path().into()).with_listing_reader(Arc::new(move |path| {
        let bytes = std::fs::read(path).unwrap();
        if counter.fetch_add(1, Ordering::Relaxed) == 0 {
            let temporary = path.with_extension("replacement");
            std::fs::write(&temporary, &replacement).unwrap();
            std::fs::rename(temporary, path).unwrap();
        }
        Ok(bytes)
    }));
    assert_eq!(
        store.list().unwrap()[0]
            .entry
            .as_ref()
            .unwrap()
            .metadata
            .name,
        "First"
    );
    assert_eq!(
        store.list().unwrap()[0]
            .entry
            .as_ref()
            .unwrap()
            .metadata
            .name,
        "Replacement"
    );
    assert_eq!(
        store.list().unwrap()[0]
            .entry
            .as_ref()
            .unwrap()
            .metadata
            .name,
        "Replacement"
    );
    assert_eq!(reads.load(Ordering::Relaxed), 2);
}

#[test]
fn transient_listing_io_errors_are_not_cached() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = TempDir::new().unwrap();
    let writer = FileSaveStore::new(dir.path().into());
    let saved = bundle(&writer, "Retry reading");
    writer
        .write(&saved, &SaveTarget::Create, &|| false)
        .unwrap();
    let fail = AtomicBool::new(true);
    let store = FileSaveStore::new(dir.path().into()).with_listing_reader(Arc::new(move |path| {
        if fail.swap(false, Ordering::Relaxed) {
            return Err(SaveError::Io {
                stage: "test",
                message: "temporary read failure".into(),
            });
        }
        Ok(std::fs::read(path).unwrap())
    }));
    assert!(store.list().unwrap()[0].entry.is_err());
    assert!(store.list().unwrap()[0].entry.is_ok());
}

#[test]
fn card_scan_skips_command_decoding_but_selected_load_rejects_invalid_commands() {
    use sha2::{Digest, Sha256};
    #[derive(serde::Deserialize, serde::Serialize)]
    struct Envelope {
        version: u32,
        length: usize,
        checksum: [u8; 32],
        payload: String,
    }
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into());
    let saved = bundle(&store, "Commands are validated on load");
    let mut envelope: Envelope =
        ron::de::from_bytes(&infrastructure::save::encode_save(&saved).unwrap()).unwrap();
    assert!(envelope.payload.contains("commands:[]"));
    envelope.payload = envelope.payload.replace(
        "commands:[]",
        "commands:[FutureCommand((name:\"nested decoy\",count:999))]",
    );
    envelope.length = envelope.payload.len();
    envelope.checksum = Sha256::digest(envelope.payload.as_bytes()).into();
    let bytes = ron::to_string(&envelope).unwrap();
    let path = dir
        .path()
        .join(format!("{}.mgsave", saved.metadata.slot.as_str()));
    std::fs::write(&path, &bytes).unwrap();
    let row = store.list().unwrap();
    assert_eq!(row[0].entry.as_ref().unwrap().metadata, saved.metadata);
    assert!(store.read(&saved.metadata.slot).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    #[test]
    fn cached_library_matches_a_fresh_scan_after_arbitrary_slot_changes(ops in proptest::collection::vec(0u8..4,1..12)) {
        let dir=TempDir::new().unwrap();let (cached,_)=counted_listing_store(dir.path());
        let writer=FileSaveStore::new(dir.path().into());let mut saved=bundle(&writer,"Initial");
        let mut receipt=Some(writer.write(&saved,&SaveTarget::Create,&||false).unwrap());
        cached.list().unwrap();
        for (index,op) in ops.into_iter().enumerate() {
            match op {
                0 if receipt.is_some() => {
                    let name=format!("Rename {index}");
                    receipt=Some(writer.rename(&saved.metadata.slot,receipt.as_ref().unwrap().revision,&name,&||false).unwrap());saved.metadata.name=name;
                }
                1 => {
                    saved.metadata.name=format!("Write {index}");
                    let target=receipt.as_ref().map_or(SaveTarget::Create,|r|SaveTarget::Replace(r.revision));
                    receipt=Some(writer.write(&saved,&target,&||false).unwrap());
                }
                2 if receipt.is_some() => {writer.delete(&saved.metadata.slot,receipt.take().unwrap().revision,&||false).unwrap();}
                _ => {}
            }
            let restarted=FileSaveStore::new(dir.path().into()).list().unwrap();
            let _=std::fs::remove_file(dir.path().join(".save-library-cache.ron"));
            let fresh=FileSaveStore::new(dir.path().into()).list().unwrap();
            proptest::prop_assert_eq!(restarted,fresh.clone());
            proptest::prop_assert_eq!(cached.list().unwrap(),fresh.clone());
            proptest::prop_assert_eq!(cached.list().unwrap(),fresh);
        }
    }
}

#[test]
fn disk_listing_cache_survives_restart_and_revalidates_changed_and_deleted_sources() {
    use std::sync::atomic::Ordering;
    let dir = TempDir::new().unwrap();
    let writer = FileSaveStore::new(dir.path().into());
    let first = bundle(&writer, "First");
    let mut second = first.clone();
    second.metadata.slot = writer.allocate_id();
    second.metadata.name = "Second".into();
    let first_receipt = writer
        .write(&first, &SaveTarget::Create, &|| false)
        .unwrap();
    let second_receipt = writer
        .write(&second, &SaveTarget::Create, &|| false)
        .unwrap();
    let (cold, reads) = counted_listing_store(dir.path());
    let expected = cold.list().unwrap();
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    drop(cold);
    let (restarted, reads) = counted_listing_store(dir.path());
    assert_eq!(restarted.list().unwrap(), expected);
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "restart must reuse disk metadata"
    );
    drop(restarted);
    writer
        .rename(
            &first.metadata.slot,
            first_receipt.revision,
            "Changed",
            &|| false,
        )
        .unwrap();
    writer
        .delete(&second.metadata.slot, second_receipt.revision, &|| false)
        .unwrap();
    let (restarted, reads) = counted_listing_store(dir.path());
    let rows = restarted.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entry.as_ref().unwrap().metadata.name, "Changed");
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    let (restarted, reads) = counted_listing_store(dir.path());
    assert_eq!(restarted.list().unwrap(), rows);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    assert_eq!(
        writer.read(&first.metadata.slot).unwrap().0.checkpoint,
        first.checkpoint
    );
}

#[test]
fn missing_corrupt_incompatible_or_unwritable_disk_cache_never_blocks_a_save() {
    use std::sync::atomic::Ordering;
    for damage in [
        "missing",
        "truncated",
        "checksum",
        "version",
        "oversize",
        "directory",
        "other-build",
    ] {
        let dir = TempDir::new().unwrap();
        let writer = FileSaveStore::new(dir.path().into());
        let saved = bundle(&writer, "Preserved");
        writer
            .write(&saved, &SaveTarget::Create, &|| false)
            .unwrap();
        let path = dir
            .path()
            .join(format!("{}.mgsave", saved.metadata.slot.as_str()));
        let original = std::fs::read(&path).unwrap();
        let expected = writer.list().unwrap();
        let index = dir.path().join(".save-library-cache.ron");
        match damage {
            "missing" => std::fs::remove_file(&index).unwrap(),
            "truncated" => std::fs::write(&index, b"(version:1,").unwrap(),
            "checksum" => {
                let bytes = std::fs::read_to_string(&index)
                    .unwrap()
                    .replace("Preserved", "Corrupted");
                std::fs::write(&index, bytes).unwrap();
            }
            "version" => {
                let bytes = std::fs::read_to_string(&index).unwrap().replacen(
                    "version:1",
                    "version:999",
                    1,
                );
                std::fs::write(&index, bytes).unwrap();
            }
            "oversize" => {
                std::fs::File::create(&index)
                    .unwrap()
                    .set_len(8 * 1024 * 1024 + 1)
                    .unwrap();
            }
            "directory" => {
                std::fs::remove_file(&index).unwrap();
                std::fs::create_dir(&index).unwrap();
            }
            "other-build" => {
                let other = FileSaveStore::with_sources(
                    dir.path().into(),
                    ReplayCompatibility {
                        build: "different build".into(),
                        platform: writer.compatibility().platform,
                    },
                    Arc::new(|| 0),
                    Arc::new(|| SaveId::new(format!("{:032x}", 99)).unwrap()),
                );
                other.list().unwrap();
            }
            _ => unreachable!(),
        }
        let (store, reads) = counted_listing_store(dir.path());
        assert_eq!(store.list().unwrap(), expected, "{damage}");
        assert_eq!(reads.load(Ordering::Relaxed), 1, "{damage}");
        assert_eq!(
            store.read(&saved.metadata.slot).unwrap().0.checkpoint,
            saved.checkpoint
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[test]
fn unavailable_cards_are_cached_across_restart_but_transient_read_errors_are_retried() {
    use std::sync::atomic::Ordering;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(format!(
        "{}.mgsave",
        SaveId::new(format!("{:032x}", 98)).unwrap().as_str()
    ));
    std::fs::write(&path, b"damaged save").unwrap();
    let (store, reads) = counted_listing_store(dir.path());
    let expected = store.list().unwrap();
    assert!(expected[0].entry.is_err());
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    let (restarted, reads) = counted_listing_store(dir.path());
    assert_eq!(restarted.list().unwrap(), expected);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    // An external addition still needs a read. A transient failure must not enter the disk index.
    let writer = FileSaveStore::new(dir.path().into());
    let saved = bundle(&writer, "Retry after restart");
    writer
        .write(&saved, &SaveTarget::Create, &|| false)
        .unwrap();
    let failed = FileSaveStore::new(dir.path().into()).with_listing_reader(Arc::new(|_| {
        Err(SaveError::Io {
            stage: "test",
            message: "temporary".into(),
        })
    }));
    assert!(
        failed
            .list()
            .unwrap()
            .iter()
            .any(|r| matches!(r.entry, Err(SaveError::Io { .. })))
    );
    let (restarted, reads) = counted_listing_store(dir.path());
    assert!(restarted.list().unwrap().iter().any(|r| r.entry.is_ok()));
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}
