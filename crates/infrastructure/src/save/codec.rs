use crate::command_log::dto::{CommandDto, LogHeaderDto};
use crate::region::load_region_bytes;
use application::*;
use domain::ids::RegionId;
use domain::session::{DayPhase, GameOverReason, SessionStatus};
use domain::state::SUPPORTED_STATE_SCHEMAS;
use domain::time::SimMonth;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SAVE_FORMAT_VERSION: u32 = 3;
pub const MAX_SAVE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SAVE_COMMANDS: usize = 1_000_000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    pub(super) version: u32,
    length: usize,
    checksum: [u8; 32],
    pub(super) payload: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Payload<C> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) snapshot: Option<SnapshotDto>,
    pub(super) build: String,
    pub(super) platform: String,
    pub(super) slot: String,
    pub(super) run: String,
    pub(super) automatic: bool,
    pub(super) name: String,
    pub(super) saved_at: u64,
    pub(super) header: LogHeaderDto,
    pub(super) region: Vec<u8>,
    pub(super) commands: Vec<C>,
    pub(super) count: u64,
    pub(super) state_hash: u64,
    pub(super) year: i32,
    pub(super) month: u8,
    pub(super) tick: u32,
    pub(super) phase: PhaseDto,
    pub(super) status: StatusDto,
}
/// Only the stamp of a [`Payload`], read when the full payload doesn't parse, so a save another
/// build wrote in DTO shapes this build doesn't know still names that build. It deliberately
/// allows unknown fields: RON is self-describing, so serde skips every other field's value,
/// whatever its shape, through `deserialize_ignored_any`. Only top-level keys match, so a `build`
/// nested inside a command can't be mistaken for the stamp. Payloads are written without RON
/// struct names; if that ever changes, this struct's name check would fail and such saves would
/// read as `Malformed`.
#[derive(Deserialize)]
struct Stamp {
    build: String,
    platform: String,
}
#[derive(Serialize, Deserialize)]
pub(super) enum PhaseDto {
    Planning,
    Operating,
    Review,
}
#[derive(Serialize, Deserialize)]
pub(super) enum StatusDto {
    Active,
    Bankrupt,
    LicenseRevoked,
}

pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub(super) fn malformed(error: impl std::fmt::Display) -> SaveError {
    SaveError::Malformed(error.to_string())
}

pub fn encode_save(bundle: &SaveBundle) -> Result<Vec<u8>, SaveError> {
    encode_save_as(bundle, &bundle.compatibility)
}

/// Encodes `bundle` stamped with `compatibility`, whatever the bundle's own stamp says: a store
/// stamps its own build.
pub(crate) fn encode_save_as(
    bundle: &SaveBundle,
    compatibility: &ReplayCompatibility,
) -> Result<Vec<u8>, SaveError> {
    bundle.metadata.validate()?;
    let c = &bundle.checkpoint;
    if c.commands.len() > MAX_SAVE_COMMANDS || c.region_source.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    if c.commands.len() as u64 != c.command_count {
        return Err(SaveError::Restore(RestoreError::CommandCountMismatch));
    }
    let m = &bundle.metadata;
    let payload = Payload {
        snapshot: c.snapshot.as_ref().map(SnapshotDto::encode).transpose()?,
        build: compatibility.build.clone(),
        platform: compatibility.platform.clone(),
        slot: m.slot.as_str().into(),
        run: m.run.as_str().into(),
        automatic: m.kind == SaveKind::Automatic,
        name: m.name.clone(),
        saved_at: m.saved_at,
        header: (&c.header).into(),
        region: c.region_source.clone(),
        commands: super::journal::encode(&c.commands),
        count: c.command_count,
        state_hash: c.state_hash,
        year: c.summary.month.year(),
        month: c.summary.month.month(),
        tick: c.summary.tick,
        phase: match c.summary.phase {
            DayPhase::Planning => PhaseDto::Planning,
            DayPhase::Operating => PhaseDto::Operating,
            DayPhase::Review => PhaseDto::Review,
        },
        status: match c.summary.status {
            SessionStatus::Active => StatusDto::Active,
            SessionStatus::GameOver(GameOverReason::Bankrupt) => StatusDto::Bankrupt,
            SessionStatus::GameOver(GameOverReason::LicenseRevoked) => StatusDto::LicenseRevoked,
        },
    };
    let payload = ron::to_string(&payload).map_err(malformed)?;
    if payload.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    let envelope = Envelope {
        version: SAVE_FORMAT_VERSION,
        length: payload.len(),
        checksum: digest(payload.as_bytes()),
        payload,
    };
    let bytes = ron::to_string(&envelope).map_err(malformed)?.into_bytes();
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    Ok(bytes)
}

/// Verifies a save's format, integrity, versions and embedded region, then decodes it. The
/// recorded build and platform come back in the bundle, for diagnostics: they are not a gate,
/// because replay verification (`SaveBundle::restore`) decides whether this build can reproduce
/// the save. They name the failure, though: a save from a build other than `running` that parses
/// but doesn't decode is `OtherVersionUnreadable`, not `Malformed`.
pub fn decode_save(
    bytes: &[u8],
    running: &ReplayCompatibility,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    decode_save_cancellable(bytes, running, &|| false)
}
pub fn decode_save_cancellable(
    bytes: &[u8],
    running: &ReplayCompatibility,
    cancelled: &dyn Fn() -> bool,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    decode_save_inner(bytes, running, cancelled)
}
pub(super) fn decode_preserving(
    bytes: &[u8],
    running: &ReplayCompatibility,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    decode_save_inner(bytes, running, &|| false)
}
pub(super) fn read_envelope(bytes: &[u8]) -> Result<Envelope, SaveError> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    let options = ron::Options::default().with_recursion_limit(64);
    let e: Envelope = options.from_bytes(bytes).map_err(malformed)?;
    if !(1..=SAVE_FORMAT_VERSION).contains(&e.version) {
        return Err(SaveError::SaveFormatVersionMismatch {
            recorded: e.version,
            supported: SAVE_FORMAT_VERSION,
        });
    }
    if e.length != e.payload.len() || e.checksum != digest(e.payload.as_bytes()) {
        return Err(SaveError::ChecksumMismatch);
    }
    Ok(e)
}

fn decode_save_inner(
    bytes: &[u8],
    running: &ReplayCompatibility,
    cancelled: &dyn Fn() -> bool,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    if cancelled() {
        return Err(SaveError::Cancelled);
    }
    let e = read_envelope(bytes)?;
    if e.version == 3 {
        decode_typed_payload::<super::journal::JournalRecord>(e, running, cancelled)
    } else {
        decode_typed_payload::<CommandDto>(e, running, cancelled)
    }
}
fn decode_typed_payload<C: for<'de> Deserialize<'de> + Into<super::journal::JournalRecord>>(
    e: Envelope,
    running: &ReplayCompatibility,
    cancelled: &dyn Fn() -> bool,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    let options = ron::Options::default().with_recursion_limit(64);
    let p: Payload<C> = match options.from_str(&e.payload) {
        Ok(p) => p,
        Err(error) => {
            return Err(match options.from_str::<Stamp>(&e.payload) {
                Ok(stamp) => {
                    let recorded = ReplayCompatibility {
                        build: stamp.build,
                        platform: stamp.platform,
                    };
                    if &recorded == running {
                        malformed(error)
                    } else {
                        SaveError::OtherVersionUnreadable {
                            recorded,
                            cause: error.to_string(),
                        }
                    }
                }
                Err(_) => malformed(error),
            });
        }
    };
    if p.commands.len() > MAX_SAVE_COMMANDS || p.region.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    if (e.version == 1 && p.snapshot.is_some()) || (e.version == 2 && p.snapshot.is_none()) {
        return Err(malformed("save envelope and snapshot schema disagree"));
    }
    let recorded = ReplayCompatibility {
        build: p.build.clone(),
        platform: p.platform.clone(),
    };
    decode_payload(p, recorded.clone(), cancelled).map_err(|error| match error {
        SaveError::Malformed(cause) if &recorded != running => {
            SaveError::OtherVersionUnreadable { recorded, cause }
        }
        error => error,
    })
}

fn decode_payload<C: Into<super::journal::JournalRecord>>(
    p: Payload<C>,
    compatibility: ReplayCompatibility,
    cancelled: &dyn Fn() -> bool,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    if p.count > MAX_SAVE_COMMANDS as u64 {
        return Err(SaveError::SaveTooLarge);
    }
    let header: LogHeader = p.header.try_into().map_err(malformed)?;
    if header.log_format_version != LOG_FORMAT_VERSION {
        return Err(RestoreError::Replay(ReplayError::LogFormatVersionMismatch {
            recorded: header.log_format_version,
            supported: LOG_FORMAT_VERSION,
        })
        .into());
    }
    let compatible_snapshot = p.snapshot.is_some()
        && PortableState::rules_compatible(header.sim_rules_version, domain::SIM_RULES_VERSION);
    if header.sim_rules_version != domain::SIM_RULES_VERSION && !compatible_snapshot {
        return Err(RestoreError::Replay(ReplayError::RulesVersionMismatch {
            recorded: header.sim_rules_version,
            supported: domain::SIM_RULES_VERSION,
        })
        .into());
    }
    // Metadata-only scans must reject unknown schemas too. Otherwise a future converted copy
    // could suppress the still-usable legacy original without being restorable by this build.
    if p.snapshot
        .as_ref()
        .is_some_and(|s| !SUPPORTED_STATE_SCHEMAS.contains(&s.schema))
    {
        return Err(SaveError::Restore(RestoreError::InvalidSnapshot));
    }
    let actual = RegionFingerprint::new(digest(&p.region));
    if actual != header.region_fingerprint {
        return Err(
            RestoreError::Replay(ReplayError::RegionFingerprintMismatch {
                expected: header.region_fingerprint,
                actual,
            })
            .into(),
        );
    }
    let commands = super::journal::expand(p.commands, p.count, cancelled)?;
    let loaded = load_region_bytes(p.region.clone(), &RegionId::new(header.region.as_str()))
        .map_err(malformed)?;
    let metadata = SaveMetadata {
        slot: SaveId::new(p.slot)?,
        run: SaveId::new(p.run)?,
        kind: if p.automatic {
            SaveKind::Automatic
        } else {
            SaveKind::Manual
        },
        name: p.name,
        saved_at: p.saved_at,
    };
    metadata.validate()?;
    let summary = CheckpointSummary {
        month: SimMonth::new(p.year, p.month).map_err(malformed)?,
        tick: p.tick,
        phase: match p.phase {
            PhaseDto::Planning => DayPhase::Planning,
            PhaseDto::Operating => DayPhase::Operating,
            PhaseDto::Review => DayPhase::Review,
        },
        status: match p.status {
            StatusDto::Active => SessionStatus::Active,
            StatusDto::Bankrupt => SessionStatus::GameOver(GameOverReason::Bankrupt),
            StatusDto::LicenseRevoked => SessionStatus::GameOver(GameOverReason::LicenseRevoked),
        },
    };
    let checkpoint = GameCheckpoint {
        snapshot: p.snapshot.map(SnapshotDto::decode).transpose()?,
        header,
        region_source: p.region,
        commands,
        command_count: p.count,
        state_hash: p.state_hash,
        summary,
    };
    Ok((
        SaveBundle {
            compatibility,
            metadata,
            checkpoint,
        },
        loaded,
    ))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OriginDto {
    slot: String,
    revision: [u8; 32],
    rules: u32,
    state_hash: u64,
    command_count: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotDto {
    schema: u32,
    session: Vec<u8>,
    projections: Vec<u8>,
    journal_hash: u64,
    origin: Option<OriginDto>,
}
impl SnapshotDto {
    fn encode(state: &PortableState) -> Result<Self, SaveError> {
        use std::io::Write;
        fn compress(bytes: &[u8]) -> Result<Vec<u8>, SaveError> {
            if bytes.len() > MAX_SAVE_BYTES {
                return Err(SaveError::SaveTooLarge);
            }
            let mut out = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            out.write_all(bytes).map_err(malformed)?;
            out.finish().map_err(malformed)
        }
        Ok(Self {
            schema: state.schema,
            session: compress(&state.session)?,
            projections: compress(&state.projections)?,
            journal_hash: state.journal_hash,
            origin: state.origin.as_ref().map(|o| OriginDto {
                slot: o.slot.as_str().into(),
                revision: o.revision.0,
                rules: o.rules,
                state_hash: o.state_hash,
                command_count: o.command_count,
            }),
        })
    }
    fn decode(self) -> Result<PortableState, SaveError> {
        use std::io::Read;
        fn decompress(bytes: &[u8]) -> Result<Vec<u8>, SaveError> {
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
        if !SUPPORTED_STATE_SCHEMAS.contains(&self.schema) {
            return Err(SaveError::Restore(RestoreError::InvalidSnapshot));
        }
        Ok(PortableState {
            schema: self.schema,
            session: decompress(&self.session)?,
            projections: decompress(&self.projections)?,
            journal_hash: self.journal_hash,
            origin: self
                .origin
                .map(|o| {
                    Ok::<_, SaveError>(MigrationOrigin {
                        slot: SaveId::new(o.slot)?,
                        revision: SaveRevision(o.revision),
                        rules: o.rules,
                        state_hash: o.state_hash,
                        command_count: o.command_count,
                    })
                })
                .transpose()?,
        })
    }
}

impl OriginDto {
    pub(super) fn decode(&self) -> Result<MigrationOrigin, SaveError> {
        Ok(MigrationOrigin {
            slot: SaveId::new(self.slot.clone())?,
            revision: SaveRevision(self.revision),
            rules: self.rules,
            state_hash: self.state_hash,
            command_count: self.command_count,
        })
    }
}
