use super::{
    MAX_SAVE_BYTES,
    codec::{digest, encode_save_as},
    decode_save, replay_compatibility,
};
use application::*;
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
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
/// Test seam: synchronises one directory. The store's default is [`sync_directory`]; tests inject
/// a recorder to see which directories a write synchronises.
#[doc(hidden)]
pub type DirectorySync = Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

pub struct FileSaveStore {
    root: PathBuf,
    compatibility: ReplayCompatibility,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    ids: Arc<dyn Fn() -> SaveId + Send + Sync>,
    fault: Fault,
    sync: DirectorySync,
    /// Directories that gained an entry when this store created a directory, and whose sync has
    /// not yet succeeded. A later write, Retry included, syncs them before it reports success.
    ///
    /// The set is per store instance and in memory: a restart loses it, and a second
    /// `FileSaveStore` on the same root doesn't inherit it. So directories created by an earlier
    /// process that crashed or quit may still be unsynced when a later process reports a durable
    /// receipt. That gap is negligible in practice, because filesystem journals commit within
    /// seconds.
    pending: Mutex<BTreeSet<PathBuf>>,
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
            sync: Arc::new(sync_directory),
            pending: Mutex::new(BTreeSet::new()),
        }
    }
    /// Test seam: inject a failure at a publication stage.
    #[doc(hidden)]
    pub fn with_fault(mut self, fault: Fault) -> Self {
        self.fault = fault;
        self
    }
    /// Test seam: replace the directory sync, so a test can see which directories a write
    /// synchronises.
    #[doc(hidden)]
    pub fn with_directory_sync(mut self, sync: DirectorySync) -> Self {
        self.sync = sync;
        self
    }
    fn path(&self, id: &SaveId) -> PathBuf {
        self.root.join(format!("{}.mgsave", id.as_str()))
    }
    fn bytes(&self, id: &SaveId) -> Result<Vec<u8>, SaveError> {
        read_bounded(&self.path(id), MAX_SAVE_BYTES)
    }
    fn lock_slot(&self, id: &SaveId) -> Result<File, SaveError> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(format!("{}.lock", id.as_str())))
            .map_err(|e| io("lock", e))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => SaveError::SlotBusy,
            std::fs::TryLockError::Error(error) => io("lock", error),
        })?;
        // Keep the lock inode after writes and deletes, so every window locks the same file.
        Ok(lock)
    }
    fn publish(
        &self,
        bundle: &SaveBundle,
        bytes: &[u8],
        target: &SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError> {
        {
            // Create under the lock, so a concurrent write on this store that finds the new
            // directories already there still finds them pending.
            make_directory(&self.root, &mut self.pending())?;
        }
        let _lock = self.lock_slot(&bundle.metadata.slot)?;
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
            if digest(bytes) != revision.0 {
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
            SaveRevision(digest(bytes))
        };
        let unconfirmed = || SaveError::DurabilityUnconfirmed {
            slot: bundle.metadata.slot.clone(),
            revision,
        };
        (self.fault)(PublishStage::SyncDirectory).map_err(|_| unconfirmed())?;
        self.sync_directories().map_err(|_| unconfirmed())?;
        Ok(SaveReceipt {
            metadata: bundle.metadata.clone(),
            revision,
        })
    }
    fn pending(&self) -> MutexGuard<'_, BTreeSet<PathBuf>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Sync the destination's parent, then every pending directory, deepest first. Each pending
    /// directory leaves the set only once its own sync succeeds.
    fn sync_directories(&self) -> std::io::Result<()> {
        (self.sync)(&self.root)?;
        let pending: Vec<PathBuf> = self.pending().iter().rev().cloned().collect();
        for dir in pending {
            (self.sync)(&dir)?;
            self.pending().remove(&dir);
        }
        Ok(())
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

/// Synchronise one directory, so the entries added to it survive a crash. A failed platform
/// operation is reported, never treated as durability.
#[cfg(not(windows))]
pub(crate) fn sync_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}
/// On Windows there is no directory sync, and this opens nothing.
///
/// On NTFS the rename is journaled, so a crash leaves either the previous or the new complete
/// file. The rename reaches disk when NTFS commits its log, so a receipt on Windows means the
/// contents were flushed and publication was atomic, not that the directory entry is synced.
/// Non-journaled volumes such as FAT32 or exFAT get neither guarantee. `FlushFileBuffers` on a
/// directory needs `GENERIC_WRITE`, which is why this path doesn't attempt it.
#[cfg(windows)]
pub(crate) fn sync_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
/// Create `root` and any missing ancestors. Adds to `gained` the directories that gained an
/// entry: the parent of each directory this call created. `root` itself is not added, as the
/// caller syncs the destination's parent on every publication.
///
/// A failed creation can still have made some directories (`a` of `a/<a name too long>`), so
/// those are added whatever `create_dir_all` returns: a later save must sync their parents too.
pub(crate) fn make_directory(root: &Path, gained: &mut BTreeSet<PathBuf>) -> Result<(), SaveError> {
    let mut missing = Vec::new();
    let mut at = root;
    while !at.exists() {
        missing.push(at);
        let parent = entry_parent(at);
        if parent == at {
            break;
        }
        at = parent;
    }
    let result = fs::create_dir_all(root).map_err(|e| io("directory", e));
    gained.extend(
        missing
            .into_iter()
            .filter(|dir| dir.exists())
            .map(|dir| entry_parent(dir).to_path_buf()),
    );
    result
}
/// The directory that holds `dir`'s entry: a bare relative name lives in the current directory.
fn entry_parent(dir: &Path) -> &Path {
    dir.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
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
            let mut revision = None;
            let item = id
                .as_ref()
                .ok_or(SaveError::InvalidId)
                .and_then(|id| self.bytes(id))
                .and_then(|bytes| {
                    revision = Some(SaveRevision(digest(&bytes)));
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
                        compatibility: bundle.compatibility,
                    })
                });
            result.push(SaveListing {
                id,
                file_label,
                modified_at,
                revision,
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
    fn revision(&self, id: &SaveId) -> Result<SaveRevision, SaveError> {
        self.bytes(id).map(|bytes| SaveRevision(digest(&bytes)))
    }
    fn read(&self, id: &SaveId) -> Result<(SaveBundle, LoadedRegion), SaveError> {
        let result = decode_save(&self.bytes(id)?, &self.compatibility)?;
        if &result.0.metadata.slot != id {
            return Err(SaveError::InvalidId);
        }
        Ok(result)
    }
    fn delete(
        &self,
        id: &SaveId,
        revision: SaveRevision,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), SaveError> {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let _lock = self.lock_slot(id)?;
        let path = self.path(id);
        match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_SAVE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| io("delete revision", e))?;
                if bytes.len() > MAX_SAVE_BYTES {
                    return Err(SaveError::SaveTooLarge);
                }
                if SaveRevision(digest(&bytes)) != revision {
                    return Err(SaveError::SlotChanged);
                }
                (self.fault)(PublishStage::Publish)?;
                if cancelled() {
                    return Err(SaveError::Cancelled);
                }
                fs::remove_file(&path).map_err(|e| io("delete", e))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io("delete revision", error)),
        }
        // Once removal is visible it cannot be cancelled; Retry re-confirms this absence but
        // never removes a recreated slot whose bytes differ from the confirmed revision.
        (self.fault)(PublishStage::SyncDirectory)
            .and_then(|()| {
                self.sync_directories()
                    .map_err(|error| io("delete sync", error))
            })
            .map_err(|_| SaveError::DeleteDurabilityUnconfirmed { slot: id.clone() })
    }
    fn rename(
        &self,
        id: &SaveId,
        revision: SaveRevision,
        name: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError> {
        if cancelled() {
            return Err(SaveError::Cancelled);
        }
        let original = self.bytes(id)?;
        if SaveRevision(digest(&original)) != revision {
            return Err(SaveError::SlotChanged);
        }
        let (mut bundle, _) = decode_save(&original, &self.compatibility)?;
        if &bundle.metadata.slot != id {
            return Err(SaveError::InvalidId);
        }
        bundle.metadata.name = name.into();
        // Renaming is metadata maintenance, not a save made by the running simulation build.
        let bytes = encode_save_as(&bundle, &bundle.compatibility)?;
        self.publish(&bundle, &bytes, &SaveTarget::Replace(revision), cancelled)
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
        // A store stamps only its own build, so a game resumed from another build's save saves
        // as this one. A bundle stamped elsewhere is a caller bug.
        debug_assert_eq!(
            bundle.compatibility, self.compatibility,
            "a store stamps only its own build"
        );
        let bytes = encode_save_as(bundle, &self.compatibility)?;
        self.publish(bundle, &bytes, target, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn make_directory_adds_the_parent_of_each_directory_it_created() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("a/b");
        let mut gained = BTreeSet::new();
        make_directory(&root, &mut gained).unwrap();
        assert_eq!(
            gained,
            BTreeSet::from([dir.path().join("a"), dir.path().to_path_buf()])
        );
        assert!(root.is_dir());
        let mut again = BTreeSet::new();
        make_directory(&root, &mut again).unwrap();
        assert!(again.is_empty(), "an existing root gains nothing");
    }

    #[test]
    fn make_directory_adds_what_a_failed_creation_left_behind() {
        let dir = TempDir::new().unwrap();
        let mut gained = BTreeSet::new();
        let root = dir.path().join("a").join("n".repeat(300));
        assert!(make_directory(&root, &mut gained).is_err());
        assert!(dir.path().join("a").is_dir());
        assert_eq!(gained, BTreeSet::from([dir.path().to_path_buf()]));
    }

    #[test]
    fn a_bare_relative_directory_has_its_entry_in_the_current_directory() {
        assert_eq!(entry_parent(Path::new("saves")), Path::new("."));
        assert_eq!(entry_parent(Path::new("saves/slot")), Path::new("saves"));
        assert_eq!(entry_parent(Path::new(".")), Path::new("."));
    }

    fn bundle(store: &FileSaveStore) -> SaveBundle {
        let catalog = crate::region::RonRegionCatalog::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"),
        );
        let game = Game::start_new(
            &catalog,
            &domain::ids::RegionId::new("eastern_interconnection"),
            3,
            domain::session::GameSettings::default(),
            Box::new(InMemoryCommandLog::default()),
        )
        .unwrap();
        SaveBundle {
            compatibility: store.compatibility(),
            metadata: SaveMetadata {
                slot: store.allocate_id(),
                run: store.allocate_id(),
                kind: SaveKind::Manual,
                name: "Partial".into(),
                saved_at: store.now(),
            },
            checkpoint: game.checkpoint(),
        }
    }

    /// The stamp is structural: what a store writes carries its own build, whatever the bundle
    /// says. (Debug builds also catch the mismatch; see `save_store.rs`.)
    #[test]
    fn a_store_encodes_with_its_own_stamp() {
        let dir = TempDir::new().unwrap();
        let store = FileSaveStore::new(dir.path().into());
        let mut b = bundle(&store);
        b.compatibility.build = "older".into();
        let bytes = super::super::codec::encode_save_as(&b, &store.compatibility).unwrap();
        let (decoded, _) = decode_save(&bytes, &store.compatibility).unwrap();
        assert_eq!(decoded.compatibility, store.compatibility);
    }

    #[test]
    fn a_directory_left_behind_by_a_failed_creation_stays_pending_until_a_save_syncs_it() {
        let dir = TempDir::new().unwrap();
        // create_dir_all makes `a`, then fails on a name longer than NAME_MAX.
        let mut store = FileSaveStore::new(dir.path().join("a").join("n".repeat(300)));
        let synced = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let record = synced.clone();
        store = store.with_directory_sync(Arc::new(move |d: &Path| {
            record.lock().unwrap().push(d.to_path_buf());
            Ok(())
        }));
        let b = bundle(&store);
        assert!(matches!(
            store.write(&b, &SaveTarget::Create, &|| false),
            Err(SaveError::Io {
                stage: "directory",
                ..
            })
        ));
        assert!(dir.path().join("a").is_dir(), "the failure left `a` behind");
        assert_eq!(
            *store.pending(),
            BTreeSet::from([dir.path().to_path_buf()]),
            "the entry for `a` is not yet durable"
        );
        // A transient failure clears on the next attempt; here the store moves to a root that
        // creates nothing new, so only the earlier failure can make the TempDir pending.
        store.root = dir.path().join("a");
        store.write(&b, &SaveTarget::Create, &|| false).unwrap();
        assert_eq!(
            *synced.lock().unwrap(),
            vec![dir.path().join("a"), dir.path().to_path_buf()]
        );
        assert!(store.pending().is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn sync_directory_opens_the_directory_and_reports_a_failure() {
        let dir = TempDir::new().unwrap();
        sync_directory(dir.path()).unwrap();
        assert!(sync_directory(&dir.path().join("missing")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn sync_directory_on_windows_returns_ok_without_opening_the_directory() {
        let dir = TempDir::new().unwrap();
        // A missing directory can't be opened, so Ok shows that nothing was opened.
        sync_directory(&dir.path().join("missing")).unwrap();
    }
}
