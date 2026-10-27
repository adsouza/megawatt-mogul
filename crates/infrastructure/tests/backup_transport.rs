use infrastructure::backup::{decode_backup, encode_backup};
use infrastructure::save::{decode_archive, read_archive_directory, replay_compatibility};
fn fixture() -> infrastructure::save::EncodedArchive {
    read_archive_directory(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/monthly-directory-v4/cccccccccccccccccccccccccccccccc.mgsave"),
    )
    .unwrap()
}
#[test]
fn backup_round_trip_preserves_every_original_archive_byte_and_restores() {
    let archive = fixture();
    let bytes = encode_backup(&archive).unwrap();
    let restored = decode_backup(&bytes).unwrap();
    assert_eq!(restored, archive);
    let (a, region) = decode_archive(&archive, &replay_compatibility(), &|| false).unwrap();
    let game = a
        .restore(
            region,
            &replay_compatibility(),
            Box::<application::InMemoryCommandLog>::default(),
            &|| false,
        )
        .unwrap();
    assert!(game.checkpoint().snapshot.is_some());
}
#[test]
fn unsupported_transport_version_is_refused_without_changing_the_source() {
    let archive = fixture();
    let mut bytes = encode_backup(&archive).unwrap();
    *bytes.last_mut().unwrap() = b'2';
    assert!(decode_backup(&bytes).is_err());
    assert_eq!(fixture(), archive);
}
proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn arbitrary_backup_bytes_never_panic(bytes in proptest::collection::vec(proptest::num::u8::ANY,0..32768)) {let _=decode_backup(&bytes);}
}

fn raw_zip(names: &[&str]) -> Vec<u8> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer.set_comment("megawatt-mogul-backup:1").unwrap();
    for name in names {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(b"test").unwrap();
    }
    writer.finish().unwrap().into_inner()
}
#[test]
fn backup_rejects_traversal_duplicates_encryption_and_bounded_metadata_before_decode() {
    use application::SaveError;
    let mut duplicate = raw_zip(&["a.bin.z", "b.bin.z"]);
    for index in 0..duplicate.len() - 7 {
        if &duplicate[index..index + 7] == b"b.bin.z" {
            duplicate[index] = b'a';
        }
    }
    assert!(
        matches!(decode_backup(&duplicate),Err(SaveError::Malformed(message)) if message.contains("duplicate"))
    );
    let mut path = raw_zip(&["a.bin.z"]);
    for index in 0..path.len() - 7 {
        if &path[index..index + 7] == b"a.bin.z" {
            path[index] = b'/';
        }
    }
    assert!(
        matches!(decode_backup(&path),Err(SaveError::Malformed(message)) if message.contains("name"))
    );
    let mut oversized = raw_zip(&["snapshot.ron.z"]);
    let central = oversized
        .windows(4)
        .position(|bytes| bytes == b"PK\x01\x02")
        .unwrap();
    oversized[central + 24..central + 28]
        .copy_from_slice(&((infrastructure::save::MAX_SAVE_BYTES + 1) as u32).to_le_bytes());
    assert_eq!(decode_backup(&oversized), Err(SaveError::SaveTooLarge));
    let mut encrypted = raw_zip(&["snapshot.ron.z"]);
    let central = encrypted
        .windows(4)
        .position(|bytes| bytes == b"PK\x01\x02")
        .unwrap();
    encrypted[central + 8] |= 1;
    assert!(
        matches!(decode_backup(&encrypted),Err(SaveError::Malformed(message)) if message.contains("encrypted"))
    );
}

#[test]
fn released_portable_schemas_pass_the_transport_and_continue_like_the_source() {
    use application::*;
    use domain::session::DayPhase;
    use infrastructure::save::{decode_save, encode_archive};
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/portable-saves");
    let mut paths = std::fs::read_dir(root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "mgsave"))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 4);
    for path in paths {
        let source = std::fs::read(&path).unwrap();
        let (bundle, region) = decode_save(&source, &replay_compatibility()).unwrap();
        let mut baseline = bundle
            .clone()
            .restore(
                region,
                &replay_compatibility(),
                Box::<InMemoryCommandLog>::default(),
                &|| false,
            )
            .unwrap();
        let archive = encode_archive(&bundle, &bundle.compatibility).unwrap();
        let returned = decode_backup(&encode_backup(&archive).unwrap()).unwrap();
        let (bundle, region) =
            decode_archive(&returned, &replay_compatibility(), &|| false).unwrap();
        let mut candidate = bundle
            .restore(
                region,
                &replay_compatibility(),
                Box::<InMemoryCommandLog>::default(),
                &|| false,
            )
            .unwrap();
        assert_eq!(baseline.checkpoint(), candidate.checkpoint());
        let command = match baseline.session().phase() {
            DayPhase::Planning => Command::BeginOperating,
            DayPhase::Operating => Command::AdvanceTicks { n: 10 },
            DayPhase::Review => Command::FinishReview,
        };
        assert_eq!(
            baseline.execute(command.clone()),
            candidate.execute(command)
        );
        assert_eq!(baseline.checkpoint(), candidate.checkpoint());
        assert_eq!(baseline.projections(), candidate.projections());
        assert_eq!(source, std::fs::read(path).unwrap());
    }
}

#[test]
fn transport_rejects_unreferenced_and_missing_entries() {
    use std::io::Write;
    let archive = fixture();
    for extra in [false, true] {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer.set_comment("megawatt-mogul-backup:1").unwrap();
        writer
            .start_file("snapshot.ron.z", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&archive.manifest).unwrap();
        if extra {
            for (name, bytes) in &archive.files {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer
                .start_file("extra.bin.z", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"extra").unwrap();
        }
        assert!(decode_backup(&writer.finish().unwrap().into_inner()).is_err());
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn corrupt_structured_backup_decode_and_restore_never_panic(index in proptest::num::usize::ANY, mask in 1u8..=255) {
        let mut bytes = encode_backup(&fixture()).unwrap();
        let at = index % bytes.len(); bytes[at] ^= mask;
        if let Ok(archive) = decode_backup(&bytes)
            && let Ok((bundle, region)) = decode_archive(&archive, &replay_compatibility(), &|| false) {
            let _ = bundle.restore(region, &replay_compatibility(), Box::<application::InMemoryCommandLog>::default(), &|| false);
        }
    }
}
