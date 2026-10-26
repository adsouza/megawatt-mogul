use application::*;
use domain::{ids::RegionId, session::GameSettings};
use infrastructure::{region::RonRegionCatalog, save::FileSaveStore};
use std::{path::Path, sync::Arc};
use tempfile::TempDir;
fn bundle(store: &FileSaveStore) -> SaveBundle {
    let catalog =
        RonRegionCatalog::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    let mut game = Game::start_new(
        &catalog,
        &RegionId::new("western_interconnection"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    for _ in 0..2 {
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        game.execute(Command::FinishReview).unwrap();
    }
    SaveBundle {
        compatibility: store.compatibility(),
        metadata: SaveMetadata {
            slot: store.allocate_id(),
            run: store.allocate_id(),
            kind: SaveKind::Manual,
            name: "Binary charts".into(),
            saved_at: 42,
        },
        checkpoint: game.checkpoint(),
    }
}
#[test]
fn directory_snapshot_has_binary_files_per_month_and_restores_exactly() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let slot = dir
        .path()
        .join(format!("{}.mgsave", saved.metadata.slot.as_str()));
    assert!(slot.is_dir());
    let names = std::fs::read_dir(&slot)
        .unwrap()
        .map(|p| p.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names.iter().filter(|s| s.ends_with(".bin.z")).count(), 2);
    assert!(slot.join("snapshot.ron.z").is_file());
    let (decoded, region) = store.read(&saved.metadata.slot).unwrap();
    assert_eq!(decoded, saved);
    let game = decoded
        .restore(
            region,
            &store.compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    assert_eq!(game.checkpoint(), saved.checkpoint);
    assert_eq!(
        store.list().unwrap()[0].entry.as_ref().unwrap().summary,
        saved.checkpoint.summary
    );
}

fn slot(dir: &TempDir, id: &SaveId) -> std::path::PathBuf {
    dir.path().join(format!("{}.mgsave", id.as_str()))
}
fn contents(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(p).unwrap(),
            )
        })
        .collect()
}
fn advanced(saved: &SaveBundle, store: &FileSaveStore) -> SaveBundle {
    let (_, region) = store.read(&saved.metadata.slot).unwrap();
    let mut game = saved
        .clone()
        .restore(
            region,
            &store.compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    game.execute(Command::BeginOperating).unwrap();
    game.execute(Command::AdvanceTicks { n: 30 }).unwrap();
    let mut next = saved.clone();
    next.checkpoint = game.checkpoint();
    next
}
#[test]
fn directory_overwrite_is_atomic_at_every_publication_fault_and_retry_confirms_durability() {
    use infrastructure::save::PublishStage;
    for stage in PublishStage::ALL {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
        let saved = bundle(&store);
        let old = store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
        let before = contents(&slot(&dir, &saved.metadata.slot));
        let next = advanced(&saved, &store);
        let faulty = FileSaveStore::new(dir.path().into())
            .with_monthly_charts()
            .with_fault(Arc::new(move |s| {
                if s == stage {
                    Err(SaveError::Io {
                        stage: "test",
                        message: "interrupted".into(),
                    })
                } else {
                    Ok(())
                }
            }));
        let error = faulty
            .write(&next, &SaveTarget::Replace(old.revision), &|| false)
            .unwrap_err();
        if stage == PublishStage::SyncDirectory {
            let SaveError::DurabilityUnconfirmed { revision, .. } = error else {
                panic!("{error:?}")
            };
            assert_eq!(store.read(&saved.metadata.slot).unwrap().0, next);
            store
                .write(&next, &SaveTarget::ConfirmDurability(revision), &|| false)
                .unwrap();
        } else {
            assert_eq!(
                contents(&slot(&dir, &saved.metadata.slot)),
                before,
                "{stage:?}"
            );
            assert_eq!(store.read(&saved.metadata.slot).unwrap().0, saved);
        }
    }
}
#[test]
fn new_directory_is_not_visible_before_a_complete_publish_and_cancel_never_exposes_one() {
    use infrastructure::save::PublishStage;
    for stage in PublishStage::ALL {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
        let saved = bundle(&store);
        let faulty = FileSaveStore::new(dir.path().into())
            .with_monthly_charts()
            .with_fault(Arc::new(move |s| {
                if s == stage {
                    Err(SaveError::Io {
                        stage: "test",
                        message: "interrupted".into(),
                    })
                } else {
                    Ok(())
                }
            }));
        assert!(
            faulty
                .write(&saved, &SaveTarget::Create, &|| false)
                .is_err()
        );
        if stage == PublishStage::SyncDirectory {
            assert_eq!(store.read(&saved.metadata.slot).unwrap().0, saved);
        } else {
            assert!(!slot(&dir, &saved.metadata.slot).exists());
            assert!(store.list().unwrap().is_empty());
        }
    }
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    assert_eq!(
        store.write(&saved, &SaveTarget::Create, &|| true),
        Err(SaveError::Cancelled)
    );
    assert!(!slot(&dir, &saved.metadata.slot).exists());
}
#[test]
fn monthly_files_are_reused_and_obsolete_versions_are_removed_after_success() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    let receipt = store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let path = slot(&dir, &saved.metadata.slot);
    let first = std::fs::read_dir(&path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("2027-01-")
        })
        .unwrap();
    let modified = std::fs::metadata(&first).unwrap().modified().unwrap();
    let bytes = std::fs::read(&first).unwrap();
    let next = advanced(&saved, &store);
    store
        .write(&next, &SaveTarget::Replace(receipt.revision), &|| false)
        .unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(&first).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        contents(&path)
            .keys()
            .filter(|n| n.ends_with(".bin.z"))
            .count(),
        3
    );
    assert_eq!(store.read(&saved.metadata.slot).unwrap().0, next);
    assert!(matches!(
        store.write(&saved, &SaveTarget::Replace(receipt.revision), &|| false),
        Err(SaveError::SlotChanged)
    ));
}
#[test]
fn directory_rename_and_delete_preserve_unselected_history_and_reject_stale_revisions() {
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    let receipt = store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let path = slot(&dir, &saved.metadata.slot);
    let before = contents(&path);
    let renamed = store
        .rename(&saved.metadata.slot, receipt.revision, "New name", &|| {
            false
        })
        .unwrap();
    let after = contents(&path);
    for (name, bytes) in &before {
        if name.ends_with(".bin.z") {
            assert_eq!(after[name], *bytes);
        }
    }
    let decoded = store.read(&saved.metadata.slot).unwrap().0;
    assert_eq!(decoded.checkpoint, saved.checkpoint);
    assert_eq!(decoded.metadata.saved_at, 42);
    assert!(matches!(
        store.delete(&saved.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::SlotChanged)
    ));
    store
        .delete(&saved.metadata.slot, renamed.revision, &|| false)
        .unwrap();
    assert!(!path.exists());
    assert!(store.list().unwrap().is_empty());
    store
        .delete(&saved.metadata.slot, renamed.revision, &|| false)
        .unwrap();
}
#[test]
fn cached_directory_cards_survive_restart_and_detect_missing_changed_or_deleted_parts() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    let receipt = store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let rows = store.list().unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let restarted = FileSaveStore::new(dir.path().into()).with_listing_reader(Arc::new(move |p| {
        count.fetch_add(1, Ordering::Relaxed);
        Ok(std::fs::read(p).unwrap())
    }));
    assert_eq!(restarted.list().unwrap(), rows);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    let path = slot(&dir, &saved.metadata.slot);
    let chart = std::fs::read_dir(&path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with(".bin.z"))
        .unwrap();
    let original = std::fs::read(&chart).unwrap();
    std::fs::remove_file(&chart).unwrap();
    assert!(restarted.list().unwrap()[0].entry.is_err());
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert!(restarted.read(&saved.metadata.slot).is_err());
    std::fs::write(&chart, &original).unwrap();
    assert!(restarted.list().unwrap()[0].entry.is_ok());
    let mut corrupt = original.clone();
    corrupt[10] ^= 1;
    std::fs::write(&chart, &corrupt).unwrap();
    assert!(restarted.read(&saved.metadata.slot).is_err());
    assert!(restarted.revision(&saved.metadata.slot).is_err());
    // Even corrupt chart data remains deletable by its manifest revision.
    store
        .delete(&saved.metadata.slot, receipt.revision, &|| false)
        .unwrap();
    assert!(restarted.list().unwrap().is_empty());
}

#[test]
fn directory_delete_retries_durability_without_deleting_a_recreated_slot() {
    use infrastructure::save::PublishStage;
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    let receipt = store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let faulty = FileSaveStore::new(dir.path().into())
        .with_monthly_charts()
        .with_fault(Arc::new(|s| {
            if s == PublishStage::SyncDirectory {
                Err(SaveError::Io {
                    stage: "test",
                    message: "sync".into(),
                })
            } else {
                Ok(())
            }
        }));
    assert!(matches!(
        faulty.delete(&saved.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::DeleteDurabilityUnconfirmed { .. })
    ));
    assert!(store.list().unwrap().is_empty());
    let mut changed = saved.clone();
    changed.metadata.name = "Replacement".into();
    let recreated = store
        .write(&changed, &SaveTarget::Create, &|| false)
        .unwrap();
    assert!(matches!(
        store.delete(&saved.metadata.slot, receipt.revision, &|| false),
        Err(SaveError::SlotChanged)
    ));
    assert_eq!(store.read(&saved.metadata.slot).unwrap().0, changed);
    store
        .delete(&saved.metadata.slot, recreated.revision, &|| false)
        .unwrap();
    store
        .delete(&saved.metadata.slot, receipt.revision, &|| false)
        .unwrap();
    assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".deleted-")
    }));
}

#[test]
fn chart_references_cannot_escape_the_snapshot_or_alias_another_month() {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Envelope {
        version: u32,
        length: usize,
        checksum: [u8; 32],
        payload: String,
    }
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let path = slot(&dir, &saved.metadata.slot);
    let file = path.join("snapshot.ron.z");
    let original = std::fs::read(&file).unwrap();
    let mut text = String::new();
    flate2::read::ZlibDecoder::new(original.as_slice())
        .read_to_string(&mut text)
        .unwrap();
    for replacement in ["../outside.bin.z", "/outside.bin.z", "2027-01-0000.bin.z"] {
        let mut e: Envelope = ron::from_str(&text).unwrap();
        let first = std::fs::read_dir(&path)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .find(|s| s.ends_with(".bin.z"))
            .unwrap();
        e.payload = e.payload.replace(&first, replacement);
        e.length = e.payload.len();
        e.checksum = Sha256::digest(e.payload.as_bytes()).into();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(ron::to_string(&e).unwrap().as_bytes()).unwrap();
        std::fs::write(&file, z.finish().unwrap()).unwrap();
        assert!(store.read(&saved.metadata.slot).is_err());
    }
    std::fs::write(file, original).unwrap();
    assert_eq!(store.read(&saved.metadata.slot).unwrap().0, saved);
}

#[cfg(unix)]
#[test]
fn selected_directory_reads_refuse_symlinked_snapshots_and_chart_files() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().unwrap();
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let saved = bundle(&store);
    store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let path = slot(&dir, &saved.metadata.slot);
    let chart = std::fs::read_dir(&path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with(".bin.z"))
        .unwrap();
    let outside = dir.path().join("outside");
    std::fs::rename(&chart, &outside).unwrap();
    symlink(&outside, &chart).unwrap();
    assert!(store.read(&saved.metadata.slot).is_err());
    std::fs::remove_file(&chart).unwrap();
    std::fs::rename(&outside, &chart).unwrap();
    let other = dir.path().join("real");
    std::fs::rename(&path, &other).unwrap();
    symlink(&other, &path).unwrap();
    assert!(store.read(&saved.metadata.slot).is_err());
    assert!(other.exists());
}

#[test]
fn frozen_format4_charged_battery_restores_and_continues_like_the_original_format2_fixture() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support/monthly-directory-v4/cccccccccccccccccccccccccccccccc.mgsave");
    let frozen = contents(&source);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(source.file_name().unwrap());
    std::fs::create_dir(&path).unwrap();
    for (name, bytes) in &frozen {
        std::fs::write(path.join(name), bytes).unwrap();
    }
    let store = FileSaveStore::new(dir.path().into()).with_monthly_charts();
    let id = SaveId::new("c".repeat(32)).unwrap();
    let (new, region) = store.read(&id).unwrap();
    let (old, old_region) = infrastructure::save::decode_save(
        include_bytes!("support/portable-saves/schema1-charged-battery.mgsave"),
        &store.compatibility(),
    )
    .unwrap();
    assert_eq!(
        new, old,
        "a retained binary fixture must keep the historical canonical bytes"
    );
    let mut resumed = new
        .clone()
        .restore(
            region,
            &store.compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    let mut baseline = old
        .restore(
            old_region,
            &store.compatibility(),
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    let battery = resumed
        .session()
        .plant_views()
        .iter()
        .find(|p| p.battery.is_some())
        .unwrap()
        .id;
    assert!(
        resumed
            .session()
            .plant_views()
            .iter()
            .any(|p| p.battery.as_ref().is_some_and(|b| b.stored.value() > 0.0))
    );
    for command in [
        Command::AdvanceTicks { n: 3 },
        Command::SetBatteryEnabled {
            plant: battery,
            on: false,
        },
        Command::AdvanceTicks { n: 2 },
    ] {
        assert_eq!(resumed.execute(command.clone()), baseline.execute(command));
        assert_eq!(resumed.checkpoint(), baseline.checkpoint());
        assert_eq!(resumed.projections(), baseline.projections());
    }
    let mut saved = new;
    saved.compatibility = store.compatibility();
    saved.checkpoint = resumed.checkpoint();
    let revision = store.revision(&id).unwrap();
    store
        .write(&saved, &SaveTarget::Replace(revision), &|| false)
        .unwrap();
    assert_eq!(store.read(&id).unwrap().0, saved);
    assert_eq!(
        contents(&source),
        frozen,
        "released compatibility fixtures are never rewritten"
    );
}

#[test]
fn wind_and_overlapping_battery_policy_restore_through_flat_and_monthly_snapshots() {
    let dir = TempDir::new().unwrap();
    let source = include_str!("support/minimal_region.ron").replacen(
        "fleet: [",
        "fleet: [(Wind, 4.5, 6.5), (Battery, 4.5, 5.5),",
        1,
    );
    let regions = dir.path().join("regions");
    std::fs::create_dir(&regions).unwrap();
    std::fs::write(regions.join("minimal.ron"), source).unwrap();
    let catalog = RonRegionCatalog::new(regions);
    let mut game = Game::start_new(
        &catalog,
        &RegionId::new("minimal"),
        41,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    game.execute(Command::BeginOperating).unwrap();
    let store = FileSaveStore::new(dir.path().join("saves")).with_monthly_charts();
    let run = store.allocate_id();
    for boundary in [0, 300, 1020, 1079, 1080, 1440] {
        let remaining = boundary - game.session().clock().tick();
        if remaining > 0 {
            game.execute(Command::AdvanceTicks { n: remaining })
                .unwrap();
        }
        let saved = SaveBundle {
            compatibility: store.compatibility(),
            metadata: SaveMetadata {
                slot: store.allocate_id(),
                run: run.clone(),
                kind: SaveKind::Manual,
                name: format!("Wind boundary {boundary}"),
                saved_at: 42,
            },
            checkpoint: game.checkpoint(),
        };
        assert_eq!(saved.checkpoint.snapshot.as_ref().unwrap().schema, 3);
        store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
        let (decoded, region) = store.read(&saved.metadata.slot).unwrap();
        let mut resumed = decoded
            .restore(
                region,
                &store.compatibility(),
                Box::new(InMemoryCommandLog::default()),
                &|| false,
            )
            .unwrap();
        assert!(
            resumed.checkpoint() == game.checkpoint(),
            "monthly snapshot at {boundary}"
        );
        let flat = infrastructure::save::encode_save(&saved).unwrap();
        let (flat, region) =
            infrastructure::save::decode_save(&flat, &store.compatibility()).unwrap();
        let flat = flat
            .restore(
                region,
                &store.compatibility(),
                Box::new(InMemoryCommandLog::default()),
                &|| false,
            )
            .unwrap();
        assert!(
            flat.checkpoint() == game.checkpoint(),
            "flat snapshot at {boundary}"
        );
        if boundary < 1440 {
            assert_eq!(
                game.execute(Command::AdvanceTicks { n: 1 }).unwrap(),
                resumed.execute(Command::AdvanceTicks { n: 1 }).unwrap()
            );
            assert!(resumed.checkpoint() == game.checkpoint());
        }
    }
}
