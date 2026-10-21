use super::{MAX_SAVE_BYTES, codec::digest, decode_save, encode_save, replay_compatibility};
use application::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishStage {
    CreateTemp,
    PartialWrite,
    Flush,
    SyncFile,
    Publish,
    SyncDirectory,
}
impl PublishStage {
    pub const ALL: [Self; 6] = [
        Self::CreateTemp,
        Self::PartialWrite,
        Self::Flush,
        Self::SyncFile,
        Self::Publish,
        Self::SyncDirectory,
    ];
}
type Fault = Arc<dyn Fn(PublishStage) -> Result<(), SaveError> + Send + Sync>;

pub struct FileSaveStore {
    root: PathBuf,
    compatibility: ReplayCompatibility,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    ids: Arc<dyn Fn() -> SaveId + Send + Sync>,
    fault: Fault,
}
impl FileSaveStore {
    pub fn new(root: PathBuf) -> Self {
        let counter = AtomicU64::new(0);
        Self::with_sources(
            root,
            replay_compatibility(),
            Arc::new(|| now_nanos() as u64 / 1_000_000_000),
            Arc::new(move || {
                let value = format!(
                    "{}:{}:{}",
                    std::process::id(),
                    now_nanos(),
                    counter.fetch_add(1, Ordering::Relaxed)
                );
                let id: String = digest(value.as_bytes())[..16]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                SaveId::new(id).expect("generated lowercase hex id")
            }),
        )
    }
    pub fn with_sources(
        root: PathBuf,
        compatibility: ReplayCompatibility,
        clock: Arc<dyn Fn() -> u64 + Send + Sync>,
        ids: Arc<dyn Fn() -> SaveId + Send + Sync>,
    ) -> Self {
        Self {
            root,
            compatibility,
            clock,
            ids,
            fault: Arc::new(|_| Ok(())),
        }
    }
    pub fn with_fault(mut self, fault: Fault) -> Self {
        self.fault = fault;
        self
    }
    fn path(&self, id: &SaveId) -> PathBuf {
        self.root.join(format!("{}.mgsave", id.as_str()))
    }
    fn bytes(&self, id: &SaveId) -> Result<Vec<u8>, SaveError> {
        read_bounded(&self.path(id), MAX_SAVE_BYTES)
    }
}
fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn io(stage: &'static str, error: impl std::fmt::Display) -> SaveError {
    SaveError::Io {
        stage,
        message: error.to_string(),
    }
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, SaveError> {
    let file = File::open(path).map_err(|e| io("read", e))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io("read", e))?;
    if bytes.len() > limit {
        return Err(SaveError::SaveTooLarge);
    }
    Ok(bytes)
}

/// Synchronise a directory, including newly created ancestors. On Windows, opening a directory
/// needs BACKUP_SEMANTICS. A failed platform operation is reported, never treated as durability.
pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let directory = {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(0x02000000)
            .open(path)?
    };
    #[cfg(not(windows))]
    let directory = File::open(path)?;
    directory.sync_all()
}
pub(crate) fn make_directory(root: &Path) -> Result<Vec<PathBuf>, SaveError> {
    let mut missing = Vec::new();
    let mut at = root;
    while !at.exists() {
        missing.push(at.to_path_buf());
        at = at
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
    }
    fs::create_dir_all(root).map_err(|e| io("directory", e))?;
    let mut sync = vec![root.to_path_buf()];
    for dir in missing {
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            sync.push(parent.to_path_buf());
        } else {
            sync.push(PathBuf::from("."));
        }
    }
    // Retry must also sync ancestors created by a previous uncertain publication.
    let mut parent = root.parent();
    while let Some(dir) = parent.filter(|p| !p.as_os_str().is_empty()) {
        sync.push(dir.to_path_buf());
        parent = dir.parent();
    }
    if root.is_relative() {
        sync.push(PathBuf::from("."));
    }
    sync.dedup();
    Ok(sync)
}

impl SaveStore for FileSaveStore {
    fn compatibility(&self) -> ReplayCompatibility {
        self.compatibility.clone()
    }
    fn allocate_id(&self) -> SaveId {
        (self.ids)()
    }
    fn now(&self) -> u64 {
        (self.clock)()
    }
    fn list(&self) -> Result<Vec<SaveListing>, SaveError> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io("list", e)),
        };
        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io("list", e))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("mgsave") {
                continue;
            }
            let file_label = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let id = SaveId::new(file_label.clone()).ok();
            let modified_at = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let item = id
                .as_ref()
                .ok_or(SaveError::InvalidId)
                .and_then(|id| self.bytes(id))
                .and_then(|bytes| {
                    let (bundle, loaded) = decode_save(&bytes, &self.compatibility)?;
                    if Some(&bundle.metadata.slot) != id.as_ref() {
                        return Err(SaveError::InvalidId);
                    }
                    Ok(SaveEntry {
                        region: bundle.checkpoint.header.region.clone(),
                        region_name_key: loaded.definition.name_key().into(),
                        metadata: bundle.metadata,
                        summary: bundle.checkpoint.summary,
                        revision: SaveRevision(digest(&bytes)),
                    })
                });
            result.push(SaveListing {
                id,
                file_label,
                modified_at,
                entry: item,
            });
        }
        result.sort_by(|a, b| {
            let time = |e: &SaveListing| {
                e.entry
                    .as_ref()
                    .map_or(e.modified_at, |e| e.metadata.saved_at)
            };
            time(b).cmp(&time(a)).then(a.file_label.cmp(&b.file_label))
        });
        Ok(result)
    }
    fn read(&self, id: &SaveId) -> Result<(SaveBundle, LoadedRegion), SaveError> {
        let result = decode_save(&self.bytes(id)?, &self.compatibility)?;
        if &result.0.metadata.slot != id {
            return Err(SaveError::InvalidId);
        }
        Ok(result)
    }
    fn write(
        &self,
        bundle: &SaveBundle,
        target: &SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError> {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        if bundle.compatibility != self.compatibility {
            return Err(SaveError::ReplayBuildMismatch);
        }
        let bytes = encode_save(bundle)?;
        let directories = make_directory(&self.root)?;
        let lock_path = self
            .root
            .join(format!("{}.lock", bundle.metadata.slot.as_str()));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|e| io("lock", e))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => SaveError::SlotBusy,
            std::fs::TryLockError::Error(error) => io("lock", error),
        })?;
        // The OS releases this lock even on process death. Keep its inode for future writers.
        let path = self.path(&bundle.metadata.slot);
        let current = match File::open(&path) {
            Ok(file) => {
                let mut data = Vec::new();
                file.take(MAX_SAVE_BYTES as u64 + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| io("revision", e))?;
                Some(SaveRevision(digest(&data)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io("revision", e)),
        };
        match target {
            SaveTarget::Create if current.is_none() => (),
            SaveTarget::Replace(expected) | SaveTarget::ConfirmDurability(expected)
                if Some(*expected) == current => {}
            _ => return Err(SaveError::SlotChanged),
        }
        let revision = if let SaveTarget::ConfirmDurability(revision) = target {
            if digest(&bytes) != revision.0 {
                return Err(SaveError::SlotChanged);
            }
            *revision
        } else {
            (self.fault)(PublishStage::CreateTemp)?;
            let mut temp =
                tempfile::NamedTempFile::new_in(&self.root).map_err(|e| io("temporary file", e))?;
            let half = bytes.len() / 2;
            temp.write_all(&bytes[..half]).map_err(|e| io("write", e))?;
            (self.fault)(PublishStage::PartialWrite)?;
            temp.write_all(&bytes[half..]).map_err(|e| io("write", e))?;
            (self.fault)(PublishStage::Flush)?;
            temp.flush().map_err(|e| io("flush", e))?;
            (self.fault)(PublishStage::SyncFile)?;
            temp.as_file().sync_all().map_err(|e| io("sync file", e))?;
            if cancelled() {
                return Err(SaveError::Cancelled);
            }
            (self.fault)(PublishStage::Publish)?;
            temp.persist(&path).map_err(|e| io("publish", e))?;
            SaveRevision(digest(&bytes))
        };
        let unconfirmed = || SaveError::DurabilityUnconfirmed {
            slot: bundle.metadata.slot.clone(),
            revision,
        };
        (self.fault)(PublishStage::SyncDirectory).map_err(|_| unconfirmed())?;
        for dir in directories {
            sync_directory(&dir).map_err(|_| unconfirmed())?;
        }
        Ok(SaveReceipt {
            metadata: bundle.metadata.clone(),
            revision,
        })
    }
}
