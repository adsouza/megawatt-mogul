//! Client tutorial bookkeeping. Kept outside portable simulation state and replay hashes.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTutorial {
    pub version: u32,
    pub played: BTreeSet<String>,
    pub starter_batteries: BTreeSet<u32>,
    pub starter_generators: BTreeSet<u32>,
    pub supply_kind: String,
    pub seen_review: bool,
    pub outlook_used: bool,
    pub payoff_month: Option<(i32, u8)>,
}

impl SavedTutorial {
    pub fn encode(&self) -> Option<Vec<u8>> {
        ron::to_string(self).ok().map(String::into_bytes)
    }

    /// Invalid or future tutorial bookkeeping never prevents restoring a valid game.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 64 * 1024 {
            return None;
        }
        let saved: Self = ron::Options::default()
            .with_recursion_limit(16)
            .from_bytes(bytes)
            .ok()?;
        (saved.version == 1).then_some(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
        #[test]
        fn arbitrary_tutorial_bytes_are_safe_to_ignore(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..1024)) {
            let _ = SavedTutorial::decode(&bytes);
        }
    }
}
