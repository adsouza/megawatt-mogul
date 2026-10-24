#[path = "../build_support.rs"]
mod build_support;
use build_support::{backend_inputs, fingerprint};
use std::{fs, path::Path};
use tempfile::TempDir;
fn fixture(root: &Path) {
    for p in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/infrastructure/build.rs",
        "crates/infrastructure/build_support.rs",
        "crates/domain/Cargo.toml",
        "crates/application/Cargo.toml",
        "crates/infrastructure/Cargo.toml",
        "crates/domain/src/lib.rs",
        "crates/application/src/lib.rs",
        "crates/infrastructure/src/lib.rs",
        "crates/game/src/lib.rs",
    ] {
        let path = root.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, p).unwrap();
    }
}
#[test]
fn save_compatibility_tracks_backend_bytes_and_added_files_but_not_checkout_or_rendering() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    fixture(first.path());
    fixture(second.path());
    let baseline = fingerprint(backend_inputs(first.path()).unwrap());
    assert_eq!(
        baseline,
        fingerprint(backend_inputs(second.path()).unwrap())
    );
    fs::write(
        second.path().join("crates/game/src/lib.rs"),
        "rendering polish",
    )
    .unwrap();
    assert_eq!(
        baseline,
        fingerprint(backend_inputs(second.path()).unwrap())
    );
    for p in [
        "Cargo.lock",
        "Cargo.toml",
        "crates/domain/Cargo.toml",
        "crates/domain/src/lib.rs",
        "crates/application/src/lib.rs",
        "crates/infrastructure/src/lib.rs",
        "crates/domain/src/new.rs",
    ] {
        let path = second.path().join(p);
        let original = fs::read(&path).ok();
        fs::write(&path, "changed").unwrap();
        assert_ne!(
            baseline,
            fingerprint(backend_inputs(second.path()).unwrap()),
            "{p}"
        );
        if let Some(bytes) = original {
            fs::write(path, bytes).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
}
#[test]
fn save_compatibility_build_settings_and_field_boundaries_change_identity() {
    for key in [
        "compiler",
        "TARGET",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_FEATURE_SIMULATION",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        assert_ne!(
            fingerprint(vec![(key.into(), b"one".to_vec())]),
            fingerprint(vec![(key.into(), b"two".to_vec())])
        );
    }
    assert_ne!(
        fingerprint(vec![("a".into(), b"bc".to_vec())]),
        fingerprint(vec![("ab".into(), b"c".to_vec())])
    );
    assert_eq!(
        fingerprint(vec![("b".into(), vec![1]), ("a".into(), vec![2])]),
        fingerprint(vec![("a".into(), vec![2]), ("b".into(), vec![1])])
    );
}
