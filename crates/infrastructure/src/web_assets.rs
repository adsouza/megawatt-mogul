//! Versioned deployment inventory shared by the native packager and browser loader.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetManifest {
    pub version: u32,
    pub build: String,
    pub assets: Vec<AssetFile>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetFile {
    pub name: String,
    pub url: String,
    pub length: usize,
    pub checksum: [u8; 32],
}
impl AssetFile {
    pub fn verifies(&self, bytes: &[u8]) -> bool {
        bytes.len() == self.length && <[u8; 32]>::from(Sha256::digest(bytes)) == self.checksum
    }
}
