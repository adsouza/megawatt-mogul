//! The `game` crate's integration tests.
//!
//! They all live in this one test binary on purpose: every Bevy-linked test binary costs hundreds
//! of MB of disk and its own link step. Add a module under `tests/game/` rather than a new
//! top-level file in `tests/`. (A test crate root resolves `mod` paths from `tests/`, hence the
//! `#[path]` attributes.)

#[path = "game/battery.rs"]
mod battery;
#[path = "game/city_card.rs"]
mod city_card;
#[path = "game/cli.rs"]
mod cli;
#[path = "game/construction.rs"]
mod construction;
#[path = "game/contrast.rs"]
mod contrast;
#[path = "game/end_to_end.rs"]
mod end_to_end;
#[path = "game/hud.rs"]
mod hud;
#[path = "game/hud_smoke.rs"]
mod hud_smoke;
#[path = "game/l10n_coverage.rs"]
mod l10n_coverage;
#[path = "game/layout.rs"]
mod layout;
#[path = "game/line_cancellation.rs"]
mod line_cancellation;
#[path = "game/map_legend.rs"]
mod map_legend;
#[path = "game/operating_side.rs"]
mod operating_side;
#[path = "game/panel_resize.rs"]
mod panel_resize;
#[path = "game/phase_change.rs"]
mod phase_change;
#[path = "game/planning_performance.rs"]
mod planning_performance;
#[path = "game/player_workflows.rs"]
mod player_workflows;
#[path = "game/preferences.rs"]
mod preferences;
#[path = "game/region_picker.rs"]
mod region_picker;
#[path = "game/review_smoke.rs"]
mod review_smoke;
#[path = "game/save_resume.rs"]
mod save_resume;
#[path = "game/save_snapshots.rs"]
mod save_snapshots;
#[path = "game/shortfall_prompt.rs"]
mod shortfall_prompt;
#[path = "game/simulation.rs"]
mod simulation;
#[path = "game/states.rs"]
mod states;
#[path = "game/supply_chart.rs"]
mod supply_chart;
#[path = "game/support/mod.rs"]
mod support;

#[path = "game/world.rs"]
mod world;

#[path = "game/wind.rs"]
mod wind;

#[path = "game/currency.rs"]
mod currency;

#[path = "game/global_foundations.rs"]
mod global_foundations;
