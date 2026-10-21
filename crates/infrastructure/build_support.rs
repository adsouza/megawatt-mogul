//! Build identity inputs are relative names and exact bytes; reusable in isolated fixture tests.
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            visit(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}
pub fn backend_inputs(root: &Path) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut paths = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("crates/infrastructure/build.rs"),
        root.join("crates/infrastructure/build_support.rs"),
    ];
    for name in ["domain", "application", "infrastructure"] {
        let dir = root.join("crates").join(name);
        paths.push(dir.join("Cargo.toml"));
        visit(&dir.join("src"), &mut paths)?;
    }
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            Ok((
                p.strip_prefix(root)
                    .expect("input under root")
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(&p)?,
            ))
        })
        .collect()
}
pub fn fingerprint(mut inputs: Vec<(String, Vec<u8>)>) -> String {
    inputs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hash = Sha256::new();
    for (key, value) in inputs {
        for bytes in [key.as_bytes(), value.as_slice()] {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
