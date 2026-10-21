use crate::command_log::dto::{CommandDto, LogHeaderDto};
use crate::region::load_region_bytes;
use application::*;
use domain::ids::RegionId;
use domain::session::{DayPhase, GameOverReason, SessionStatus};
use domain::time::SimMonth;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SAVE_FORMAT_VERSION: u32 = 1;
pub const MAX_SAVE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SAVE_COMMANDS: usize = 1_000_000;

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
struct Payload {
    build: String,
    platform: String,
    slot: String,
    run: String,
    automatic: bool,
    name: String,
    saved_at: u64,
    header: LogHeaderDto,
    region: Vec<u8>,
    commands: Vec<CommandDto>,
    count: u64,
    state_hash: u64,
    year: i32,
    month: u8,
    tick: u32,
    phase: PhaseDto,
    status: StatusDto,
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
enum PhaseDto {
    Planning,
    Operating,
    Review,
}
#[derive(Serialize, Deserialize)]
enum StatusDto {
    Active,
    Bankrupt,
    LicenseRevoked,
}

pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn malformed(error: impl std::fmt::Display) -> SaveError {
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
        build: compatibility.build.clone(),
        platform: compatibility.platform.clone(),
        slot: m.slot.as_str().into(),
        run: m.run.as_str().into(),
        automatic: m.kind == SaveKind::Automatic,
        name: m.name.clone(),
        saved_at: m.saved_at,
        header: (&c.header).into(),
        region: c.region_source.clone(),
        commands: c.commands.iter().map(Into::into).collect(),
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
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    let options = ron::Options::default().with_recursion_limit(64);
    let e: Envelope = options.from_bytes(bytes).map_err(malformed)?;
    if e.version != SAVE_FORMAT_VERSION {
        return Err(SaveError::SaveFormatVersionMismatch {
            recorded: e.version,
            supported: SAVE_FORMAT_VERSION,
        });
    }
    if e.length != e.payload.len() || e.checksum != digest(e.payload.as_bytes()) {
        return Err(SaveError::ChecksumMismatch);
    }
    let p: Payload = match options.from_str(&e.payload) {
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
    let recorded = ReplayCompatibility {
        build: p.build.clone(),
        platform: p.platform.clone(),
    };
    decode_payload(p, recorded.clone()).map_err(|error| match error {
        SaveError::Malformed(cause) if &recorded != running => {
            SaveError::OtherVersionUnreadable { recorded, cause }
        }
        error => error,
    })
}

fn decode_payload(
    p: Payload,
    compatibility: ReplayCompatibility,
) -> Result<(SaveBundle, LoadedRegion), SaveError> {
    if p.commands.len() > MAX_SAVE_COMMANDS {
        return Err(SaveError::SaveTooLarge);
    }
    if p.count != p.commands.len() as u64 {
        return Err(RestoreError::CommandCountMismatch.into());
    }
    let header: LogHeader = p.header.try_into().map_err(malformed)?;
    if header.log_format_version != LOG_FORMAT_VERSION {
        return Err(RestoreError::Replay(ReplayError::LogFormatVersionMismatch {
            recorded: header.log_format_version,
            supported: LOG_FORMAT_VERSION,
        })
        .into());
    }
    if header.sim_rules_version != domain::SIM_RULES_VERSION {
        return Err(RestoreError::Replay(ReplayError::RulesVersionMismatch {
            recorded: header.sim_rules_version,
            supported: domain::SIM_RULES_VERSION,
        })
        .into());
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
        header,
        region_source: p.region,
        commands: p
            .commands
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<_, _>>()
            .map_err(malformed)?,
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
