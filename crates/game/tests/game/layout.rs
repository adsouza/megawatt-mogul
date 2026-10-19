//! Guards the one-test-binary layout (see `tests/game.rs`).

use std::fs;
use std::path::Path;

fn rust_files(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The lib and bin unit-test harnesses are disabled in `Cargo.toml`, so a unit test under `src/`
/// would compile away silently. Tests belong in a module under `tests/game/`.
#[test]
fn src_has_no_unit_tests() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty());
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        assert!(
            !text.contains("#[cfg(test)]") && !text.contains("#[test]"),
            "{} has unit tests, which never run here: move them to tests/game/",
            file.display()
        );
    }
}

/// Only `tests/game.rs` may be a test crate; other test code is a module under `tests/game/`.
#[test]
fn tests_has_a_single_top_level_binary() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut crates: Vec<String> = fs::read_dir(&tests)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "rs") || path.join("main.rs").is_file()
        })
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    crates.sort();
    assert_eq!(crates, ["game.rs"]);
}
