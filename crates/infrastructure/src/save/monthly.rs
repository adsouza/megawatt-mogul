//! Directory save format 4. Compressed text references immutable, checksummed monthly binary
//! chart files. The canonical in-memory portable state and its hashes remain unchanged.
use super::{
    MAX_SAVE_BYTES,
    codec::{digest, malformed},
    file_store::read_bounded,
};
use application::{
    projections::{ArchivedChartMonth, Projections},
    *,
};
use domain::{state::State, time::SimMonth};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
};
pub(crate) const MANIFEST: &str = "snapshot.ron.z";
const MAX_MONTHS: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    length: usize,
    checksum: [u8; 32],
    payload: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    state: String,
    charts: Vec<ChartRef>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartRef {
    file: String,
    year: i32,
    month: u8,
    length: usize,
    decoded: usize,
    checksum: [u8; 32],
}
#[derive(Debug)]
pub(crate) struct EncodedArchive {
    pub manifest: Vec<u8>,
    pub files: BTreeMap<String, Vec<u8>>,
}
fn compress(bytes: &[u8]) -> Result<Vec<u8>, SaveError> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(bytes).map_err(malformed)?;
    z.finish().map_err(malformed)
}
fn inflate(bytes: &[u8]) -> Result<Vec<u8>, SaveError> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(bytes)
        .take(MAX_SAVE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(malformed)?;
    if out.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    Ok(out)
}
fn file_name(year: i32, month: u8, checksum: &[u8; 32]) -> String {
    let hash = checksum
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("{year:04}-{month:02}-{hash}.bin.z")
}
fn unpack(bytes: &[u8]) -> Result<Manifest, SaveError> {
    let raw = inflate(bytes)?;
    let options = ron::Options::default().with_recursion_limit(64);
    let e: Envelope = options.from_bytes(&raw).map_err(malformed)?;
    if e.version != 4 {
        return Err(SaveError::SaveFormatVersionMismatch {
            recorded: e.version,
            supported: 4,
        });
    }
    if e.length != e.payload.len() || e.checksum != digest(e.payload.as_bytes()) {
        return Err(SaveError::ChecksumMismatch);
    }
    let m: Manifest = options.from_str(&e.payload).map_err(malformed)?;
    if m.charts.len() > MAX_MONTHS {
        return Err(SaveError::SaveTooLarge);
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    let mut plain = 0usize;
    let mut last = None;
    for c in &m.charts {
        let month = SimMonth::new(c.year, c.month).map_err(malformed)?;
        if last.is_some_and(|p| p >= month)
            || c.file != file_name(c.year, c.month, &c.checksum)
            || !names.insert(&c.file)
        {
            return Err(malformed("invalid monthly chart reference"));
        }
        last = Some(month);
        total = total
            .checked_add(c.length)
            .filter(|n| *n <= MAX_SAVE_BYTES)
            .ok_or(SaveError::SaveTooLarge)?;
        plain = plain
            .checked_add(c.decoded)
            .filter(|n| *n <= MAX_SAVE_BYTES)
            .ok_or(SaveError::SaveTooLarge)?;
    }
    Ok(m)
}
pub(crate) fn encode(
    bundle: &SaveBundle,
    compatibility: &ReplayCompatibility,
) -> Result<EncodedArchive, SaveError> {
    let mut reduced = bundle.clone();
    let snapshot = reduced
        .checkpoint
        .snapshot
        .as_mut()
        .ok_or(RestoreError::InvalidSnapshot)?;
    let p = Projections::from_state_bytes_schema(&snapshot.projections, snapshot.schema)
        .map_err(malformed)?;
    let (rest, charts) = p
        .split_charts_schema(bundle.checkpoint.summary.month, snapshot.schema)
        .map_err(malformed)?;
    snapshot.projections = rest.state_bytes_schema(snapshot.schema);
    if charts.len() > MAX_MONTHS {
        return Err(SaveError::SaveTooLarge);
    }
    let mut references = Vec::new();
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for chart in charts {
        total = total
            .checked_add(chart.bytes.len())
            .filter(|n| *n <= MAX_SAVE_BYTES)
            .ok_or(SaveError::SaveTooLarge)?;
        let bytes = compress(&chart.bytes)?;
        let checksum = digest(&bytes);
        let file = file_name(chart.month.year(), chart.month.month(), &checksum);
        references.push(ChartRef {
            file: file.clone(),
            year: chart.month.year(),
            month: chart.month.month(),
            length: bytes.len(),
            decoded: chart.bytes.len(),
            checksum,
        });
        files.insert(file, bytes);
    }
    let state = String::from_utf8(super::codec::encode_save_as(&reduced, compatibility)?)
        .map_err(malformed)?;
    let payload = ron::to_string(&Manifest {
        state,
        charts: references,
    })
    .map_err(malformed)?;
    let envelope = Envelope {
        version: 4,
        length: payload.len(),
        checksum: digest(payload.as_bytes()),
        payload,
    };
    let manifest = compress(ron::to_string(&envelope).map_err(malformed)?.as_bytes())?;
    Ok(EncodedArchive { manifest, files })
}
fn safe_read(path: &Path, limit: usize) -> Result<Vec<u8>, SaveError> {
    let meta = std::fs::symlink_metadata(path).map_err(malformed)?;
    if !meta.is_file() || meta.is_symlink() {
        return Err(malformed("invalid snapshot file"));
    }
    read_bounded(path, limit)
}
pub(crate) fn read_manifest(path: &Path) -> Result<Vec<u8>, SaveError> {
    let m = std::fs::symlink_metadata(path).map_err(malformed)?;
    if !m.is_dir() || m.is_symlink() {
        return Err(malformed("invalid snapshot directory"));
    }
    safe_read(&path.join(MANIFEST), MAX_SAVE_BYTES)
}
pub(crate) fn inspect(
    bytes: &[u8],
    root: &Path,
    revision: SaveRevision,
    running: &ReplayCompatibility,
) -> Result<(SaveEntry, Option<MigrationOrigin>), SaveError> {
    let m = unpack(bytes)?;
    for c in &m.charts {
        let meta = std::fs::symlink_metadata(root.join(&c.file)).map_err(malformed)?;
        if !meta.is_file() || meta.is_symlink() || meta.len() != c.length as u64 {
            return Err(SaveError::ChecksumMismatch);
        }
    }
    super::listing::inspect(m.state.as_bytes(), revision, running)
}
pub(crate) fn verify_revision(root: &Path) -> Result<SaveRevision, SaveError> {
    let bytes = read_manifest(root)?;
    let m = unpack(&bytes)?;
    for c in m.charts {
        let data = safe_read(&root.join(c.file), MAX_SAVE_BYTES)?;
        if data.len() != c.length || digest(&data) != c.checksum {
            return Err(SaveError::ChecksumMismatch);
        }
    }
    Ok(SaveRevision(digest(&bytes)))
}
pub(crate) fn read(
    root: &Path,
    running: &ReplayCompatibility,
    cancelled: &dyn Fn() -> bool,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    let bytes = read_manifest(root)?;
    let m = unpack(&bytes)?;
    let (mut bundle, region) =
        super::decode_save_cancellable(m.state.as_bytes(), running, cancelled)?;
    let mut charts = Vec::new();
    for c in m.charts {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let data = safe_read(&root.join(c.file), MAX_SAVE_BYTES)?;
        if data.len() != c.length || digest(&data) != c.checksum {
            return Err(SaveError::ChecksumMismatch);
        }
        let bytes = inflate(&data)?;
        if bytes.len() != c.decoded {
            return Err(SaveError::ChecksumMismatch);
        }
        charts.push(ArchivedChartMonth {
            month: SimMonth::new(c.year, c.month).map_err(malformed)?,
            bytes,
        });
    }
    let snapshot = bundle
        .checkpoint
        .snapshot
        .as_mut()
        .ok_or(RestoreError::InvalidSnapshot)?;
    snapshot.projections =
        Projections::from_state_bytes_schema(&snapshot.projections, snapshot.schema)
            .map_err(malformed)?
            .restore_charts_schema(charts, snapshot.schema)
            .map_err(malformed)?
            .state_bytes_schema(snapshot.schema);
    if snapshot.projections.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    Ok((bundle, region))
}

/// Diagnostic checkpoints use the same self-contained archive without becoming player slots.
pub(crate) fn publish_fresh(
    path: &Path,
    archive: &EncodedArchive,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), SaveError> {
    let parent = path.parent().ok_or_else(|| malformed("snapshot parent"))?;
    if path.exists() {
        return Err(SaveError::SlotChanged);
    }
    let temp = tempfile::TempDir::new_in(parent).map_err(malformed)?;
    for (name, bytes) in archive
        .files
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .chain(std::iter::once((MANIFEST, archive.manifest.as_slice())))
    {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let mut file = std::fs::File::create(temp.path().join(name)).map_err(malformed)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(malformed)?;
    }
    super::file_store::sync_directory(temp.path()).map_err(malformed)?;
    if cancelled() {
        return Err(SaveError::Cancelled);
    }
    std::fs::rename(temp.path(), path).map_err(malformed)?;
    super::file_store::sync_directory(parent).map_err(malformed)
}
