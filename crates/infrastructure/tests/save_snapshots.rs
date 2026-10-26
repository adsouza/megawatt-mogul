//! Frozen released snapshots remain compatible without historical simulation engines.
use application::*;
use domain::session::DayPhase;
use domain::state::State;
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

/// Keep historical hash receipts in their recorded schema while new captures evolve.
fn hash_in_schema(game: &Game, schema: u32) -> u64 {
    let mut snapshot = game.checkpoint().snapshot.unwrap();
    snapshot.schema = schema;
    snapshot.session = game.session().snapshot_state_schema(schema);
    snapshot.projections = game.projections().state_bytes_schema(schema);
    snapshot.state_hash()
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
        let source = &bundle.checkpoint;
        let snapshot = source.snapshot.as_ref().unwrap();
        assert_eq!(
            before.snapshot.as_ref().unwrap().schema,
            domain::state::CURRENT_STATE_SCHEMA
        );
        assert!(
            game.session().snapshot_state_schema(1) == snapshot.session,
            "all schema-1 gameplay fields and RNG preserved"
        );
        assert!(
            game.projections().state_bytes_schema(1) == snapshot.projections,
            "history and reports preserved"
        );
        assert_eq!(before.summary, source.summary);
        assert_eq!(before.commands, source.commands);
        assert_eq!(before.region_source, source.region_source);
        assert_eq!(before.snapshot.as_ref().unwrap().origin, snapshot.origin);
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

fn golden_bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/portable-saves")
            .join(name),
    )
    .unwrap()
}

fn close_current_month(game: &mut Game) -> Vec<domain::events::DomainEvent> {
    if game.session().phase() == DayPhase::Review {
        game.execute(Command::FinishReview).unwrap();
    }
    if game.session().phase() == DayPhase::Planning {
        game.execute(Command::BeginOperating).unwrap();
    }
    game.execute(Command::AdvanceTicks {
        n: domain::time::TICKS_PER_DAY,
    })
    .unwrap()
}

#[test]
fn an_old_battery_is_accredited_without_rewriting_the_save() {
    use domain::company::{AssetProfile, AssetRef};
    use domain::region::PlantKind;
    let bytes = golden_bytes("schema1-charged-battery.mgsave");
    let (bundle, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
    let source = bundle.checkpoint;
    let mut game = restored(&bytes);
    assert!(
        game.session().snapshot_state_schema(1) == source.snapshot.as_ref().unwrap().session,
        "schema migration preserves original battery profiles and all other old fields"
    );
    let expected = game.checkpoint();
    let batteries: Vec<_> = game
        .session()
        .plant_views()
        .into_iter()
        .filter(|p| p.kind == PlantKind::Battery)
        .map(|p| p.id)
        .collect();
    assert!(!batteries.is_empty());
    let firm = |game: &Game, id| match game
        .session()
        .company()
        .rate_base()
        .asset(AssetRef::Plant(id))
        .unwrap()
        .profile
    {
        AssetProfile::Plant { firm, .. } => firm.value(),
        _ => panic!("plant profile"),
    };
    for &id in &batteries {
        assert_eq!(firm(&game, id), 0.0);
    }
    let estimate = game.session().rate_review_estimate();
    let session = game.session();
    let accreditation = &session.region().regulator().storage_accreditation;
    let total = session.power().firm_capacity(accreditation).value();
    // Independently add the shipped 170 MW for each commissioned battery to the generator credits.
    let expected_total: f64 = session
        .plant_views()
        .iter()
        .filter(|p| {
            !matches!(
                p.status,
                domain::generation::PlantStatus::UnderConstruction { .. }
            )
        })
        .map(|p| {
            if p.kind == PlantKind::Battery {
                170.0
            } else {
                let spec = session.region().spec(p.kind);
                spec.capacity_credit.of(spec.capacity).value()
            }
        })
        .sum();
    assert!((total - expected_total).abs() < 1e-8);
    assert!(
        (estimate.reserve_margin
            - (total
                / session
                    .regulator()
                    .tally()
                    .peak_demand
                    .max(
                        session.power().projected_connected_peak(
                            session.region(),
                            session.clock().month().next()
                        )
                    )
                    .value()
                - 1.0))
            .abs()
            < 1e-12
    );
    assert_eq!(
        game.checkpoint(),
        expected,
        "a preview must not refresh saved profiles"
    );
    loop {
        close_current_month(&mut game);
        if game.session().clock().month().month() == 12 {
            break;
        }
    }
    for &id in &batteries {
        assert_eq!(firm(&game, id), 170.0);
    }
}

#[test]
fn a_resumed_december_review_is_not_reviewed_again() {
    let bytes = golden_bytes("schema1-migrated-review.mgsave");
    let (mut bundle, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
    let mut game = restored(&bytes);
    loop {
        close_current_month(&mut game);
        if game.session().clock().month().month() == 12 {
            break;
        }
    }
    let december = game.projections().monthly_reports.latest().unwrap().clone();
    assert!(december.rate_review.is_some());
    bundle.checkpoint = game.checkpoint();
    bundle.compatibility = replay_compatibility();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("december.mgsave");
    std::fs::write(&path, encode_save(&bundle).unwrap()).unwrap();
    let mut resumed = restored(&std::fs::read(path).unwrap());
    assert_eq!(resumed.checkpoint(), game.checkpoint());
    let mut events = resumed.execute(Command::FinishReview).unwrap();
    events.extend(resumed.execute(Command::BeginOperating).unwrap());
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, domain::events::DomainEvent::RateReviewCompleted { .. }))
            .count(),
        0
    );
    assert_eq!(
        resumed
            .projections()
            .monthly_reports
            .latest()
            .unwrap()
            .rate_review,
        december.rate_review
    );
    assert_eq!(
        resumed.session().company().rate_base(),
        game.session().company().rate_base()
    );
}

#[test]
fn archived_na_saves_resume_identically() {
    use domain::ids::RegionId;
    use domain::region::SolarModel;
    use domain::units::Currency;
    use infrastructure::region::RonRegionCatalog;
    use infrastructure::save::FileSaveStore;
    use std::io::Read;
    for (id, after60, review) in [
        (
            "eastern_interconnection",
            9516849278041982929,
            17612825350226170891,
        ),
        (
            "western_interconnection",
            5632289864882056421,
            15184034769020035595,
        ),
    ] {
        let dir = TempDir::new().unwrap();
        let current = include_str!("support/minimal_region_v4.ron")
            .replace("id: \"minimal\"", &format!("id: \"{id}\""));
        std::fs::write(dir.path().join(format!("{id}.ron")), current).unwrap();
        let current_catalog = RonRegionCatalog::new(dir.path());
        let current_region = current_catalog.load(&RegionId::new(id)).unwrap();
        assert_eq!(current_region.definition.currency(), Currency::Eur);
        assert!(matches!(
            current_region.definition.solar().model,
            SolarModel::Astronomical
        ));
        let baseline =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/global-foundations");
        let compressed = std::fs::read(baseline.join(format!("{id}-schema3.mgsave.z"))).unwrap();
        let mut bytes = Vec::new();
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut bytes)
            .unwrap();
        let (original, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
        let store = FileSaveStore::new(dir.path().join("saves"));
        std::fs::create_dir(dir.path().join("saves")).unwrap();
        let save_path = dir
            .path()
            .join("saves")
            .join(format!("{}.mgsave", original.metadata.slot.as_str()));
        std::fs::write(&save_path, &bytes).unwrap();
        let (mut bundle, archived) = store.read(&original.metadata.slot).unwrap();
        let archived_bytes = std::fs::read(baseline.join(format!("{id}-v3.ron"))).unwrap();
        assert_eq!(archived.source_bytes, archived_bytes);
        assert_eq!(
            archived.fingerprint,
            original.checkpoint.header.region_fingerprint
        );
        assert_ne!(archived.fingerprint, current_region.fingerprint);
        assert_eq!(archived.definition.currency(), Currency::Usd);
        assert!(matches!(
            archived.definition.solar().model,
            SolarModel::DaylightTable(_)
        ));
        assert_eq!(archived.definition.system_zone().standard_offset_minutes, 0);
        assert_eq!(bundle.checkpoint.header.sim_rules_version, 15);
        let snapshot = bundle.checkpoint.snapshot.clone().unwrap();
        assert_eq!(snapshot.schema, 3);
        let mut game = bundle
            .clone()
            .restore(
                archived,
                &store.compatibility(),
                Box::new(InMemoryCommandLog::default()),
                &|| false,
            )
            .unwrap();
        assert_eq!(
            game.session().snapshot_state_schema(snapshot.schema),
            snapshot.session
        );
        assert_eq!(
            game.projections().state_bytes_schema(snapshot.schema),
            snapshot.projections
        );
        assert_eq!(
            game.checkpoint().region_source,
            original.checkpoint.region_source
        );
        assert_eq!(
            hash_in_schema(&game, snapshot.schema),
            original.checkpoint.state_hash
        );
        assert_eq!(game.checkpoint().summary.tick, 900);
        assert!(game.session().plant_views().iter().any(|p| {
            p.battery
                .as_ref()
                .is_some_and(|b| b.stored.value() == 800.0)
        }));
        // On every target, resave the frozen source boundary, restore, and
        // compare full checkpoints at both continuation boundaries.
        bundle.compatibility = replay_compatibility();
        bundle.checkpoint = game.checkpoint();
        let mut resumed = restored(&encode_save(&bundle).unwrap());
        game.execute(Command::AdvanceTicks { n: 60 }).unwrap();
        resumed.execute(Command::AdvanceTicks { n: 60 }).unwrap();
        assert_eq!(
            resumed.checkpoint(),
            game.checkpoint(),
            "{id} after 60 ticks"
        );
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                hash_in_schema(&game, snapshot.schema),
                after60,
                "{id} after 60 ticks"
            );
        }
        bundle.checkpoint = resumed.checkpoint();
        let mut resumed = restored(&encode_save(&bundle).unwrap());
        game.execute(Command::AdvanceTicks { n: 480 }).unwrap();
        resumed.execute(Command::AdvanceTicks { n: 480 }).unwrap();
        assert_eq!(resumed.checkpoint(), game.checkpoint(), "{id} at review");
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                hash_in_schema(&game, snapshot.schema),
                review,
                "{id} at review"
            );
        }
        bundle.compatibility = replay_compatibility();
        bundle.checkpoint = game.checkpoint();
        let rewritten = encode_save(&bundle).unwrap();
        let resumed = restored(&rewritten);
        assert_eq!(resumed.checkpoint(), game.checkpoint());
        assert_eq!(
            resumed.checkpoint().snapshot.as_ref().unwrap().schema,
            domain::state::CURRENT_STATE_SCHEMA
        );
        assert_eq!(
            resumed.checkpoint().region_source,
            original.checkpoint.region_source
        );
        assert_eq!(std::fs::read(save_path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(baseline.join(format!("{id}-schema3.mgsave.z"))).unwrap(),
            compressed
        );
    }
}

#[test]
fn calibrated_solar_regions_resume_identically_with_their_archived_calibration() {
    use infrastructure::region::RonRegionCatalog;
    let catalog =
        RonRegionCatalog::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    for listing in catalog.list().unwrap() {
        let mut game = Game::start_new(
            &catalog,
            &listing.id,
            7,
            domain::session::GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap();
        assert!(game.session().region().solar().annual_site_clearness);
        use domain::{ids::CellCoord, region::PlantKind};
        let home = game
            .session()
            .city_views()
            .into_iter()
            .find(|c| c.id == game.session().home_city())
            .unwrap();
        let r = game.session().region();
        let mut cells: Vec<_> = (0..r.terrain().height())
            .flat_map(|y| (0..r.terrain().width()).map(move |x| CellCoord { x, y }))
            .collect();
        cells.sort_by_key(|c| {
            u32::from(c.x.abs_diff(home.cell.x)) + u32::from(c.y.abs_diff(home.cell.y))
        });
        let cell = cells
            .into_iter()
            .find(|c| game.session().plant_quote(PlantKind::Solar, *c).is_ok())
            .unwrap();
        let months = game
            .session()
            .plant_quote(PlantKind::Solar, cell)
            .unwrap()
            .months;
        game.execute(Command::BuildPlant {
            kind: PlantKind::Solar,
            cell,
        })
        .unwrap();
        let solar = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.cell == cell)
            .unwrap();
        let line_months = game
            .session()
            .line_quote(home.node, solar.node)
            .unwrap()
            .months;
        game.execute(Command::BuildLine {
            from: home.node,
            to: solar.node,
        })
        .unwrap();
        for _ in 0..months.max(line_months) {
            game.execute(Command::BeginOperating).unwrap();
            game.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
            game.execute(Command::FinishReview).unwrap();
        }
        let month = game.session().clock().month();
        let peak = (0..1440)
            .max_by(|a, b| {
                game.session()
                    .region()
                    .sun(cell, month, *a)
                    .total_cmp(&game.session().region().sun(cell, month, *b))
            })
            .unwrap();
        let saved_tick = peak + 1;
        game.execute(Command::BeginOperating).unwrap();
        game.execute(Command::AdvanceTicks { n: saved_tick })
            .unwrap();
        let output = game
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.id == solar.id)
            .unwrap()
            .output;
        assert!(
            output.value() > 0.0,
            "{} solar must be generating at capture",
            listing.id.as_str()
        );
        let bundle = SaveBundle {
            compatibility: replay_compatibility(),
            metadata: SaveMetadata {
                slot: SaveId::new("a".repeat(32)).unwrap(),
                run: SaveId::new("b".repeat(32)).unwrap(),
                kind: SaveKind::Manual,
                name: "Solar calibration".into(),
                saved_at: 42,
            },
            checkpoint: game.checkpoint(),
        };
        let encoded = encode_save(&bundle).unwrap();
        let mut resumed = restored(&encoded);
        assert_eq!(
            resumed.checkpoint(),
            game.checkpoint(),
            "{}",
            listing.id.as_str()
        );
        assert!(resumed.session().region().solar().annual_site_clearness);
        let restored_output = resumed
            .session()
            .plant_views()
            .into_iter()
            .find(|p| p.id == solar.id)
            .unwrap()
            .output;
        assert_eq!(restored_output, output);
        // Cross the next weather draw as well as the persisted intra-day boundary.
        for command in [
            Command::AdvanceTicks {
                n: 1440 - saved_tick,
            },
            Command::FinishReview,
            Command::BeginOperating,
            Command::AdvanceTicks { n: saved_tick },
        ] {
            let uninterrupted = game.execute(command.clone()).unwrap();
            let continued = resumed.execute(command).unwrap();
            assert_eq!(continued, uninterrupted, "{}", listing.id.as_str());
        }
        assert_eq!(
            resumed.checkpoint(),
            game.checkpoint(),
            "{}",
            listing.id.as_str()
        );
    }
}
