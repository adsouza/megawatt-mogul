//! Self-contained, verified saves and durable slot publication.
mod codec;
pub use codec::{MAX_SAVE_BYTES, MAX_SAVE_COMMANDS, SAVE_FORMAT_VERSION, decode_save, encode_save};

pub fn replay_compatibility() -> application::ReplayCompatibility {
    application::ReplayCompatibility {
        build: env!("SIM_BUILD_FINGERPRINT").into(),
        platform: env!("SIM_TARGET").into(),
    }
}

pub(crate) mod file_store;
pub use file_store::{DirectorySync, FileSaveStore, PublishStage};
