use application::{
    Command, Game, GameCheckpoint, InMemoryCommandLog, ReplayCompatibility, SaveBundle, SaveError,
    SaveId, SaveKind, SaveMetadata,
};
use domain::ids::RegionId;
use domain::session::GameSettings;
use infrastructure::region::RonRegionCatalog;
use infrastructure::save::{decode_save, encode_save};
use proptest::prelude::*;
use std::path::Path;

fn identity() -> ReplayCompatibility {
    ReplayCompatibility {
        build: "test".into(),
        platform: "test".into(),
    }
}
fn capture() -> GameCheckpoint {
    let catalog =
        RonRegionCatalog::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    Game::start_new(
        &catalog,
        &RegionId::new("western_interconnection"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
    .checkpoint()
}
fn bundle() -> SaveBundle {
    SaveBundle {
        compatibility: identity(),
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            kind: SaveKind::Manual,
            name: "My game".into(),
            saved_at: 42,
        },
        checkpoint: capture(),
    }
}

#[test]
fn save_codec_standalone_bundle_restores_and_refuses_truncated_bytes() {
    let bundle = bundle();
    let bytes = encode_save(&bundle).unwrap();
    let (decoded, region) = decode_save(&bytes, &identity()).unwrap();
    assert_eq!(decoded, bundle);
    Game::restore(
        region,
        decoded.checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert!(decode_save(&bytes[..bytes.len() / 2], &identity()).is_err());
}

/// The build and platform are diagnostics, not a gate: replay verification decides compatibility.
#[test]
fn a_save_from_another_build_or_platform_decodes_with_its_metadata() {
    let elsewhere = |build: &str, platform: &str| ReplayCompatibility {
        build: build.into(),
        platform: platform.into(),
    };
    for recorded in [
        elsewhere("older", "test"),
        elsewhere("test", "other-target"),
        elsewhere("older", "other-target"),
    ] {
        let mut saved = bundle();
        saved.compatibility = recorded.clone();
        let bytes = encode_save(&saved).unwrap();
        let (decoded, region) = decode_save(&bytes, &identity()).unwrap();
        assert_eq!(
            decoded.compatibility, recorded,
            "the creating build is kept"
        );
        assert_eq!(decoded.metadata, saved.metadata);
        assert_eq!(decoded.checkpoint, saved.checkpoint);
        Game::restore(
            region,
            decoded.checkpoint,
            Box::new(InMemoryCommandLog::default()),
            &|| false,
        )
        .unwrap();
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn save_codec_round_trip_original_batches(seed in any::<u64>(), ns in prop::collection::vec(0u32..32, 0..8)) {
        let mut b = bundle(); b.checkpoint.header.seed = seed;
        b.checkpoint.commands = std::iter::once(Command::BeginOperating).chain(ns.into_iter().map(|n| Command::AdvanceTicks { n })).collect();
        b.checkpoint.command_count = b.checkpoint.commands.len() as u64;
        let bytes = encode_save(&b).unwrap();
        prop_assert_eq!(decode_save(&bytes, &identity()).unwrap().0, b);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    version: u32,
    length: usize,
    checksum: [u8; 32],
    payload: String,
}
fn rewrite(bytes: &[u8], mutate: impl FnOnce(&mut Envelope), rehash: bool) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut e: Envelope = ron::de::from_bytes(bytes).unwrap();
    mutate(&mut e);
    if rehash {
        e.length = e.payload.len();
        e.checksum = Sha256::digest(e.payload.as_bytes()).into();
    }
    ron::to_string(&e).unwrap().into_bytes()
}
#[test]
fn save_codec_checks_versions_and_exact_payload_integrity_before_decoding_commands() {
    let b = bundle();
    let bytes = encode_save(&b).unwrap();
    let unknown = rewrite(
        &bytes,
        |e| {
            e.version = 99;
            e.payload = "not a payload".into();
        },
        false,
    );
    assert!(matches!(
        decode_save(&unknown, &identity()),
        Err(SaveError::SaveFormatVersionMismatch { recorded: 99, .. })
    ));
    for broken in [
        rewrite(&bytes, |e| e.length += 1, false),
        rewrite(&bytes, |e| e.payload.push(' '), false),
    ] {
        assert!(matches!(
            decode_save(&broken, &identity()),
            Err(SaveError::ChecksumMismatch)
        ));
    }
    let wrong_rules = rewrite(
        &bytes,
        |e| {
            e.payload = e.payload.replace(
                &format!("sim_rules_version:{}", domain::SIM_RULES_VERSION),
                "sim_rules_version:999",
            )
        },
        true,
    );
    assert!(matches!(
        decode_save(&wrong_rules, &identity()),
        Err(SaveError::Restore(application::RestoreError::Replay(
            application::ReplayError::RulesVersionMismatch { .. }
        )))
    ));
    let fingerprint: String = b
        .checkpoint
        .header
        .region_fingerprint
        .as_bytes()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect();
    let wrong_region = rewrite(
        &bytes,
        |e| e.payload = e.payload.replace(&fingerprint, &"0".repeat(64)),
        true,
    );
    assert!(matches!(
        decode_save(&wrong_region, &identity()),
        Err(SaveError::Restore(application::RestoreError::Replay(
            application::ReplayError::RegionFingerprintMismatch { .. }
        )))
    ));
}
#[test]
fn save_codec_refuses_oversized_histories_and_input_instead_of_truncating() {
    let mut b = bundle();
    b.checkpoint.commands =
        vec![Command::AdvanceTicks { n: 0 }; infrastructure::save::MAX_SAVE_COMMANDS + 1];
    b.checkpoint.command_count = b.checkpoint.commands.len() as u64;
    assert!(matches!(encode_save(&b), Err(SaveError::SaveTooLarge)));
    let bytes = vec![0; infrastructure::save::MAX_SAVE_BYTES + 1];
    assert!(matches!(
        decode_save(&bytes, &identity()),
        Err(SaveError::SaveTooLarge)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn save_codec_any_single_payload_byte_corruption_is_rejected(offset in any::<usize>()) {
        let b = bundle(); let bytes = encode_save(&b).unwrap();
        let corrupt = rewrite(&bytes,|e| {let mut data = e.payload.as_bytes().to_vec(); let at = offset % data.len(); data[at] ^= 1; e.payload=String::from_utf8(data).unwrap();},false);
        prop_assert!(matches!(decode_save(&corrupt, &identity()),Err(SaveError::ChecksumMismatch)));
    }
}

#[test]
fn save_codec_terminal_review_restores_the_complete_report_and_game_over_status() {
    use domain::session::SessionStatus;
    let catalog =
        RonRegionCatalog::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    let mut ended = Game::start_new(
        &catalog,
        &RegionId::new("western_interconnection"),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap();
    for plant in ended
        .session()
        .plant_views()
        .into_iter()
        .filter(|p| p.kind.is_dispatchable())
    {
        ended
            .execute(Command::StopPlant { plant: plant.id })
            .unwrap();
    }
    for _ in 0..3 {
        ended.execute(Command::BeginOperating).unwrap();
        ended.execute(Command::AdvanceTicks { n: 1440 }).unwrap();
        if ended.session().status() != SessionStatus::Active {
            break;
        }
        ended.execute(Command::FinishReview).unwrap();
    }
    assert!(matches!(
        ended.session().status(),
        SessionStatus::GameOver(_)
    ));
    let mut b = bundle();
    b.checkpoint = ended.checkpoint();
    let (b, region) = decode_save(&encode_save(&b).unwrap(), &identity()).unwrap();
    let restored = Game::restore(
        region,
        b.checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();
    assert_eq!(restored.checkpoint(), ended.checkpoint());
    assert_eq!(restored.projections(), ended.projections());
}

/// Saves that parse but no longer decode on this build, encoded: an embedded region this build
/// can't read (its fingerprint still matches its bytes) and a command DTO it refuses (a
/// non-finite dispatch target).
fn undecodable(compatibility: ReplayCompatibility) -> Vec<Vec<u8>> {
    use sha2::{Digest, Sha256};
    let mut region = bundle();
    region.compatibility = compatibility.clone();
    region.checkpoint.region_source = b"(not: a region)".to_vec();
    region.checkpoint.header.region_fingerprint = application::RegionFingerprint::new(
        Sha256::digest(&region.checkpoint.region_source).into(),
    );
    let mut command = bundle();
    command.compatibility = compatibility;
    command.checkpoint.commands = vec![Command::SetDispatchTarget {
        plant: domain::ids::PlantId(0),
        mw: domain::units::Megawatts::new(12345.5),
    }];
    command.checkpoint.command_count = 1;
    let command = rewrite(
        &encode_save(&command).unwrap(),
        |e| {
            assert!(e.payload.contains("mw:12345.5"));
            e.payload = e.payload.replace("mw:12345.5", "mw:NaN");
        },
        true,
    );
    vec![encode_save(&region).unwrap(), command]
}

/// A save another build wrote that this build can't decode keeps its recorded identity, so the
/// player learns it is from another version, not that it is corrupt.
#[test]
fn a_save_from_another_build_that_no_longer_decodes_keeps_its_build_and_cause() {
    let older = ReplayCompatibility {
        build: "older".into(),
        platform: "other-target".into(),
    };
    for bytes in undecodable(older.clone()) {
        match decode_save(&bytes, &identity()) {
            Err(SaveError::OtherVersionUnreadable { recorded, cause }) => {
                assert_eq!(recorded, older);
                assert!(!cause.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }
    for bytes in undecodable(identity()) {
        assert!(
            matches!(
                decode_save(&bytes, &identity()),
                Err(SaveError::Malformed(_))
            ),
            "the same build keeps the malformed copy"
        );
    }
}

/// Payloads that no longer parse as this build's `Payload` at all, because the save's DTOs changed
/// shape: an unknown command variant, a reshaped command and a header field this build lacks.
fn unparseable(compatibility: ReplayCompatibility) -> Vec<Vec<u8>> {
    let mut b = bundle();
    b.compatibility = compatibility;
    b.checkpoint.commands = vec![Command::BeginOperating, Command::AdvanceTicks { n: 77 }];
    b.checkpoint.command_count = 2;
    let bytes = encode_save(&b).unwrap();
    let edit = |from: &'static str, to: &'static str| {
        rewrite(
            &bytes,
            |e| {
                assert_eq!(
                    e.payload.matches(from).count(),
                    1,
                    "{from} in {}",
                    e.payload
                );
                e.payload = e.payload.replacen(from, to, 1);
            },
            true,
        )
    };
    vec![
        edit("BeginOperating", "TeleportPlant"),
        edit(
            "AdvanceTicks(n:77)",
            "AdvanceTicks(n:77,batch:(size:2,label:\"x\"))",
        ),
        edit("seed:7", "seed:7,weather:Some([1,2,3])"),
    ]
}

/// A save another build wrote whose DTOs changed shape fails to parse at all, yet still names the
/// build that wrote it. The same payload stamped by this build is plainly malformed.
#[test]
fn a_save_from_another_build_whose_payload_no_longer_parses_names_its_build() {
    let older = ReplayCompatibility {
        build: "older".into(),
        platform: "other-target".into(),
    };
    // Differing in the build alone, the platform alone, or both is each a foreign stamp.
    let foreign = [
        older.clone(),
        ReplayCompatibility {
            build: "older".into(),
            platform: identity().platform,
        },
        ReplayCompatibility {
            build: identity().build,
            platform: "other-target".into(),
        },
    ];
    for stamp in foreign {
        for bytes in unparseable(stamp.clone()) {
            match decode_save(&bytes, &identity()) {
                Err(SaveError::OtherVersionUnreadable { recorded, cause }) => {
                    assert_eq!(recorded, stamp);
                    assert!(!cause.is_empty());
                }
                other => panic!("{stamp:?}: {other:?}"),
            }
        }
    }
    for bytes in unparseable(identity()) {
        assert!(
            matches!(
                decode_save(&bytes, &identity()),
                Err(SaveError::Malformed(_))
            ),
            "the same build keeps the malformed copy"
        );
    }
    // A foreign payload too damaged to read even its stamp is plainly malformed too.
    let mut b = bundle();
    b.compatibility = older;
    let truncated = rewrite(
        &encode_save(&b).unwrap(),
        |e| e.payload.truncate(e.payload.len() / 2),
        true,
    );
    assert!(matches!(
        decode_save(&truncated, &identity()),
        Err(SaveError::Malformed(_))
    ));
}
