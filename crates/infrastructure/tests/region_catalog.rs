//! Integration tests for `RonRegionCatalog`. Every test works in its own
//! `TempDir` with a copy of the minimal region; nothing touches the cwd or
//! environment.

use std::fs;
use std::path::Path;

use application::{RegionCatalog, RegionLoadError, RegionSummary};
use domain::ids::{CellCoord, CityId, RegionId};
use domain::region::fixtures::tiny_region;
use domain::region::{PlantKind, RegionError};
use infrastructure::region::{RonRegionCatalog, load_region_bytes};
use proptest::prelude::*;
use tempfile::TempDir;

#[path = "support/legacy_raster.rs"]
mod legacy_raster;

const MINIMAL: &str = include_str!("support/minimal_region.ron");

/// SHA-256 of `support/minimal_region.ron`, computed independently with
/// `shasum -a 256`.
const MINIMAL_SHA256: &str = "3f03f8b0da392b2056dca2913733fcf80b402f352f4bd88586453eb89b2b561b";

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
    assert_eq!(loaded.definition.start().home_city, CityId(1));
}

fn without_field(source: &str, field: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with(&format!("{field}:")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_schema2_region_maps_the_a12_fields() {
    let dir = dir_with(&[("minimal.ron", MINIMAL)]);
    let loaded = catalog(dir.path()).load(&RegionId::new("minimal")).unwrap();
    let params = loaded.definition.regulator();
    assert_eq!(params.storage_accreditation.credit(4.0).value(), 0.85);
    assert_eq!(params.satisfaction_target, 90.0);
    assert_eq!(params.return_per_satisfaction_point, 0.0005);
}

#[test]
fn schema_fields_round_trip_with_the_region_dto() {
    let file: infrastructure::region::RegionFile = ron::de::from_str(MINIMAL).unwrap();
    let encoded = ron::ser::to_string(&file).unwrap();
    let decoded: infrastructure::region::RegionFile = ron::de::from_str(&encoded).unwrap();
    assert_eq!(decoded.region_schema, Some(3));
    assert_eq!(
        decoded.regulator.storage_accreditation,
        file.regulator.storage_accreditation
    );
    assert_eq!(decoded.regulator.satisfaction_target, Some(90.0));
}

#[test]
fn schema_probe_preserves_explicit_region_names_without_relaxing_dto_gates() {
    use infrastructure::region::{RegionFile, RegionFileV4};
    let id = RegionId::new("minimal");
    let file: RegionFile = ron::from_str(MINIMAL).unwrap();
    let anonymous = ron::to_string(&file).unwrap();
    let named = format!("RegionFile{anonymous}");
    // The pre-Plan-6 loader directly decoded this unchanged legacy DTO.
    let _: RegionFile = ron::from_str(&named).unwrap();
    let before = load_region_bytes(anonymous.into_bytes(), &id).unwrap();
    let after = load_region_bytes(named.into_bytes(), &id).unwrap();
    assert_eq!(before.definition, after.definition);
    let file: RegionFileV4 = ron::from_str(V4).unwrap();
    let anonymous = ron::to_string(&file).unwrap();
    let named = format!("RegionFileV4{anonymous}");
    let before = load_region_bytes(anonymous.clone().into_bytes(), &id).unwrap();
    let after = load_region_bytes(named.clone().into_bytes(), &id).unwrap();
    assert_eq!(before.definition, after.definition);
    for wrong in [
        format!("RegionFile{anonymous}"),
        format!("Wrong{anonymous}"),
    ] {
        assert!(matches!(
            load_region_bytes(wrong.into_bytes(), &id),
            Err(RegionLoadError::Parse { .. })
        ));
    }
    let unknown = named.replacen("region_schema:4", "region_schema:99", 1);
    assert!(matches!(
        load_region_bytes(unknown.into_bytes(), &id),
        Err(RegionLoadError::Invalid {
            error: RegionError::UnsupportedRegionSchema(99),
            ..
        })
    ));
    let duplicate = named.replacen("region_schema:4", "region_schema:4,region_schema:3", 1);
    assert!(matches!(
        load_region_bytes(duplicate.into_bytes(), &id),
        Err(RegionLoadError::Parse { .. })
    ));
    let unknown_field = named.replacen("region_schema:4", "region_schema:4,unknown:0", 1);
    assert!(matches!(
        load_region_bytes(unknown_field.into_bytes(), &id),
        Err(RegionLoadError::Parse { .. })
    ));
    let missing = named.replacen("system_zone:7,", "", 1);
    assert_ne!(missing, named);
    assert!(
        matches!(load_region_bytes(missing.into_bytes(), &id), Err(RegionLoadError::Invalid { error: RegionError::MissingField {field}, .. }) if field == "system_zone")
    );
}

#[test]
fn a_schema2_region_missing_an_a12_field_names_it() {
    for field in [
        "storage_accreditation",
        "satisfaction_target",
        "return_per_satisfaction_point",
    ] {
        let text = without_field(MINIMAL, field);
        let dir = dir_with(&[("minimal.ron", &text)]);
        let error = catalog(dir.path())
            .load(&RegionId::new("minimal"))
            .unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Invalid { error: RegionError::MissingField { field: missing }, .. } if missing == format!("regulator.{field}")),
            "{error:?}"
        );
    }
}

#[test]
fn a_schema1_region_gets_the_frozen_a12_defaults() {
    use infrastructure::region::legacy;
    let mut text = MINIMAL.to_owned();
    for field in [
        "region_schema",
        "wind",
        "wind_potential",
        "storage_accreditation",
        "satisfaction_target",
        "return_per_satisfaction_point",
    ] {
        text = without_field(&text, field);
    }
    text = text
        .lines()
        .filter(|line| !line.contains("kind: Wind,"))
        .collect::<Vec<_>>()
        .join("\n");
    text = text.replace(", charge_end_hour: 22", "");
    let dir = dir_with(&[("minimal.ron", &text)]);
    let loaded = catalog(dir.path()).load(&RegionId::new("minimal")).unwrap();
    let params = loaded.definition.regulator();
    for &(hours, credit) in &legacy::A12_STORAGE_ACCREDITATION {
        assert_eq!(params.storage_accreditation.credit(hours).value(), credit);
    }
    assert_eq!(params.satisfaction_target, legacy::A12_SATISFACTION_TARGET);
    assert_eq!(
        params.return_per_satisfaction_point,
        legacy::A12_RETURN_PER_SATISFACTION_POINT
    );
}

#[test]
fn a_schema1_file_with_a12_fields_is_rejected() {
    let text = without_field(MINIMAL, "region_schema");
    let dir = dir_with(&[("minimal.ron", &text)]);
    assert!(matches!(
        catalog(dir.path()).load(&RegionId::new("minimal")),
        Err(RegionLoadError::Invalid {
            error: RegionError::MissingField {
                field: "region_schema"
            },
            ..
        })
    ));
}

#[test]
fn an_unknown_region_schema_is_rejected() {
    let text = MINIMAL.replace("region_schema: 3", "region_schema: 9");
    let dir = dir_with(&[("minimal.ron", &text)]);
    assert!(matches!(
        catalog(dir.path()).load(&RegionId::new("minimal")),
        Err(RegionLoadError::Invalid {
            error: RegionError::UnsupportedRegionSchema(9),
            ..
        })
    ));
}

#[test]
fn satisfaction_parameters_are_validated() {
    for (field, old, value) in [
        ("satisfaction_target", "90.0", "101.0"),
        ("satisfaction_target", "90.0", "-1.0"),
        ("satisfaction_target", "90.0", "NaN"),
        ("return_per_satisfaction_point", "0.0005", "inf"),
    ] {
        let text = MINIMAL.replace(&format!("{field}: {old}"), &format!("{field}: {value}"));
        let dir = dir_with(&[("minimal.ron", &text)]);
        let error = catalog(dir.path())
            .load(&RegionId::new("minimal"))
            .unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Invalid { error: RegionError::InvalidShape { field: invalid }, .. } if invalid == format!("regulator.{field}")),
            "{error:?}"
        );
    }
}

#[test]
fn home_city_must_be_known_and_connected_in_region_data() {
    for id in [3, 99] {
        let err = load_edited("home_city: 1", &format!("home_city: {id}")).unwrap_err();
        assert!(
            matches!(err, RegionLoadError::Invalid {
            error: RegionError::InvalidHomeCity(city), ..
        } if city == CityId(id)),
            "{err:?}"
        );
    }
}

#[test]
fn mapped_region_cells_cannot_be_shared_by_starting_assets() {
    // Different coordinates in the same raster cell are still the same site.
    for (from, to) in [
        ("(Peaker, 6.5, 2.5)", "(Peaker, 7.6, 4.6)"),
        ("(Peaker, 6.5, 2.5)", "(Peaker, 6.6, 3.6)"),
        ("lon: 8.5, lat: 1.5", "lon: 6.6, lat: 3.6"),
    ] {
        assert!(MINIMAL.contains(from));
        let error = load_edited(from, to).unwrap_err();
        assert!(
            matches!(
                error,
                RegionLoadError::Invalid {
                    error: RegionError::CityCellOccupied { .. }
                        | RegionError::InvalidStartSite {
                            reason: domain::region::SiteError::Occupied,
                            ..
                        },
                    ..
                }
            ),
            "{error:?}"
        );
    }
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
fn maps_the_same_pre_geothermal_values_as_the_domain_fixture() {
    let dir = dir_with(&[("minimal.ron", MINIMAL)]);
    let def = catalog(dir.path())
        .load(&RegionId::new("minimal"))
        .unwrap()
        .definition;
    let tiny = tiny_region();
    let old_catalog = |region: &domain::region::RegionDefinition| {
        region
            .catalog()
            .iter()
            .filter(|s| s.kind != PlantKind::Geothermal)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(old_catalog(&def), old_catalog(&tiny));
    assert!(
        def.geothermal().is_none(),
        "legacy data receives the unavailable stub, not current tuning"
    );
    assert_eq!(def.cities(), tiny.cities());
    assert_eq!(def.demand_noise(), tiny.demand_noise());
    for m in 1..=12 {
        for t in 0..1440 {
            let month = domain::time::SimMonth::new(2027, m).unwrap();
            assert_eq!(
                def.load_factor(def.cities()[0].id, month, t).to_bits(),
                tiny.load_factor(tiny.cities()[0].id, month, t).to_bits()
            );
        }
    }
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
    let ids: Vec<_> = listed.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(
        listed[0].entry,
        Ok(RegionSummary {
            id: RegionId::new("a"),
            name_key: "region-a".to_owned(),
        })
    );
}

#[test]
fn a_broken_region_file_is_listed_as_unavailable() {
    let dir = dir_with(&[
        ("a.ron", &MINIMAL.replace("\"minimal\"", "\"a\"")),
        ("b.ron", "("),
    ]);
    let listed = catalog(dir.path()).list().unwrap();
    assert_eq!(listed.len(), 2, "the good file is still listed");
    assert_eq!(listed[0].id, RegionId::new("a"));
    assert!(listed[0].entry.is_ok());
    assert_eq!(listed[1].id, RegionId::new("b"));
    assert!(matches!(
        &listed[1].entry,
        Err(RegionLoadError::Parse { region, .. }) if *region == RegionId::new("b")
    ));
}

#[test]
fn a_file_declaring_another_id_is_listed_as_a_mismatch() {
    // `load` would refuse this file by its name; so does the listing.
    let dir = dir_with(&[("b.ron", MINIMAL)]);
    let listed = catalog(dir.path()).list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, RegionId::new("b"));
    assert_eq!(
        listed[0].entry,
        Err(RegionLoadError::IdMismatch {
            requested: RegionId::new("b"),
            found: RegionId::new("minimal"),
        })
    );
}

#[test]
fn listing_does_not_rasterise() {
    // A city in water parses as a DTO but fails mapping and rasterisation.
    let wet = MINIMAL.replace(
        "(id: 1, name_key: \"city-tiny-1\", lon: 6.5,",
        "(id: 1, name_key: \"city-tiny-1\", lon: 0.5,",
    );
    assert_ne!(wet, MINIMAL);
    let dir = dir_with(&[("minimal.ron", &wet)]);
    let listed = catalog(dir.path()).list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].entry,
        Ok(RegionSummary {
            id: RegionId::new("minimal"),
            name_key: "region-minimal".to_owned(),
        })
    );
    // Only loading, which validates in full, finds the problem.
    assert!(matches!(
        catalog(dir.path()).load(&RegionId::new("minimal")),
        Err(RegionLoadError::Invalid { .. })
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
fn new_frequency_parameters_are_validated_when_loaded() {
    let bad_band = |from: &str, to: &str, field: &'static str| {
        let edited = MINIMAL.replace(from, to);
        assert_ne!(edited, MINIMAL);
        let dir = dir_with(&[("minimal.ron", &edited)]);
        let err = catalog(dir.path())
            .load(&RegionId::new("minimal"))
            .unwrap_err();
        assert_eq!(
            err,
            RegionLoadError::Invalid {
                region: RegionId::new("minimal"),
                error: RegionError::InvalidBand { field },
            }
        );
    };
    bad_band(
        "trip_above_hz: 1.5",
        "trip_above_hz: 0.5",
        "frequency.trip_above",
    );
    bad_band(
        "trip_after_ticks: 5",
        "trip_after_ticks: 0",
        "frequency.trip_after_ticks",
    );
    bad_band(
        "shed_stage_spacing_hz: 0.2",
        "shed_stage_spacing_hz: 0.0",
        "frequency.shed_stage_spacing",
    );
    bad_band(
        "frequency_band_hz: 0.2",
        "frequency_band_hz: 0.0",
        "regulator.frequency_band",
    );
}

#[test]
fn an_out_of_range_violation_share_is_rejected_when_loaded() {
    let edited = MINIMAL.replace(
        "frequency_violation_share: 0.05",
        "frequency_violation_share: 1.5",
    );
    assert_ne!(edited, MINIMAL);
    let err = load_region_bytes(edited.into_bytes(), &RegionId::new("minimal")).unwrap_err();
    assert!(
        format!("{err:?}").contains("regulator.frequency_violation_share"),
        "{err:?}"
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

/// The schema-4 fast path in `parse_region_file` checks the id too.
#[test]
fn a_v4_file_declaring_another_id_is_an_id_mismatch() {
    let dir = dir_with(&[("other.ron", V4)]);
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

#[test]
fn schema3_requires_explicit_wind_and_battery_cutoff_fields() {
    for field in ["wind", "wind_potential"] {
        let text = without_field(MINIMAL, field);
        let error = load_region_bytes(text.into_bytes(), &RegionId::new("minimal")).unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Invalid { error: RegionError::MissingField { field: missing }, .. } if missing == field)
        );
    }
    let text = MINIMAL.replace(", charge_end_hour: 22", "");
    let error = load_region_bytes(text.into_bytes(), &RegionId::new("minimal")).unwrap_err();
    assert!(matches!(
        error,
        RegionLoadError::Invalid {
            error: RegionError::MissingField {
                field: "battery.charge_end_hour"
            },
            ..
        }
    ));
}

#[test]
fn wind_map_and_catalog_are_authored_and_the_band_is_validated() {
    let loaded = load_region_bytes(MINIMAL.as_bytes().to_vec(), &RegionId::new("minimal")).unwrap();
    let wind = loaded.definition.spec(PlantKind::Wind);
    assert_eq!(wind.capacity.value(), 300.0);
    assert_eq!(wind.capital_cost.cents(), 30_000_000_000);
    assert_eq!(wind.build_months, 3);
    assert!(!PlantKind::Wind.is_dispatchable());
    assert!(!PlantKind::Battery.is_variable());
    let text = MINIMAL.replace("band: 0.25", "band: 1.1");
    assert!(load_region_bytes(text.into_bytes(), &RegionId::new("minimal")).is_err());
    let text = MINIMAL.replace("persistence: 0.995", "persistence: 1.0");
    assert!(load_region_bytes(text.into_bytes(), &RegionId::new("minimal")).is_err());
}

#[test]
fn legacy_wind_extension_payloads_are_frozen() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        hex(&Sha256::digest(include_bytes!(
            "../src/region/wind_east_v1.ron"
        ))),
        "3a2099ade97ec96b2b1d2a528639227efd97280c3653608d3e303df27a5ee713"
    );
    assert_eq!(
        hex(&Sha256::digest(include_bytes!(
            "../src/region/wind_west_v1.ron"
        ))),
        "d1f9c72b29d08ee94e9696116e91573f9267df8173e13c80c4c8fd8583788d5d"
    );
    assert_eq!(
        hex(&Sha256::digest(include_bytes!(
            "../src/region/wind_other_v1.ron"
        ))),
        "27a82606b9e94372fce6c3b7a1f27d0b5b2bb1c5daa16c95f580e579ed7ec0a2"
    );
}

const V4: &str = include_str!("support/minimal_region_v4.ron");

#[test]
fn a_v4_file_lists_loads_and_has_an_outline() {
    let dir = dir_with(&[("minimal.ron", V4)]);
    let catalog = catalog(dir.path());
    assert_eq!(
        catalog.list().unwrap()[0].entry.as_ref().unwrap().name_key,
        "region-minimal"
    );
    let loaded = catalog.load(&RegionId::new("minimal")).unwrap();
    assert_eq!(loaded.source_bytes, V4.as_bytes());
    let r = &loaded.definition;
    assert_eq!(r.currency(), domain::units::Currency::Eur);
    assert_eq!(r.system_zone().standard_offset_minutes, -180);
    assert_eq!(
        r.local_minute(CityId(2), domain::time::SimMonth::new(2027, 7).unwrap(), 0),
        585
    );
    assert_eq!(
        r.load_factor(CityId(1), domain::time::SimMonth::new(2027, 1).unwrap(), 0),
        3.0
    );
    assert_eq!(
        r.load_factor(CityId(2), domain::time::SimMonth::new(2027, 1).unwrap(), 0),
        0.5
    );
    assert_eq!(
        r.season(domain::time::SimMonth::new(2027, 1).unwrap()),
        domain::time::Season::Summer
    );
    assert!(matches!(
        r.solar().model,
        domain::region::SolarModel::Astronomical
    ));
    let outline = infrastructure::region::region_outline(&loaded.source_bytes, r.id()).unwrap();
    assert_eq!(outline.cities, 3);
    assert_eq!(outline.home, Some(("city-tiny-1".into(), 6.5, 3.5)));
}

#[test]
fn a_v4_file_rejects_a_missing_calendar() {
    let source = without_field(V4, "calendar");
    assert!(matches!(
        load_region_bytes(source.into_bytes(), &RegionId::new("minimal")),
        Err(RegionLoadError::Invalid {
            error: RegionError::MissingField { field: "calendar" },
            ..
        })
    ));
}

#[test]
fn a_v4_file_rejects_a_257_wide_grid() {
    let source = V4.replace("width: 12", "width: 257");
    assert!(matches!(
        load_region_bytes(source.into_bytes(), &RegionId::new("minimal")),
        Err(RegionLoadError::Invalid {
            error: RegionError::InvalidShape { field: "grid" },
            ..
        })
    ));
}

#[test]
fn a_legacy_file_preserves_non_earth_bounds() {
    let mut file: infrastructure::region::RegionFile = ron::from_str(MINIMAL).unwrap();
    file.bounds.west += 360.0;
    file.bounds.east += 360.0;
    for ring in file.land.iter_mut().chain(file.exclude.iter_mut()) {
        for (lon, _) in ring {
            *lon += 360.0;
        }
    }
    for field in [
        &mut file.height,
        &mut file.irradiance,
        &mut file.cooling_water,
        &mut file.hydro_potential,
    ] {
        for blob in &mut field.blobs {
            blob.lon += 360.0;
        }
    }
    for city in &mut file.cities {
        city.lon += 360.0;
    }
    for (_, lon, _) in &mut file.start.fleet {
        *lon += 360.0;
    }
    let bytes = ron::ser::to_string(&file).unwrap().into_bytes();
    let loaded = load_region_bytes(bytes.clone(), &RegionId::new("minimal")).unwrap();
    assert_eq!(loaded.source_bytes, bytes);
    assert_eq!(loaded.definition.geo().west, 360.0);
    let original =
        load_region_bytes(MINIMAL.as_bytes().to_vec(), &RegionId::new("minimal")).unwrap();
    assert_eq!(loaded.definition.terrain(), original.definition.terrain());
    assert_eq!(
        loaded
            .definition
            .sun(
                CellCoord { x: 6, y: 4 },
                domain::time::SimMonth::new(2027, 1).unwrap(),
                720
            )
            .to_bits(),
        original
            .definition
            .sun(
                CellCoord { x: 6, y: 4 },
                domain::time::SimMonth::new(2027, 1).unwrap(),
                720
            )
            .to_bits()
    );
}

#[test]
fn a_v4_file_maps_every_money_field_and_currency() {
    use domain::units::Currency;
    for (code, expected) in [
        ("USD", Currency::Usd),
        ("EUR", Currency::Eur),
        ("CNY", Currency::Cny),
        ("INR", Currency::Inr),
        ("RUB", Currency::Rub),
        ("BRL", Currency::Brl),
    ] {
        let loaded = load_region_bytes(
            V4.replace("currency: EUR", &format!("currency: {code}"))
                .into_bytes(),
            &RegionId::new("minimal"),
        )
        .unwrap();
        let r = loaded.definition;
        assert_eq!(r.currency(), expected);
        let p = r.spec(PlantKind::Coal);
        assert_eq!(p.startup_cost.cents(), 15_000_000);
        assert_eq!(p.marginal_cost.value(), 30.0);
        assert_eq!(p.fixed_om_per_month.cents(), 200_000_000);
        assert_eq!(p.capital_cost.cents(), 200_000_000_000);
        assert_eq!(r.regulator().starting_tariff.value(), 95.0);
        assert_eq!(r.regulator().reference_tariff.value(), 100.0);
        assert_eq!(r.lines().cost_per_cell.cents(), 4_000_000_000);
        assert_eq!(r.start().cash.cents(), 200_000_000_000);
        assert_eq!(r.start_line_name(CityId(2)), Some("line-starter"));
        assert_eq!(r.start_line_name(CityId(1)), None);
        assert_eq!(r.demand_noise().forecast_error.value(), 0.07);
        assert_eq!(r.demand_noise().error_persistence, 0.8);
    }
}

#[test]
fn a_v4_file_rejects_usd_suffixed_fields_at_every_level() {
    for (field, old) in [
        ("startup_cost", "startup_cost_usd"),
        ("marginal_cost_per_mwh", "marginal_cost_usd_per_mwh"),
        ("fixed_om_per_month", "fixed_om_usd_per_month"),
        ("capital_cost", "capital_cost_usd"),
        ("starting_tariff_per_mwh", "starting_tariff_usd_per_mwh"),
        ("reference_tariff_per_mwh", "reference_tariff_usd_per_mwh"),
        ("cost_per_cell", "cost_per_cell_usd"),
        ("cash", "cash_usd"),
    ] {
        let text = V4.replacen(&format!("{field}:"), &format!("{old}:"), 1);
        let error = load_region_bytes(text.into_bytes(), &RegionId::new("minimal")).unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Parse {ref message, ..} if message.contains(old)),
            "{field}: {error:?}"
        );
    }
    for (needle, injected) in [
        ("solar: (", "daylight: [],"),
        ("battery: Some((", "cash_usd: 2,"),
        ("calendar: (", "cash_usd: 2,"),
        ("demand_noise: (", "cash_usd: 2,"),
        ("atlas: (", "cash_usd: 2,"),
        ("(id: 7,", "cash_usd: 2,"),
        ("(id: 10,", "cash_usd: 2,"),
    ] {
        let text = V4.replacen(needle, &format!("{needle}{injected}"), 1);
        assert!(matches!(
            load_region_bytes(text.into_bytes(), &RegionId::new("minimal")),
            Err(RegionLoadError::Parse { .. })
        ));
    }
}

#[test]
fn a_v4_file_requires_authored_fields_with_typed_errors() {
    // Remove fields from a serialized DTO with balanced containers, so each case
    // reaches the real decoder rather than failing on malformed test syntax.
    fn remove(source: &str, field: &str) -> String {
        let start = source
            .match_indices(&format!("{field}:"))
            .find(|(i, _)| {
                *i == 0
                    || !source.as_bytes()[i - 1].is_ascii_alphanumeric()
                        && source.as_bytes()[i - 1] != b'_'
            })
            .unwrap()
            .0;
        let value = start + field.len() + 1;
        let mut depth = 0;
        let mut quoted = false;
        let mut end = source.len();
        for (offset, ch) in source[value..].char_indices() {
            match ch {
                '"' => quoted = !quoted,
                '(' | '[' | '{' if !quoted => depth += 1,
                ')' | ']' | '}' if !quoted && depth == 0 => {
                    end = value + offset;
                    break;
                }
                ')' | ']' | '}' if !quoted => depth -= 1,
                ',' if !quoted && depth == 0 => {
                    end = value + offset + 1;
                    break;
                }
                _ => {}
            }
        }
        format!("{}{}", &source[..start], &source[end..])
    }
    for field in [
        "system_zone",
        "time_zones",
        "climates",
        "demand_noise",
        "calendar",
        "currency",
        "wind",
        "wind_potential",
        "zone",
        "climate",
        "shapes",
        "month_shapes",
        "monthly",
        "dst_months",
        "seasons",
        "startup_cost",
        "marginal_cost_per_mwh",
        "fixed_om_per_month",
        "capital_cost",
        "starting_tariff_per_mwh",
        "reference_tariff_per_mwh",
        "cost_per_cell",
        "cash",
        "storage_accreditation",
        "satisfaction_target",
        "return_per_satisfaction_point",
        "clearness_mean",
        "clearness_sd",
    ] {
        let error = load_region_bytes(remove(V4, field).into_bytes(), &RegionId::new("minimal"))
            .unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Invalid {error: RegionError::MissingField {field: missing}, ..} if missing == field),
            "{field}: {error:?}"
        );
    }
}

#[test]
fn a_v4_file_rejects_invalid_authored_values() {
    for (needle, value, field) in [
        ("currency: EUR", "currency: JPY", "currency"),
        (
            "standard_offset_minutes: -180",
            "standard_offset_minutes: 1",
            "time_zones.offset",
        ),
        ("system_zone: 7", "system_zone: 99", "system_zone"),
        ("zone: 8", "zone: 99", "cities.zone"),
        ("climate: 20", "climate: 99", "cities.climate"),
        ("width: 12", "width: 0", "grid"),
        ("height: 8", "height: 129", "grid"),
        ("west: 0.0", "west: -181.0", "bounds"),
        (
            r#"month_shapes: ["hot""#,
            r#"month_shapes: ["missing""#,
            "climates.month_shapes",
        ),
        (
            r#"line_names: [(2, "line-starter")]"#,
            r#"line_names: [(3, "line-starter")]"#,
            "start.line_names",
        ),
        (
            r#"line_names: [(2, "line-starter")]"#,
            r#"line_names: [(1, "line-starter")]"#,
            "start.line_names",
        ),
        (
            r#"line_names: [(2, "line-starter")]"#,
            r#"line_names: [(2, "")]"#,
            "start.line_names",
        ),
        (
            r#"line_names: [(2, "line-starter")]"#,
            r#"line_names: [(2, "a"), (2, "b")]"#,
            "start.line_names",
        ),
    ] {
        assert!(V4.contains(needle), "{needle}");
        let error = load_region_bytes(
            V4.replacen(needle, value, 1).into_bytes(),
            &RegionId::new("minimal"),
        )
        .unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Invalid {error: RegionError::InvalidShape {field: f}, ..} if f == field),
            "{field}: {error:?}"
        );
    }
}

#[test]
fn atlas_colour_does_not_require_an_illustration_letter() {
    let id = RegionId::new("minimal");
    let expected = load_region_bytes(V4.as_bytes().to_vec(), &id).unwrap();
    let outline = infrastructure::region::region_outline(&expected.source_bytes, &id).unwrap();
    assert_eq!(outline.atlas.as_ref().unwrap().colour, "#123456");
    for source in [
        V4.to_owned(),
        V4.replace("atlas: (", "atlas: AtlasDto("),
        V4.replace("atlas: (", r#"atlas: (letter: "B", "#),
        V4.replace("atlas: (", r#"atlas: AtlasDto(letter: "B", "#),
    ] {
        let loaded = load_region_bytes(source.as_bytes().to_vec(), &id).unwrap();
        assert_eq!(loaded.source_bytes.as_slice(), source.as_bytes());
        assert_eq!(loaded.definition, expected.definition);
        assert_eq!(
            infrastructure::region::region_outline(&loaded.source_bytes, &id).unwrap(),
            outline
        );
    }
    let serialized = ron::to_string(outline.atlas.as_ref().unwrap()).unwrap();
    assert!(!serialized.contains("letter"));
}

#[test]
fn atlas_metadata_reaches_the_outline_only() {
    use infrastructure::region::region_outline;
    let id = RegionId::new("minimal");
    let a = load_region_bytes(V4.as_bytes().to_vec(), &id).unwrap();
    let edited = V4.replace(r##"colour: "#123456""##, r##"colour: "#abcdef""##);
    let b = load_region_bytes(edited.as_bytes().to_vec(), &id).unwrap();
    assert_eq!(a.definition, b.definition);
    assert_ne!(a.fingerprint, b.fingerprint);
    let atlas = region_outline(&b.source_bytes, &id).unwrap().atlas.unwrap();
    assert_eq!(atlas.colour, "#abcdef");
    let omitted = load_region_bytes(
        without_field(&without_field(V4, "atlas"), "line_names").into_bytes(),
        &id,
    )
    .unwrap();
    assert!(
        region_outline(&omitted.source_bytes, &id)
            .unwrap()
            .atlas
            .is_none()
    );
    assert!(omitted.definition.start_line_name(CityId(2)).is_none());
}

#[test]
fn archived_v3_bytes_preserve_terrain_factors_sun_and_locale() {
    use domain::time::{Season, SimMonth};
    use sha2::{Digest, Sha256};
    for id in ["eastern_interconnection", "western_interconnection"] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/global-foundations");
        let bytes = fs::read(root.join(format!("{id}-v3.ron"))).unwrap();
        let file: infrastructure::region::RegionFile = ron::de::from_bytes(&bytes).unwrap();
        let loaded = load_region_bytes(bytes, &RegionId::new(id)).unwrap();
        let r = loaded.definition;
        assert_eq!(r.currency(), domain::units::Currency::Usd);
        assert_eq!(r.system_zone().standard_offset_minutes, 0);
        assert_eq!(r.system_zone().dst_months, [false; 12]);
        assert_eq!(r.system_zone().name_key, None);
        assert!(matches!(
            r.solar().model,
            domain::region::SolarModel::DaylightTable(_)
        ));
        let t = r.terrain();
        assert_eq!(
            *t,
            legacy_raster::rasterize(&file),
            "{id} old raster formula"
        );
        let mut h = Sha256::new();
        h.update(t.width().to_le_bytes());
        h.update(t.height().to_le_bytes());
        for y in 0..t.height() {
            for x in 0..t.width() {
                let c = t.get(CellCoord { x, y }).unwrap();
                h.update([u8::from(c.land)]);
                for f in [
                    c.height,
                    c.irradiance,
                    c.wind_potential,
                    c.cooling_water,
                    c.hydro_potential,
                ] {
                    h.update(f.value().to_bits().to_le_bytes());
                }
            }
        }
        // Only the capture platform promises exact dynamic libm bits (§5.7).
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                hex(&h.finalize()),
                fs::read_to_string(root.join(format!("{id}-terrain-sha256.txt")))
                    .unwrap()
                    .trim()
            );
        }
        for m in 1..=12 {
            let month = SimMonth::new(2027, m).unwrap();
            assert_eq!(r.season(month), month.season());
            let shape = match month.season() {
                Season::Winter => &file.demand.winter,
                Season::Summer => &file.demand.summer,
                _ => &file.demand.shoulder,
            };
            let (rise, set) = file.solar.daylight[usize::from(m - 1)];
            for tick in 0..1440 {
                let hour = (tick / 60) as usize;
                let next = (hour + 1) % 24;
                let fraction = f64::from(tick % 60) / 60.0;
                // Frozen pre-foundation interpolation and daylight formula.
                let demand = (shape[hour] + (shape[next] - shape[hour]) * fraction)
                    * file.demand.monthly[usize::from(m - 1)];
                let time = f64::from(tick) / 60.0;
                let sun = if time <= rise || time >= set {
                    0.0
                } else {
                    (std::f64::consts::PI * (time - rise) / (set - rise)).sin()
                };
                for c in r.cities() {
                    assert_eq!(r.city_zone(c.id), r.system_zone());
                    assert_eq!(r.load_factor(c.id, month, tick).to_bits(), demand.to_bits());
                    assert_eq!(
                        r.sun(c.cell, month, tick).to_bits(),
                        sun.to_bits(),
                        "{id} {m} {tick}"
                    );
                }
            }
        }
    }
}

proptest! {
    #[test]
    fn v4_starter_line_names_reference_connected_non_home_cities(city in 0u32..8, blank in any::<bool>()) {
        let mut file: infrastructure::region::RegionFileV4 = ron::from_str(V4).unwrap();
        file.start.line_names = vec![(city, if blank {" ".into()} else {"line-test".into()})];
        let result = infrastructure::region::map_region_v4(file);
        prop_assert_eq!(result.is_ok(), city == 2 && !blank);
        if let Err(error) = result {prop_assert_eq!(error, RegionError::InvalidShape {field: "start.line_names"});}
    }
}

fn v4_irradiance(source: &str) -> String {
    V4.replace(
        "irradiance: (base: 0.8, blobs: [])",
        &format!("irradiance: {source}"),
    )
}

#[test]
fn v4_irradiance_retains_explicit_field_names_and_named_roundtrips() {
    use infrastructure::region::IrradianceV4;
    let anonymous = load_region_bytes(V4.as_bytes().to_vec(), &RegionId::new("minimal")).unwrap();
    let named = load_region_bytes(
        v4_irradiance("Field(base: 0.8, blobs: [])").into_bytes(),
        &RegionId::new("minimal"),
    )
    .unwrap();
    assert_eq!(named.definition, anonymous.definition);
    for source in ["(base: 0.8, blobs: [])", "(cells: [0.0, 0.5, 1.0])"] {
        let field: IrradianceV4 = ron::from_str(source).unwrap();
        let named = ron::ser::to_string_pretty(
            &field,
            ron::ser::PrettyConfig::default().struct_names(true),
        )
        .unwrap();
        assert!(named.starts_with("Field("));
        let roundtrip: IrradianceV4 = ron::from_str(&named).unwrap();
        assert_eq!(
            ron::to_string(&roundtrip).unwrap(),
            ron::to_string(&field).unwrap()
        );
    }
}

#[test]
fn v4_irradiance_grid_maps_exact_row_major_values_and_roundtrips() {
    let values: Vec<_> = (0..96).map(|i| f64::from(i) / 95.0).collect();
    let text = v4_irradiance(&format!("(cells: {values:?})"));
    let loaded = load_region_bytes(text.into_bytes(), &RegionId::new("minimal")).unwrap();
    for y in 0..8 {
        for x in 0..12 {
            assert_eq!(
                loaded
                    .definition
                    .terrain()
                    .get(CellCoord { x, y })
                    .unwrap()
                    .irradiance
                    .value(),
                values[usize::from(y) * 12 + usize::from(x)]
            );
        }
    }
    let dto: infrastructure::region::RegionFileV4 =
        ron::de::from_bytes(&loaded.source_bytes).unwrap();
    let encoded = ron::to_string(&dto).unwrap();
    let roundtrip = load_region_bytes(encoded.into_bytes(), &RegionId::new("minimal")).unwrap();
    assert_eq!(roundtrip.definition, loaded.definition);
}

#[test]
fn v4_irradiance_grid_rejects_nonfinite_and_out_of_range_values() {
    for value in ["NaN", "inf", "-inf", "-0.01", "1.01"] {
        let values = std::iter::once(value)
            .chain(std::iter::repeat_n("0.5", 95))
            .collect::<Vec<_>>()
            .join(",");
        let error = load_region_bytes(
            v4_irradiance(&format!("(cells: [{values}])")).into_bytes(),
            &RegionId::new("minimal"),
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                RegionLoadError::Invalid {
                    error: RegionError::InvalidShape {
                        field: "irradiance.cells"
                    },
                    ..
                }
            ),
            "{value}: {error:?}"
        );
    }
}

#[test]
fn v4_irradiance_requires_exactly_one_complete_encoding() {
    for source in [
        "(base: 0.8, blobs: [], cells: [])",
        "(base: 0.8, cells: [])",
        "(blobs: [], cells: [])",
        "(cells: [], typo: 1)",
    ] {
        assert!(
            matches!(
                load_region_bytes(
                    v4_irradiance(source).into_bytes(),
                    &RegionId::new("minimal")
                ),
                Err(RegionLoadError::Parse { .. })
            ),
            "{source}"
        );
    }
    for (source, field) in [
        ("(base: 0.8)", "blobs"),
        ("(blobs: [])", "base"),
        ("()", "base"),
    ] {
        let error = load_region_bytes(
            v4_irradiance(source).into_bytes(),
            &RegionId::new("minimal"),
        )
        .unwrap_err();
        assert!(
            matches!(error, RegionLoadError::Invalid {error: RegionError::MissingField {field: got}, ..} if got == field),
            "{source}: {error:?}"
        );
    }
    let source = without_field(V4, "irradiance");
    assert!(matches!(
        load_region_bytes(source.into_bytes(), &RegionId::new("minimal")),
        Err(RegionLoadError::Invalid {
            error: RegionError::MissingField {
                field: "irradiance"
            },
            ..
        })
    ));
    let legacy = MINIMAL.replace(
        "irradiance: (base: 0.8, blobs: [])",
        "irradiance: (cells: [])",
    );
    assert!(matches!(
        load_region_bytes(legacy.into_bytes(), &RegionId::new("minimal")),
        Err(RegionLoadError::Parse { .. })
    ));
}

proptest! {
    #[test]
    fn v4_irradiance_grid_length_is_exact_and_values_reach_their_cell(length in 0usize..193, at in 0usize..96, value in 0.0f64..=1.0) {
        let mut values = vec![0.5;length];
        if at < length {values[at]=value;}
        let result=load_region_bytes(v4_irradiance(&format!("(cells: {values:?})")).into_bytes(), &RegionId::new("minimal"));
        if length == 96 {
            let r=result.unwrap().definition;
            prop_assert_eq!(r.terrain().get(CellCoord{x:(at%12) as u16,y:(at/12) as u16}).unwrap().irradiance.value(),value);
        } else {
            prop_assert!(matches!(result, Err(RegionLoadError::Invalid {error: RegionError::InvalidShape {field: "irradiance.cells"}, ..})), "length {length}");
        }
    }
}

#[test]
fn economic_zones_load_strictly_and_reach_quotes_and_outline() {
    let source = include_bytes!("support/economic_region_v4.ron");
    let loaded = load_region_bytes(source.to_vec(), &RegionId::new("minimal")).unwrap();
    let spec = loaded
        .definition
        .plant_spec_at(domain::region::PlantKind::Wind, CellCoord { x: 8, y: 4 })
        .unwrap();
    assert_eq!(spec.capital_cost.cents(), 200_000_000);
    assert_eq!(spec.build_months, 3);
    infrastructure::region::region_outline(source, &RegionId::new("minimal")).unwrap();
}

#[test]
fn economics_rejects_malformed_present_data_and_legacy_extensions() {
    let source = include_str!("support/economic_region_v4.ron");
    for (old, new) in [
        ("id: 1, name_key:", "id: 0, name_key:"),
        ("id: 2, name_key:", "id: 1, name_key:"),
        ("economic-zone-west", " "),
        ("build_months: 2)", "build_months: 0)"),
        ("build_months: 2)", "build_months: 1000001)"),
        ("capital_cost: 1000000", "capital_cost: -1"),
        ("capital_cost: 1000000", "capital_cost: 9223372036854775807"),
        ("cost_per_cell: 300000", "cost_per_cell: -1"),
        ("cells_per_month: 2)", "cells_per_month: 0)"),
        (
            "kind: Wind, capital_cost: 1000000",
            "kind: Unknown, capital_cost: 1000000",
        ),
        (
            "id: 1, name_key:",
            "id: 1, hvdc: (cells_per_month: 0), name_key:",
        ),
        (
            "kind: Wind, capital_cost: 1000000",
            "kind: Wind, capacity_mw: 400.0, capital_cost: 1000000",
        ),
        ("0,1,1,1,1,1,2,2,2,2,2,2", "1,1,1,1,1,1,2,2,2,2,2,2"),
        ("0,1,1,1,1,1,2,2,2,2,2,2", "0,99,1,1,1,1,2,2,2,2,2,2"),
        ("0,1,1,1,1,1,2,2,2,2,2,2", "0,1,1,1,1,1,2,2,2,2,2"),
        ("economic_zones: (", "economic_zones: None"),
    ] {
        assert!(source.contains(old), "{old}");
        let invalid = source.replacen(old, new, 1);
        assert!(
            load_region_bytes(invalid.as_bytes().to_vec(), &RegionId::new("minimal")).is_err(),
            "{new}"
        );
        assert!(
            infrastructure::region::region_outline(invalid.as_bytes(), &RegionId::new("minimal"))
                .is_err(),
            "outline: {new}"
        );
    }
    let legacy = MINIMAL.replace(
        "id: \"minimal\",",
        "id: \"minimal\", economic_zones: (zones: [], cells: []),",
    );
    assert!(load_region_bytes(legacy.into_bytes(), &RegionId::new("minimal")).is_err());
    let dto: infrastructure::region::RegionFileV4 = ron::from_str(source).unwrap();
    let named =
        ron::ser::to_string_pretty(&dto, ron::ser::PrettyConfig::default().struct_names(true))
            .unwrap();
    let a = load_region_bytes(source.as_bytes().to_vec(), &RegionId::new("minimal")).unwrap();
    let b = load_region_bytes(named.into_bytes(), &RegionId::new("minimal")).unwrap();
    assert_eq!(a.definition, b.definition);
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("minimal.ron"), source).unwrap();
    let catalog = RonRegionCatalog::new(dir.path());
    assert!(catalog.list().unwrap()[0].entry.is_ok());
    assert!(
        catalog
            .load(&RegionId::new("minimal"))
            .unwrap()
            .definition
            .has_economic_zones()
    );
}

#[test]
fn overflowing_economic_work_rates_keep_the_typed_zone_diagnostic() {
    let source = include_str!("support/economic_region_v4.ron")
        .replace("cells_per_month: 2)", "cells_per_month: 4294967291)")
        .replace("cells_per_month: 3)", "cells_per_month: 4294967279)");
    // The regional speed is 2. These two coprime overrides cannot share a u64 denominator.
    let error = load_region_bytes(source.into_bytes(), &RegionId::new("minimal")).unwrap_err();
    assert!(
        matches!(
            error,
            RegionLoadError::Invalid {
                error: RegionError::InvalidEconomicZone {
                    zone: Some(domain::region::EconomicZoneId(2)),
                    field: "economic_zones.ac_lines.cells_per_month"
                },
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn generated_zone_cells_load_through_the_catalog() {
    use infrastructure::region::{
        RegionFileV4, ZonePolygons, economic_zone_cells, economic_zone_cells_sidecar,
    };
    let source = include_str!("support/economic_region_v4.ron");
    let file: RegionFileV4 = ron::from_str(source).unwrap();
    // Unlike the fixture's split at lon 6, these polygons split the map at lon 4.
    let square = |id, west: f64, east: f64| ZonePolygons {
        id,
        polygons: vec![vec![(west, -1.0), (east, -1.0), (east, 9.0), (west, 9.0)]],
    };
    let cells = economic_zone_cells(&file, &[square(1, -1.0, 4.0), square(2, 4.0, 13.0)]).unwrap();
    let sidecar = economic_zone_cells_sidecar("minimal.ron", "z.ron", b"", &cells, file.grid.width);
    let start = source.find("        cells: [").unwrap();
    let end = start + source[start..].find("        ],\n").unwrap() + "        ],\n".len();
    let main = format!(
        "{}        cells_file: \"economic_zone_cells.ron\",\n{}",
        &source[..start],
        &source[end..]
    );

    let dir = dir_with(&[("minimal.ron", &main)]);
    fs::create_dir(dir.path().join("minimal")).unwrap();
    fs::write(dir.path().join("minimal/economic_zone_cells.ron"), sidecar).unwrap();
    let loaded = catalog(dir.path()).load(&RegionId::new("minimal")).unwrap();
    let zone = |x, y| {
        loaded
            .definition
            .economic_zone_at(CellCoord { x, y })
            .unwrap()
            .0
    };
    for y in 0..8 {
        assert_eq!(zone(0, y), 0, "water column, row {y}");
        for x in 1..4 {
            assert_eq!(zone(x, y), 1, "cell ({x}, {y})");
        }
        for x in 4..12 {
            assert_eq!(zone(x, y), 2, "cell ({x}, {y})");
        }
    }
}

#[test]
fn solar_calibration_is_explicit_roundtrips_and_defaults_to_archived_behaviour() {
    use infrastructure::region::{RegionFileV4, SolarCalibrationDto, map_region_v4};
    let old: RegionFileV4 = ron::from_str(V4).unwrap();
    assert_eq!(old.solar.calibration, SolarCalibrationDto::Legacy);
    assert!(!ron::to_string(&old).unwrap().contains("calibration"));
    let legacy = map_region_v4(old.clone()).unwrap();
    assert!(!legacy.solar().annual_site_clearness);
    for calibration in [
        SolarCalibrationDto::AngstromPrescott,
        SolarCalibrationDto::ClearSkyFraction,
    ] {
        let mut file = old.clone();
        file.solar.calibration = calibration;
        let encoded = ron::to_string(&file).unwrap();
        let decoded: RegionFileV4 = ron::from_str(&encoded).unwrap();
        assert_eq!(decoded.solar.calibration, calibration);
        let calibrated = map_region_v4(decoded).unwrap();
        assert!(calibrated.solar().annual_site_clearness);
        assert_eq!(calibrated, map_region_v4(file).unwrap());
        if calibration == SolarCalibrationDto::ClearSkyFraction {
            assert_eq!(calibrated.terrain(), legacy.terrain());
        } else {
            assert_ne!(calibrated.terrain(), legacy.terrain());
        }
    }
}

#[test]
fn calibrated_solar_rejects_monthly_means_outside_the_weather_draw_bounds() {
    use infrastructure::region::{RegionFileV4, SolarCalibrationDto, map_region_v4};
    for mean in [0.0, 0.1, 1.01, f64::INFINITY, f64::NAN] {
        let mut file: RegionFileV4 = ron::from_str(V4).unwrap();
        file.solar.calibration = SolarCalibrationDto::AngstromPrescott;
        file.solar.clearness_mean[0] = mean;
        assert_eq!(
            map_region_v4(file).unwrap_err(),
            RegionError::InvalidShape {
                field: "solar.clearness_mean"
            }
        );
    }
}
