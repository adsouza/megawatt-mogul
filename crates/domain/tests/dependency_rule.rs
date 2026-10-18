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
