//! Region sidecars and the canonical bundle (spec §28.11.1). Every test works in its own
//! `TempDir`; nothing touches the cwd or environment.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::Path;

use application::{
    Game, InMemoryCommandLog, RegionBundleProblem, RegionCatalog, RegionLoadError,
    ReplayCompatibility, ReplayError, RestoreError, SaveBundle, SaveError, SaveId, SaveKind,
    SaveMetadata, SaveStore, SaveTarget, SidecarProblem,
};
use domain::ids::RegionId;
use domain::region::RegionError;
use domain::session::GameSettings;
use infrastructure::region::{
    RonRegionCatalog, decode_region_bundle, encode_region_bundle, load_region_bytes, region_outline,
};
use infrastructure::save::{FileSaveStore, decode_save, encode_save};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const ECONOMIC: &str = include_str!("support/economic_region_v4.ron");
const MAGIC: &str = "megawatt-region-bundle 1\n";
const LAND: &str = "[[(1.0, 0.0), (12.0, 0.0), (12.0, 8.0), (1.0, 8.0)]]";
/// A generated ring over water, which the split moves to a sidecar.
const EXCLUDE: &str = "[(0.2, 3.2), (0.8, 3.2), (0.8, 3.8), (0.2, 3.8)]";
/// A hand-authored ring, which the split keeps inline after the generated ones.
const NUDGE: &str = "[(2.1, 2.1), (2.9, 2.1), (2.9, 2.9), (2.1, 2.9)]";

fn id() -> RegionId {
    RegionId::new("minimal")
}

/// 96 distinct values in `[0, 1]`, one per cell of the 12 x 8 fixture.
fn irradiance_values() -> String {
    (0..96)
        .map(|i| format!("{:?}", 0.5 + f64::from(i) / 1000.0))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The fixture's `economic_zones.cells` member, from `cells: [` to its closing `],`.
fn zone_cells_member() -> &'static str {
    let start = ECONOMIC.find("        cells: [").unwrap();
    let end = start + ECONOMIC[start..].find("        ],\n").unwrap() + "        ],\n".len();
    &ECONOMIC[start..end]
}

/// The one-file `land`: the generated ring, then the hand-authored one.
fn inline_land() -> String {
    format!("{}, {NUDGE}]", &LAND[..LAND.len() - 1])
}

/// The economic fixture as one file, with exact irradiance cells and three rings.
fn inline_source() -> String {
    ECONOMIC
        .replace(
            "irradiance: (base: 0.8, blobs: [])",
            &format!("irradiance: (cells: [{}])", irradiance_values()),
        )
        .replace(
            &format!("    land: {LAND},\n    exclude: [],\n"),
            &format!("    land: {},\n    exclude: [{EXCLUDE}],\n", inline_land()),
        )
}

/// The same region split into a main file and three sidecars.
fn split_source() -> (String, BTreeMap<String, String>) {
    let zone_cells = zone_cells_member();
    let main = inline_source()
        .replace(
            &format!("irradiance: (cells: [{}])", irradiance_values()),
            "irradiance: (cells_file: \"irradiance_cells.ron\")",
        )
        .replace(
            zone_cells,
            "        cells_file: \"economic_zone_cells.ron\",\n",
        )
        .replace(
            &format!("    land: {},", inline_land()),
            &format!("    land: [{NUDGE}],\n    geography: [\"natural_earth.ron\"],"),
        )
        .replace(&format!("    exclude: [{EXCLUDE}],"), "    exclude: [],");
    let zone_values = zone_cells
        .trim()
        .trim_start_matches("cells: [")
        .trim_end_matches("],");
    let sidecars = BTreeMap::from([
        (
            "irradiance_cells.ron".to_owned(),
            format!(
                "// Generated irradiance.\n(cells: [{}])\n",
                irradiance_values()
            ),
        ),
        (
            "economic_zone_cells.ron".to_owned(),
            format!("(cells: [{zone_values}])\n"),
        ),
        (
            "natural_earth.ron".to_owned(),
            format!("(land: {LAND}, exclude: [{EXCLUDE}])\n"),
        ),
    ]);
    (main, sidecars)
}

/// A regions directory holding `minimal.ron` and, when given, `minimal/<sidecar>`.
fn regions_dir(main: &str, sidecars: &BTreeMap<String, String>) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("minimal.ron"), main).unwrap();
    if !sidecars.is_empty() {
        fs::create_dir(dir.path().join("minimal")).unwrap();
    }
    for (name, contents) in sidecars {
        fs::write(dir.path().join("minimal").join(name), contents).unwrap();
    }
    dir
}

fn split_dir() -> TempDir {
    let (main, sidecars) = split_source();
    regions_dir(&main, &sidecars)
}

fn sidecar_error(file: &str, problem: SidecarProblem) -> RegionLoadError {
    RegionLoadError::Sidecar {
        region: id(),
        file: file.into(),
        problem: Box::new(problem),
    }
}

fn bundle_error(problem: RegionBundleProblem) -> RegionLoadError {
    RegionLoadError::Bundle {
        region: id(),
        problem,
    }
}

#[test]
fn a_sidecar_region_maps_to_its_inlined_definition_and_outline() {
    let inline = inline_source();
    let (main, _) = split_source();
    assert!(
        !main.contains("cells: [") && main.contains(&format!("land: [{NUDGE}],")),
        "the split moved the generated blocks"
    );
    let one_file = load_region_bytes(inline.clone().into_bytes(), &id()).unwrap();
    let dir = split_dir();
    let split = RonRegionCatalog::new(dir.path()).load(&id()).unwrap();
    assert_eq!(split.definition, one_file.definition);
    assert_ne!(split.fingerprint, one_file.fingerprint);
    assert_eq!(
        region_outline(&split.source_bytes, &id()).unwrap(),
        region_outline(inline.as_bytes(), &id()).unwrap()
    );
}

#[test]
fn a_one_file_region_is_its_own_bundle() {
    let inline = inline_source();
    let dir = regions_dir(&inline, &BTreeMap::new());
    let loaded = RonRegionCatalog::new(dir.path()).load(&id()).unwrap();
    assert_eq!(loaded.source_bytes, inline.as_bytes());
    assert_eq!(
        loaded.fingerprint.as_bytes(),
        &<[u8; 32]>::from(Sha256::digest(inline.as_bytes()))
    );
}

#[test]
fn the_bundle_frames_the_main_file_then_sorted_sidecars_and_is_stable() {
    let (main, sidecars) = split_source();
    let mut expected = format!("{MAGIC}minimal.ron {}\n{main}", main.len());
    for (name, contents) in &sidecars {
        expected.push_str(&format!("minimal/{name} {}\n{contents}", contents.len()));
    }
    let dir = split_dir();
    let catalog = RonRegionCatalog::new(dir.path());
    let first = catalog.load(&id()).unwrap();
    assert_eq!(
        String::from_utf8(first.source_bytes.clone()).unwrap(),
        expected
    );
    assert_eq!(
        first.fingerprint.as_bytes(),
        &<[u8; 32]>::from(Sha256::digest(expected.as_bytes()))
    );
    let second = catalog.load(&id()).unwrap();
    assert_eq!(second.source_bytes, first.source_bytes);
    assert_eq!(second.fingerprint, first.fingerprint);

    // The archived bundle alone reproduces the region, with no sidecar files on disk.
    let archived = load_region_bytes(first.source_bytes.clone(), &id()).unwrap();
    assert_eq!(archived.fingerprint, first.fingerprint);
    assert_eq!(archived.definition, first.definition);
}

#[test]
fn a_missing_sidecar_on_disk_is_named() {
    let dir = split_dir();
    fs::remove_file(dir.path().join("minimal/irradiance_cells.ron")).unwrap();
    assert_eq!(
        RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err(),
        sidecar_error("irradiance_cells.ron", SidecarProblem::Missing)
    );
}

#[test]
fn an_unreferenced_sidecar_file_is_rejected_and_other_files_are_ignored() {
    let dir = split_dir();
    fs::write(dir.path().join("minimal/.DS_Store"), "finder").unwrap();
    fs::write(dir.path().join("minimal/README.md"), "notes").unwrap();
    RonRegionCatalog::new(dir.path()).load(&id()).unwrap();

    fs::write(dir.path().join("minimal/stale.ron"), "(cells: [])").unwrap();
    assert_eq!(
        RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err(),
        sidecar_error("stale.ron", SidecarProblem::Unreferenced)
    );
}

#[test]
fn sidecar_directories_are_not_listed_as_regions() {
    let dir = split_dir();
    let listings = RonRegionCatalog::new(dir.path()).list().unwrap();
    assert_eq!(listings.len(), 1);
    assert_eq!(listings[0].id, id());
    assert!(listings[0].entry.is_ok());
}

#[test]
fn sidecar_names_are_plain_file_names() {
    for name in [
        "../minimal.ron",
        "sub/cells.ron",
        "Cells.ron",
        "cells.txt",
        ".ron",
        "",
    ] {
        let (main, sidecars) = split_source();
        let main = main.replace("\"irradiance_cells.ron\"", &format!("{name:?}"));
        let dir = regions_dir(&main, &sidecars);
        assert_eq!(
            RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err(),
            sidecar_error(name, SidecarProblem::InvalidName),
            "{name:?}"
        );
    }
}

#[test]
fn a_malformed_sidecar_is_a_parse_error_naming_it() {
    let (main, mut sidecars) = split_source();
    sidecars.insert("irradiance_cells.ron".into(), "(cells: [0.5,".into());
    let dir = regions_dir(&main, &sidecars);
    let error = RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err();
    assert!(
        matches!(
            &error,
            RegionLoadError::Sidecar { file, problem, .. }
                if file.as_ref() == "irradiance_cells.ron" && matches!(**problem, SidecarProblem::Parse(_))
        ),
        "{error:?}"
    );
}

#[test]
fn a_short_cell_sidecar_is_the_inline_validation_error() {
    let (main, mut sidecars) = split_source();
    sidecars.insert("irradiance_cells.ron".into(), "(cells: [0.5])".into());
    let dir = regions_dir(&main, &sidecars);
    assert_eq!(
        RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err(),
        RegionLoadError::Invalid {
            region: id(),
            error: RegionError::InvalidShape {
                field: "irradiance.cells"
            },
        }
    );
}

#[test]
fn inline_and_sidecar_cells_are_exclusive() {
    let (main, sidecars) = split_source();
    let both = main.replace(
        "irradiance: (cells_file: \"irradiance_cells.ron\")",
        "irradiance: (cells: [0.5], cells_file: \"irradiance_cells.ron\")",
    );
    let dir = regions_dir(&both, &sidecars);
    assert!(matches!(
        RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err(),
        RegionLoadError::Parse { .. }
    ));

    let both = main.replace(
        "        cells_file: \"economic_zone_cells.ron\",\n",
        &format!(
            "{}        cells_file: \"economic_zone_cells.ron\",\n",
            zone_cells_member()
        ),
    );
    let dir = regions_dir(&both, &sidecars);
    assert_eq!(
        RonRegionCatalog::new(dir.path()).load(&id()).unwrap_err(),
        RegionLoadError::Invalid {
            region: id(),
            error: RegionError::InvalidShape {
                field: "economic_zones.cells"
            },
        }
    );
}

/// The split region's bundle, as the catalog assembles it.
fn split_bundle() -> Vec<u8> {
    let dir = split_dir();
    RonRegionCatalog::new(dir.path())
        .load(&id())
        .unwrap()
        .source_bytes
}

fn entries(bundle: &[u8]) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
    let decoded = decode_region_bundle(bundle, &id()).unwrap();
    (
        decoded.main.to_vec(),
        decoded
            .sidecars
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect(),
    )
}

#[test]
fn bundle_framing_errors_are_typed() {
    let bundle = String::from_utf8(split_bundle()).unwrap();
    let (main, sidecars) = entries(bundle.as_bytes());
    let load = |bytes: &str| load_region_bytes(bytes.as_bytes().to_vec(), &id()).unwrap_err();

    assert_eq!(
        load(&bundle[..bundle.len() - 1]),
        bundle_error(RegionBundleProblem::Truncated)
    );
    assert_eq!(
        load(&bundle.replacen("bundle 1\n", "bundle 2\n", 1)),
        bundle_error(RegionBundleProblem::UnsupportedVersion)
    );
    assert_eq!(
        load(&bundle.replacen("minimal.ron ", "other.ron ", 1)),
        bundle_error(RegionBundleProblem::MainName)
    );
    let main_len = main.len().to_string();
    assert_eq!(
        load(&bundle.replacen(
            &format!("minimal.ron {main_len}\n"),
            &format!("minimal.ron 0{main_len}\n"),
            1
        )),
        bundle_error(RegionBundleProblem::BadHeader)
    );
    assert_eq!(
        load(&format!("{bundle}trailing")),
        bundle_error(RegionBundleProblem::BadHeader)
    );
    let frame = |entries: &[(&str, &[u8])]| {
        let mut out = MAGIC.as_bytes().to_vec();
        for (name, bytes) in entries {
            out.extend(format!("{name} {}\n", bytes.len()).bytes());
            out.extend_from_slice(bytes);
        }
        String::from_utf8(out).unwrap()
    };
    assert_eq!(
        load(&frame(&[("minimal.ron", &main)])),
        bundle_error(RegionBundleProblem::NoSidecars)
    );
    let irradiance = &sidecars["irradiance_cells.ron"];
    let land = &sidecars["natural_earth.ron"];
    let zones = &sidecars["economic_zone_cells.ron"];
    assert_eq!(
        load(&frame(&[
            ("minimal.ron", &main),
            ("minimal/irradiance_cells.ron", irradiance),
            ("minimal/economic_zone_cells.ron", zones),
            ("minimal/natural_earth.ron", land),
        ])),
        bundle_error(RegionBundleProblem::Unsorted)
    );
    assert_eq!(
        load(&frame(&[
            ("minimal.ron", &main),
            ("minimal/economic_zone_cells.ron", zones),
            ("minimal/economic_zone_cells.ron", zones),
            ("minimal/irradiance_cells.ron", irradiance),
            ("minimal/natural_earth.ron", land),
        ])),
        bundle_error(RegionBundleProblem::Unsorted)
    );
    assert_eq!(
        load(&frame(&[
            ("minimal.ron", &main),
            ("other/economic_zone_cells.ron", zones),
        ])),
        bundle_error(RegionBundleProblem::BadHeader)
    );
    assert_eq!(
        load(&frame(&[
            ("minimal.ron", &main),
            ("minimal/economic_zone_cells.ron", zones),
            ("minimal/natural_earth.ron", land),
        ])),
        sidecar_error("irradiance_cells.ron", SidecarProblem::Missing)
    );
    assert_eq!(
        load(&frame(&[
            ("minimal.ron", &main),
            ("minimal/economic_zone_cells.ron", zones),
            ("minimal/extra.ron", zones),
            ("minimal/irradiance_cells.ron", irradiance),
            ("minimal/natural_earth.ron", land),
        ])),
        sidecar_error("extra.ron", SidecarProblem::Unreferenced)
    );
    // A main file that references sidecars is not a bundle on its own.
    assert_eq!(
        load(std::str::from_utf8(&main).unwrap()),
        sidecar_error("economic_zone_cells.ron", SidecarProblem::Missing)
    );
    // A legacy main file has no reference fields, so it cannot carry sidecars.
    let legacy = include_bytes!("support/minimal_region.ron");
    assert_eq!(
        load(&frame(&[
            ("minimal.ron", legacy),
            ("minimal/economic_zone_cells.ron", zones),
        ])),
        sidecar_error("economic_zone_cells.ron", SidecarProblem::Unreferenced)
    );
}

fn identity() -> ReplayCompatibility {
    ReplayCompatibility {
        build: "test".into(),
        platform: "test".into(),
    }
}

fn save_of(catalog: &RonRegionCatalog) -> SaveBundle {
    let checkpoint = Game::start_new(
        catalog,
        &id(),
        7,
        GameSettings::default(),
        Box::new(InMemoryCommandLog::default()),
    )
    .unwrap()
    .checkpoint();
    SaveBundle {
        compatibility: identity(),
        metadata: SaveMetadata {
            slot: SaveId::new("a".repeat(32)).unwrap(),
            run: SaveId::new("b".repeat(32)).unwrap(),
            kind: SaveKind::Manual,
            name: "Sidecars".into(),
            saved_at: 42,
        },
        checkpoint,
    }
}

#[test]
fn a_save_archives_the_bundle_and_refuses_a_tampered_sidecar() {
    let dir = split_dir();
    let catalog = RonRegionCatalog::new(dir.path());
    let saved = save_of(&catalog);
    let bundle = catalog.load(&id()).unwrap().source_bytes;
    assert_eq!(saved.checkpoint.region_source, bundle);

    // The save restores from its archive alone: the sidecar files are gone.
    fs::remove_dir_all(dir.path().join("minimal")).unwrap();
    let (decoded, region) = decode_save(&encode_save(&saved).unwrap(), &identity()).unwrap();
    assert_eq!(decoded, saved);
    assert_eq!(region.source_bytes, bundle);
    Game::restore(
        region,
        decoded.checkpoint,
        Box::new(InMemoryCommandLog::default()),
        &|| false,
    )
    .unwrap();

    let mut tampered = saved;
    let text = String::from_utf8(tampered.checkpoint.region_source).unwrap();
    let at = text.find("minimal/irradiance_cells.ron").unwrap();
    let (head, tail) = text.split_at(at);
    tampered.checkpoint.region_source =
        format!("{head}{}", tail.replacen("0.5,", "0.6,", 1)).into_bytes();
    assert!(matches!(
        decode_save(&encode_save(&tampered).unwrap(), &identity()),
        Err(SaveError::Restore(RestoreError::Replay(
            ReplayError::RegionFingerprintMismatch { .. }
        )))
    ));
}

#[test]
fn the_save_listing_reads_the_name_from_a_bundles_main_file() {
    let regions = split_dir();
    let saves = TempDir::new().unwrap();
    let store = FileSaveStore::new(saves.path().into());
    let mut saved = save_of(&RonRegionCatalog::new(regions.path()));
    saved.compatibility = store.compatibility();
    store.write(&saved, &SaveTarget::Create, &|| false).unwrap();
    let listing = store.list().unwrap();
    let entry = listing[0].entry.as_ref().unwrap();
    assert_eq!(entry.region, id());
    assert_eq!(entry.region_name_key, "region-minimal");
}

#[test]
fn archived_one_file_saves_restore_byte_identically() {
    let baseline = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/global-foundations");
    let shipped =
        RonRegionCatalog::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"));
    for id in ["eastern_interconnection", "western_interconnection"] {
        let compressed = fs::read(baseline.join(format!("{id}-schema3.mgsave.z"))).unwrap();
        let mut bytes = Vec::new();
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut bytes)
            .unwrap();
        let (saved, archived) = decode_save(&bytes, &identity()).unwrap();
        let original = fs::read(baseline.join(format!("{id}-v3.ron"))).unwrap();
        assert_eq!(archived.source_bytes, original, "{id}");
        assert_eq!(
            archived.fingerprint, saved.checkpoint.header.region_fingerprint,
            "{id}"
        );
        let current = shipped.load(&RegionId::new(id)).unwrap();
        assert!(current.source_bytes.starts_with(MAGIC.as_bytes()), "{id}");
        assert_ne!(current.fingerprint, archived.fingerprint, "{id}");
    }
}

fn sidecar_name() -> impl Strategy<Value = String> {
    "[a-z0-9_]{1,8}".prop_map(|stem| format!("{stem}.ron"))
}

proptest! {
    #[test]
    fn bundles_round_trip_and_have_one_encoding(
        main in proptest::collection::vec(any::<u8>(), 0..64),
        sidecars in proptest::collection::btree_map(
            sidecar_name(),
            proptest::collection::vec(any::<u8>(), 0..64),
            1..4,
        ),
    ) {
        let bundle = encode_region_bundle("minimal", &main, &sidecars);
        let decoded = decode_region_bundle(&bundle, &id()).unwrap();
        prop_assert_eq!(decoded.main, &main[..]);
        let round: BTreeMap<String, Vec<u8>> = decoded
            .sidecars
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect();
        prop_assert_eq!(&round, &sidecars);
        prop_assert_eq!(encode_region_bundle("minimal", decoded.main, &round), bundle);
    }

    #[test]
    fn a_region_without_sidecars_is_its_bytes(main in "[^m].{0,64}") {
        let bundle = encode_region_bundle("minimal", main.as_bytes(), &BTreeMap::new());
        prop_assert_eq!(&bundle, main.as_bytes());
        let decoded = decode_region_bundle(&bundle, &id()).unwrap();
        prop_assert_eq!(decoded.main, main.as_bytes());
        prop_assert!(decoded.sidecars.is_empty());
    }
}
