use application::SaveError;
use infrastructure::client_progress::{
    ClientProgress, decode_progress, encode_progress, read_progress, write_progress,
};
use std::collections::BTreeSet;
use tempfile::TempDir;

fn malformed<T>(result: Result<T, SaveError>) -> bool {
    matches!(result, Err(SaveError::Malformed(ref m)) if m == "client progress")
}
fn regions(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

#[test]
fn missing_file_reads_default() {
    let dir = TempDir::new().unwrap();
    let progress = read_progress(dir.path()).unwrap();
    assert!(progress.tutorial_pending);
    assert!(progress.played_regions.is_empty());
    assert_eq!(progress, ClientProgress::default());
    assert_eq!(progress.version, 1);
    // A missing root directory is also just "no progress yet".
    assert_eq!(
        read_progress(&dir.path().join("absent")).unwrap(),
        ClientProgress::default()
    );
}

#[test]
fn round_trip() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("settings");
    let progress = ClientProgress {
        tutorial_pending: false,
        played_regions: regions(&["eastern", "western"]),
        ..Default::default()
    };
    write_progress(&root, &progress).unwrap();
    assert_eq!(read_progress(&root).unwrap(), progress);
    assert_eq!(
        decode_progress(&encode_progress(&progress).unwrap()).unwrap(),
        progress
    );
}

#[test]
fn absent_keys_take_defaults() {
    let bare = decode_progress(b"(version:1)").unwrap();
    assert!(bare.tutorial_pending);
    assert!(bare.outlook_tutorial_pending);
    assert!(bare.played_regions.is_empty());
    // Delete each key in turn from a record whose other fields are non-default: the deleted
    // one takes its default and the others keep their values.
    let without_version =
        decode_progress(b"(tutorial_pending:false,played_regions:[\"eastern\"])").unwrap();
    assert_eq!(without_version.version, 1);
    assert!(!without_version.tutorial_pending);
    assert_eq!(without_version.played_regions, regions(&["eastern"]));
    let without_flag = decode_progress(b"(version:1,played_regions:[\"eastern\"])").unwrap();
    assert!(without_flag.tutorial_pending);
    assert_eq!(without_flag.played_regions, regions(&["eastern"]));
    let without_regions = decode_progress(b"(version:1,tutorial_pending:false)").unwrap();
    assert!(!without_regions.tutorial_pending);
    assert!(without_regions.played_regions.is_empty());
}

#[test]
fn unknown_key_is_malformed() {
    assert!(matches!(
        decode_progress(b"(version:1,mystery:true)"),
        Err(SaveError::Malformed(_))
    ));
}

#[test]
fn rejects_bad_version_and_oversized_region_lists() {
    assert!(malformed(decode_progress(b"(version:2)")));
    assert!(malformed(decode_progress(b"(version:0)")));
    let bad_version = ClientProgress {
        version: 2,
        ..Default::default()
    };
    assert!(malformed(encode_progress(&bad_version)));

    let at_limit = ClientProgress {
        played_regions: (0..256).map(|i| format!("r{i:03}")).collect(),
        ..Default::default()
    };
    assert_eq!(
        decode_progress(&encode_progress(&at_limit).unwrap()).unwrap(),
        at_limit
    );
    let too_many = ClientProgress {
        played_regions: (0..257).map(|i| format!("r{i:03}")).collect(),
        ..Default::default()
    };
    assert!(malformed(encode_progress(&too_many)));
    assert!(malformed(decode_progress(
        format!("(played_regions:[{}])", {
            (0..257)
                .map(|i| format!("\"r{i:03}\""))
                .collect::<Vec<_>>()
                .join(",")
        })
        .as_bytes()
    )));

    // Region ids are 1-64 bytes.
    let empty_id = ClientProgress {
        played_regions: regions(&[""]),
        ..Default::default()
    };
    assert!(malformed(encode_progress(&empty_id)));
    let longest = ClientProgress {
        played_regions: regions(&[&"a".repeat(64)]),
        ..Default::default()
    };
    assert!(encode_progress(&longest).is_ok());
    let too_long = ClientProgress {
        played_regions: regions(&[&"a".repeat(65)]),
        ..Default::default()
    };
    assert!(malformed(encode_progress(&too_long)));
    assert!(malformed(decode_progress(
        format!("(played_regions:[\"{}\"])", "a".repeat(65)).as_bytes()
    )));
    // Length is in bytes, not characters: 33 two-byte characters is 66 bytes.
    let wide = ClientProgress {
        played_regions: regions(&[&"é".repeat(33)]),
        ..Default::default()
    };
    assert!(malformed(encode_progress(&wide)));
}

#[test]
fn oversized_input_is_too_large() {
    assert!(matches!(
        decode_progress(&vec![b' '; 16 * 1024 + 1]),
        Err(SaveError::SaveTooLarge)
    ));
}

#[test]
fn invalid_values_do_not_replace_the_previous_file() {
    let dir = TempDir::new().unwrap();
    let good = ClientProgress {
        tutorial_pending: false,
        ..Default::default()
    };
    write_progress(dir.path(), &good).unwrap();
    let bad = ClientProgress {
        version: 9,
        ..Default::default()
    };
    assert!(write_progress(dir.path(), &bad).is_err());
    assert_eq!(read_progress(dir.path()).unwrap(), good);
    // An unreadable file is an error, never silently the default: the caller must not overwrite it.
    std::fs::write(dir.path().join("progress.ron"), "broken").unwrap();
    assert!(read_progress(dir.path()).is_err());
}

#[test]
fn write_is_atomic_in_its_own_tempdir() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("settings");
    let mut progress = ClientProgress::default();
    write_progress(&root, &progress).unwrap();
    progress.tutorial_pending = false;
    progress.played_regions.insert("eastern".into());
    write_progress(&root, &progress).unwrap();
    assert_eq!(read_progress(&root).unwrap(), progress);
    let names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec!["progress.ron".to_string()]);
}

#[test]
fn progress_and_preferences_share_a_directory_without_interfering() {
    use infrastructure::client_preferences::{
        ClientPreferences, read_preferences, write_preferences,
    };
    let dir = TempDir::new().unwrap();
    let preferences = ClientPreferences {
        speed: 4,
        ..Default::default()
    };
    let progress = ClientProgress {
        tutorial_pending: false,
        ..Default::default()
    };
    write_preferences(dir.path(), &preferences).unwrap();
    write_progress(dir.path(), &progress).unwrap();
    assert_eq!(read_preferences(dir.path()).unwrap(), preferences);
    assert_eq!(read_progress(dir.path()).unwrap(), progress);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

proptest::proptest! {
    #[test]
    fn valid_progress_round_trips_exactly(
        pending in proptest::bool::ANY,
        outlook_pending in proptest::bool::ANY,
        ids in proptest::collection::btree_set("[a-z0-9_-]{1,64}", 0..32),
    ) {
        let progress = ClientProgress { tutorial_pending: pending, outlook_tutorial_pending: outlook_pending, played_regions: ids, ..Default::default() };
        proptest::prop_assert_eq!(decode_progress(&encode_progress(&progress).unwrap()).unwrap(), progress);
    }
}
