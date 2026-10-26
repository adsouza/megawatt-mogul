//! Region bundles (spec §28.11.1): a main region file and its sidecars as one canonical byte
//! string, which is what a region is parsed from, fingerprinted by and archived as.
//!
//! A main file that references no sidecar is its own bundle, byte for byte. Otherwise the bundle
//! is framed: `megawatt-region-bundle 1\n`, then one `<name> <length>\n<bytes>` entry for the
//! main file (`<id>.ron`) and one for each sidecar (`<id>/<file>`) in ascending name order.
//! Decoding is strict, so a region has exactly one bundle.

use std::collections::{BTreeMap, BTreeSet};

use application::{RegionBundleProblem, RegionLoadError, SidecarProblem};
use domain::ids::RegionId;
use domain::region::RegionError;
use serde::de::DeserializeOwned;

use super::dto::{CellsSidecar, GeographySidecar, IrradianceV4, ParsedRegionFile};

const MAGIC: &[u8] = b"megawatt-region-bundle ";
const VERSION: &[u8] = b"1\n";
/// The longest entry header accepted, newline included.
const MAX_HEADER: usize = 256;

/// A decoded bundle: views of the main file and of each sidecar, keyed by its file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionBundle<'a> {
    pub main: &'a [u8],
    pub sidecars: BTreeMap<&'a str, &'a [u8]>,
}

/// The canonical bundle of region `id`. `sidecars` is keyed by file name, without `<id>/`.
pub fn encode_region_bundle(
    id: &str,
    main: &[u8],
    sidecars: &BTreeMap<String, Vec<u8>>,
) -> Vec<u8> {
    if sidecars.is_empty() {
        return main.to_vec();
    }
    let mut out = [MAGIC, VERSION].concat();
    let mut entry = |name: &str, bytes: &[u8]| {
        out.extend_from_slice(format!("{name} {}\n", bytes.len()).as_bytes());
        out.extend_from_slice(bytes);
    };
    entry(&format!("{id}.ron"), main);
    for (name, bytes) in sidecars {
        entry(&format!("{id}/{name}"), bytes);
    }
    out
}

/// Splits a bundle into its files without parsing them. Bytes that do not start with the
/// bundle magic are a one-file region. Framed bytes must be canonical for `region`.
pub fn decode_region_bundle<'a>(
    bytes: &'a [u8],
    region: &RegionId,
) -> Result<RegionBundle<'a>, RegionLoadError> {
    let Some(rest) = bytes.strip_prefix(MAGIC) else {
        return Ok(RegionBundle {
            main: bytes,
            sidecars: BTreeMap::new(),
        });
    };
    let fail = |problem| RegionLoadError::Bundle {
        region: region.clone(),
        problem,
    };
    let mut rest = rest
        .strip_prefix(VERSION)
        .ok_or(fail(RegionBundleProblem::UnsupportedVersion))?;
    let main_name = format!("{}.ron", region.as_str());
    let sidecar_prefix = format!("{}/", region.as_str());
    let mut main = None;
    let mut sidecars = BTreeMap::new();
    while !rest.is_empty() {
        let (name, body, after) = entry(rest).map_err(fail)?;
        rest = after;
        if main.is_none() {
            if name != main_name {
                return Err(fail(RegionBundleProblem::MainName));
            }
            main = Some(body);
            continue;
        }
        let file = name
            .strip_prefix(&sidecar_prefix)
            .ok_or(fail(RegionBundleProblem::BadHeader))?;
        if sidecars
            .last_key_value()
            .is_some_and(|(last, _)| *last >= file)
        {
            return Err(fail(RegionBundleProblem::Unsorted));
        }
        sidecars.insert(file, body);
    }
    let main = main.ok_or(fail(RegionBundleProblem::Truncated))?;
    if sidecars.is_empty() {
        return Err(fail(RegionBundleProblem::NoSidecars));
    }
    Ok(RegionBundle { main, sidecars })
}

/// One `<name> <length>\n<bytes>` entry: its name, its bytes and what follows it.
fn entry(bytes: &[u8]) -> Result<(&str, &[u8], &[u8]), RegionBundleProblem> {
    let newline = bytes
        .iter()
        .take(MAX_HEADER)
        .position(|&b| b == b'\n')
        .ok_or(RegionBundleProblem::BadHeader)?;
    let header =
        std::str::from_utf8(&bytes[..newline]).map_err(|_| RegionBundleProblem::BadHeader)?;
    let (name, length) = header
        .rsplit_once(' ')
        .ok_or(RegionBundleProblem::BadHeader)?;
    let canonical =
        length.bytes().all(|b| b.is_ascii_digit()) && (length == "0" || !length.starts_with('0'));
    let length: usize = match length.parse() {
        Ok(length) if canonical && !name.is_empty() => length,
        _ => return Err(RegionBundleProblem::BadHeader),
    };
    let rest = &bytes[newline + 1..];
    if rest.len() < length {
        return Err(RegionBundleProblem::Truncated);
    }
    let (body, after) = rest.split_at(length);
    Ok((name, body, after))
}

/// The sidecar files a main file references. Legacy schemas have no reference fields.
pub(super) fn references(file: &ParsedRegionFile) -> BTreeSet<&str> {
    let ParsedRegionFile::V4(file) = file else {
        return BTreeSet::new();
    };
    let irradiance = match &file.irradiance {
        IrradianceV4::CellsFile(name) => Some(name),
        IrradianceV4::Gaussian(_) | IrradianceV4::Cells(_) => None,
    };
    let zones = file
        .economic_zones
        .as_ref()
        .and_then(|z| z.cells_file.as_ref());
    file.geography
        .iter()
        .chain(file.areas.iter().flat_map(|a| a.geography.iter()))
        .chain(irradiance)
        .chain(zones)
        .map(String::as_str)
        .collect()
}

/// A plain sidecar file name: `[a-z0-9_]+\.ron`, so it cannot leave the region's directory.
fn plain_name(name: &str) -> bool {
    name.strip_suffix(".ron").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

fn sidecar_error(region: &RegionId, file: &str, problem: SidecarProblem) -> RegionLoadError {
    RegionLoadError::Sidecar {
        region: region.clone(),
        file: file.into(),
        problem: Box::new(problem),
    }
}

/// Checks that the references are plain names and that `available` holds exactly the
/// referenced files, in that order: names first, then missing files, then unreferenced ones.
pub(super) fn check_references<'a>(
    file: &ParsedRegionFile,
    available: impl IntoIterator<Item = &'a str>,
    region: &RegionId,
) -> Result<(), RegionLoadError> {
    let referenced = references(file);
    if let Some(name) = referenced.iter().find(|name| !plain_name(name)) {
        return Err(sidecar_error(region, name, SidecarProblem::InvalidName));
    }
    let available: BTreeSet<&str> = available.into_iter().collect();
    if let Some(name) = referenced.difference(&available).next() {
        return Err(sidecar_error(region, name, SidecarProblem::Missing));
    }
    if let Some(name) = available.difference(&referenced).next() {
        return Err(sidecar_error(region, name, SidecarProblem::Unreferenced));
    }
    Ok(())
}

/// Inlines every referenced sidecar, so mapping sees what the one-file form would give it.
/// Generated rings go ahead of the inline ones, in reference order. `sidecars` must hold every
/// reference (see `check_references`).
pub(super) fn resolve<S: AsRef<[u8]>>(
    file: ParsedRegionFile,
    sidecars: &BTreeMap<&str, S>,
    region: &RegionId,
) -> Result<ParsedRegionFile, RegionLoadError> {
    let ParsedRegionFile::V4(mut file) = file else {
        return Ok(file);
    };
    let read = |name: &str| -> Result<&[u8], RegionLoadError> {
        sidecars
            .get(name)
            .map(AsRef::as_ref)
            .ok_or_else(|| sidecar_error(region, name, SidecarProblem::Missing))
    };

    for area in &mut file.areas {
        for name in std::mem::take(&mut area.geography) {
            let rings: GeographySidecar = parse_sidecar(read(&name)?, &name, region)?;
            area.polygons.extend(rings.land);
        }
    }
    let (mut land, mut exclude) = (Vec::new(), Vec::new());
    for name in std::mem::take(&mut file.geography) {
        let rings: GeographySidecar = parse_sidecar(read(&name)?, &name, region)?;
        land.extend(rings.land);
        exclude.extend(rings.exclude);
    }
    land.append(&mut file.land);
    exclude.append(&mut file.exclude);
    (file.land, file.exclude) = (land, exclude);

    if let IrradianceV4::CellsFile(name) = &file.irradiance {
        let sidecar: CellsSidecar<f64> = parse_sidecar(read(name)?, name, region)?;
        file.irradiance = IrradianceV4::Cells(sidecar.cells);
    }
    if let Some(zones) = &mut file.economic_zones
        && let Some(name) = zones.cells_file.take()
    {
        if !zones.cells.is_empty() {
            return Err(RegionLoadError::Invalid {
                region: region.clone(),
                error: RegionError::InvalidShape {
                    field: "economic_zones.cells",
                },
            });
        }
        let sidecar: CellsSidecar<u32> = parse_sidecar(read(&name)?, &name, region)?;
        zones.cells = sidecar.cells;
    }
    Ok(ParsedRegionFile::V4(file))
}

fn parse_sidecar<T: DeserializeOwned>(
    bytes: &[u8],
    name: &str,
    region: &RegionId,
) -> Result<T, RegionLoadError> {
    let parse_error = |message: String| sidecar_error(region, name, SidecarProblem::Parse(message));
    let text = std::str::from_utf8(bytes)
        .map_err(|e| parse_error(format!("invalid UTF-8 at byte {}", e.valid_up_to())))?;
    ron::de::from_str(text).map_err(|e| parse_error(e.to_string()))
}
