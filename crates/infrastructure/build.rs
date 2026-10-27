mod build_support;
use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=src/browser_storage.js");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let mut inputs = build_support::backend_inputs(&root).expect("backend fingerprint inputs");
    // Track the directories too: adding or removing a source must rerun the generator.
    for name in ["domain", "application", "infrastructure"] {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("crates").join(name).join("src").display()
        );
    }
    for (path, _) in &inputs {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let compiler = Command::new(env::var_os("RUSTC").unwrap())
        .arg("-vV")
        .output()
        .expect("compiler identity");
    assert!(compiler.status.success());
    inputs.push(("build/compiler".into(), compiler.stdout));
    let keys = [
        "TARGET",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTFLAGS",
    ];
    for key in keys {
        println!("cargo:rerun-if-env-changed={key}");
    }
    for (key, value) in env::vars()
        .filter(|(key, _)| key.starts_with("CARGO_FEATURE_") || keys.contains(&key.as_str()))
    {
        inputs.push((format!("build/{key}"), value.into_bytes()));
    }
    println!(
        "cargo:rustc-env=SIM_BUILD_FINGERPRINT={}",
        build_support::fingerprint(inputs)
    );
    println!("cargo:rustc-env=SIM_TARGET={}", env::var("TARGET").unwrap());
}
