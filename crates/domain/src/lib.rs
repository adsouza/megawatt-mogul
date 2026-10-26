#![forbid(unsafe_code)]

pub mod company;
pub mod demand;
pub mod error;
pub mod events;
pub mod generation;
pub mod grid;
pub mod ids;
pub mod region;
pub mod regulation;
pub mod rng;
pub mod session;
pub mod state;
pub mod time;
pub mod units;

/// Version of the simulation rules, recorded in every command-log header.
///
/// Bump it whenever a change alters outcomes for the same inputs: game rules,
/// RNG algorithms, numerical behaviour or the mapping from region data to a
/// definition. A replay refuses logs recorded under a different version.
pub const SIM_RULES_VERSION: u32 = 22;
