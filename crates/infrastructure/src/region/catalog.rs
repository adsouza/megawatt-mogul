//! `RonRegionCatalog`: region files on disk, one `<id>.ron` per region.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use application::{LoadedRegion, RegionCatalog, RegionFingerprint, RegionLoadError, RegionSummary};
use domain::ids::RegionId;
use sha2::{Digest, Sha256};

use super::dto::RegionFile;
use super::mapping::map_region;

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

/// Parses, maps and fingerprints one region from its exact source bytes. This
/// is the single path from bytes to `LoadedRegion`, used for shipped files and
/// for archived copies alike. `region` is the id the caller asked for: it gives
/// errors their context and must match the id the file declares.
pub fn load_region_bytes(
    source_bytes: Vec<u8>,
    region: &RegionId,
) -> Result<LoadedRegion, RegionLoadError> {
    let parse_error = |message: String| RegionLoadError::Parse {
        region: region.clone(),
        message,
    };
    let text = std::str::from_utf8(&source_bytes)
        .map_err(|e| parse_error(format!("invalid UTF-8 at byte {}", e.valid_up_to())))?;
    // RON's error display leads with the `line:col` span.
    let file: RegionFile = ron::de::from_str(text).map_err(|e| parse_error(e.to_string()))?;

    let found = RegionId::new(&file.id);
    if found != *region {
        return Err(RegionLoadError::IdMismatch {
            requested: region.clone(),
            found,
        });
    }
    let definition = map_region(file).map_err(|error| RegionLoadError::Invalid {
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
    fn list(&self) -> Result<Vec<RegionSummary>, RegionLoadError> {
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

        files
            .iter()
            .map(|path| {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let region = RegionId::new(&stem);
                let loaded = load_region_bytes(read_once(path, &region)?, &region)?;
                Ok(RegionSummary {
                    id: loaded.definition.id().clone(),
                    name_key: loaded.definition.name_key().to_owned(),
                })
            })
            .collect()
    }

    fn load(&self, id: &RegionId) -> Result<LoadedRegion, RegionLoadError> {
        let path = self
            .path_for(id)
            .ok_or_else(|| RegionLoadError::NotFound(id.clone()))?;
        load_region_bytes(read_once(&path, id)?, id)
    }
}
