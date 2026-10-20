//! The `game` crate's integration tests.
//!
//! They all live in this one test binary on purpose: every Bevy-linked test binary costs hundreds
//! of MB of disk and its own link step. Add a module under `tests/game/` rather than a new
//! top-level file in `tests/`. (A test crate root resolves `mod` paths from `tests/`, hence the
//! `#[path]` attributes.)

#[path = "game/cli.rs"]
mod cli;
#[path = "game/hud.rs"]
mod hud;
#[path = "game/hud_smoke.rs"]
mod hud_smoke;
#[path = "game/l10n_coverage.rs"]
mod l10n_coverage;
#[path = "game/layout.rs"]
mod layout;
#[path = "game/simulation.rs"]
mod simulation;
#[path = "game/states.rs"]
mod states;
#[path = "game/support/mod.rs"]
mod support;
