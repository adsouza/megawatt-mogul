# Day-ahead Weather and Automatic Commitment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw a day-ahead weather forecast when Planning begins. Then, by default, trim expensive thermal commitments to that forecast plus a cushion at the start of every Planning phase, and open Planning on the Forecast tab.

**Architecture:**
- **Part A** (Tasks 1–7) is a domain rules change. Region data gains `surprise_share`. `PowerSystem` gains a persisted `WeatherForecast`, drawn at `GameSession::new` and `finish_review`. The actual weather becomes forecast plus surprise, and every "expected weather" reader switches to the forecast.
- **Part B** (Tasks 8–12) adds a pure planner query, `GameSession::automatic_commitment_plan`. It evaluates trial clones of the session. An application helper turns the plan into `CommitPlant` commands, which the game sends on entering Planning, behind a client preference and a checkbox.

**Tech Stack:** Rust 1.98, Bevy 0.20, bevy_egui 0.43, proptest, insta, cargo-nextest.

**Spec:** `docs/superpowers/specs/2026-10-31-automatic-commitment-design.md` (sections are cited as §n).

## Global Constraints

- `domain` depends only on `rand_chacha` and `thiserror`. No Bevy, serde, I/O or tracing.
- Determinism: `BTreeMap`/`BTreeSet` only, all randomness via `SimRng`, ties broken by ID.
- No user-facing strings in `domain` or `application`. New text goes in `assets/locales/en-US/ui.ftl`, and its key goes in `crates/game/src/l10n/keys.rs`.
- Tuning lives only in region RON data and `test-fixtures`, never in constants: `surprise_share`, `forecast_error`, `clearness_sd` and the wind parameters.
- Thresholds derived from f64 state compare with `1e-9` tolerance.
- Game integration tests are modules under `crates/game/tests/game/`, registered in `crates/game/tests/game.rs`. Never add a new file in `tests/`.
- HUD: a pure presenter model, a draw function returning `Vec<UiAction>`, and `dispatch` as the only place a click becomes a message or a resource write.
- Version numbers:
  - `SIM_RULES_VERSION` 31 → 32;
  - `CURRENT_STATE_SCHEMA` 8 → 9;
  - `CURRENT_REGION_SCHEMA` 5 → 6.
- Surprise share is `0.5` in every shipped region and in `test-fixtures`, and `1.0` for region schema ≤ 5.
- Keep `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` green at every commit. Use conventional commits, ending with:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`

## Review Focus

1. **Loading a save made in Planning before this change.** It must restore, show the month's mean weather (neutral forecast), and play on. *Task 5.*
2. **SGCC (automatic HVDC).** Trimming one island must not leave the other uncovered once transfers are re-planned. *Task 9: `linked_islands_stay_covered`.*
3. **A player who disabled AGC on a coal plant, or switched nuclear off.** The planner must not touch those patterns. *Task 9: `leaves_manual_and_fixed_kinds_alone`.*
4. **Clicking the checkbox off and on repeatedly during Planning.** Each "on" re-plans exactly once, and "off" changes nothing. *Task 11: `toggling_on_replans_once_and_off_changes_nothing`.*
5. **An open unapplied draft in the grid when the planner runs.** The draft is discarded, and *Start the day* is not blocked by a stale draft. *Task 12: `planner_discards_an_open_draft`.*

---

## Part A — day-ahead weather

### Task 1: `surprise_share` region data (region schema 6)

**Files:**
- Modify: `crates/domain/src/region/params.rs` (add `WeatherParams`)
- Modify: `crates/domain/src/region/mod.rs` (`RegionDefinitionParts.weather`, `RegionDefinition::weather()`, validation in `validate_shapes` ~486)
- Modify: `crates/domain/src/region/fixtures.rs`. Set `weather: WeatherParams { surprise_share: 0.5 }` in `tiny_region_parts`. Every other builder derives from it or sets the field.
- Modify: `crates/domain/src/demand/mod.rs:144` (the other parts constructor)
- Modify: `crates/infrastructure/src/region/dto.rs`. Add `WeatherDto { surprise_share: f64 }` (`deny_unknown_fields`) and an optional `weather: Option<WeatherDto>` on `RegionFileV4` using the `present_option` pattern of `geothermal` (397–402). **Do not touch `WindDto`**: the frozen wind payloads deserialize it.
- Modify: `crates/infrastructure/src/region/mod.rs:24` (`CURRENT_REGION_SCHEMA = 6`)
- Modify: `crates/infrastructure/src/region/catalog.rs:154,163` (`4 | 5` → `4..=6`)
- Modify: `crates/infrastructure/src/region/mapping.rs`:
  - `check_v4_geography` ~599: schema 6 requires `weather`, otherwise `MissingField { field: "weather" }`; schema ≤ 5 with `weather` set is `InvalidShape { field: "region_schema" }`;
  - `map_region_v4` ~770: maps `weather`, or `1.0` for schema ≤ 5;
  - `map_region` (legacy): always `1.0`;
  - keep the `== 5` geothermal gate (`:606`, `:794`) meaning "schema ≥ 5".
- Modify: all 8 `assets/regions/*.ron`: `region_schema: 6` and `weather: (surprise_share: 0.5),`
- Test: `crates/infrastructure/tests/region_catalog.rs`, `crates/domain/src/region/mod.rs` tests

**Interfaces:**
- Produces:
  - `pub struct WeatherParams { pub surprise_share: f64 }` (Debug, Clone, Copy, PartialEq);
  - `RegionDefinitionParts::weather: WeatherParams`;
  - `RegionDefinition::weather(&self) -> &WeatherParams`;
  - `WeatherParams::surprise_sd(&self, sd: f64) -> f64`, which is `share × sd`;
  - `WeatherParams::forecast_sd(&self, sd: f64) -> f64`, which is `sqrt(1 − share²) × sd`.

- [ ] **Step 1: Write the failing tests.**
  - Domain: `surprise_share_must_lie_in_unit_interval` uses `parts_with(|p| p.weather.surprise_share = 1.5)` and `-0.1` and `f64::NAN`, each giving `shape("weather.surprise_share")`. Add `weather_sds_split_the_variance`: with share 0.5 and sd 0.18, assert `surprise_sd` is `0.09` and `forecast_sd² + surprise_sd² == 0.18²` within `1e-12`.
  - Infrastructure, modelled on `geothermal.rs:126`, `region_schema6_requires_weather_and_schema5_defaults_to_one`:
    - removing `weather:` from a shipped schema-6 file gives `RegionError::MissingField { field: "weather" }`;
    - flipping `region_schema: 6` to `5` while keeping `weather` gives `InvalidShape { field: "region_schema" }`;
    - flipping to 5 and removing `weather` loads with `weather().surprise_share == 1.0`;
    - every shipped region loads with `0.5`.
- [ ] **Step 2: Run** `cargo nextest run -p domain -p infrastructure -E 'test(surprise_share) | test(weather)'`. Expected: FAIL (no field).
- [ ] **Step 3: Implement** the structs, validation (`check_shape(share.is_finite() && (0.0..=1.0).contains(&share), "weather.surprise_share")`), DTO, mapping, schema gates and the RON edits. Add `"weather"` to the strict-field list in `a_v4_file_requires_authored_fields_with_typed_errors` (`region_catalog.rs:1003`), gated to schema 6.
- [ ] **Step 4: Run** `cargo nextest run -p domain -p infrastructure`. Expected: PASS. Frozen-payload and hash tests (`legacy_wind_extension_payloads_are_frozen`, `MINIMAL_SHA256`) are untouched and pass.
- [ ] **Step 5: Commit** with `feat: add surprise_share region data (region schema 6)`.

### Task 2: `WeatherForecast` state, drawn when Planning begins

**Files:**
- Create: `crates/domain/src/grid/power_system/weather.rs` (declared in `power_system.rs` next to `mod wind;`)
- Modify: `crates/domain/src/grid/power_system.rs`:
  - field `weather_forecast: WeatherForecast` on `PowerSystem` (96–145), neutral in `new()`;
  - `write_schema` and `read` (1469–1585): write and read it only when `schema >= 9`, defaulting to neutral (`WeatherForecast::default()`).
- Modify: `crates/domain/src/state.rs:8,12` (`CURRENT_STATE_SCHEMA = 9`, `1..=9`)
- Modify: `crates/domain/src/session/mod.rs`. `new()` (after `rng` exists, ~335) and `finish_review()` (412) call `self.power.draw_weather_forecast(&self.region, self.clock.month(), &mut self.rng)`. In `finish_review` the call comes after `clock.next_month()`.
- Modify the copy sites so every scoped or island `PowerSystem` carries the parent's forecast:
  - `islands.rs`: `scoped_island_with_service` after `apply` (~492), `begin_islands` 531, `tick_islands` 635;
  - `transfer_control.rs`: 255, which today also lacks `clearness`, so copy that too, and 388.
- Test: `crates/domain/src/grid/power_system/weather.rs` (`#[cfg(test)] mod tests`), `crates/domain/src/session/tests.rs`

**Interfaces:**
- Produces, in `weather.rs`:
  - `pub struct WeatherForecast { pub month: Option<SimMonth>, pub clearness: f64, pub expected_clearness: f64, pub wind_path: Vec<f64> }`. Debug, Clone, PartialEq, Default. Default means `month: None`, clearness and expected `1.0` (matching `new()`'s `clearness: 1.0`), and an empty path (meaning `f ≡ 0`).
  - `PowerSystem::draw_weather_forecast(&mut self, region: &RegionDefinition, month: SimMonth, rng: &mut SimRng)`
  - `PowerSystem::weather_forecast(&self) -> &WeatherForecast`
  - `WeatherForecast::wind_deviation(&self, tick: u32) -> f64`, returning `wind_path.get(tick).copied().unwrap_or(0.0)`
  - `WeatherForecast::neutral(region: &RegionDefinition, month: SimMonth) -> Self`, with `clearness = clearness_mean[m]`, `expected_clearness = region.expected_clearness(month)` and an empty path. Task 5 uses it.
  - a `State` impl for `WeatherForecast`, using `state_record!` or a hand-written impl like the neighbours; `wind_path` is written as its length followed by its values.
- The draw order is fixed and pinned by a test. Exactly `1 + 1440` `standard_normal` draws:
  - `z_c` first, giving `clearness = clearness_mean[m] + forecast_sd(clearness_sd) × z_c` (unclamped);
  - then `f_0 = forecast_sd(initial_sd) × z`;
  - then `f_t = persistence × f_{t−1} + forecast_sd(innovation_sd) × z` for `t = 1..1439`.

  Draw every one even when its coefficient is 0. `expected_clearness` is:
  - `mean_clearness(clearness, surprise_sd(clearness_sd))` when `solar().annual_site_clearness`;
  - otherwise `clearness.clamp(MIN_CLEARNESS, MAX_CLEARNESS)`.

  Make `MIN_CLEARNESS`/`MAX_CLEARNESS` `pub(crate)` for this.

- [ ] **Step 1: Write the failing tests.**
  - `draw_order_is_clearness_then_1440_wind_normals`: a reference `SimRng` with the same seed replays the draws, then `assert_eq!(rng, reference)`, and the path has length 1440.
  - `share_one_gives_a_neutral_mean_forecast`: on a fixture with share 1.0, `clearness == clearness_mean[m]` and every `wind_path` entry is `0.0`.
  - `forecast_round_trips_at_schema_9_and_is_absent_at_8`: `state_bytes_schema(8)` of a session with a forecast equals that of a neutral session's at 8, and schema 9 round-trips equal.
  - `finish_review_draws_a_new_forecast_for_the_next_month`: `weather_forecast().month == Some(next)`.
  - `island_copies_carry_the_forecast`: on `links.rs` `automatic_game()`, `island_session(id).power().weather_forecast() == session.power().weather_forecast()` for every island.
- [ ] **Step 2: Run** `cargo nextest run -p domain -E 'test(forecast) | test(draw_order)'`. Expected: FAIL.
- [ ] **Step 3: Implement** as specified above.
- [ ] **Step 4: Run** `cargo nextest run -p domain`. The new tests pass. Existing RNG-position tests that run across a `GameSession::new` or `finish_review` now fail; record their names for Task 4 and don't fix them yet. Rules-sensitive snapshots are updated in Task 4.
- [ ] **Step 5: Commit** with `feat: draw a day-ahead weather forecast when Planning begins`.

### Task 3: Actual weather is forecast plus surprise

**Files:**
- Modify: `crates/domain/src/generation/wind.rs`. Add `wind_step_with_forecast`, keeping `wind_step`.
- Modify: `crates/domain/src/grid/power_system.rs`:
  - `begin_single_day` 360–366:
    - `clearness = clamp(forecast.clearness + surprise_sd(clearness_sd) × z, MIN, MAX)`;
    - `wind_factor = clamp(m + f_0 + surprise_sd(initial_sd) × z, 0, 1)`.
  - `draw_tick_weather` 563–584: for `tick > 0`, `wind_factor = wind_step_with_forecast(wind_factor, m, f(tick−1), f(tick), persistence, surprise_sd(innovation_sd), z)`.
- Modify: `crates/domain/src/grid/power_system/islands.rs:503-506` (the same two day-start draws on the parent)
- Test: `crates/domain/src/grid/power_system/tests.rs` (870, 897), `crates/domain/src/generation/wind.rs` tests

**Interfaces:**
- Produces `pub fn wind_step_with_forecast(previous: f64, mean: f64, forecast_previous: f64, forecast_now: f64, persistence: f64, surprise_sd: f64, innovation: f64) -> f64`. It computes `clamp(mean + forecast_now + persistence × (previous − mean − forecast_previous) + surprise_sd × innovation, 0, 1)`. This is §3.1's post-clamp residual form: `e_{t−1} = previous − mean − forecast_previous`.
- The draw count per day is unchanged: two at day start, then demand and wind each tick.

- [ ] **Step 1: Write the failing tests.**
  - `with_a_zero_forecast_the_step_equals_todays_wind_step`: proptest over `previous ∈ [0,1]`, `mean ∈ [0,1]`, `z ∈ [-5,5]`. Assert `wind_step_with_forecast(p, m, 0.0, 0.0, ρ, σ, z) == wind_step(p, m, ρ, σ, z)` exactly.
  - Update `begin_day_draws_clearness_then_wind_once_and_reports_boundary_starts` (870) so the expected values use the forecast and the surprise sds.
  - `share_one_reproduces_todays_day_bit_for_bit`: with share 1.0, run Planning → BeginOperating → 1440 ticks. Compare the clearness and every tick's `wind_factor` with a reference that replays today's formulas (`draw_clearness`, `wind_step`) on an RNG advanced past the `1 + 1440` forecast draws. They must be exactly equal.
  - `share_zero_makes_the_day_the_forecast`: with share 0, `clearness == forecast.clearness.clamp(..)`, and each tick's `wind_factor == clamp(m + f_t)`.
- [ ] **Step 2: Run** `cargo nextest run -p domain -E 'test(wind_step) | test(share_) | test(begin_day_draws)'`. Expected: FAIL.
- [ ] **Step 3: Implement** the two day-start formulas and the per-tick step.
- [ ] **Step 4: Run** the same filter. Expected: PASS.
- [ ] **Step 5: Commit** with `feat: actual weather is the forecast plus a surprise`.

### Task 4: Forecast readers, solar band, rules 32

**Files:**
- Modify: `crates/domain/src/grid/power_system.rs:1375-1388`. `expected_solar` uses `self.weather_forecast.expected_clearness` in place of `region.expected_clearness(month)`.
- Modify: `crates/domain/src/grid/power_system/battery.rs:151`. Same change, via `power.weather_forecast().expected_clearness`.
- Modify: `crates/domain/src/grid/power_system/wind.rs` (9–28):
  - `wind_factor_forecast` central: in Planning (`wind_month != Some(month)`) it is `m + f_tick`; in Operating it is `m + f_tick + ρ^(tick − wind_tick) × (w − m − f_{wind_tick})`, clamped.
  - the band is `± 2 × surprise_sd(initial_sd)`, clamped to `[0, 1]`, replacing `w.band`.
- Modify: `crates/domain/src/grid/power_system/islands.rs:237-269`. `wind_for_node` calls a shared `fn wind_factor_forecast_from(state…)` instead of duplicating the formula.
- Create, in `weather.rs`: `PowerSystem::solar_forecast(&self, region, month, tick) -> (Megawatts, Megawatts, Megawatts)`, giving central, low and high:
  - central is `expected_solar`;
  - low and high are computed like `expected_solar` but with clearness `clamp(forecast.clearness ∓ 2 × surprise_sd(clearness_sd))`.

  Sum over islands the way `wind_forecast` does (30–56).
- Modify: `crates/domain/src/lib.rs:23` (`SIM_RULES_VERSION = 32`). Also update `crates/infrastructure/examples/capture_geothermal_baseline.rs:20` only if it fails to compile or run under the new number; it pins 27 for a frozen capture, so leave its assert as is unless `cargo build --examples` breaks.
- Modify: `crates/domain/src/session/forecast_contract_tests.rs:136-143`. Planning solar and wind now come from `weather_forecast()`.
- Regenerate: `crates/application/tests/snapshots/*.snap` via `cargo insta review`, reviewing each diff (rules line, outcome digests, scenario numbers only).
- Test: `crates/domain/src/grid/power_system/tests/wind.rs`, `crates/domain/src/grid/power_system/tests.rs:942`, `crates/domain/src/generation/solar.rs:168-209`

**Interfaces:**
- Produces:
  - `PowerSystem::solar_forecast(&self, region: &RegionDefinition, month: SimMonth, tick: u32) -> (Megawatts, Megawatts, Megawatts)`, also exposed as `GameSession::solar_forecast(&self, tick: u32)` in `views.rs` (multi-area: summed over islands as `supply_forecast` does);
  - `wind_forecast`'s low and high now reflect the surprise band.

- [ ] **Step 1: Write and update the tests.**
  - `expected_solar_uses_the_forecast_clearness` replaces 942's hard-coded `0.75`.
  - `planning_wind_forecast_follows_the_forecast_path`: central at tick t equals `m + f_t`, and the band is `±2 × 0.5 × initial_sd`.
  - `solar_band_brackets_the_central_forecast`: `low ≤ central ≤ high` at every hour.
  - Extend `calibrated_annual_resource_and_forecast_include_bounded_weather` (solar.rs:168). Over 100k draws of the forecast, `mean(expected_clearness(forecast))` must equal the sampled mean of the actual clearness within 0.002. This is the unbiased check from §3.3.
  - Update `shared_wind_does_not_draw_per_farm_or_during_queries`. Its band expectation `(240, 120, 360)` becomes the surprise band.
- [ ] **Step 2: Run** `cargo nextest run -p domain`. Expected: the new tests fail, and the list from Task 2 Step 4 still fails.
- [ ] **Step 3: Implement** the readers and the rules bump. Fix each recorded RNG-position test by replaying the `1 + 1440` forecast draws in its reference RNG. Never loosen an assertion to make it pass.
- [ ] **Step 4: Run** `cargo nextest run --workspace`, then `cargo insta review`. Accept only diffs that the rules change explains. Run `cargo nextest run --workspace` again. Expected: PASS, including the shipped-region year-one suites. If `assert_no_outages_year_one` fails for a region, stop and investigate: that is a real regression of the hands-off plan.
- [ ] **Step 5: Commit** with `feat: forecast readers use the day-ahead weather (rules 32)`.

### Task 5: Upcast older saves

**Files:**
- Modify: `crates/domain/src/session/mod.rs`. Add `pub fn upgrade_weather_forecast(&mut self)`, which sets `power.weather_forecast = WeatherForecast::neutral(&region, clock.month())`.
- Modify: `crates/application/src/game.rs:149-155`. Call it after the round-trip check, when `snapshot.schema < 9`, next to the `schema < 8` upcast.
- Test: `crates/application/tests/save_resume.rs`, following `schema_7_snapshot_restores_with_seeded_expansion` (827)

**Interfaces:**
- Consumes `WeatherForecast::neutral` (Task 2).

- [ ] **Step 1: Write the failing test** `schema_8_snapshots_restore_in_every_phase_with_a_neutral_forecast`. For each of Planning, Operating (mid-day) and Review:
  - build the game and rewrite its snapshot to schema 8 (`snapshot_state_schema(8)`, `state_bytes_schema(8)`, recomputed `state_hash`);
  - `Game::restore` it twice and assert the two `state_hash()`es are equal;
  - assert `weather_forecast() == WeatherForecast::neutral(region, month)`;
  - play to the next month close without error.
- [ ] **Step 2: Run** `cargo nextest run -p application -E 'test(schema_8)'`. Expected: FAIL. Restore either errors or leaves the default forecast instead of the neutral one.
- [ ] **Step 3: Implement** the upcast.
- [ ] **Step 4: Run** `cargo nextest run -p application -p infrastructure`. Expected: PASS, including the existing golden-save suites (`save_snapshots.rs`, `geothermal.rs:367`).
- [ ] **Step 5: Commit** with `feat: upcast pre-forecast saves with a neutral forecast`.

### Task 6: Long-run weather statistics per shipped region

**Files:**
- Test: `crates/infrastructure/tests/shipped_regions.rs`. Add a `mod weather_statistics` near the calibration tests (~3841).
- Modify: `crates/domain/src/generation/mod.rs:21-23`. Make `mean_clearness` `pub`, since infrastructure tests need it.

**Interfaces:**
- Consumes:
  - `region(id)` (shipped_regions.rs:126);
  - `domain::rng::SimRng`;
  - `domain::generation::{wind_step, wind_step_with_forecast, mean_clearness}`;
  - `RegionDefinition::{solar, wind, weather}`.

- [ ] **Step 1: Write the tests.** All are deterministic, with fixed seed `20261031`.
  - `split_clearness_matches_todays_mean`: for every shipped region and month, simulate 200,000 days of `clamp(mean + forecast_sd × z₁ + surprise_sd × z₂)`. Assert the mean is within `0.005` of `mean_clearness(mean, clearness_sd)`.
  - `split_wind_matches_todays_process`: for every shipped region, take the months with the highest and the lowest `monthly` mean. Simulate 4,000 days of 1,440 minutes with today's `wind_step` and with the forecast path plus `wind_step_with_forecast`. Assert:
    - the mean factors are within `0.005` of each other;
    - the sample standard deviations are within 5%.

    This covers India's monsoon (`m = 0.95`) and IPS's July (`m = 0.26`). If a case misses only through Monte Carlo noise, raise the day count. **Never raise the tolerance** (spec §6).
- [ ] **Step 2: Run** `cargo nextest run -p infrastructure -E 'test(weather_statistics)'`. Expected: PASS. If wind fails at the bounds, the residual form in Task 3 is wrong. Fix the domain, not the test.
- [ ] **Step 3: Commit** with `test: pin long-run weather statistics under the forecast split`.

### Task 7: Solar forecast band on the chart

**Files:**
- Modify: `crates/game/src/hud/forecast_chart.rs`:
  - `ForecastModel` (58–103): add `solar_low: [f64; 24]`, `solar_high: [f64; 24]` and `solar_range_label: String`;
  - fill them from `session.solar_forecast(h*60 + 30)` (or the island session in island view, as `forecast_model` 164 does);
  - draw a `FilledArea` in the solar colour, like wind's (615–617), and add a legend entry with `KeyMark::Band`.
- Modify: `assets/locales/en-US/ui.ftl` (after `forecast-solar` at 267): `forecast-solar-range = Solar forecast range`
- Modify: `crates/game/src/l10n/keys.rs` (after `"forecast-solar"` at 521): `"forecast-solar-range"`
- Test: the `forecast_chart.rs` unit tests

- [ ] **Step 1: Write the failing test** `solar_band_brackets_the_solar_line`. On a session with solar, `solar_low[h] ≤ solar[h] ≤ solar_high[h]` holds for every h, and on a session without solar all three are 0.
- [ ] **Step 2: Run** `cargo nextest run -p game -E 'test(solar_band)'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo nextest run -p game`. Expected: PASS, including `every_ui_key_exists_in_en_us`.
- [ ] **Step 5: Commit** with `feat: show the solar forecast band on the forecast chart`.

---

## Part B — automatic commitment

### Task 8: Battery capability in the supply forecast

**Files:**
- Modify: `crates/domain/src/grid/power_system/battery.rs`. Add to `SupplyForecastPoint` (28):
  - `battery_power: Megawatts`: the summed discharge power limit of enabled, connected batteries at that minute;
  - `battery_deliverable_energy: MegawattHours`: the previewed stored energy × discharge efficiency, summed.

  Fill both in `project_storage_supply_with_inputs` (~666–739). Both are zero on the non-storage paths. In `session/views.rs:288`, multi-area sums them like the other fields.
- Test: `crates/domain/src/session/battery_tests.rs`

**Interfaces:**
- Produces `SupplyForecastPoint::{battery_power, battery_deliverable_energy}`.

- [ ] **Step 1: Write the failing test** `supply_forecast_reports_battery_power_and_deliverable_energy`. On the tiny fixture with its 200 MW / 800 MWh battery:
  - `battery_power == 200` at 18:00;
  - `battery_deliverable_energy` is non-increasing across the discharge window while it discharges, and never exceeds `800 × discharge_efficiency`.
- [ ] **Step 2: Run** `cargo nextest run -p domain -E 'test(battery_power)'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo nextest run -p domain`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat: expose battery capability in the supply forecast`.

### Task 9: The planner query

**Files:**
- Create: `crates/domain/src/session/auto_commit.rs` (declared in `session/mod.rs`)
- Create: `crates/domain/src/session/auto_commit_tests.rs` (`#[cfg(test)]`)
- Modify: `crates/domain/src/grid/power_system/weather.rs`. Add `WeatherForecast::low_edge(&self, region) -> WeatherForecast`:
  - clearness lowered by `2 × surprise_sd(clearness_sd)`;
  - `expected_clearness` set to the clamped low clearness;
  - every `wind_path` entry lowered by `2 × surprise_sd(initial_sd)`.

**Interfaces:**
- Consumes:
  - `supply_forecast()`, `island_session(id)`, `minimum_generation_forecast()`, `hourly_forecast()`, `forecast_net_load(tick)`;
  - `CommitmentSchedule::{all_day, try_new, hours}`, `Plant::{commitment_spec, set_commitment, auto_follow, kind, status, min_output, available_capacity, initial_target}`, `PowerSystem::plant_mut`;
  - `SupplyForecastPoint::{battery_power, battery_deliverable_energy}` from Task 8;
  - `WeatherForecast::low_edge`.
- Produces `pub fn automatic_commitment_plan(&self) -> BTreeMap<PlantId, CommitmentSchedule>` on `GameSession`. It returns a pattern for each trimmable plant (§4.1): connected, not `UnderConstruction`, `auto_follow()`, and kind `Coal | Ccgt | Peaker`. Nothing else.

**Algorithm (§4.3–§4.5). Private helpers in `auto_commit.rs`:**
- `fn trial(&self, plan: &BTreeMap<PlantId, CommitmentSchedule>) -> GameSession` clones `self` and sets each pattern via `power.plant_mut(id)?.set_commitment(s)`.
- `fn coverage_supply(trial: &GameSession) -> Vec<Vec<(u32, f64, f64)>>` returns `(tick, required, supply)` per island (one island in single-area regions), using that island's `island_session` (which re-plans transfers):
  - `required = demand × (1 + 2 × forecast_error)`;
  - `supply = gross_supply + battery_credit − restart_deficit`, from a copy of the island session whose `power.weather_forecast` is replaced by `low_edge`;
  - `battery_credit = min(battery_power − battery_discharge, battery_deliverable_energy ÷ hours_left_in_window)` inside the discharge window, otherwise 0, where `hours_left_in_window = (1440 − tick) / 60`;
  - `restart_deficit` is summed over trimmable plants: for committed ticks following an off-gap, `max(0, available_capacity − (min_output + ramp.per_tick × (tick − gap_end_tick)))`.
- `covers(floor, supply)`: every sample satisfies `supply ≥ floor − 1e-9`. The floor is `min(required, supply_all_day)` per sample (§4.3).
- `fn excess(trial) -> f64` is `Σ max(0, committed_minimum − net_load)` over each island's `minimum_generation_forecast()`.
- `fn cost(trial) -> f64`, in dollars per day, as an `f64` sum for comparison. For each hour h, sampled at minute 30 with mean weather (the real forecast, not the low edge):
  - dispatch `forecast_net_load` minus committed manual targets and the hydro preview over committed AGC thermal plants, `boundary_dispatch`-style: each plant at minimum, then fill by `(marginal_cost, id)`;
  - add `Σ output × marginal_cost`;
  - then add `startup_cost` once per off-gap per pattern.
- Search: start all-day, visit trimmable plants by descending `(marginal_cost, id)`, and consider all-off plus every valid single wrap-around gap. To stay fast:
  - screen each candidate with an additive estimate: the plant's own credited supply removed from the current per-sample slack;
  - fully evaluate candidates in order of estimated `(excess, cost)` gain, at most 8 per plant;
  - accept the first that covers and beats the current pattern lexicographically by `(excess, cost)`.

- [ ] **Step 1: Write the failing tests** in `auto_commit_tests.rs`. Use `session_with` (session/tests.rs:1158) and `automatic_game` (links.rs:308) on the tiny fixture: Coal 600 @ $30, CCGT 500 @ $45, Peaker 200 @ $150.

  Unit tests:
  - `peaker_comes_off_when_the_fleet_covers_the_band_without_it`;
  - `peaker_stays_when_needed_for_the_band`: raise demand so coal + CCGT fall short at the evening peak;
  - `battery_spare_discharge_lets_an_evening_peaker_go`;
  - `gaps_too_short_for_shutdown_and_startup_are_never_chosen`: every returned pattern passes `try_new` with `commitment_spec()`;
  - `startup_cost_outweighing_fuel_keeps_the_plant_all_day`: set the coal `startup_cost` very high, so coal returns all-day;
  - `restart_ramp_keeps_a_plant_that_cannot_make_the_morning_ramp`;
  - `leaves_manual_and_fixed_kinds_alone`: with nuclear switched off and coal set manual (`auto_follow = false`), neither appears in the plan;
  - `one_tight_hour_still_allows_overnight_trimming`;
  - `sunny_forecast_lets_a_ccgt_off_at_midday_that_a_dull_one_keeps`: set `weather_forecast.clearness` directly;
  - `linked_islands_stay_covered`: on `automatic_game()`, after applying the plan, every island's coverage holds with transfers re-planned.

  Properties (proptest, 24 cases, demand scale `0.5..1.5` via `parts.cities` demand):
  - `plan_is_valid_covered_and_never_worse`: valid, covers, and `(excess, cost)` is `≤` all-day's;
  - `plan_is_pure_and_deterministic`: `state_hash` is unchanged, and two calls give equal results.
- [ ] **Step 2: Run** `cargo nextest run -p domain -E 'test(auto_commit)'`. Expected: FAIL (the function doesn't exist).
- [ ] **Step 3: Implement** `auto_commit.rs` and `low_edge`.
- [ ] **Step 4: Run** `cargo nextest run -p domain`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat: add the automatic commitment planner query`.

### Task 10: Application helper and shipped-region acceptance

**Files:**
- Create: `crates/application/src/auto_commit.rs`, exported from `lib.rs`
- Test: `crates/application/tests/` (add to the existing `state_machine.rs`), `crates/infrastructure/tests/shipped_regions.rs`

**Interfaces:**
- Produces `pub fn automatic_commitment_commands(game: &Game) -> Vec<Command>`. It returns one `Command::CommitPlant { plant, hours }` per plant whose planned `hours()` differs from its current `commitment`, in ascending `PlantId` order, and is empty unless `phase() == DayPhase::Planning`.

- [ ] **Step 1: Write the failing tests.**
  - Application, `automatic_commitment_commands_apply_cleanly_and_are_idempotent`: executing every command succeeds; afterwards `automatic_commitment_commands` is empty, and outside Planning it is empty.
  - Infrastructure: `assert_auto_plan_year_one(id, seed)`, modelled on `assert_no_outages_year_one` (1104). Each month: `hands_off_game` setup, then execute `automatic_commitment_commands` before `play_month`. Assert:
    - no rotating outage, and 12 months played;
    - fuel plus start-up spend (`-(report.fuel + report.start_up)`, summed) is `≤` a parallel hands-off run with the same seed;
    - wire it per region through `shipped_scenario_cases!` as a new `$auto_plan` module.

    Add `auto_plan_spends_less_in_total` over all regions and seeds 0–7: `Σ auto < Σ hands-off`.
- [ ] **Step 2: Run** `cargo nextest run -p application -p infrastructure -E 'test(auto_plan) | test(automatic_commitment)'`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the helper.
- [ ] **Step 4: Run** the same filter. Expected: PASS. **If any region orders an outage, fix the planner** (cushion, ramp or battery credit) in Task 9's code and add the failing case as a domain unit test. Do not loosen the test.
- [ ] **Step 5: Commit** with `feat: application helper for automatic commitments; shipped-region acceptance`.

### Task 11: Preference, checkbox and dispatch

**Files:**
- Modify: `crates/infrastructure/src/client_preferences.rs:22-47`. Add `pub auto_commitments: bool` with `true` in `Default`.
- Modify: `crates/game/src/hud/ui_state.rs`:
  - add `pub auto_commitments: bool` to `UiState` (default `true`);
  - `UiAction::SetAutoCommitments(bool)` sets it;
  - `UiAction::OpenPlanningTab(PlanningTab)` sets `tab` only, with no other effect.
- Modify: `crates/game/src/preferences.rs`. In `load` (83), seed `UiState.auto_commitments` from preferences; in `observe_egui` (163), copy it back. Follow `supply_chart_layout` exactly, including surviving `reset_ui_state`.
- Modify: `crates/game/src/hud/action.rs`:
  - add `UiAction::{SetAutoCommitments(bool), OpenPlanningTab(PlanningTab), RunAutoCommitments}`;
  - in `dispatch`, `SetAutoCommitments(true)` while the session is in Planning also dispatches `RunAutoCommitments`;
  - `RunAutoCommitments` discards any draft (`ui.draft = None`) and writes one `PlannerCommand` per `automatic_commitment_commands`;
  - add `planner: MessageWriter<PlannerCommand>` to `ActionSink`.
- Modify: `crates/game/src/sim/mod.rs`. Add `#[derive(Message)] pub struct PlannerCommand(pub Command, pub u64)`. `apply_player_commands` (199) reads `PlannerCommand`s **before** `PlayerCommand`s, executes them identically (same journal and command log), and on `Err` logs `tracing::error!` and writes **no** `CommandFailed`.
- Modify: `crates/game/src/hud/commitment.rs`:
  - add `pub auto: ToggleModel` and `pub auto_tip: String` to `CommitmentModel` (109), with action `SetAutoCommitments(!ui.auto_commitments)`;
  - in `draw_commitment` (548), draw it first as an `egui::Checkbox` above the grid and push the model's action on click.
- Modify: `assets/locales/en-US/ui.ftl` (commitment block 241–264) and `keys.rs`:
  - `commitment-auto = Plan commitments automatically each month`
  - `commitment-auto-tip = Each Planning phase, switches off expensive plants the forecast doesn't need, keeping a cushion above the forecast.`
- Test: `crates/game/tests/game/auto_commitment.rs` (new module in `tests/game.rs`), `crates/game/tests/game/preferences.rs`

**Interfaces:**
- Consumes `application::automatic_commitment_commands`.
- Produces `UiAction::{SetAutoCommitments, OpenPlanningTab, RunAutoCommitments}`, `PlannerCommand`, `UiState::auto_commitments`, `ClientPreferences::auto_commitments`.

- [ ] **Step 1: Write the failing tests.**
  - `preferences.rs`, `auto_commitments_defaults_on_and_round_trips`: an older file without the field reads `true`, and toggling writes `false`.
  - `auto_commitment.rs`, glue tests through `hud_smoke::dispatch_in`:
    - `set_auto_commitments_on_in_planning_sends_planner_commands`;
    - `toggling_on_replans_once_and_off_changes_nothing`: on, off, on gives exactly two rounds, and off sends nothing;
    - `open_planning_tab_sets_only_the_tab`: `last_planning_island` and island are unchanged;
    - `planner_refusal_logs_without_a_toast`: inject an invalid `PlannerCommand`, then `sent::<CommandFailed>` is empty.
  - Presenter test: `the_checkbox_reflects_the_preference`.
- [ ] **Step 2: Run** `cargo nextest run -p game -p infrastructure -E 'test(auto_commitments) | test(planner) | test(open_planning_tab) | test(checkbox)'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo nextest run -p game`. Expected: PASS, including l10n coverage.
- [ ] **Step 5: Commit** with `feat: automatic commitments preference, checkbox and dispatch`.

### Task 12: Run the planner on entering Planning; open on Forecast

**Files:**
- Modify: `crates/game/src/hud/mod.rs`:
  - new system `auto_commit_on_planning`, in `Update` after `SimSet::Apply`. It reads `NewGameStarted` and `DomainEventMessage(PhaseChanged { to: Planning, .. })`. When `ui.auto_commitments` is on, it calls `dispatch(vec![UiAction::RunAutoCommitments], &mut sink)`. It never reacts to a load: loads write neither message.
  - new system `open_planning_tab` on `OnEnter(PhaseState::Planning)`. It dispatches `OpenPlanningTab(Forecast)` when `auto_commitments` is on, otherwise `OpenPlanningTab(Commitments)`. It runs after `reset_planning_state`, which is `OnExit` and therefore earlier.
- Test: `crates/game/tests/game/auto_commitment.rs`

- [ ] **Step 1: Write the failing integration tests.** Use `request(app, region)` and `play_a_month` / `save_resume` helpers.
  - `new_game_opens_on_forecast_with_trimmed_patterns`: Eastern; the peaker patterns differ from all-day.
  - `finish_review_replans_and_opens_on_forecast`.
  - `with_the_preference_off_nothing_is_replanned_and_the_tab_is_commitments`.
  - `loading_a_planning_manual_save_keeps_its_patterns`: hand-edit a row, save manually, load, and the patterns are equal and no `PlannerCommand` was sent.
  - `planner_discards_an_open_draft`.
- [ ] **Step 2: Run** `cargo nextest run -p game -E 'test(auto_commitment)'`. Expected: FAIL.
- [ ] **Step 3: Implement** both systems. If `OnEnter(PhaseState::Planning)` doesn't fire on a new game or on load, set the tab from the same triggers instead, and use `persistence.resume_phase` on load. Make the tests decide.
- [ ] **Step 4: Run** `cargo nextest run -p game`. Expected: PASS, including `tutorial` and `shortfall_prompt`.
- [ ] **Step 5: Commit** with `feat: run automatic commitments when Planning begins and open on Forecast`.

### Task 13: Whole-branch verification

- [ ] **Step 1: Format.** Run `cargo fmt --all --check`. Expected: no diff.
- [ ] **Step 2: Lint.** Run `cargo clippy --workspace --all-targets -- -D warnings`. Expected: clean.
- [ ] **Step 3: Test.** Run `cargo nextest run --workspace`. Expected: all pass.
- [ ] **Step 4: Check wasm.** Run wasm32 clippy with the pinned toolchain's own cargo and rustc. See memory *wasm-check-needs-toolchain-binaries*, and use whatever command CI uses (`.github/workflows`). Expected: clean.
- [ ] **Step 5: Smoke-launch.** Run `cargo run -p game -- --exit-after-frames 120`. Expected: exit 0.
- [ ] **Step 6: Measure performance.** Time `automatic_commitment_plan` on the largest shipped region with a temporary `#[ignore]` test using `std::time::Instant`. Record the result in the PR description. If it exceeds 250 ms native, tighten Task 9's screening before finishing.
- [ ] **Step 7: Review.** Have a fresh reviewer go over the whole branch against the spec, then address its findings.
