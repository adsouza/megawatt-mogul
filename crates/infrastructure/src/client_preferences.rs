//! Automatically retained client preferences; independent of command checkpoints.
use crate::save::file_store::{make_directory, read_bounded, sync_directory};
use application::SaveError;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceWindow {
    pub width: f32,
    pub height: f32,
    pub x: i32,
    pub y: i32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientPreferences {
    pub version: u32,
    pub window: Option<PreferenceWindow>,
    pub zoom: f32,
    pub speed: u32,
    /// egui panel IDs mapped to outer sizes in logical points.
    pub panels: BTreeMap<String, [f32; 2]>,
    /// Movable tool IDs mapped to position and size in logical points.
    pub tools: BTreeMap<String, [f32; 4]>,
}
impl Default for ClientPreferences {
    fn default() -> Self {
        Self {
            version: 1,
            window: None,
            zoom: 1.0,
            speed: 1,
            panels: BTreeMap::new(),
            tools: BTreeMap::new(),
        }
    }
}
impl ClientPreferences {
    fn validate(&self) -> Result<(), SaveError> {
        let finite = |v: &f32| v.is_finite() && v.abs() <= 100_000.0;
        if self.version != 1
            || !(0.2..=5.0).contains(&self.zoom)
            || ![1, 2, 4, 8].contains(&self.speed)
            || self.panels.len() > 32
            || self.tools.len() > 32
            || self
                .panels
                .values()
                .any(|v| !v.iter().all(finite) || v.iter().any(|x| *x <= 0.0))
            || self
                .tools
                .values()
                .any(|v| !v.iter().all(finite) || v[2] <= 0.0 || v[3] <= 0.0)
            || self.window.as_ref().is_some_and(|w| {
                ![w.width, w.height].iter().all(finite) || w.width < 64.0 || w.height < 64.0
            })
        {
            return Err(SaveError::Malformed("client preferences".into()));
        }
        Ok(())
    }
}
fn io(stage: &'static str, error: impl std::fmt::Display) -> SaveError {
    SaveError::Io {
        stage,
        message: error.to_string(),
    }
}
pub fn read_preferences(root: &Path) -> Result<ClientPreferences, SaveError> {
    let path = root.join("preferences.ron");
    if let Err(e) = fs::metadata(&path) {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Ok(ClientPreferences::default());
        }
        return Err(io("preferences read", e));
    }
    let bytes = read_bounded(&path, 128 * 1024)?;
    let value: ClientPreferences = ron::Options::default()
        .with_recursion_limit(32)
        .from_bytes(&bytes)
        .map_err(|e| SaveError::Malformed(e.to_string()))?;
    value.validate()?;
    Ok(value)
}
pub fn write_preferences(root: &Path, preferences: &ClientPreferences) -> Result<(), SaveError> {
    write_preferences_with(root, preferences, &sync_directory)
}
/// Test seam: [`write_preferences`] with an injected directory sync, so a test can see what it
/// synchronises.
///
/// Preferences are best-effort (§17.1: missing preferences mean defaults, and malformed ones don't
/// block boot), so unlike the save store there is no pending set and no retry contract.
#[doc(hidden)]
pub fn write_preferences_with(
    root: &Path,
    preferences: &ClientPreferences,
    sync: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Result<(), SaveError> {
    preferences.validate()?;
    let bytes = ron::to_string(preferences).map_err(|e| SaveError::Malformed(e.to_string()))?;
    if bytes.len() > 128 * 1024 {
        return Err(SaveError::SaveTooLarge);
    }
    let mut gained = BTreeSet::new();
    make_directory(root, &mut gained)?;
    let mut file =
        tempfile::NamedTempFile::new_in(root).map_err(|e| io("preferences temporary file", e))?;
    file.write_all(bytes.as_bytes())
        .map_err(|e| io("preferences write", e))?;
    file.flush()
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| io("preferences sync", e))?;
    file.persist(root.join("preferences.ron"))
        .map_err(|e| io("preferences publish", e))?;
    // The file's own directory, then each directory that gained an entry because this call
    // created a directory in it.
    std::iter::once(root)
        .chain(gained.iter().rev().map(PathBuf::as_path))
        .try_for_each(sync)
        .map_err(|e| io("preferences directory sync", e))?;
    Ok(())
}
