//! `RonRegionCatalog`: region files on disk, one `<id>.ron` per region.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use application::{
    LoadedRegion, RegionCatalog, RegionFingerprint, RegionListing, RegionLoadError, RegionSummary,
};
use domain::ids::RegionId;
use sha2::{Digest, Sha256};

use super::dto::{ParsedRegionFile, SchemaProbe};
use super::mapping::{map_region, map_region_v4};

/// Reads regions from `<dir>/<id>.ron`. Files are self-contained: no includes
/// and no external inputs, so the file's bytes fully determine the region.
#[derive(Debug, Clone)]
pub struct RonRegionCatalog {
    dir: PathBuf,
}

impl RonRegionCatalog {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The file for `id`, or `None` when the id could point outside the directory.
    fn path_for(&self, id: &RegionId) -> Option<PathBuf> {
        let name = id.as_str();
        let plain = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        plain.then(|| self.dir.join(format!("{name}.ron")))
    }
}

/// Decodes a region file's DTO from its exact source bytes and checks that it
/// declares the id the caller asked for. No mapping and no rasterisation: the
/// cheap first half of loading, which listing also uses. `region` gives errors
/// their context.
pub(super) fn parse_region_file(
    source_bytes: &[u8],
    region: &RegionId,
) -> Result<ParsedRegionFile, RegionLoadError> {
    let parse_error = |message: String| RegionLoadError::Parse {
        region: region.clone(),
        message,
    };
    let text = std::str::from_utf8(source_bytes)
        .map_err(|e| parse_error(format!("invalid UTF-8 at byte {}", e.valid_up_to())))?;
    // RON's error display leads with the `line:col` span.
    let invalid = |error| RegionLoadError::Invalid {
        region: region.clone(),
        error,
    };
    let probe: SchemaProbe = ron::de::from_str(text).map_err(|e| parse_error(e.to_string()))?;
    let file = match probe.region_schema {
        None | Some(2 | 3) => ParsedRegionFile::Legacy(
            ron::de::from_str(text).map_err(|e| parse_error(e.to_string()))?,
        ),
        Some(4) => ParsedRegionFile::V4(ron::de::from_str(text).map_err(|e| match e.code {
            ron::error::Error::MissingStructField { field, .. } => {
                invalid(domain::region::RegionError::MissingField { field })
            }
            _ => parse_error(e.to_string()),
        })?),
        Some(schema) => {
            return Err(invalid(
                domain::region::RegionError::UnsupportedRegionSchema(schema),
            ));
        }
    };

    let found = RegionId::new(file.id());
    if found != *region {
        return Err(RegionLoadError::IdMismatch {
            requested: region.clone(),
            found,
        });
    }
    Ok(file)
}

/// Parses, maps and fingerprints one region from its exact source bytes. This
/// is the single path from bytes to `LoadedRegion`, used for shipped files and
/// for archived copies alike. `region` is the id the caller asked for: it gives
/// errors their context and must match the id the file declares.
pub fn load_region_bytes(
    source_bytes: Vec<u8>,
    region: &RegionId,
) -> Result<LoadedRegion, RegionLoadError> {
    let file = parse_region_file(&source_bytes, region)?;
    let definition = match file {
        ParsedRegionFile::Legacy(f) => map_region(f),
        ParsedRegionFile::V4(f) => map_region_v4(f),
    }
    .map_err(|error| RegionLoadError::Invalid {
        region: region.clone(),
        error,
    })?;
    let fingerprint = RegionFingerprint::new(Sha256::digest(&source_bytes).into());
    tracing::debug!(
        region = region.as_str(),
        bytes = source_bytes.len(),
        "loaded region"
    );
    Ok(LoadedRegion {
        definition: definition.into(),
        source_bytes,
        fingerprint,
    })
}

/// Reads a file once, mapping a missing file to `NotFound`.
fn read_once(path: &Path, region: &RegionId) -> Result<Vec<u8>, RegionLoadError> {
    fs::read(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => RegionLoadError::NotFound(region.clone()),
        _ => RegionLoadError::Io {
            region: region.clone(),
            message: e.to_string(),
        },
    })
}

impl RegionCatalog for RonRegionCatalog {
    /// One listing per `.ron` file, by file name. A file that cannot be read,
    /// decoded or that declares another id is listed with its error; only an
    /// unreadable directory fails the whole listing. Listing decodes each
    /// file's DTO for its id and name only: it neither maps nor rasterises, so
    /// a region that is valid as text but invalid as a map is listed, and
    /// `load` is what refuses it.
    fn list(&self) -> Result<Vec<RegionListing>, RegionLoadError> {
        // Directory-level failures have no region; the directory stands in.
        let dir_error = |e: io::Error| RegionLoadError::Io {
            region: RegionId::new(&self.dir.to_string_lossy()),
            message: e.to_string(),
        };
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(dir_error)? {
            let path = entry.map_err(dir_error)?.path();
            if path.extension().is_some_and(|ext| ext == "ron") && path.is_file() {
                files.push(path);
            }
        }
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        Ok(files
            .iter()
            .map(|path| {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let id = RegionId::new(&stem);
                let entry = read_once(path, &id)
                    .and_then(|bytes| parse_region_file(&bytes, &id))
                    .map(|file| RegionSummary {
                        id: RegionId::new(file.id()),
                        name_key: file.name_key().to_owned(),
                    });
                RegionListing { id, entry }
            })
            .collect())
    }

    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        let path = self
            .path_for(id)
            .ok_or_else(|| RegionLoadError::NotFound(id.clone()))?;
        load_region_bytes(read_once(&path, id)?, id)
    }
}
