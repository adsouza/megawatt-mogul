//! Disposable metadata hints. The directory and each source stamp are checked on every scan;
//! selected loads never consult this index. Cache failures must not prevent playing or saving.
use super::{
    codec::{PhaseDto, StatusDto, digest},
    file_store::{CachedListing, ListingStamp, read_bounded},
};
use application::*;
use domain::{
    ids::RegionId,
    session::{DayPhase, GameOverReason, SessionStatus},
    time::SimMonth,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

pub(super) const NAME: &str = ".save-library-cache.ron";
const VERSION: u32 = 1;
const LIMIT: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    checksum: [u8; 32],
    payload: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    build: String,
    platform: String,
    rows: Vec<Row>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    label: String,
    stamp: ListingStamp,
    revision: Option<[u8; 32]>,
    entry: Result<Entry, Problem>,
    original: Option<(String, [u8; 32])>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    region: String,
    name_key: String,
    run: String,
    automatic: bool,
    name: String,
    saved_at: u64,
    year: i32,
    month: u8,
    tick: u32,
    phase: PhaseDto,
    status: StatusDto,
    build: String,
    platform: String,
}
/// Only stable inspection errors are cached; I/O and unsupported variants are retried.
#[derive(Serialize, Deserialize)]
enum Problem {
    InvalidId,
    InvalidSaveName,
    Format(u32, u32),
    Checksum,
    TooLarge,
    Malformed(String),
    OtherVersion(String, String, String),
    LogFormat(u32, u32),
    Rules(u32, u32),
    InvalidSnapshot,
    RegionId,
    RegionFingerprint([u8; 32], [u8; 32]),
}
impl Problem {
    fn encode(e: &SaveError) -> Option<Self> {
        Some(match e {
            SaveError::InvalidId => Self::InvalidId,
            SaveError::InvalidSaveName => Self::InvalidSaveName,
            SaveError::SaveFormatVersionMismatch {
                recorded,
                supported,
            } => Self::Format(*recorded, *supported),
            SaveError::ChecksumMismatch => Self::Checksum,
            SaveError::SaveTooLarge => Self::TooLarge,
            SaveError::Malformed(s) => Self::Malformed(s.clone()),
            SaveError::OtherVersionUnreadable { recorded, cause } => Self::OtherVersion(
                recorded.build.clone(),
                recorded.platform.clone(),
                cause.clone(),
            ),
            SaveError::Restore(RestoreError::InvalidSnapshot) => Self::InvalidSnapshot,
            SaveError::Restore(RestoreError::RegionIdMismatch) => Self::RegionId,
            SaveError::Restore(RestoreError::Replay(ReplayError::LogFormatVersionMismatch {
                recorded,
                supported,
            })) => Self::LogFormat(*recorded, *supported),
            SaveError::Restore(RestoreError::Replay(ReplayError::RulesVersionMismatch {
                recorded,
                supported,
            })) => Self::Rules(*recorded, *supported),
            SaveError::Restore(RestoreError::Replay(ReplayError::RegionFingerprintMismatch {
                expected,
                actual,
            })) => Self::RegionFingerprint(*expected.as_bytes(), *actual.as_bytes()),
            _ => return None,
        })
    }
    fn decode(self) -> SaveError {
        match self {
            Self::InvalidId => SaveError::InvalidId,
            Self::InvalidSaveName => SaveError::InvalidSaveName,
            Self::Format(recorded, supported) => SaveError::SaveFormatVersionMismatch {
                recorded,
                supported,
            },
            Self::Checksum => SaveError::ChecksumMismatch,
            Self::TooLarge => SaveError::SaveTooLarge,
            Self::Malformed(s) => SaveError::Malformed(s),
            Self::OtherVersion(build, platform, cause) => SaveError::OtherVersionUnreadable {
                recorded: ReplayCompatibility { build, platform },
                cause,
            },
            Self::InvalidSnapshot => RestoreError::InvalidSnapshot.into(),
            Self::RegionId => RestoreError::RegionIdMismatch.into(),
            Self::LogFormat(recorded, supported) => {
                RestoreError::Replay(ReplayError::LogFormatVersionMismatch {
                    recorded,
                    supported,
                })
                .into()
            }
            Self::Rules(recorded, supported) => {
                RestoreError::Replay(ReplayError::RulesVersionMismatch {
                    recorded,
                    supported,
                })
                .into()
            }
            Self::RegionFingerprint(expected, actual) => {
                RestoreError::Replay(ReplayError::RegionFingerprintMismatch {
                    expected: RegionFingerprint::new(expected),
                    actual: RegionFingerprint::new(actual),
                })
                .into()
            }
        }
    }
}
impl Row {
    fn encode(c: &CachedListing) -> Option<Self> {
        let entry = match &c.row.entry {
            Ok(e) => Ok(Entry {
                region: e.region.as_str().into(),
                name_key: e.region_name_key.clone(),
                run: e.metadata.run.as_str().into(),
                automatic: e.metadata.kind == SaveKind::Automatic,
                name: e.metadata.name.clone(),
                saved_at: e.metadata.saved_at,
                year: e.summary.month.year(),
                month: e.summary.month.month(),
                tick: e.summary.tick,
                phase: match e.summary.phase {
                    DayPhase::Planning => PhaseDto::Planning,
                    DayPhase::Operating => PhaseDto::Operating,
                    DayPhase::Review => PhaseDto::Review,
                },
                status: match e.summary.status {
                    SessionStatus::Active => StatusDto::Active,
                    SessionStatus::GameOver(GameOverReason::Bankrupt) => StatusDto::Bankrupt,
                    SessionStatus::GameOver(GameOverReason::LicenseRevoked) => {
                        StatusDto::LicenseRevoked
                    }
                },
                build: e.compatibility.build.clone(),
                platform: e.compatibility.platform.clone(),
            }),
            Err(e) => Err(Problem::encode(e)?),
        };
        Some(Self {
            label: c.row.file_label.clone(),
            stamp: c.stamp.clone(),
            revision: c.row.revision.map(|r| r.0),
            entry,
            original: c
                .original
                .as_ref()
                .map(|(id, rev)| (id.as_str().into(), rev.0)),
        })
    }
    fn decode(self) -> Option<CachedListing> {
        // Never let a cache supply a path outside this library, even for unavailable cards.
        if self.label.is_empty()
            || self.label.contains(['/', '\\'])
            || matches!(self.label.as_str(), "." | "..")
        {
            return None;
        }
        let id = SaveId::new(self.label.clone()).ok();
        let revision = self.revision.map(SaveRevision);
        let entry = match self.entry {
            Ok(e) => {
                let metadata = SaveMetadata {
                    slot: id.clone()?,
                    run: SaveId::new(e.run).ok()?,
                    kind: if e.automatic {
                        SaveKind::Automatic
                    } else {
                        SaveKind::Manual
                    },
                    name: e.name,
                    saved_at: e.saved_at,
                };
                metadata.validate().ok()?;
                Ok(SaveEntry {
                    region: RegionId::new(&e.region),
                    region_name_key: e.name_key,
                    metadata,
                    summary: CheckpointSummary {
                        month: SimMonth::new(e.year, e.month).ok()?,
                        tick: e.tick,
                        phase: match e.phase {
                            PhaseDto::Planning => DayPhase::Planning,
                            PhaseDto::Operating => DayPhase::Operating,
                            PhaseDto::Review => DayPhase::Review,
                        },
                        status: match e.status {
                            StatusDto::Active => SessionStatus::Active,
                            StatusDto::Bankrupt => {
                                SessionStatus::GameOver(GameOverReason::Bankrupt)
                            }
                            StatusDto::LicenseRevoked => {
                                SessionStatus::GameOver(GameOverReason::LicenseRevoked)
                            }
                        },
                    },
                    revision: revision?,
                    compatibility: ReplayCompatibility {
                        build: e.build,
                        platform: e.platform,
                    },
                })
            }
            Err(e) => Err(e.decode()),
        };
        let original = match self.original {
            Some((id, rev)) => Some((SaveId::new(id).ok()?, SaveRevision(rev))),
            None => None,
        };
        let modified_at = self
            .stamp
            .modified
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        Some(CachedListing {
            stamp: self.stamp,
            row: SaveListing {
                id,
                file_label: self.label,
                modified_at,
                revision,
                entry,
            },
            original,
        })
    }
}
pub(super) fn read(
    root: &Path,
    running: &ReplayCompatibility,
) -> Option<BTreeMap<PathBuf, CachedListing>> {
    let bytes = read_bounded(&root.join(NAME), LIMIT).ok()?;
    let options = ron::Options::default().with_recursion_limit(32);
    let envelope: Envelope = options.from_bytes(&bytes).ok()?;
    if envelope.version != VERSION || envelope.checksum != digest(envelope.payload.as_bytes()) {
        return None;
    }
    let index: Index = options.from_str(&envelope.payload).ok()?;
    if index.build != running.build
        || index.platform != running.platform
        || index.rows.len() > MAX_ENTRIES
    {
        return None;
    }
    let mut rows = BTreeMap::new();
    for row in index.rows {
        let row = row.decode()?;
        let path = root.join(format!("{}.mgsave", row.row.file_label));
        if rows.insert(path, row).is_some() {
            return None;
        }
    }
    Some(rows)
}
pub(super) fn write(
    root: &Path,
    running: &ReplayCompatibility,
    rows: &BTreeMap<PathBuf, CachedListing>,
) {
    // Best effort, atomically replace a disposable hint. Never create the save directory here.
    let attempt = || -> Option<()> {
        if rows.len() > MAX_ENTRIES {
            return None;
        }
        let payload = ron::to_string(&Index {
            build: running.build.clone(),
            platform: running.platform.clone(),
            rows: rows.values().filter_map(Row::encode).collect(),
        })
        .ok()?;
        if payload.len() > LIMIT {
            return None;
        }
        let bytes = ron::to_string(&Envelope {
            version: VERSION,
            checksum: digest(payload.as_bytes()),
            payload,
        })
        .ok()?;
        if bytes.len() > LIMIT {
            return None;
        }
        let mut temp = tempfile::NamedTempFile::new_in(root).ok()?;
        temp.write_all(bytes.as_bytes()).ok()?;
        temp.persist(root.join(NAME)).ok()?;
        Some(())
    };
    let _ = attempt();
}
