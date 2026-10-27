//! Browser adapters. JavaScript handles stay inside local futures, outside ECS resources.
use crate::{
    backup::{MAX_BACKUP_BYTES, decode_backup, encode_backup},
    command_log::dto::LogHeaderDto,
    region::load_region_bytes,
    save::*,
};
use application::*;
use domain::{
    ids::RegionId,
    session::{DayPhase, GameOverReason, SessionStatus},
    state::State,
    time::SimMonth,
};
use js_sys::{Array, Uint8Array};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/browser_storage.js")]
extern "C" {
    #[wasm_bindgen(js_name=prepareCodec)]
    pub fn prepare_codec(build: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name=list)]
    fn db_list() -> js_sys::Promise;
    #[wasm_bindgen(js_name=read)]
    fn db_read(id: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name=revision)]
    fn db_revision(id: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name=mutate)]
    fn db_mutate(
        op: &str,
        id: &str,
        expected: JsValue,
        archive: JsValue,
        entry: JsValue,
    ) -> js_sys::Promise;
    #[wasm_bindgen(js_name=codec)]
    fn worker(op: &str, payload: JsValue) -> js_sys::Promise;
    #[wasm_bindgen(js_name=chooseBackup)]
    fn choose_backup(limit: usize) -> js_sys::Promise;
    #[wasm_bindgen(js_name=downloadBackup)]
    fn download_backup(bytes: &[u8], filename: &str);
    #[wasm_bindgen(js_name=randomId)]
    fn random_id() -> String;
    #[wasm_bindgen(js_name=preferenceRead)]
    fn pref_read() -> js_sys::Promise;
    #[wasm_bindgen(js_name=preferenceWrite)]
    fn pref_write(bytes: &[u8]) -> js_sys::Promise;
    #[wasm_bindgen(js_name=requestPersistence)]
    pub fn request_persistence() -> js_sys::Promise;
}
fn malformed(e: impl std::fmt::Display) -> SaveError {
    SaveError::Malformed(e.to_string())
}
fn field(value: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(value, &key.into()).ok()?.as_string()
}
fn restore_kind(error: &RestoreError) -> &'static str {
    match error {
        RestoreError::Cancelled => "Cancelled",
        RestoreError::RegionIdMismatch => "RegionId",
        RestoreError::RegionSourceMismatch => "RegionSource",
        RestoreError::StateHashMismatch => "StateHash",
        RestoreError::CommandCountMismatch => "CommandCount",
        RestoreError::SummaryMismatch => "Summary",
        _ => "Snapshot",
    }
}
fn restored_error(kind: &str) -> RestoreError {
    match kind {
        "Cancelled" => RestoreError::Cancelled,
        "RegionId" => RestoreError::RegionIdMismatch,
        "RegionSource" => RestoreError::RegionSourceMismatch,
        "StateHash" => RestoreError::StateHashMismatch,
        "CommandCount" => RestoreError::CommandCountMismatch,
        "Summary" => RestoreError::SummaryMismatch,
        _ => RestoreError::InvalidSnapshot,
    }
}
fn worker_error(error: SaveError) -> JsValue {
    let value = js_sys::Object::new();
    let kind = match &error {
        SaveError::InvalidId => "InvalidId",
        SaveError::InvalidSaveName => "InvalidName",
        SaveError::ChecksumMismatch => "Checksum",
        SaveError::SaveTooLarge => "Size",
        SaveError::Cancelled => "Cancelled",
        SaveError::Malformed(_) => "Malformed",
        SaveError::SaveFormatVersionMismatch {
            recorded,
            supported,
        } => {
            let _ = js_sys::Reflect::set(
                &value,
                &"recorded".into(),
                &JsValue::from_f64(*recorded as f64),
            );
            let _ = js_sys::Reflect::set(
                &value,
                &"supported".into(),
                &JsValue::from_f64(*supported as f64),
            );
            "Format"
        }
        SaveError::Restore(error) => restore_kind(error),
        SaveError::OtherVersionDiverged { recorded, error } => {
            let _ = js_sys::Reflect::set(&value, &"build".into(), &recorded.build.clone().into());
            let _ = js_sys::Reflect::set(
                &value,
                &"platform".into(),
                &recorded.platform.clone().into(),
            );
            let _ = js_sys::Reflect::set(&value, &"restore".into(), &restore_kind(error).into());
            "Foreign"
        }
        SaveError::OtherVersionUnreadable { recorded, .. } => {
            let _ = js_sys::Reflect::set(&value, &"build".into(), &recorded.build.clone().into());
            let _ = js_sys::Reflect::set(
                &value,
                &"platform".into(),
                &recorded.platform.clone().into(),
            );
            "Unreadable"
        }
        _ => "Browser",
    };
    let _ = js_sys::Reflect::set(&value, &"kind".into(), &kind.into());
    let _ = js_sys::Reflect::set(&value, &"message".into(), &error.to_string().into());
    value.into()
}
fn error(value: JsValue) -> SaveError {
    let message = js_sys::Reflect::get(&value, &"message".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| format!("{value:?}"));
    if let Some(kind) = field(&value, "kind") {
        let decoded = match kind.as_str() {
            "InvalidId" => Some(SaveError::InvalidId),
            "InvalidName" => Some(SaveError::InvalidSaveName),
            "Checksum" => Some(SaveError::ChecksumMismatch),
            "Size" => Some(SaveError::SaveTooLarge),
            "Cancelled" => Some(SaveError::Cancelled),
            "Malformed" => Some(SaveError::Malformed(message.clone())),
            "Format" => Some(SaveError::SaveFormatVersionMismatch {
                recorded: js_sys::Reflect::get(&value, &"recorded".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as u32,
                supported: js_sys::Reflect::get(&value, &"supported".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as u32,
            }),
            "Foreign" => Some(SaveError::OtherVersionDiverged {
                recorded: ReplayCompatibility {
                    build: field(&value, "build").unwrap_or_default(),
                    platform: field(&value, "platform").unwrap_or_default(),
                },
                error: restored_error(&field(&value, "restore").unwrap_or_default()),
            }),
            "Unreadable" => Some(SaveError::OtherVersionUnreadable {
                recorded: ReplayCompatibility {
                    build: field(&value, "build").unwrap_or_default(),
                    platform: field(&value, "platform").unwrap_or_default(),
                },
                cause: message.clone(),
            }),
            "Snapshot" | "RegionId" | "RegionSource" | "StateHash" | "CommandCount" | "Summary" => {
                Some(SaveError::Restore(restored_error(&kind)))
            }
            _ => None,
        };
        if let Some(error) = decoded {
            return error;
        }
    }
    if message == "Cancelled" {
        SaveError::Cancelled
    } else if message == "BackupTooLarge" {
        SaveError::SaveTooLarge
    } else if message == "SlotChanged" {
        SaveError::SlotChanged
    } else {
        SaveError::Io {
            stage: if message == "NewerSchema"
                || js_sys::Reflect::get(&value, &"name".into())
                    .ok()
                    .and_then(|v| v.as_string())
                    .as_deref()
                    == Some("VersionError")
            {
                "browser-version"
            } else if message == "DatabaseBlocked" {
                "browser-blocked"
            } else {
                "browser"
            },
            message,
        }
    }
}
async fn wait(promise: js_sys::Promise) -> Result<JsValue, SaveError> {
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(error)
}
fn get(value: &JsValue, name: &str) -> Result<JsValue, SaveError> {
    js_sys::Reflect::get(value, &name.into()).map_err(error)
}
fn text(value: JsValue) -> Result<String, SaveError> {
    value.as_string().ok_or_else(|| malformed("expected text"))
}
fn array(value: JsValue) -> Result<Array, SaveError> {
    value
        .dyn_into::<Array>()
        .map_err(|_| malformed("expected array"))
}
fn bytes(value: JsValue) -> Result<Vec<u8>, SaveError> {
    bounded_bytes(value, MAX_SAVE_BYTES)
}
fn bounded_bytes(value: JsValue, limit: usize) -> Result<Vec<u8>, SaveError> {
    let value = value
        .dyn_into::<Uint8Array>()
        .map_err(|_| malformed("expected bytes"))?;
    if value.length() as usize > limit {
        return Err(SaveError::SaveTooLarge);
    }
    Ok(value.to_vec())
}
fn hex(revision: SaveRevision) -> String {
    revision.0.iter().map(|b| format!("{b:02x}")).collect()
}
fn revision(value: JsValue) -> Result<SaveRevision, SaveError> {
    let value = text(value)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(malformed("revision length"));
    }
    let mut result = [0; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(malformed)?;
    }
    Ok(SaveRevision(result))
}
#[derive(Serialize, Deserialize)]
struct Card {
    slot: String,
    run: String,
    name: String,
    automatic: bool,
    saved_at: u64,
    region: String,
    name_key: String,
    year: i32,
    month: u8,
    tick: u32,
    phase: u8,
    status: u8,
    build: String,
    platform: String,
}
impl Card {
    fn of(bundle: &SaveBundle, name_key: String) -> Self {
        let summary = &bundle.checkpoint.summary;
        Self {
            slot: bundle.metadata.slot.as_str().into(),
            run: bundle.metadata.run.as_str().into(),
            name: bundle.metadata.name.clone(),
            automatic: bundle.metadata.kind == SaveKind::Automatic,
            saved_at: bundle.metadata.saved_at,
            region: bundle.checkpoint.header.region.as_str().into(),
            name_key,
            year: summary.month.year(),
            month: summary.month.month(),
            tick: summary.tick,
            phase: phase(summary.phase),
            status: status(summary.status),
            build: bundle.compatibility.build.clone(),
            platform: bundle.compatibility.platform.clone(),
        }
    }
    fn metadata(&self) -> Result<SaveMetadata, SaveError> {
        let value = SaveMetadata {
            slot: SaveId::new(self.slot.clone())?,
            run: SaveId::new(self.run.clone())?,
            kind: if self.automatic {
                SaveKind::Automatic
            } else {
                SaveKind::Manual
            },
            name: self.name.clone(),
            saved_at: self.saved_at,
        };
        value.validate()?;
        Ok(value)
    }
    fn summary(&self) -> Result<CheckpointSummary, SaveError> {
        Ok(CheckpointSummary {
            month: SimMonth::new(self.year, self.month).map_err(malformed)?,
            tick: self.tick,
            phase: match self.phase {
                0 => DayPhase::Planning,
                1 => DayPhase::Operating,
                2 => DayPhase::Review,
                _ => return Err(malformed("phase")),
            },
            status: match self.status {
                0 => SessionStatus::Active,
                1 => SessionStatus::GameOver(GameOverReason::Bankrupt),
                2 => SessionStatus::GameOver(GameOverReason::LicenseRevoked),
                _ => return Err(malformed("status")),
            },
        })
    }
    fn entry(self, revision: SaveRevision) -> Result<SaveEntry, SaveError> {
        Ok(SaveEntry {
            metadata: self.metadata()?,
            summary: self.summary()?,
            region: RegionId::new(&self.region),
            region_name_key: self.name_key,
            revision,
            compatibility: ReplayCompatibility {
                build: self.build,
                platform: self.platform,
            },
        })
    }
}
fn phase(value: DayPhase) -> u8 {
    match value {
        DayPhase::Planning => 0,
        DayPhase::Operating => 1,
        DayPhase::Review => 2,
    }
}
fn status(value: SessionStatus) -> u8 {
    match value {
        SessionStatus::Active => 0,
        SessionStatus::GameOver(GameOverReason::Bankrupt) => 1,
        SessionStatus::GameOver(GameOverReason::LicenseRevoked) => 2,
    }
}
type WireOrigin = (String, [u8; 32], u32, u64, u64);
type WireSnapshot = (u32, u64, Option<WireOrigin>);
#[derive(Serialize, Deserialize)]
struct WireMeta {
    card: Card,
    header: LogHeaderDto,
    count: u64,
    hash: u64,
    snapshot: Option<WireSnapshot>,
}
fn wire(bundle: &SaveBundle) -> Result<JsValue, SaveError> {
    let checkpoint = &bundle.checkpoint;
    let meta = WireMeta {
        card: Card::of(bundle, String::new()),
        header: (&checkpoint.header).into(),
        count: checkpoint.command_count,
        hash: checkpoint.state_hash,
        snapshot: checkpoint.snapshot.as_ref().map(|s| {
            (
                s.schema,
                s.journal_hash,
                s.origin.as_ref().map(|o| {
                    (
                        o.slot.as_str().into(),
                        o.revision.0,
                        o.rules,
                        o.state_hash,
                        o.command_count,
                    )
                }),
            )
        }),
    };
    let output = Array::new();
    output.push(&ron::to_string(&meta).map_err(malformed)?.into());
    for buffer in [
        checkpoint.region_source.clone(),
        checkpoint.commands.state_bytes(),
        checkpoint
            .snapshot
            .as_ref()
            .map(|s| s.session.clone())
            .unwrap_or_default(),
        checkpoint
            .snapshot
            .as_ref()
            .map(|s| s.projections.clone())
            .unwrap_or_default(),
    ] {
        output.push(&Uint8Array::from(buffer.as_slice()));
    }
    Ok(output.into())
}
fn unwire(value: JsValue) -> Result<SaveBundle, SaveError> {
    let a = array(value)?;
    if a.length() != 5 {
        return Err(malformed("wire shape"));
    }
    let meta: WireMeta = ron::Options::default()
        .with_recursion_limit(32)
        .from_str(&text(a.get(0))?)
        .map_err(malformed)?;
    let summary = meta.card.summary()?;
    let metadata = meta.card.metadata()?;
    let snapshot = meta
        .snapshot
        .map(|(schema, journal_hash, origin)| {
            Ok::<_, SaveError>(PortableState {
                schema,
                journal_hash,
                origin: origin
                    .map(|(slot, revision, rules, state_hash, command_count)| {
                        Ok::<_, SaveError>(MigrationOrigin {
                            slot: SaveId::new(slot)?,
                            revision: SaveRevision(revision),
                            rules,
                            state_hash,
                            command_count,
                        })
                    })
                    .transpose()?,
                session: bytes(a.get(3))?,
                projections: bytes(a.get(4))?,
            })
        })
        .transpose()?;
    Ok(SaveBundle {
        metadata,
        compatibility: ReplayCompatibility {
            build: meta.card.build,
            platform: meta.card.platform,
        },
        checkpoint: GameCheckpoint {
            header: meta.header.try_into().map_err(malformed)?,
            snapshot,
            region_source: bytes(a.get(1))?,
            commands: Vec::<Command>::from_state_bytes(&bytes(a.get(2))?).map_err(malformed)?,
            command_count: meta.count,
            state_hash: meta.hash,
            summary,
        },
    })
}
fn archive_js(archive: &EncodedArchive) -> Result<JsValue, SaveError> {
    let value = js_sys::Object::new();
    let files = Array::new();
    for (name, buffer) in &archive.files {
        let pair = Array::new();
        pair.push(&name.clone().into());
        pair.push(&Uint8Array::from(buffer.as_slice()));
        files.push(&pair);
    }
    js_sys::Reflect::set(
        &value,
        &"manifest".into(),
        &Uint8Array::from(archive.manifest.as_slice()),
    )
    .map_err(error)?;
    js_sys::Reflect::set(&value, &"files".into(), &files).map_err(error)?;
    Ok(value.into())
}
fn archive_from(value: &JsValue) -> Result<EncodedArchive, SaveError> {
    let manifest = bytes(get(value, "manifest")?)?;
    let rows = array(get(value, "files")?)?;
    if rows.length() > 4096 {
        return Err(SaveError::SaveTooLarge);
    }
    let mut files = std::collections::BTreeMap::new();
    let mut total = 0usize;
    for row in rows {
        let row = array(row)?;
        let name = text(row.get(0))?;
        let data = bytes(row.get(1))?;
        total = total
            .checked_add(data.len())
            .ok_or(SaveError::SaveTooLarge)?;
        if total > MAX_SAVE_BYTES || files.insert(name, data).is_some() {
            return Err(SaveError::SaveTooLarge);
        }
    }
    Ok(EncodedArchive { manifest, files })
}
fn encode_for_worker(
    bundle: &SaveBundle,
    compatibility: &ReplayCompatibility,
) -> Result<JsValue, SaveError> {
    let archive = encode_archive(bundle, compatibility)?;
    let card = Card::of(
        bundle,
        inspect_archive(&archive.manifest, compatibility)?.region_name_key,
    );
    let value = archive_js(&archive)?;
    js_sys::Reflect::set(
        &value,
        &"card".into(),
        &ron::to_string(&card).map_err(malformed)?.into(),
    )
    .map_err(error)?;
    Ok(value)
}
/// Worker entry point. Its WASM instance is disposable after a panic or decode failure.
#[wasm_bindgen]
pub fn browser_codec(operation: &str, payload: JsValue) -> Result<JsValue, JsValue> {
    codec(operation, payload).map_err(worker_error)
}
fn codec(operation: &str, payload: JsValue) -> Result<JsValue, SaveError> {
    match operation {
        "export" => {
            Ok(Uint8Array::from(encode_backup(&archive_from(&payload)?)?.as_slice()).into())
        }
        "import" => {
            let archive =
                decode_backup(&bounded_bytes(get(&payload, "bytes")?, MAX_BACKUP_BYTES)?)?;
            let (mut bundle, region) =
                decode_archive(&archive, &replay_compatibility(), &|| false)?;
            let game = bundle.clone().restore(
                region,
                &replay_compatibility(),
                Box::<InMemoryCommandLog>::default(),
                &|| false,
            )?;
            bundle.checkpoint = game.checkpoint();
            bundle.metadata.slot = SaveId::new(text(get(&payload, "slot")?)?)?;
            bundle.metadata.kind = SaveKind::Manual;
            encode_for_worker(&bundle, &replay_compatibility())
        }
        "encode" => encode_for_worker(&unwire(payload)?, &replay_compatibility()),
        "decode" => {
            let archive = archive_from(&payload)?;
            let (mut bundle, region) =
                decode_archive(&archive, &replay_compatibility(), &|| false)?;
            let game = bundle.clone().restore(
                region,
                &replay_compatibility(),
                Box::<InMemoryCommandLog>::default(),
                &|| false,
            )?;
            bundle.checkpoint = game.checkpoint();
            wire(&bundle)
        }
        "rename" => {
            let archive = archive_from(&get(&payload, "archive")?)?;
            let (mut bundle, _) = decode_archive(&archive, &replay_compatibility(), &|| false)?;
            bundle.metadata.name = text(get(&payload, "name")?)?;
            bundle.metadata.validate()?;
            encode_for_worker(&bundle, &bundle.compatibility)
        }
        _ => Err(malformed("unknown codec operation")),
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct BrowserSaveStore;
impl BrowserSaveStore {
    pub fn compatibility(&self) -> ReplayCompatibility {
        replay_compatibility()
    }
    pub fn allocate_id(&self) -> SaveId {
        SaveId::new(random_id()).expect("crypto UUID without separators")
    }
    pub fn now(&self) -> u64 {
        (js_sys::Date::now().max(0.0) / 1000.0) as u64
    }
    pub async fn export(&self, id: &SaveId, cancelled: &dyn Fn() -> bool) -> Result<(), SaveError> {
        let archive = wait(db_read(id.as_str())).await?;
        let bytes = bounded_bytes(wait(worker("export", archive)).await?, MAX_BACKUP_BYTES)?;
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        download_backup(&bytes, &format!("{}.mgsave.zip", id.as_str()));
        Ok(())
    }
    pub async fn import(&self, cancelled: &dyn Fn() -> bool) -> Result<SaveReceipt, SaveError> {
        let bytes = wait(choose_backup(MAX_BACKUP_BYTES)).await?;
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let slot = self.allocate_id();
        let payload = js_sys::Object::new();
        js_sys::Reflect::set(&payload, &"bytes".into(), &bytes).map_err(error)?;
        js_sys::Reflect::set(&payload, &"slot".into(), &slot.as_str().into()).map_err(error)?;
        let archive = wait(worker("import", payload.into())).await?;
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let card_source = get(&archive, "card")?;
        let card: Card = ron::Options::default()
            .with_recursion_limit(32)
            .from_str(&text(card_source.clone())?)
            .map_err(malformed)?;
        let metadata = card.metadata()?;
        let revision = revision(
            wait(db_mutate(
                "write",
                slot.as_str(),
                JsValue::NULL,
                archive,
                card_source,
            ))
            .await?,
        )?;
        Ok(SaveReceipt { metadata, revision })
    }
    pub async fn list(&self) -> Result<Vec<SaveListing>, SaveError> {
        let rows = array(wait(db_list()).await?)?;
        let mut output = Vec::new();
        for row in rows {
            let id = text(get(&row, "id")?)?;
            let rev = revision(get(&row, "revision")?);
            let entry = (|| {
                if get(&row, "schema")?.as_f64() != Some(1.0) {
                    return Err(SaveError::Io {
                        stage: "browser-version",
                        message: "newer card schema".into(),
                    });
                }
                let source = text(get(&row, "entry")?)?;
                if source.len() > 16 * 1024 {
                    return Err(SaveError::SaveTooLarge);
                }
                let card: Card = ron::Options::default()
                    .with_recursion_limit(32)
                    .from_str(&source)
                    .map_err(malformed)?;
                if card.slot != id {
                    return Err(SaveError::SlotChanged);
                }
                card.entry(rev.clone()?)
            })();
            output.push(SaveListing {
                id: SaveId::new(id.clone()).ok(),
                file_label: id,
                modified_at: entry.as_ref().map_or(0, |e| e.metadata.saved_at),
                revision: rev.ok(),
                entry,
            });
        }
        Ok(output)
    }
    pub async fn revision(&self, id: &SaveId) -> Result<SaveRevision, SaveError> {
        revision(wait(db_revision(id.as_str())).await?)
    }
    pub async fn read_cancellable(
        &self,
        id: &SaveId,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(SaveBundle, LoadedRegion), SaveError> {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let archive = wait(db_read(id.as_str())).await?;
        let bundle = unwire(wait(worker("decode", archive)).await?)?;
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let region = load_region_bytes(
            bundle.checkpoint.region_source.clone(),
            &bundle.checkpoint.header.region,
        )
        .map_err(malformed)?;
        Ok((bundle, region))
    }
    pub async fn write(
        &self,
        bundle: &SaveBundle,
        target: &SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError> {
        let archive = wait(worker("encode", wire(bundle)?)).await?;
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let card = get(&archive, "card")?;
        let expected = match target {
            SaveTarget::Create => JsValue::NULL,
            SaveTarget::Replace(r) | SaveTarget::ConfirmDurability(r) => hex(*r).into(),
        };
        let revision = revision(
            wait(db_mutate(
                "write",
                bundle.metadata.slot.as_str(),
                expected,
                archive,
                card,
            ))
            .await?,
        )?;
        Ok(SaveReceipt {
            metadata: bundle.metadata.clone(),
            revision,
        })
    }
    pub async fn rename(
        &self,
        id: &SaveId,
        expected: SaveRevision,
        name: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError> {
        let source = wait(db_read(id.as_str())).await?;
        let payload = js_sys::Object::new();
        js_sys::Reflect::set(&payload, &"archive".into(), &source).map_err(error)?;
        js_sys::Reflect::set(&payload, &"name".into(), &name.into()).map_err(error)?;
        let archive = wait(worker("rename", payload.into())).await?;
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let card_source = get(&archive, "card")?;
        let card: Card = ron::Options::default()
            .with_recursion_limit(32)
            .from_str(&text(card_source.clone())?)
            .map_err(malformed)?;
        let metadata = card.metadata()?;
        let revision = revision(
            wait(db_mutate(
                "write",
                id.as_str(),
                hex(expected).into(),
                archive,
                card_source,
            ))
            .await?,
        )?;
        Ok(SaveReceipt { metadata, revision })
    }
    pub async fn delete(
        &self,
        id: &SaveId,
        expected: SaveRevision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), SaveError> {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        wait(db_mutate(
            "delete",
            id.as_str(),
            hex(expected).into(),
            JsValue::NULL,
            JsValue::NULL,
        ))
        .await?;
        Ok(())
    }
}
pub async fn read_preferences() -> Result<crate::client_preferences::ClientPreferences, SaveError> {
    let value = wait(pref_read()).await?;
    if value.is_null() {
        Ok(Default::default())
    } else {
        crate::client_preferences::decode_preferences(&bytes(value)?)
    }
}
pub async fn write_preferences(
    value: &crate::client_preferences::ClientPreferences,
) -> Result<(), SaveError> {
    wait(pref_write(&crate::client_preferences::encode_preferences(
        value,
    )?))
    .await?;
    Ok(())
}
