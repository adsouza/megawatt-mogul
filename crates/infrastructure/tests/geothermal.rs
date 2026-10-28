//! Geothermal save compatibility and strict region data contracts.
use application::{Command, Game, InMemoryCommandLog, SaveBundle, SaveId, SaveKind, SaveMetadata};
use domain::{
    ids::RegionId,
    region::{PlantKind, SiteError},
    session::{DayPhase, GameSettings},
    state::State,
};
use infrastructure::{
    region::{RonRegionCatalog, load_region_bytes},
    save::{decode_save, encode_save, replay_compatibility},
};
fn restore(bytes: &[u8]) -> Game {
    let compatibility = replay_compatibility();
    let (bundle, region) = decode_save(bytes, &compatibility).unwrap();
    bundle
        .restore(
            region,
            &compatibility,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap()
}

fn assert_legacy_continuation(
    original: &SaveBundle,
    game: &mut Game,
    command: Command,
    root: &std::path::Path,
    phase: &str,
) {
    // Verify exact resave/restore continuation on every target, including targets whose
    // libm results differ in the last bit from the frozen capture's platform.
    let bundle = SaveBundle {
        compatibility: replay_compatibility(),
        metadata: original.metadata.clone(),
        checkpoint: game.checkpoint(),
    };
    let mut resumed = restore(&encode_save(&bundle).unwrap());
    assert_eq!(resumed.checkpoint(), game.checkpoint(), "{phase}: boundary");
    assert_eq!(
        game.execute(command.clone()).unwrap(),
        resumed.execute(command).unwrap(),
        "{phase}: continuation events"
    );
    assert_eq!(
        resumed.checkpoint(),
        game.checkpoint(),
        "{phase}: continued"
    );

    // Source-schema restore bytes are checked unconditionally by the callers. Only
    // dynamic continuation receipts depend on the platform that captured them.
    if original.compatibility.platform == replay_compatibility().platform {
        assert_eq!(
            game.session().snapshot_state_schema(5),
            std::fs::read(root.join(format!("{phase}-continued-session.bin"))).unwrap(),
            "{phase}: captured session"
        );
        assert_eq!(
            game.projections().state_bytes_schema(5),
            std::fs::read(root.join(format!("{phase}-continued-projections.bin"))).unwrap(),
            "{phase}: captured projections"
        );
    }
}

#[test]
fn geothermal_prechange_schema5_saves_verify_original_bytes_and_remain_unavailable() {
    for bytes in [
        include_bytes!("support/pre-geothermal-rules27/planning.mgsave").as_slice(),
        include_bytes!("support/pre-geothermal-rules27/operating.mgsave").as_slice(),
        include_bytes!("support/pre-geothermal-rules27/review.mgsave").as_slice(),
    ] {
        let (original, _) = decode_save(bytes, &replay_compatibility()).unwrap();
        let source = original.checkpoint.snapshot.as_ref().unwrap();
        assert_eq!(source.schema, 5);
        let mut game = restore(bytes);
        assert_eq!(game.session().snapshot_state_schema(5), source.session);
        assert_eq!(game.projections().state_bytes_schema(5), source.projections);
        assert!(!game.session().geothermal_available());
        for y in 0..game.session().region().terrain().height() {
            for x in 0..game.session().region().terrain().width() {
                assert_eq!(
                    game.session()
                        .plant_site_check(PlantKind::Geothermal, domain::ids::CellCoord { x, y }),
                    Err(SiteError::GeothermalUnavailable)
                );
            }
        }
        let command = match game.session().phase() {
            DayPhase::Planning => Command::BeginOperating,
            DayPhase::Operating => Command::AdvanceTicks { n: 60 },
            DayPhase::Review => Command::FinishReview,
        };
        let phase = match original.checkpoint.summary.phase {
            DayPhase::Planning => "planning",
            DayPhase::Operating => "operating",
            DayPhase::Review => "review",
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/pre-geothermal-rules27");
        assert_legacy_continuation(&original, &mut game, command, &root, phase);
        let bundle = SaveBundle {
            checkpoint: game.checkpoint(),
            ..original
        };
        let rewritten = encode_save(&bundle).unwrap();
        let again = restore(&rewritten);
        assert!(!again.session().geothermal_available());
        assert_eq!(again.checkpoint().snapshot.unwrap().schema, 6);
        assert_eq!(
            again.session().snapshot_state(),
            game.session().snapshot_state()
        );
    }
}
#[test]
fn geothermal_region_schema5_requires_fields_and_keeps_six_regions_resource_free() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions");
    let catalog = RonRegionCatalog::new(&root);
    use application::RegionCatalog;
    for id in [
        "eastern_interconnection",
        "western_interconnection",
        "continental_europe_maghreb",
        "brazil_sin",
        "india_nepal_bhutan",
        "ips_ups",
        "sgcc",
        "southern_china",
    ] {
        let loaded = catalog.load(&RegionId::new(id)).unwrap();
        let r = loaded.definition;
        assert_eq!(r.catalog().len(), 9);
        assert!(r.geothermal().is_some());
        let peak = (0..r.terrain().height())
            .flat_map(|y| (0..r.terrain().width()).map(move |x| domain::ids::CellCoord { x, y }))
            .map(|at| r.terrain().get(at).unwrap().geothermal_potential.value())
            .fold(0.0, f64::max);
        assert_eq!(
            peak > 0.3,
            matches!(id, "western_interconnection" | "continental_europe_maghreb")
        );
    }
    let text = std::fs::read_to_string(root.join("western_interconnection.ron")).unwrap();
    assert!(
        load_region_bytes(
            text.replace("geothermal: (", "missing_geothermal: (")
                .into_bytes(),
            &RegionId::new("western_interconnection")
        )
        .is_err()
    );
}
#[test]
fn geothermal_region_stub_payload_is_frozen() {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(include_bytes!(
        "../src/region/geothermal_unavailable_v1.ron"
    ))
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect::<String>();
    assert_eq!(
        digest,
        "a7077634f397dad9f6fbc73ca9648ad688a3a1c6adb85f352ae6d335586bce22"
    );
}
#[test]
fn geothermal_pipeline_roundtrips_exploration_and_reuse_of_a_dry_cell() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut text = include_str!("support/economic_region_v4.ron").to_owned();
    text = text.replace("region_schema: 4", "region_schema: 5");
    let shipped = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/regions/eastern_interconnection.ron"),
    )
    .unwrap();
    let geothermal = shipped
        .lines()
        .find(|l| l.trim_start().starts_with("geothermal:"))
        .unwrap()
        .replace("success_min: 0.45", "success_min: 0.0")
        .replace("success_max: 0.80", "success_max: 0.0");
    let plant = shipped
        .lines()
        .find(|l| l.contains("kind: Geothermal"))
        .unwrap();
    text = text.replacen("    catalog: [", &format!("    geothermal_potential: (base: 1.0, blobs: []),\n{geothermal}\n    catalog: [\n{plant}"),1);
    std::fs::write(dir.path().join("minimal.ron"), text).unwrap();
    let mut game = Game::start_new(
        &RonRegionCatalog::new(dir.path()),
        &RegionId::new("minimal"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    game.execute(Command::BuildPlant {
        kind: PlantKind::Geothermal,
        cell: domain::ids::CellCoord { x: 4, y: 4 },
    })
    .unwrap();
    let bundle = SaveBundle {
        compatibility: replay_compatibility(),
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            kind: SaveKind::Manual,
            name: "geothermal".into(),
            saved_at: 0,
        },
        checkpoint: game.checkpoint(),
    };
    let mut restored = restore(&encode_save(&bundle).unwrap());
    assert_eq!(game.checkpoint(), restored.checkpoint());
    for command in [Command::BeginOperating, Command::AdvanceTicks { n: 1440 }] {
        assert_eq!(game.execute(command.clone()), restored.execute(command));
    }
    assert_eq!(
        game.session().snapshot_state(),
        restored.session().snapshot_state()
    );
    for _ in 0..2 {
        for command in [
            Command::FinishReview,
            Command::BeginOperating,
            Command::AdvanceTicks { n: 1440 },
        ] {
            assert_eq!(game.execute(command.clone()), restored.execute(command));
        }
    }
    game.execute(Command::FinishReview).unwrap();
    let cell = domain::ids::CellCoord { x: 4, y: 4 };
    let dry = *game.session().dry_cells().get(&cell).unwrap();
    game.execute(Command::BuildPlant {
        kind: PlantKind::Peaker,
        cell,
    })
    .unwrap();
    let reused = SaveBundle {
        checkpoint: game.checkpoint(),
        ..bundle
    };
    let flat = restore(&encode_save(&reused).unwrap());
    assert_eq!(flat.checkpoint(), game.checkpoint());
    let archive = infrastructure::save::encode_archive(&reused, &reused.compatibility).unwrap();
    let (decoded, region) =
        infrastructure::save::decode_archive(&archive, &reused.compatibility, &|| false).unwrap();
    let mut archived = decoded
        .restore(
            region,
            &reused.compatibility,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    assert_eq!(archived.checkpoint(), game.checkpoint());
    assert_eq!(archived.session().dry_cells().get(&cell), Some(&dry));
    assert_eq!(
        archived
            .session()
            .plant_site_check(PlantKind::Geothermal, cell),
        Err(SiteError::ExploredDry)
    );
    for command in [Command::BeginOperating, Command::AdvanceTicks { n: 1440 }] {
        assert_eq!(game.execute(command.clone()), archived.execute(command));
    }
    assert_eq!(archived.checkpoint(), game.checkpoint());
}

#[test]
fn geothermal_prechange_multi_island_hvdc_saves_keep_original_canonical_continuations() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support/pre-geothermal-multiarea-rules27");
    for phase in ["planning", "operating", "review"] {
        let bytes = std::fs::read(root.join(format!("{phase}.mgsave"))).unwrap();
        let (original, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
        let snapshot = original.checkpoint.snapshot.as_ref().unwrap();
        let mut game = restore(&bytes);
        assert_eq!(game.session().snapshot_state_schema(5), snapshot.session);
        assert_eq!(
            game.projections().state_bytes_schema(5),
            snapshot.projections
        );
        assert!(!game.session().geothermal_available());
        assert_eq!(game.session().power().network().links().count(), 1);
        let command = match phase {
            "planning" => Command::BeginOperating,
            "operating" => Command::AdvanceTicks { n: 60 },
            _ => Command::FinishReview,
        };
        assert_legacy_continuation(&original, &mut game, command, &root, phase);
    }
}

#[test]
fn geothermal_frozen_schema4_planning_operating_and_review_boundaries_upcast_without_geothermal() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support/pre-geothermal-schema4-rules27");
    for phase in ["planning", "operating", "review"] {
        let bytes = std::fs::read(root.join(format!("{phase}.mgsave"))).unwrap();
        let (original, _) = decode_save(&bytes, &replay_compatibility()).unwrap();
        let source = original.checkpoint.snapshot.as_ref().unwrap();
        assert_eq!(source.schema, 4);
        let game = restore(&bytes);
        assert_eq!(game.session().snapshot_state_schema(4), source.session);
        assert_eq!(game.projections().state_bytes_schema(4), source.projections);
        assert!(!game.session().geothermal_available());
        let rewritten = encode_save(&SaveBundle {
            checkpoint: game.checkpoint(),
            ..original
        })
        .unwrap();
        assert!(!restore(&rewritten).session().geothermal_available());
    }
}
