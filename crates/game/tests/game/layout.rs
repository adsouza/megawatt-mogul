//! Guards the one-test-binary layout (see `tests/game.rs`).

use std::fs;
use std::path::Path;

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
