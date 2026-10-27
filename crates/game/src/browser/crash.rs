//! Browser execution has no file-based crash context. Bevy reports panics to the console.
use bevy::prelude::Resource;
#[derive(Resource, Debug, Clone, Default)]
pub struct CrashContext;
impl CrashContext {
    pub fn set_game_log(&self, _: Option<std::path::PathBuf>) {}
}
