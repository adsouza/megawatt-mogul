#![forbid(unsafe_code)]

//! Adapters for the outside world: RON region files and the command log.

pub mod command_log;
pub mod region;

pub mod save;

pub mod client_preferences;

pub mod web_assets;

#[cfg(target_arch = "wasm32")]
pub mod browser;

pub mod backup;
