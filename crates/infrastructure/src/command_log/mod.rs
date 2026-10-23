//! The command log on disk: one RON record per line, plus a byte-exact archive
//! of the region file beside it so a game can be replayed after the shipped
//! data has been retuned or removed.

pub mod dto;
mod file_log;

pub use file_log::{
    FileCommandLog, PreparedRecording, ReplayInput, ReplayLoadError, checkpoint_archive_path,
    load_replay, read_command_log, region_archive_path,
};
