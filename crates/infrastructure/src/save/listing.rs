//! Card metadata only. Unknown payload fields are skipped, never decoded into a game or terrain.
//! These cards are hints; selected saves always use the independent full reader and restore path.
use super::codec::{
    MAX_SAVE_BYTES, MAX_SAVE_COMMANDS, OriginDto, PhaseDto, StatusDto, digest, malformed,
    read_envelope,
};
use crate::command_log::dto::LogHeaderDto;
use application::*;
use domain::{
    ids::RegionId,
    session::{DayPhase, GameOverReason, SessionStatus},
    time::SimMonth,
};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Fast path for our canonical RON writer: delimit field slices without interpreting millions
/// of numeric snapshot bytes. Strings are opaque and nesting is bounded. Noncanonical syntax
/// (comments, raw strings or chars) falls back to RON's normal metadata deserializer.
fn fields(text: &str) -> Option<BTreeMap<&str, &str>> {
    let text = text.trim();
    let inner = text.strip_prefix('(')?.strip_suffix(')')?;
    let bytes = inner.as_bytes();
    let mut at = 0;
    let mut result = BTreeMap::new();
    while at < bytes.len() {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let key_start = at;
        while bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            at += 1;
        }
        if at == key_start {
            return None;
        }
        let key = &inner[key_start..at];
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b':') {
            return None;
        }
        at += 1;
        let start = at;
        let mut stack = Vec::new();
        let mut quoted = false;
        while at < bytes.len() {
            let b = bytes[at];
            if quoted {
                match b {
                    b'\\' => {
                        at = at.checked_add(2)?;
                        continue;
                    }
                    b'"' => quoted = false,
                    _ => {}
                }
            } else {
                match b {
                    b'"' => {
                        if at > 0 && bytes[at - 1] == b'r' {
                            return None;
                        }
                        quoted = true;
                    }
                    b'\'' | b'/' | b'#' => return None,
                    b'(' | b'[' | b'{' => {
                        if stack.len() >= 64 {
                            return None;
                        }
                        stack.push(b);
                    }
                    b')' | b']' | b'}' => {
                        let open = stack.pop()?;
                        if !matches!((open, b), (b'(', b')') | (b'[', b']') | (b'{', b'}')) {
                            return None;
                        }
                    }
                    b',' if stack.is_empty() => break,
                    _ => {}
                }
            }
            at += 1;
        }
        if quoted || !stack.is_empty() || at > bytes.len() {
            return None;
        }
        let value = inner[start..at].trim();
        if value.is_empty() || result.insert(key, value).is_some() {
            return None;
        }
        if at < bytes.len() {
            at += 1;
        }
    }
    Some(result)
}

fn projected_card(payload: &str) -> Option<String> {
    let source = fields(payload)?;
    let mut out = String::from("(");
    for key in [
        "snapshot",
        "build",
        "platform",
        "slot",
        "run",
        "automatic",
        "name",
        "saved_at",
        "header",
        "region",
        "count",
        "year",
        "month",
        "tick",
        "phase",
        "status",
    ] {
        let Some(value) = source.get(key) else {
            continue;
        };
        out.push_str(key);
        out.push(':');
        if key == "snapshot" && *value != "None" {
            let snapshot = fields(value.strip_prefix("Some(")?.strip_suffix(')')?)?;
            out.push_str("Some((schema:");
            out.push_str(snapshot.get("schema")?);
            if let Some(origin) = snapshot.get("origin") {
                out.push_str(",origin:");
                out.push_str(origin);
            }
            out.push_str("))");
        } else {
            out.push_str(value);
        }
        out.push(',');
    }
    out.push(')');
    Some(out)
}

#[derive(Deserialize)]
struct Card {
    #[serde(default)]
    snapshot: Option<SnapshotCard>,
    build: String,
    platform: String,
    slot: String,
    run: String,
    automatic: bool,
    name: String,
    saved_at: u64,
    header: LogHeaderDto,
    region: Vec<u8>,
    count: u64,
    year: i32,
    month: u8,
    tick: u32,
    phase: PhaseDto,
    status: StatusDto,
}
#[derive(Deserialize)]
struct SnapshotCard {
    schema: u32,
    #[serde(default)]
    origin: Option<OriginDto>,
}
#[derive(Deserialize)]
struct RegionCard {
    id: String,
    name_key: String,
}
#[derive(Deserialize)]
struct Stamp {
    build: String,
    platform: String,
}

fn versioned_error(
    error: SaveError,
    recorded: ReplayCompatibility,
    running: &ReplayCompatibility,
) -> SaveError {
    match error {
        SaveError::Malformed(cause) if &recorded != running => {
            SaveError::OtherVersionUnreadable { recorded, cause }
        }
        error => error,
    }
}

pub(super) fn inspect(
    bytes: &[u8],
    revision: SaveRevision,
    running: &ReplayCompatibility,
) -> Result<(SaveEntry, Option<MigrationOrigin>), SaveError> {
    let envelope = read_envelope(bytes)?;
    let projection = projected_card(&envelope.payload);
    let source = projection.as_deref().unwrap_or(&envelope.payload);
    let card: Card = ron::Options::default()
        .with_recursion_limit(64)
        .from_str(source)
        .map_err(|error| {
            let error = malformed(error);
            match ron::Options::default()
                .with_recursion_limit(64)
                .from_str::<Stamp>(source)
            {
                Ok(stamp) => versioned_error(
                    error,
                    ReplayCompatibility {
                        build: stamp.build,
                        platform: stamp.platform,
                    },
                    running,
                ),
                Err(_) => error,
            }
        })?;
    let recorded = ReplayCompatibility {
        build: card.build.clone(),
        platform: card.platform.clone(),
    };
    decode_card(card, envelope.version, revision)
        .map_err(|error| versioned_error(error, recorded, running))
}

fn decode_card(
    card: Card,
    format: u32,
    revision: SaveRevision,
) -> Result<(SaveEntry, Option<MigrationOrigin>), SaveError> {
    if card.region.len() > MAX_SAVE_BYTES || card.count > MAX_SAVE_COMMANDS as u64 {
        return Err(SaveError::SaveTooLarge);
    }
    if (format == 1 && card.snapshot.is_some()) || (format == 2 && card.snapshot.is_none()) {
        return Err(malformed("save envelope and snapshot schema disagree"));
    }
    let header: LogHeader = card.header.try_into().map_err(malformed)?;
    if header.log_format_version != LOG_FORMAT_VERSION {
        return Err(RestoreError::Replay(ReplayError::LogFormatVersionMismatch {
            recorded: header.log_format_version,
            supported: LOG_FORMAT_VERSION,
        })
        .into());
    }
    if header.sim_rules_version != domain::SIM_RULES_VERSION
        && !(card.snapshot.is_some()
            && PortableState::rules_compatible(header.sim_rules_version, domain::SIM_RULES_VERSION))
    {
        return Err(RestoreError::Replay(ReplayError::RulesVersionMismatch {
            recorded: header.sim_rules_version,
            supported: domain::SIM_RULES_VERSION,
        })
        .into());
    }
    if card.snapshot.as_ref().is_some_and(|s| s.schema != 1) {
        return Err(RestoreError::InvalidSnapshot.into());
    }
    let actual = RegionFingerprint::new(digest(&card.region));
    if actual != header.region_fingerprint {
        return Err(
            RestoreError::Replay(ReplayError::RegionFingerprintMismatch {
                expected: header.region_fingerprint,
                actual,
            })
            .into(),
        );
    }
    let region: RegionCard = ron::Options::default()
        .with_recursion_limit(64)
        .from_bytes(&card.region)
        .map_err(malformed)?;
    if region.id != header.region.as_str() {
        return Err(RestoreError::RegionIdMismatch.into());
    }
    let metadata = SaveMetadata {
        slot: SaveId::new(card.slot)?,
        run: SaveId::new(card.run)?,
        kind: if card.automatic {
            SaveKind::Automatic
        } else {
            SaveKind::Manual
        },
        name: card.name,
        saved_at: card.saved_at,
    };
    metadata.validate()?;
    let origin = card
        .snapshot
        .and_then(|s| s.origin)
        .map(|o| o.decode())
        .transpose()?
        .filter(|o| o.command_count == card.count);
    Ok((
        SaveEntry {
            region: RegionId::new(&region.id),
            region_name_key: region.name_key,
            metadata,
            summary: CheckpointSummary {
                month: SimMonth::new(card.year, card.month).map_err(malformed)?,
                tick: card.tick,
                phase: match card.phase {
                    PhaseDto::Planning => DayPhase::Planning,
                    PhaseDto::Operating => DayPhase::Operating,
                    PhaseDto::Review => DayPhase::Review,
                },
                status: match card.status {
                    StatusDto::Active => SessionStatus::Active,
                    StatusDto::Bankrupt => SessionStatus::GameOver(GameOverReason::Bankrupt),
                    StatusDto::LicenseRevoked => {
                        SessionStatus::GameOver(GameOverReason::LicenseRevoked)
                    }
                },
            },
            revision,
            compatibility: ReplayCompatibility {
                build: card.build,
                platform: card.platform,
            },
        },
        origin,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[derive(Deserialize)]
    struct NameCount {
        name: String,
        count: u64,
    }
    proptest! {
        #[test]
        fn projection_retains_arbitrary_escaped_strings_and_ignores_nested_decoys(name in any::<String>()) {
            let quoted=ron::to_string(&name).unwrap();
            let text=format!("(snapshot:Some((schema:1,session:[1,2,3],projections:[4,5],origin:None)),commands:[Fake((name:\"decoy\",count:999))],name:{quoted},count:42)");
            let projected=projected_card(&text).unwrap();
            let decoded:NameCount=ron::from_str(&projected).unwrap();
            prop_assert_eq!(decoded.name,name);prop_assert_eq!(decoded.count,42);
            prop_assert!(!projected.contains("session:"));prop_assert!(!projected.contains("commands:"));
        }
        #[test]
        fn arbitrary_utf8_metadata_projection_never_panics(text in any::<String>()) { let _=projected_card(&text); }
    }
    #[test]
    fn malformed_and_noncanonical_fields_use_the_normal_ron_fallback() {
        for text in [
            "(name:\"a\",name:\"b\")",
            "(name:r\"raw\")",
            "(name:r#\"raw\"#)",
            "(name:'x')",
            "(name:1 /*comment*/)",
            "(name:[1,2))",
            "(name:\"unfinished)",
            "(name:((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((0)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))",
        ] {
            assert!(fields(text).is_none(), "{text}");
        }
    }
}
