//! Self-contained, verified saves and durable slot publication.
mod codec;
mod directory;
mod journal;
mod listing;
mod listing_cache;
pub(crate) mod monthly;
pub use codec::{
    MAX_SAVE_BYTES, MAX_SAVE_COMMANDS, SAVE_FORMAT_VERSION, decode_save, decode_save_cancellable,
    encode_save,
};

pub fn replay_compatibility() -> application::ReplayCompatibility {
    application::ReplayCompatibility {
        build: env!("SIM_BUILD_FINGERPRINT").into(),
        platform: env!("SIM_TARGET").into(),
    }
}

pub(crate) mod file_store;
pub use file_store::{DirectorySync, FileSaveStore, PublishStage};

pub use monthly::{
    EncodedArchive, MAX_PROJECTION_BYTES, decode_archive, encode as encode_archive, inspect_archive,
};

pub use monthly::read_archive_directory;
