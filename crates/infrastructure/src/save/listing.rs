//! Card metadata only. Unknown payload fields are skipped, never decoded into a game or terrain.
//! These cards are hints; selected saves always use the independent full reader and restore path.
use super::codec::{
    MAX_SAVE_BYTES, MAX_SAVE_COMMANDS, OriginDto, PhaseDto, StatusDto, digest, malformed,
    read_envelope,
};
use crate::command_log::dto::LogHeaderDto;
use application::*;
use domain::state::SUPPORTED_STATE_SCHEMAS;
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
    project(&fields(payload)?, None)
}

/// The card's fields from `source` in a fixed order, with `region`'s value replaced by
/// `region` when given.
fn project(source: &BTreeMap<&str, &str>, region: Option<&str>) -> Option<String> {
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
        } else if key == "region" {
            out.push_str(region.unwrap_or(value));
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
#[cfg_attr(test, derive(Debug, PartialEq))]
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
    let card = match fast_card(&envelope.payload) {
        Some(card) => card,
        None => full_card(&envelope.payload, running)?,
    };
    let recorded = ReplayCompatibility {
        build: card.build.clone(),
        platform: card.platform.clone(),
    };
    decode_card(card, envelope.version, revision)
        .map_err(|error| versioned_error(error, recorded, running))
}

fn options() -> ron::Options {
    ron::Options::default().with_recursion_limit(64)
}

/// The card, without RON reading the archived region one number at a time: RON spends about
/// 150 ns per element in debug builds, and a schema-4 region is 130-200 KB. When the payload is
/// canonical and its `region` is a plain decimal byte list, this decodes that list directly and
/// parses the rest of the projection with an empty one. `None` whenever it can't decide or the
/// rest doesn't parse, so `full_card` produces the exact same card or error as before.
fn fast_card(payload: &str) -> Option<Card> {
    let source = fields(payload)?;
    let region = canonical_bytes(source.get("region")?)?;
    let mut card: Card = options().from_str(&project(&source, Some("[]"))?).ok()?;
    card.region = region;
    Some(card)
}

/// A canonical RON byte list (`[0,12,255]`: no spaces, no trailing comma, no leading zeros),
/// decoded. `None` for anything else, even when RON would accept it.
fn canonical_bytes(list: &str) -> Option<Vec<u8>> {
    let inner = list.strip_prefix('[')?.strip_suffix(']')?.as_bytes();
    let mut out = Vec::with_capacity(inner.len() / 2 + 1);
    if inner.is_empty() {
        return Some(out);
    }
    // A plain loop: iterator adaptors and `str::parse` cost as much as RON in debug builds.
    let (mut value, mut digits) = (0u16, 0u8);
    for &b in inner {
        if b == b',' {
            out.push(u8::try_from(value).ok().filter(|_| digits > 0)?);
            (value, digits) = (0, 0);
        } else if b.is_ascii_digit() && digits < 3 && !(digits > 0 && value == 0) {
            value = value * 10 + u16::from(b - b'0');
            digits += 1;
        } else {
            return None;
        }
    }
    out.push(u8::try_from(value).ok().filter(|_| digits > 0)?);
    Some(out)
}

/// The card exactly as RON reads the projection (or the whole payload, when it isn't canonical).
fn full_card(payload: &str, running: &ReplayCompatibility) -> Result<Card, SaveError> {
    let projection = projected_card(payload);
    let source = projection.as_deref().unwrap_or(payload);
    options().from_str(source).map_err(|error| {
        let error = malformed(error);
        match options().from_str::<Stamp>(source) {
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
    })
}

/// The archived region's `id` and `name_key`. Reading them through `RegionCard` makes RON skip
/// every other value with `IgnoredAny`, at 10-30 µs a value: over 100 ms for a schema-4 region
/// in debug builds. A file that parses as a strict schema-4 region parses as a `RegionCard`
/// too, with the same two strings, and the typed parse takes a few milliseconds; anything else
/// takes the `RegionCard` parse, with its errors.
fn region_card(source: &[u8]) -> Result<RegionCard, SaveError> {
    if let Some(card) = schema4_region_card(source) {
        return Ok(card);
    }
    full_region_card(source)
}

fn schema4_region_card(source: &[u8]) -> Option<RegionCard> {
    // RON checks a struct name when the text gives one, so `RegionFileV4(...)` is a valid
    // schema-4 file but not a valid `RegionCard`. Leave any text mentioning it to RON.
    if source.windows(STRUCT_NAME.len()).any(|w| w == STRUCT_NAME) {
        return None;
    }
    let file: crate::region::RegionFileV4 = options().from_bytes(source).ok()?;
    Some(RegionCard {
        id: file.id,
        name_key: file.name_key,
    })
}

const STRUCT_NAME: &[u8] = b"RegionFileV4";

fn full_region_card(source: &[u8]) -> Result<RegionCard, SaveError> {
    options().from_bytes(source).map_err(malformed)
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
    if card
        .snapshot
        .as_ref()
        .is_some_and(|s| !SUPPORTED_STATE_SCHEMAS.contains(&s.schema))
    {
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
    let region = region_card(&card.region)?;
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
    fn support_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() && path.extension().is_none_or(|e| e != "mgsave") {
                support_files(&path, out);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    fn running() -> ReplayCompatibility {
        ReplayCompatibility {
            build: String::new(),
            platform: String::new(),
        }
    }
    fn manifest_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }
    fn shipped_regions() -> Vec<(String, Vec<u8>)> {
        let mut paths: Vec<_> = std::fs::read_dir(manifest_dir().join("../../assets/regions"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ron"))
            .collect();
        paths.sort();
        assert!(paths.len() >= 7, "{paths:?}");
        paths
            .into_iter()
            .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
            .collect()
    }
    /// Uncompressed save fixtures, as (name, envelope payload).
    fn fixture_payloads() -> Vec<(String, String)> {
        let mut paths = Vec::new();
        support_files(&manifest_dir().join("tests/support"), &mut paths);
        paths.sort();
        let payloads: Vec<_> = paths
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "mgsave"))
            .map(|p| {
                let envelope = read_envelope(&std::fs::read(&p).unwrap()).unwrap();
                (p.display().to_string(), envelope.payload)
            })
            .collect();
        assert!(payloads.len() >= 5, "{}", payloads.len());
        payloads
    }
    /// Region files and archived regions under `tests/support`.
    fn fixture_regions() -> Vec<(String, Vec<u8>)> {
        let mut paths = Vec::new();
        support_files(&manifest_dir().join("tests/support"), &mut paths);
        paths.sort();
        let mut regions: Vec<_> = paths
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "ron"))
            .map(|p| (p.display().to_string(), std::fs::read(&p).unwrap()))
            .filter(|(_, bytes)| bytes.windows(8).any(|w| w == b"name_key"))
            .collect();
        for (name, payload) in fixture_payloads() {
            let card = full_card(&payload, &running()).unwrap();
            regions.push((format!("{name} (archived)"), card.region));
        }
        assert!(regions.len() >= 10, "{}", regions.len());
        regions
    }
    fn minimal_v4() -> String {
        std::fs::read_to_string(manifest_dir().join("tests/support/minimal_region_v4.ron")).unwrap()
    }
    fn adversarial_regions() -> Vec<(String, Vec<u8>)> {
        let m = minimal_v4();
        let id = "    id: \"minimal\",\n";
        let name = "    name_key: \"region-minimal\",\n";
        assert!(m.contains(id) && m.contains(name));
        let open = "(\n    region_schema";
        let mut texts = vec![
            m.clone(),
            m.replace(
                id,
                "    /* id: \"decoy\", */ id: \"minimal\", // name_key: \"decoy\"\n",
            ),
            format!("// id: \"decoy\"\n/* (name_key: \"decoy\") */\n{m}"),
            m.replace(name, "    name_key: \"region-(min)) id: \\\"x\\\" (\",\n"),
            m.replace(id, "    id: \"a)b(c,d: \\\"e\",\n"),
            m.replace(id, "").replace(name, &format!("{name}{id}")),
            m.replace(id, ""),
            m.replace(name, ""),
            m.replace(id, &format!("{id}    id: \"other\",\n")),
            m.replace(name, &format!("{name}    name_key: \"other\",\n")),
            m.replace(id, &format!("{id}    extra: (id: \"x\"),\n")),
            m.replace(id, "    id: r\"minimal\",\n"),
            m.replace(id, "    id: r#\"min)\"mal\"#,\n"),
            m.replace(id, "    id: \"min\\u{69}mal\",\n"),
            m.replace(id, "    id: 7,\n"),
            m.replace(id, "    id: Some(\"minimal\"),\n"),
            m.replace(open, "RegionFileV4(\n    region_schema"),
            m.replace(open, "RegionCard(\n    region_schema"),
            m.replace(open, "Other(\n    region_schema"),
            format!("#![enable(implicit_some)]\n{m}"),
            format!("{m}\n// trailing comment\n"),
            format!("{m} x"),
            m.replace("region_schema: 4", "region_schema: 5"),
            m.trim_end().strip_suffix(')').unwrap().to_owned(),
            "(id:\"a\",name_key:\"b\")".into(),
            "(name_key:\"b\",id:\"a\")".into(),
            "(id:\"a\")".into(),
            "(name_key:\"b\")".into(),
            "()".into(),
            String::new(),
            "(id:\"a\",name_key:\"b\",id:\"c\")".into(),
            "(id:\"a\" /* , name_key:\"x\" */,name_key:\"b\")".into(),
        ];
        for number in [
            "inf", "-inf", "NaN", "1e400", "0x10", "1_000.0", "+0.07", ".5", "7", "0.07f64",
        ] {
            texts.push(m.replace("forecast_error: 0.07", &format!("forecast_error: {number}")));
        }
        let mut regions: Vec<_> = texts
            .into_iter()
            .enumerate()
            .map(|(n, t)| (format!("adversarial #{n}: {t:.200}"), t.into_bytes()))
            .collect();
        let mut invalid = vec![0xff];
        invalid.extend_from_slice(m.as_bytes());
        regions.push(("invalid UTF-8".into(), invalid));
        regions
    }
    #[test]
    fn region_identity_matches_the_full_region_card_parse() {
        let all = shipped_regions()
            .into_iter()
            .chain(fixture_regions())
            .chain(adversarial_regions());
        for (name, bytes) in all {
            assert_eq!(region_card(&bytes), full_region_card(&bytes), "{name}");
        }
    }
    /// Structural stand-in for a timing budget: every shipped region takes the typed path, never
    /// the `IgnoredAny` skip that made a cold save-library scan cost seconds.
    #[test]
    fn every_shipped_region_takes_the_schema4_identity_path() {
        for (name, bytes) in shipped_regions() {
            assert_eq!(
                schema4_region_card(&bytes),
                Some(full_region_card(&bytes).unwrap()),
                "{name}"
            );
        }
    }
    fn card_summary(card: &Card) -> String {
        format!(
            "{:?}",
            (
                card.snapshot
                    .as_ref()
                    .map(|s| (s.schema, s.origin.is_some())),
                (&card.build, &card.platform, &card.slot, &card.run),
                (card.automatic, &card.name, card.saved_at, &card.header),
                (&card.region, card.count, card.year, card.month, card.tick),
                std::mem::discriminant(&card.phase),
                std::mem::discriminant(&card.status),
            )
        )
    }
    fn either_card(payload: &str) -> Result<String, SaveError> {
        match fast_card(payload) {
            Some(card) => Ok(card_summary(&card)),
            None => full_card(payload, &running()).map(|c| card_summary(&c)),
        }
    }
    fn with_field(payload: &str, key: &str, value: Option<&str>) -> String {
        let source = fields(payload).unwrap();
        assert!(source.contains_key(key), "{key}");
        let parts: Vec<_> = source
            .iter()
            .filter_map(|(k, v)| {
                let v = if *k == key { value? } else { v };
                Some(format!("{k}:{v}"))
            })
            .collect();
        format!("({})", parts.join(","))
    }
    #[test]
    fn fast_card_matches_the_full_card_parse() {
        for (name, payload) in fixture_payloads() {
            assert!(fast_card(&payload).is_some(), "{name} takes the fast path");
            assert_eq!(
                either_card(&payload),
                full_card(&payload, &running()).map(|c| card_summary(&c)),
                "{name}"
            );
            // Only the card's fields, and a short region, keep the variants' fallback parses fast.
            let payload = with_field(
                &projected_card(&payload).unwrap(),
                "region",
                Some("[1,2,3]"),
            );
            let mut variants = vec![payload.clone()];
            for (key, value) in [
                ("region", Some("[ 1,2]")),
                ("region", Some("[1,2,]")),
                ("region", Some("[01]")),
                ("region", Some("[256]")),
                ("region", Some("[0x1]")),
                ("region", Some("[]")),
                ("region", Some("\"text\"")),
                ("region", None),
                ("month", Some("300")),
                ("count", Some("\"x\"")),
                ("name", Some("r\"raw\"")),
                ("phase", Some("Bogus")),
                ("build", None),
                ("header", Some("()")),
            ] {
                variants.push(with_field(&payload, key, value));
            }
            variants.push(payload.replacen("region:[", "region:[ ", 1));
            variants.push(payload.replacen("(", "(extra:[1,2],", 1));
            for variant in variants {
                assert_eq!(
                    either_card(&variant),
                    full_card(&variant, &running()).map(|c| card_summary(&c)),
                    "{name}: {variant:.300}"
                );
            }
        }
    }
    proptest! {
        #[test]
        fn canonical_byte_lists_decode_like_ron(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            let list = ron::to_string(&bytes).unwrap();
            prop_assert_eq!(canonical_bytes(&list), Some(bytes));
        }
        #[test]
        fn byte_list_decoding_never_disagrees_with_ron(list in "\\[[0-9, x_+\\-]{0,12}\\]?") {
            if let Some(bytes) = canonical_bytes(&list) {
                prop_assert_eq!(Some(bytes), ron::from_str::<Vec<u8>>(&list).ok());
            }
        }
    }
    #[test]
    fn noncanonical_byte_lists_are_left_to_ron() {
        for list in [
            "[ 1]", "[1 ]", "[1, 2]", "[1,]", "[,]", "[01]", "[00]", "[256]", "[1000]", "[0x1]",
            "[1_0]", "[+1]", "[-1]", "[1.0]", "(1)", "[1", "1]", "",
        ] {
            assert_eq!(canonical_bytes(list), None, "{list}");
        }
        assert_eq!(canonical_bytes("[]"), Some(vec![]));
        assert_eq!(
            canonical_bytes("[0,9,10,99,100,255]"),
            Some(vec![0, 9, 10, 99, 100, 255])
        );
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
