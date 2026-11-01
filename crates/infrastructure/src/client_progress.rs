//! Player progress kept beside the client preferences: whether the first-game tutorial is still
//! owed and which regions the player has played.
//!
//! This is a separate file from `preferences.ron` on purpose. Preferences are best-effort (a
//! malformed file just means defaults), but progress must never be silently reset: an unreadable
//! `progress.ron` is an error the caller has to surface, and the caller must not overwrite it.
use crate::client_preferences::{io, publish_atomic};
use crate::save::file_store::{read_bounded, sync_directory};
use application::SaveError;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

const MAX_BYTES: usize = 16 * 1024;
const MAX_REGIONS: usize = 256;
const MAX_REGION_ID_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientProgress {
    pub version: u32,
    /// The first-game tutorial has not yet been completed or dismissed.
    pub tutorial_pending: bool,
    /// The first-use city-demand preview walkthrough has not been completed.
    pub outlook_tutorial_pending: bool,
    /// Region ids the player has played.
    pub played_regions: BTreeSet<String>,
}
impl Default for ClientProgress {
    fn default() -> Self {
        Self {
            version: 1,
            tutorial_pending: true,
            outlook_tutorial_pending: true,
            played_regions: BTreeSet::new(),
        }
    }
}
impl ClientProgress {
    fn validate(&self) -> Result<(), SaveError> {
        if self.version != 1
            || self.played_regions.len() > MAX_REGIONS
            || self
                .played_regions
                .iter()
                .any(|id| id.is_empty() || id.len() > MAX_REGION_ID_BYTES)
        {
            return Err(SaveError::Malformed("client progress".into()));
        }
        Ok(())
    }
}

/// Read `progress.ron` under `root`. A missing file is the default; an unreadable or invalid one
/// is an error.
pub fn read_progress(root: &Path) -> Result<ClientProgress, SaveError> {
    let path = root.join("progress.ron");
    if let Err(e) = fs::metadata(&path) {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Ok(ClientProgress::default());
        }
        return Err(io("progress read", e));
    }
    decode_progress(&read_bounded(&path, MAX_BYTES)?)
}

/// Decode bounded progress bytes without any platform I/O.
pub fn decode_progress(bytes: &[u8]) -> Result<ClientProgress, SaveError> {
    if bytes.len() > MAX_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    let value: ClientProgress = ron::Options::default()
        .with_recursion_limit(32)
        .from_bytes(bytes)
        .map_err(|e| SaveError::Malformed(e.to_string()))?;
    value.validate()?;
    Ok(value)
}

/// Atomically write `progress.ron` under `root`, creating `root` if needed.
pub fn write_progress(root: &Path, value: &ClientProgress) -> Result<(), SaveError> {
    let bytes = encode_progress(value)?;
    publish_atomic(root, "progress.ron", &bytes, "progress", &sync_directory)
}

/// Encode only validated progress; native and browser share the exact representation.
pub fn encode_progress(value: &ClientProgress) -> Result<Vec<u8>, SaveError> {
    value.validate()?;
    let bytes = ron::to_string(value)
        .map_err(|e| SaveError::Malformed(e.to_string()))?
        .into_bytes();
    if bytes.len() > MAX_BYTES {
        return Err(SaveError::SaveTooLarge);
    }
    Ok(bytes)
}
