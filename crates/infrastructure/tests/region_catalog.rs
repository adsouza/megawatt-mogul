//! Integration tests for `RonRegionCatalog`. Every test works in its own
//! `TempDir` with a copy of the minimal region; nothing touches the cwd or
//! environment.

use std::fs;
use std::path::Path;

use application::{RegionCatalog, RegionLoadError};
use domain::ids::{CellCoord, CityId, RegionId};
use domain::region::RegionError;
use domain::region::fixtures::tiny_region;
use infrastructure::region::{RonRegionCatalog, load_region_bytes};
use proptest::prelude::*;
use tempfile::TempDir;

const MINIMAL: &str = include_str!("support/minimal_region.ron");

/// SHA-256 of `support/minimal_region.ron`, computed independently with
/// `shasum -a 256`.
const MINIMAL_SHA256: &str = "e48a4e80348b3abdc1e26c8bea1bdf9d4a8b0bcd3a52df7371201922b9c0581a";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn dir_with(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (name, contents) in files {
        fs::write(dir.path().join(name), contents).unwrap();
    }
    dir
}

fn catalog(dir: &Path) -> RonRegionCatalog {
    RonRegionCatalog::new(dir)
}

#[test]
fn loads_a_valid_region() {
    let dir = dir_with(&[("minimal.ron", MINIMAL)]);
    let loaded = catalog(dir.path()).load(&RegionId::new("minimal")).unwrap();
    assert_eq!(loaded.definition.id().as_str(), "minimal");
    assert_eq!(loaded.definition.terrain().width(), 12);
    assert_eq!(loaded.definition.terrain().height(), 8);
}

#[test]
fn loaded_definition_fingerprint_and_archive_source_share_one_read() {
    let dir = dir_with(&[("minimal.ron", MINIMAL)]);
    let loaded = catalog(dir.path()).load(&RegionId::new("minimal")).unwrap();
    assert_eq!(loaded.source_bytes, MINIMAL.as_bytes());
    assert_eq!(hex(loaded.fingerprint.as_bytes()), MINIMAL_SHA256);

    // Editing the file afterwards must not reach the already-loaded region.
    fs::write(dir.path().join("minimal.ron"), "changed").unwrap();
    assert_eq!(loaded.source_bytes, MINIMAL.as_bytes());
    assert_eq!(hex(loaded.fingerprint.as_bytes()), MINIMAL_SHA256);
    assert_eq!(loaded.definition.name_key(), "region-minimal");

    // A formatting-only change is different content and a different fingerprint.
    let reformatted = format!("{MINIMAL}\n");
    let reloaded = load_region_bytes(reformatted.into_bytes(), &RegionId::new("minimal")).unwrap();
    assert_ne!(reloaded.fingerprint, loaded.fingerprint);
}

#[test]
fn maps_the_same_values_as_the_domain_fixture() {
    let dir = dir_with(&[("minimal.ron", MINIMAL)]);
    let def = catalog(dir.path())
        .load(&RegionId::new("minimal"))
        .unwrap()
        .definition;
    let tiny = tiny_region();
    assert_eq!(def.catalog(), tiny.catalog());
    assert_eq!(def.cities(), tiny.cities());
    assert_eq!(def.demand(), tiny.demand());
    assert_eq!(def.solar(), tiny.solar());
    assert_eq!(def.hydro(), tiny.hydro());
    assert_eq!(def.frequency(), tiny.frequency());
    assert_eq!(def.regulator(), tiny.regulator());
    assert_eq!(def.satisfaction(), tiny.satisfaction());
    assert_eq!(def.lines(), tiny.lines());
    assert_eq!(def.start(), tiny.start());
    for y in 0..8 {
        for x in 0..12 {
            let at = CellCoord { x, y };
            assert_eq!(
                def.terrain().get(at).unwrap().land,
                tiny.terrain().get(at).unwrap().land,
                "land at {at:?}"
            );
        }
    }
}

#[test]
fn lists_regions_sorted_by_file_name() {
    // Written b first so directory order is unlikely to match name order.
    let b = MINIMAL
        .replace("id: \"minimal\"", "id: \"b\"")
        .replace("region-minimal", "region-b");
    let a = MINIMAL
        .replace("id: \"minimal\"", "id: \"a\"")
        .replace("region-minimal", "region-a");
    let dir = dir_with(&[("b.ron", &b), ("a.ron", &a), ("notes.txt", "ignored")]);
    let listed = catalog(dir.path()).list().unwrap();
    let ids: Vec<_> = listed.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(listed[0].name_key, "region-a");
}

#[test]
fn list_fails_on_a_malformed_file() {
    let dir = dir_with(&[
        ("a.ron", &MINIMAL.replace("\"minimal\"", "\"a\"")),
        ("b.ron", "("),
    ]);
    assert!(matches!(
        catalog(dir.path()).list(),
        Err(RegionLoadError::Parse { region, .. }) if region == RegionId::new("b")
    ));
}

#[test]
fn list_of_a_missing_directory_is_an_io_error() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope");
    assert!(matches!(
        catalog(&missing).list(),
        Err(RegionLoadError::Io { .. })
    ));
}

#[test]
fn missing_region_is_not_found() {
    let dir = TempDir::new().unwrap();
    let err = catalog(dir.path())
        .load(&RegionId::new("absent"))
        .unwrap_err();
    assert_eq!(err, RegionLoadError::NotFound(RegionId::new("absent")));
}

#[test]
fn ids_that_could_escape_the_directory_are_not_found() {
    let dir = dir_with(&[("minimal.ron", MINIMAL)]);
    let nested = dir.path().join("sub");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("minimal.ron"), MINIMAL).unwrap();
    for bad in ["../minimal", "sub/minimal", "", ".", "a\\b"] {
        let id = RegionId::new(bad);
        assert_eq!(
            catalog(&nested).load(&id).unwrap_err(),
            RegionLoadError::NotFound(id),
            "{bad:?}"
        );
    }
}

#[test]
fn malformed_ron_reports_position() {
    let truncated = &MINIMAL[..MINIMAL.len() / 2];
    let dir = dir_with(&[("minimal.ron", truncated)]);
    let err = catalog(dir.path())
        .load(&RegionId::new("minimal"))
        .unwrap_err();
    let RegionLoadError::Parse { region, message } = err else {
        panic!("expected a parse error, got {err:?}");
    };
    assert_eq!(region, RegionId::new("minimal"));
    // The message leads with `line:col`.
    let (line, rest) = message.split_once(':').unwrap();
    assert!(line.parse::<usize>().is_ok(), "no line number in {message}");
    let col: String = rest.chars().take_while(char::is_ascii_digit).collect();
    assert!(!col.is_empty(), "no column in {message}");
}

#[test]
fn unknown_fields_are_rejected_with_a_position() {
    let typo = MINIMAL.replace(
        "name_key: \"region-minimal\",",
        "name_kee: \"region-minimal\",",
    );
    let err = load_region_bytes(typo.into_bytes(), &RegionId::new("minimal")).unwrap_err();
    assert!(matches!(err, RegionLoadError::Parse { .. }), "{err:?}");
}

#[test]
fn invalid_utf8_is_a_parse_error() {
    let mut bytes = MINIMAL.as_bytes().to_vec();
    bytes.push(0xFF);
    let err = load_region_bytes(bytes, &RegionId::new("minimal")).unwrap_err();
    assert!(matches!(err, RegionLoadError::Parse { .. }), "{err:?}");
}

#[test]
fn city_in_water_is_invalid() {
    let wet = MINIMAL.replace(
        "(id: 1, name_key: \"city-tiny-1\", lon: 6.5,",
        "(id: 1, name_key: \"city-tiny-1\", lon: 0.5,",
    );
    assert_ne!(wet, MINIMAL);
    let dir = dir_with(&[("minimal.ron", &wet)]);
    let err = catalog(dir.path())
        .load(&RegionId::new("minimal"))
        .unwrap_err();
    assert_eq!(
        err,
        RegionLoadError::Invalid {
            region: RegionId::new("minimal"),
            error: RegionError::CityNotOnLand(CityId(1)),
        }
    );
}

#[test]
fn a_file_declaring_another_id_is_an_id_mismatch() {
    let dir = dir_with(&[("other.ron", MINIMAL)]);
    let err = catalog(dir.path())
        .load(&RegionId::new("other"))
        .unwrap_err();
    assert_eq!(
        err,
        RegionLoadError::IdMismatch {
            requested: RegionId::new("other"),
            found: RegionId::new("minimal"),
        }
    );
}

#[test]
fn out_of_range_numbers_are_invalid_not_panics() {
    let cases = [
        (
            "startup_cost_usd: 1000000,",
            "startup_cost_usd: 9223372036854775807,",
        ),
        ("capacity_mw: 1100.0,", "capacity_mw: inf,"),
        ("capacity_credit: 0.95,", "capacity_credit: 1.5,"),
        ("startup_hours: 48.0,", "startup_hours: 1e30,"),
        (
            "grid: (width: 12, height: 8)",
            "grid: (width: 0, height: 8)",
        ),
        (
            "grid: (width: 12, height: 8)",
            "grid: (width: 65535, height: 65535)",
        ),
        ("east: 12.0", "east: -3.0"),
        ("radius_deg: 0.5", "radius_deg: 0.0"),
        ("month: 4,", "month: 13,"),
    ];
    for (from, to) in cases {
        assert!(MINIMAL.contains(from), "{from}");
        let edited = MINIMAL.replacen(from, to, 1);
        let err = load_region_bytes(edited.into_bytes(), &RegionId::new("minimal")).unwrap_err();
        assert!(
            matches!(
                err,
                RegionLoadError::Invalid { .. } | RegionLoadError::Parse { .. }
            ),
            "{to}: {err:?}"
        );
    }
}

proptest! {
    #[test]
    fn garbage_never_panics(s in "\\PC{0,400}") {
        let dir = dir_with(&[("x.ron", &s)]);
        let result = catalog(dir.path()).load(&RegionId::new("x"));
        prop_assert!(result.is_err());
    }

    #[test]
    fn corrupted_valid_file_never_panics(pos in 0..MINIMAL.len(), byte in any::<u8>()) {
        let mut bytes = MINIMAL.as_bytes().to_vec();
        bytes[pos] = byte;
        // Either outcome is fine; the property is that it returns at all.
        let _ = load_region_bytes(bytes, &RegionId::new("minimal"));
    }
}

fn load_edited(from: &str, to: &str) -> Result<application::LoadedRegion, RegionLoadError> {
    assert!(MINIMAL.contains(from), "{from}");
    load_region_bytes(
        MINIMAL.replacen(from, to, 1).into_bytes(),
        &RegionId::new("minimal"),
    )
}

fn invalid_field(result: Result<application::LoadedRegion, RegionLoadError>) -> &'static str {
    match result {
        Err(RegionLoadError::Invalid {
            error: RegionError::InvalidShape { field },
            ..
        }) => field,
        other => panic!("expected InvalidShape, got {other:?}"),
    }
}

#[test]
fn points_outside_the_bounds_are_rejected_not_snapped_to_an_edge() {
    let city_lon = "(id: 1, name_key: \"city-tiny-1\", lon: 6.5,";
    assert_eq!(
        invalid_field(load_edited(
            city_lon,
            "(id: 1, name_key: \"city-tiny-1\", lon: 50.0,"
        )),
        "cities.lon"
    );
    assert_eq!(
        invalid_field(load_edited("lon: 8.5, lat: 1.5,", "lon: 8.5, lat: -1.0,")),
        "cities.lat"
    );
    assert_eq!(
        invalid_field(load_edited(
            "substation: (7.5, 3.5)",
            "substation: (75.0, 3.5)"
        )),
        "start.substation"
    );
    assert_eq!(
        invalid_field(load_edited("(Coal, 7.5, 4.5)", "(Coal, 7.5, 45.0)")),
        "start.fleet"
    );
}

#[test]
fn points_exactly_on_the_upper_or_lower_bound_still_map() {
    // lon == east maps to the last column (land); lat == south to the last row.
    let loaded = load_edited("(Peaker, 6.5, 2.5)", "(Peaker, 12.0, 0.0)").unwrap();
    let peaker = loaded
        .definition
        .start()
        .fleet
        .iter()
        .find(|p| p.kind == domain::region::PlantKind::Peaker)
        .unwrap();
    assert_eq!(peaker.cell, CellCoord { x: 11, y: 7 });
    // lon == west is the water column, so a city there is a land error, not a bounds error.
    let err = load_edited(
        "(id: 1, name_key: \"city-tiny-1\", lon: 6.5,",
        "(id: 1, name_key: \"city-tiny-1\", lon: 0.0,",
    )
    .unwrap_err();
    assert!(matches!(
        err,
        RegionLoadError::Invalid {
            error: RegionError::CityNotOnLand(CityId(1)),
            ..
        }
    ));
}
