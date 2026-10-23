//! Atomic directory-snapshot publication. Immutable chart files precede the manifest commit;
//! cancellation and pre-publication errors retain the previous complete manifest and history.
use super::{
    MAX_SAVE_BYTES,
    codec::digest,
    file_store::{FileSaveStore, PublishStage, io, make_directory, read_bounded},
    monthly::{self, EncodedArchive},
};
use application::*;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
impl FileSaveStore {
    pub(super) fn publish_directory(
        &self,
        bundle: &SaveBundle,
        archive: &EncodedArchive,
        target: &SaveTarget,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SaveReceipt, SaveError> {
        make_directory(&self.root, &mut self.pending())?;
        let _lock = self.lock_slot(&bundle.metadata.slot)?;
        let path = self.path(&bundle.metadata.slot);
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_dir() || m.is_symlink()) {
            return Err(SaveError::SlotChanged);
        }
        let current = if path.exists() {
            Some(SaveRevision(digest(&monthly::read_manifest(&path)?)))
        } else {
            None
        };
        match target {
            SaveTarget::Create if current.is_none() => {}
            SaveTarget::Replace(r) | SaveTarget::ConfirmDurability(r) if current == Some(*r) => {}
            _ => return Err(SaveError::SlotChanged),
        }
        let revision = SaveRevision(digest(&archive.manifest));
        if let SaveTarget::ConfirmDurability(r) = target {
            if *r != revision || monthly::verify_revision(&path)? != revision {
                return Err(SaveError::SlotChanged);
            }
            (self.fault)(PublishStage::SyncDirectory)
                .and_then(|()| {
                    (self.sync)(&path).map_err(|e| io("sync snapshot", e))?;
                    self.sync_directories().map_err(|e| io("sync directory", e))
                })
                .map_err(|_| SaveError::DurabilityUnconfirmed {
                    slot: bundle.metadata.slot.clone(),
                    revision,
                })?;
            self.clean_months(&path, archive);
            return Ok(SaveReceipt {
                metadata: bundle.metadata.clone(),
                revision,
            });
        }
        (self.fault)(PublishStage::CreateTemp)?;
        let staging = if current.is_none() {
            Some(tempfile::TempDir::new_in(&self.root).map_err(|e| io("temporary directory", e))?)
        } else {
            None
        };
        let work = staging.as_ref().map_or(path.as_path(), |d| d.path());
        let mut created = Vec::new();
        let mut published = false;
        let result = (|| {
            for (name, bytes) in &archive.files {
                if cancelled() {
                    return Err(SaveError::Cancelled);
                }
                let file = work.join(name);
                if file.exists() {
                    if fs::symlink_metadata(&file)
                        .map_err(|e| io("chart metadata", e))?
                        .is_symlink()
                        || read_bounded(&file, MAX_SAVE_BYTES)? != *bytes
                    {
                        return Err(SaveError::ChecksumMismatch);
                    }
                } else {
                    self.stage_file(work, &file, bytes)?;
                    created.push(file);
                }
            }
            let mut temp =
                tempfile::NamedTempFile::new_in(work).map_err(|e| io("temporary manifest", e))?;
            self.write_temp(&mut temp, &archive.manifest)?;
            (self.sync)(work).map_err(|e| io("sync charts", e))?;
            if cancelled() {
                return Err(SaveError::Cancelled);
            }
            (self.fault)(PublishStage::Publish)?;
            temp.persist(work.join(monthly::MANIFEST))
                .map_err(|e| io("publish manifest", e))?;
            if staging.is_some() {
                // Persist the final manifest name inside the staged directory before making
                // that directory visible under the slot name.
                (self.sync)(work).map_err(|e| io("sync staged manifest", e))?;
                fs::rename(work, &path).map_err(|e| io("publish snapshot", e))?;
            }
            published = true;
            (self.fault)(PublishStage::SyncDirectory)
                .and_then(|()| {
                    (self.sync)(&path).map_err(|e| io("sync snapshot", e))?;
                    self.sync_directories().map_err(|e| io("sync directory", e))
                })
                .map_err(|_| SaveError::DurabilityUnconfirmed {
                    slot: bundle.metadata.slot.clone(),
                    revision,
                })?;
            self.clean_months(&path, archive);
            Ok(SaveReceipt {
                metadata: bundle.metadata.clone(),
                revision,
            })
        })();
        if result.is_err() && !published {
            for file in created {
                let _ = fs::remove_file(file);
            }
        }
        result
    }
    fn write_temp(
        &self,
        temp: &mut tempfile::NamedTempFile,
        bytes: &[u8],
    ) -> Result<(), SaveError> {
        let half = bytes.len() / 2;
        temp.write_all(&bytes[..half]).map_err(|e| io("write", e))?;
        (self.fault)(PublishStage::PartialWrite)?;
        temp.write_all(&bytes[half..]).map_err(|e| io("write", e))?;
        (self.fault)(PublishStage::Flush)?;
        temp.flush().map_err(|e| io("flush", e))?;
        (self.fault)(PublishStage::SyncFile)?;
        temp.as_file().sync_all().map_err(|e| io("sync file", e))?;
        Ok(())
    }
    fn stage_file(&self, root: &Path, path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
        let mut temp =
            tempfile::NamedTempFile::new_in(root).map_err(|e| io("temporary chart", e))?;
        self.write_temp(&mut temp, bytes)?;
        temp.persist_noclobber(path)
            .map_err(|e| io("publish chart", e))?;
        Ok(())
    }
    fn clean_months(&self, path: &Path, archive: &EncodedArchive) {
        if let Ok(entries) = fs::read_dir(path) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".bin.z") && !archive.files.contains_key(&name) {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        let _ = (self.sync)(path);
    }
    fn deleted_path(&self, id: &SaveId, revision: SaveRevision) -> PathBuf {
        let hash = revision
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        self.root.join(format!(".deleted-{}-{hash}", id.as_str()))
    }
    pub(super) fn has_deleted_directory(&self, id: &SaveId, revision: SaveRevision) -> bool {
        self.deleted_path(id, revision).is_dir()
    }
    pub(super) fn delete_directory(
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
        let deleted = self.deleted_path(id, revision);
        if path.exists() {
            if fs::symlink_metadata(&path)
                .map_err(|e| io("snapshot metadata", e))?
                .is_symlink()
            {
                return Err(SaveError::SlotChanged);
            }
            let current = SaveRevision(digest(&monthly::read_manifest(&path)?));
            if current != revision {
                return Err(SaveError::SlotChanged);
            }
            (self.fault)(PublishStage::Publish)?;
            if cancelled() {
                return Err(SaveError::Cancelled);
            }
            fs::rename(&path, &deleted).map_err(|e| io("delete snapshot", e))?;
        }
        (self.fault)(PublishStage::SyncDirectory)
            .and_then(|()| self.sync_directories().map_err(|e| io("delete sync", e)))
            .map_err(|_| SaveError::DeleteDurabilityUnconfirmed { slot: id.clone() })?;
        if deleted.exists() {
            let _ = fs::remove_dir_all(&deleted);
        }
        Ok(())
    }
}
