//! The dependency rule (ARCHITECTURE.md), checked on the manifests: `domain` depends on no
//! framework or I/O crate, and only the `game` crate may depend on Bevy or egui.

use std::path::Path;

fn manifest(krate: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(krate)
        .join("Cargo.toml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Every crate a manifest depends on, in any dependency table (`[dependencies]`,
/// `[dev-dependencies]`, `[build-dependencies]`, a `[target.….dependencies]` or a
/// `[dependencies.<name>]` table), by its key and by the `package` it renames.
fn dependency_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_table = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            let table = line.trim_matches(['[', ']']);
            in_table = table.ends_with("dependencies");
            if let Some((_, name)) = table.split_once("dependencies.") {
                names.push(name.trim_matches('"').to_owned());
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key == "package" {
            // In a `[dependencies.<name>]` table.
            names.push(value.trim().trim_matches('"').to_owned());
        } else if in_table {
            names.push(key.split('.').next().unwrap().trim_matches('"').to_owned());
            if let Some((_, rest)) = value.split_once("package") {
                let renamed = rest.trim_start_matches([' ', '=']).split('"').nth(1);
                names.extend(renamed.map(str::to_owned));
            }
        }
    }
    names
}

/// Bevy and egui, and their satellites (`bevy_egui`, `egui_plot`, ...).
fn is_game_framework(name: &str) -> bool {
    ["bevy", "egui"].iter().any(|framework| {
        name == *framework
            || name.starts_with(&format!("{framework}_"))
            || name.starts_with(&format!("{framework}-"))
    })
}

#[test]
fn domain_manifest_has_no_framework_or_io_dependencies() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    for forbidden in ["bevy", "serde", "ron", "tokio", "tracing"] {
        assert!(
            !deps.contains(forbidden),
            "domain must not depend on {forbidden}"
        );
    }
}

#[test]
fn only_the_game_crate_depends_on_bevy_or_egui() {
    for krate in ["domain", "application", "infrastructure"] {
        let names = dependency_names(&manifest(krate));
        assert!(
            names.iter().any(|name| name == "thiserror"),
            "{krate}: the manifest was read: {names:?}"
        );
        let framework: Vec<&String> = names.iter().filter(|n| is_game_framework(n)).collect();
        assert!(
            framework.is_empty(),
            "{krate} must not depend on {framework:?}: only the game crate may"
        );
    }
}

#[test]
fn dependency_names_reads_every_dependency_table() {
    let manifest = r#"
[package]
name = "x"
edition = "2024"

[dependencies]
domain = { workspace = true }
# bevy = "0.20"
serde.workspace = true
ui = { package = "egui", version = "0.36" }

[dev-dependencies]
bevy_egui = "0.43"

[target.'cfg(unix)'.dependencies]
egui_plot = "0.37"

[dependencies.renderer]
package = "bevy"
version = "0.20"

[features]
bevy = []
"#;
    assert_eq!(
        dependency_names(manifest),
        [
            "domain",
            "serde",
            "ui",
            "egui",
            "bevy_egui",
            "egui_plot",
            "renderer",
            "bevy"
        ]
    );
    let framework: Vec<String> = dependency_names(manifest)
        .into_iter()
        .filter(|name| is_game_framework(name))
        .collect();
    assert_eq!(framework, ["egui", "bevy_egui", "egui_plot", "bevy"]);
    assert!(!is_game_framework("bevyish") && !is_game_framework("thiserror"));
}
