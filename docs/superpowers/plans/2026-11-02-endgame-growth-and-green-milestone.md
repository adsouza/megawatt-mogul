# Endgame Demand Growth and Green Milestone Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Once every city is connected, demand grows monthly at each region's real forecast rate; and a hidden bonus milestone, revealed after the three regional ones, is earned by 12 consecutive strike-free months with zero coal, CCGT and peaker energy.

**Architecture:** Part A (Tasks 1–7) adds a `DemandGrowth` factor to the domain, stored in `PowerSystem` and applied wherever authored demand becomes live or forecast demand. `forecast_demand` and `hourly_forecast` take it as a parameter so the compiler finds every call site. The factor steps at each month close where the region is complete, and forward projections look ahead with it. Part A bumps region schema to 7, state schema to 11 and rules to 34. Part B (Tasks 8–10) is read-side only: `MilestoneProjection` tracks a clean, strike-free run from game start, reveals the bonus, seeds old saves from report history, and the game toasts milestones without mutating state. It bumps state schema to 12.

**Tech Stack:** Rust workspace (`domain`, `application`, `infrastructure`, `game`), Bevy + bevy_egui, RON region data, Fluent l10n, cargo-nextest, proptest, insta.

**Spec:** `docs/superpowers/specs/2026-11-02-endgame-growth-and-green-milestone-design.md`

## Global Constraints

- `domain` may depend only on `rand_chacha` and `thiserror`; no Bevy, serde, I/O or tracing.
- No user-facing strings in `domain` or `application`; new text goes in `assets/locales/en-US/*.ftl`, literal keys also in `crates/game/src/l10n/keys.rs`.
- Determinism: `BTreeMap`/`BTreeSet` only, no new RNG draws (demand noise stays one draw per tick), no `powf` for growth: the monthly factor is `1.0 + demand_growth_per_year / 12.0` and look-ahead is repeated multiplication.
- Growth is never written into `CityDef.base_load` (`validate_snapshot`, `session/mod.rs:1058-1068`).
- A growth rate of 0 must leave every pre-change golden continuation bit for bit. Run `cargo nextest run -p infrastructure -E 'test(prechange) | test(capture)'` after every Part A task.
- Every new persisted field is gated `schema >= 11` (Part A) or `schema >= 12` (Part B); restore re-encodes at the source schema before upgrading (`application/src/game.rs:166-170`).
- Region schema 7 requires `demand_growth_per_year`; schemas ≤ 6 reject it and default to `0.0`.
- Fossil kinds are exactly `PlantKind::Coal`, `PlantKind::Ccgt`, `PlantKind::Peaker`; a month is fossil-free when each has `energy_by_kind` ≤ `1e-9` MWh (missing key = 0).
- One test binary for game integration tests: new modules under `crates/game/tests/game/`, never a new file in `tests/`.
- Conventional commits; `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` green before each commit. Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- A projected `PowerSystem` built with `PowerSystem::new` (connection preview, scoped island) without copying the factor silently under-forecasts grown demand → Task 3 tests both.
- The December close that completes the region: the reserve review runs before that close's growth step but must still project next year's grown peak → Task 5 test.
- Review's outlook and the review-time auto-commit precompute (`application/src/auto_commit.rs:83-115`) must agree on next month's grown demand → Task 4 test.
- Save and resume mid-growth and mid-green-run must continue exactly (factor and run persisted, no re-toast) → Tasks 4, 8 and 10 tests.
- An old (schema ≤ 11) save whose seeded run already reaches 12 when loaded is revealed and awarded silently, then never toasted later → Tasks 9 and 10 tests.

---

## Part A — Demand growth

### Task 1: `DemandGrowth` and the region rate

**Files:**
- Create: `crates/domain/src/demand/growth.rs`
- Modify: `crates/domain/src/demand/mod.rs` (declare module, re-export)
- Modify: `crates/domain/src/region/mod.rs:112-180` (`RegionDefinitionParts`, `RegionDefinition`, getters near `weather()` at `:302`, assignment near `:773`)
- Modify: `crates/domain/src/region/fixtures.rs:351` and every other literal `RegionDefinitionParts {` (`infrastructure/src/region/mapping.rs:47,704`, `domain/src/region/clock.rs:191`, tests)
- Test: `crates/domain/src/demand/growth.rs` (unit + proptest)

**Interfaces:**
- Produces:
  - `pub struct DemandGrowth { factor: f64 }` (`Debug, Clone, Copy, PartialEq`), `crate::state_record!(DemandGrowth { factor })`
  - `DemandGrowth::NONE` (factor `1.0`); `fn factor(self) -> f64`; `fn apply(self, demand: Megawatts) -> Megawatts` (`demand * factor`); `fn stepped(self, monthly: f64) -> Self`; `fn ahead(self, monthly: f64, steps: u32) -> Self` (repeated `stepped`)
  - `pub fn monthly_growth_factor(annual: f64) -> f64` = `1.0 + annual / 12.0`
  - `RegionDefinitionParts.demand_growth_per_year: f64`; `RegionDefinition::demand_growth_per_year(&self) -> f64`; `RegionDefinition::monthly_demand_growth(&self) -> f64`
  - `RegionDefinition::try_from` rejects a rate outside `0.0..=0.2` or non-finite with the existing `RegionError` invalid-field variant (`field: "demand_growth_per_year"`)
  - Fixtures default the rate to `0.0`

- [ ] **Step 1: Write the failing tests** in `growth.rs`:
  - `none_is_bit_exact`: proptest over finite `v in -1e7..1e7`, `DemandGrowth::NONE.apply(Megawatts::new(v)).value().to_bits() == v.to_bits()`
  - `ahead_is_repeated_stepping`: `NONE.ahead(m, 3) == NONE.stepped(m).stepped(m).stepped(m)` for `m = monthly_growth_factor(0.033)`
  - `monthly_factor_is_nominal`: `monthly_growth_factor(0.024) == 1.002`
  - In `region` tests: `rate_outside_bounds_is_rejected` for `-0.01`, `0.21`, `f64::NAN`; `fixture_rate_is_zero`
- [ ] **Step 2:** Run `cargo nextest run -p domain growth` → FAIL (module missing).
- [ ] **Step 3:** Implement the type, the region field, getters, validation and fixture defaults.
- [ ] **Step 4:** Run `cargo nextest run -p domain` → PASS.
- [ ] **Step 5:** Commit `feat: add demand growth factor and region rate`.

### Task 2: Region schema 7

**Files:**
- Modify: `crates/infrastructure/src/region/dto.rs:367-450` (`RegionFileV4`: `demand_growth_per_year: Option<f64>` with `default, with = "present_option", skip_serializing_if = "Option::is_none"`, like `weather` at `:411-417`)
- Modify: `crates/infrastructure/src/region/mapping.rs:602-611` (`check_v4_geography`: accept `4..=7`; schema ≥ 7 requires the field, < 7 rejects it), `:793-799` (map to `parts.demand_growth_per_year`, `0.0` when absent), legacy `map_region` `:28-102` (`0.0`)
- Modify: `crates/infrastructure/src/region/catalog.rs:154,163` (`4..=7`)
- Modify: `ARCHITECTURE.md:46` (region schema is 7)
- Test: `crates/infrastructure/tests/region_catalog.rs` (next to `region_schema6_requires_weather_and_schema5_defaults_to_one`, `:1671`)

**Interfaces:**
- Consumes: Task 1 `RegionDefinitionParts.demand_growth_per_year`.
- Produces: schema-7 RON files with `demand_growth_per_year: 0.019,` load.

- [ ] **Step 1: Write the failing tests:**
  - `region_schema7_requires_demand_growth`: take a shipped schema-6 file's text, set `region_schema: 7`, delete nothing else → `RegionError::MissingField { field: "demand_growth_per_year" }` (assert the specific kind)
  - `region_schema6_rejects_demand_growth_and_defaults_to_zero`: schema 6 + the field → invalid `region_schema`; schema 6 without it → `demand_growth_per_year() == 0.0`
  - `region_schema7_maps_demand_growth`: schema 7 with `0.033` → `0.033`
  - `region_schema8_is_unsupported`
- [ ] **Step 2:** Run `cargo nextest run -p infrastructure region_schema` → FAIL.
- [ ] **Step 3:** Implement DTO, checks, mapping and catalog ranges.
- [ ] **Step 4:** Run `cargo nextest run -p infrastructure` → PASS (shipped files are still schema 6).
- [ ] **Step 5:** Commit `feat: add region schema 7 with demand growth rate`.

### Task 3: Apply growth to every demand computation; persist it (state schema 11)

**Files:**
- Modify: `crates/domain/src/demand/mod.rs:59-69` (`forecast_demand` gains `growth: DemandGrowth`; returns `growth.apply(sum)`)
- Modify: `crates/domain/src/demand/forecast.rs:20-38` (`hourly_forecast` gains `growth: DemandGrowth`, passes it through)
- Modify: `crates/domain/src/grid/power_system.rs` (field `demand_growth: DemandGrowth`, `NONE` in `new` at `:185`; `pub fn demand_growth(&self) -> DemandGrowth`; `pub fn set_demand_growth(&mut self, growth: DemandGrowth)`; `connected_demand_forecast` `:1356` passes `self.demand_growth`; encoder `:1475-1514` / decoder `:1515+` gated `schema >= 11`, default `NONE`)
- Modify: `crates/domain/src/grid/power_system/islands.rs:466` (scoped island copies the factor), `grid/power_system/battery.rs:159` (pass `self.demand_growth`)
- Modify: `crates/domain/src/session/connection.rs:94` (pass `self.power.demand_growth()` for now; Task 5 makes it look ahead), `:178` (`connection_power` copies the factor; Task 5 replaces it with the look-ahead)
- Modify: `crates/domain/src/session/views.rs:421,525` (pass `self.power.demand_growth()`)
- Modify: `crates/domain/src/grid/power_system.rs:1433-1449` (`projected_connected_peak` passes `self.demand_growth` for now; Task 5 changes its signature)
- Modify: `crates/domain/src/state.rs:8,12` (`CURRENT_STATE_SCHEMA = 11`, `SUPPORTED_STATE_SCHEMAS = 1..=11`), asserts at `state.rs:397`, `application/tests/save_resume.rs:66`, `infrastructure/tests/save_codec.rs:774`; old-schema round-trip tests (grep `state_bytes_schema(` / `from_state_bytes_schema(` with literal schemas)
- Test: `crates/domain/src/grid/power_system/tests.rs`, `crates/domain/src/session/tests.rs`

**Interfaces:**
- Consumes: Task 1 `DemandGrowth`.
- Produces: `forecast_demand(cities, region, month, tick, growth: DemandGrowth) -> Megawatts`; `hourly_forecast(cities, region, month, growth: DemandGrowth) -> [ForecastPoint; 24]`; `PowerSystem::demand_growth() / set_demand_growth(..)`; `GameSession::demand_growth(&self) -> DemandGrowth` (delegates to `power`).

- [ ] **Step 1: Write the failing tests:**
  - `connected_demand_scales_by_the_growth_factor` (power_system tests): set factor `1.05`; `connected_demand_forecast(..)` equals `1.05 ×` the `NONE` value for 24 sampled ticks
  - `live_tick_demand_scales_by_the_growth_factor`: two sessions, same seed, one with `set_demand_growth` `1.05` after start; after 60 ticks each tick summary's demand is `1.05 ×` (within 1e-9 relative)
  - `connection_preview_carries_the_growth_factor`: `connection_warning` added peak for an unconnected city scales by the factor
  - `scoped_island_carries_the_growth_factor`
  - `demand_growth_round_trips_at_schema_11` and `schema_10_state_reads_no_growth`
- [ ] **Step 2:** Run `cargo nextest run -p domain growth` → FAIL.
- [ ] **Step 3:** Implement; fix every call site the compiler reports (tests pass `DemandGrowth::NONE`).
- [ ] **Step 4:** Run `cargo nextest run --workspace`, then `cargo nextest run -p infrastructure -E 'test(prechange) | test(capture)'` → PASS with no golden changes.
- [ ] **Step 5:** Commit `feat: apply demand growth to forecast and live demand`.

### Task 4: Step growth at month close; `DemandGrowthStarted`; rules 34

**Files:**
- Modify: `crates/domain/src/session/month_close.rs:85-166` (after `build_report` pushes `MonthClosed`, before `change_phase(Review)`: `self.step_demand_growth(&mut events)`)
- Modify: `crates/domain/src/session/month_close.rs:322-331` (reuse `connected_cities()`; add `fn region_complete(&self) -> bool` = connected set equals all `region.cities()` ids)
- Modify: `crates/domain/src/events.rs` (`DemandGrowthStarted { annual_rate: f64 }`, tag `38` read only `if input.schema >= 11`)
- Modify: `crates/domain/src/lib.rs:23` (`SIM_RULES_VERSION = 34`); regenerate `application/tests/snapshots/rules_version_guard__*.snap` and `scenarios__*.snap` with `cargo insta review` (bodies should only change if a scenario completes its region)
- Test: `crates/domain/src/session/month_close/tests.rs`, `crates/application/tests/save_resume.rs`

**Interfaces:**
- Consumes: Task 3 `PowerSystem::set_demand_growth`, Task 1 `monthly_demand_growth()`.
- Produces: `DomainEvent::DemandGrowthStarted { annual_rate: f64 }`, emitted at the first stepping close only.

`step_demand_growth`: if `region.demand_growth_per_year() == 0.0` or not `region_complete()`, do nothing. Otherwise, if the current factor is exactly `NONE`, push `DemandGrowthStarted`; then set `factor.stepped(monthly)`.

- [ ] **Step 1: Write the failing tests** (tiny region parts with rate `0.024`, build lines to every city):
  - `no_growth_while_a_city_is_unconnected`: factor stays `NONE` across three closes
  - `growth_starts_at_the_close_that_connects_the_last_city`: that close emits `CityConnected` then `DemandGrowthStarted { annual_rate: 0.024 }`, factor `1.002`; the next close `1.002 * 1.002`; no second event
  - `month_report_shows_the_demand_actually_served`: the completing month's `MonthClosed` demand equals the pre-step demand
  - `zero_rate_never_steps_or_announces`
  - `resume_mid_growth_continues_exactly` (application): save after two growth closes, restore, run one more month; state hash equals the uninterrupted run
  - `review_precompute_agrees_after_a_growth_close` (application, `auto_commit` tests): the precompute taken in Review after a stepping close is reused (hash match) and its forecast shows the grown demand
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement; bump rules; `cargo insta review` the guard and scenario snapshots.
- [ ] **Step 4:** Run `cargo nextest run --workspace` and the prechange/capture filter → PASS.
- [ ] **Step 5:** Commit `feat: grow demand monthly once every city is connected`.

### Task 5: Forward projections see future growth

**Files:**
- Modify: `crates/domain/src/session/mod.rs` or a new `session/growth.rs` (`impl GameSession`)
- Modify: `crates/domain/src/grid/power_system.rs:1433-1449` (`projected_connected_peak(&self, region, first_month, first: DemandGrowth, monthly: f64)`: month `i` uses `first.ahead(monthly, i)`)
- Modify: `crates/domain/src/session/month_close.rs:56-81` (`review_inputs` passes the look-ahead)
- Modify: `crates/domain/src/session/outlook.rs:149-160` and `session/connection.rs:152-185` (`connection_power(network, offset, month)` sets `growth_ahead(offset)` on the projected system), `session/connection.rs:94` (`added_peak` uses the growth of the previewed month)
- Modify: `crates/domain/src/session/operational_outlook.rs`, `session/views.rs:714-756` (capacity shortfall) wherever a future month's demand is computed
- Test: `crates/domain/src/session/tests.rs`, `crates/domain/src/session/month_close/tests.rs`

**Interfaces:**
- Consumes: Tasks 3–4.
- Produces:
  - `GameSession::first_growth_close(&self) -> Option<u32>`: `Some(0)` if the region is complete now; else the smallest `j` such that all cities are connected on `network.projected_after(j + 1)` (bounded by the largest `months_left` of lines and links under construction); `None` if rate is 0 or never complete
  - `GameSession::growth_ahead(&self, offset: u32) -> DemandGrowth` = current factor `.ahead(monthly, offset.saturating_sub(first))` where `first = first_growth_close()`; current factor when `None`
  - Offset `k` = months after the current month (the month being played, or in Review the month just closed — match each caller's existing offset convention)

- [ ] **Step 1: Write the failing tests:**
  - `review_projects_next_years_grown_peak`: complete region, rate `0.024`; `review_inputs(december).rate.projected_peak` equals the max over the 12 months with month `i` scaled by `1.002^(i+1)` relative to the current factor
  - `december_completing_close_projects_growth`: the close that completes the region in December has a projected peak above the `NONE` projection by `1.002`× at least
  - `growth_built_ahead_is_not_held`: a plant sized for the grown projected peak + reserve margin raises no `CapitalHeld` at that review
  - `outlook_months_scale_with_growth` and `pending_line_starts_growth_in_the_outlook`: a line completing the region in 3 months shows growth only from offset 4 onward
  - `first_growth_close_is_none_at_zero_rate`
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run --workspace` → PASS.
- [ ] **Step 5:** Commit `feat: project future demand growth in reviews and outlooks`.

### Task 6: Growth in the game: toast, city card, city height

**Files:**
- Modify: `crates/domain/src/session/views.rs:90-112,406-446` (`CityDetail.growth: f64`, the current factor; `share` unchanged, `peak` already grown via Task 3)
- Modify: `crates/game/src/l10n/localize.rs:705+` (toast arm for `DemandGrowthStarted`, percentage to one decimal)
- Modify: `crates/game/src/hud/city_card.rs:60` (`city-base-load` shows `base_load × growth`; when `growth != 1.0` add stat `city-demand-growth` with the region rate)
- Modify: `crates/game/src/world/model.rs:355-380` (Planning height uses `base_load × growth`; `max_load` stays the authored maximum)
- Modify: `assets/locales/en-US/*.ftl`, `crates/game/src/l10n/keys.rs`, `crates/game/tests/game/l10n_coverage.rs`
- Test: `crates/game/tests/game/` (existing city card / world model / toast modules)

**Interfaces:**
- Consumes: Tasks 3–4.
- Produces: `.ftl` keys `toast-demand-growth-started = Every city is connected. Demand will now grow about { $percent }% a year.` and `city-demand-growth = Demand growth` (value: `{ $percent }% a year`).

- [ ] **Step 1: Write the failing tests:**
  - `demand_growth_started_toasts_once` (glue test through the real dispatch/toast path)
  - `city_card_shows_grown_base_load_and_rate`
  - `planning_city_height_grows_with_demand`: factor `1.1` → scale `1.1 ×` the `NONE` scale
  - l10n coverage includes the new keys
- [ ] **Step 2:** Run `cargo nextest run -p game` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run -p game`; then `cargo run -p game -- --exit-after-frames 120` exits 0.
- [ ] **Step 5:** Commit `feat: show demand growth on the map, city card and toast`.

### Task 7: Ship the regional growth rates

**Files:**
- Modify: all eight `assets/regions/*.ron` (`region_schema: 7`, `demand_growth_per_year`, a citation comment in the file's existing style)
- Modify: `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md` §2.4 (one paragraph pointing to the endgame design)
- Test: `crates/infrastructure/tests/shipped_regions.rs`; any shipped-region snapshots (`cargo insta review`)

Values (spec §6, sources verbatim there):

| Region | `demand_growth_per_year` |
|---|---|
| `eastern_interconnection` | `0.019` |
| `western_interconnection` | `0.022` |
| `brazil_sin` | `0.033` |
| `continental_europe_maghreb` | `0.024` |
| `india_nepal_bhutan` | `0.053` |
| `ips_ups` | `0.026` |
| `sgcc` | `0.052` |
| `southern_china` | `0.052` |

- [ ] **Step 1: Write the failing test** `shipped_regions_declare_their_growth_rates`: each region id maps to the value above.
- [ ] **Step 2:** Run `cargo nextest run -p infrastructure shipped_regions` → FAIL.
- [ ] **Step 3:** Edit the eight files; re-check the low-confidence rows (`ips_ups`, `southern_china`) against their sources and note any change in the spec table.
- [ ] **Step 4:** Run `cargo nextest run --workspace` (long shipped-region tests may need the hands-off plan, see TESTING.md) and the launch smoke → PASS.
- [ ] **Step 5:** Commit `feat: ship real-world demand growth rates for every region`.

---

## Part B — The green milestone

### Task 8: `FossilFreeYear` in the milestone projection (state schema 12)

**Files:**
- Modify: `crates/application/src/projections/milestones.rs`
- Modify: `crates/domain/src/state.rs:8,12` (`12`) and the same asserts as Task 3
- Test: `crates/application/src/projections/milestones.rs` tests

**Interfaces:**
- Produces:
  - `MilestoneKind::FossilFreeYear` (state tag `4`, read only `if input.schema >= 12`); `same_objective` matches it with itself
  - `MilestoneProgress::CleanMonths { months: u32, needed: u32 }` (tag `3`, `schema >= 12`)
  - `pub const FOSSIL_FREE_MONTHS: u32 = 12;` `const FOSSIL_EPSILON_MWH: f64 = 1e-9;`
  - Fields `clean_run: u32` (default `0`) and `bonus_revealed: Option<SimMonth>` (default `None`), persisted `since12`. Replace the `state_record!` call with a hand-written `State` impl carrying the `since10 { goals }` and `since12 { clean_run, bonus_revealed }` groups.
  - `pub fn bonus(&self) -> Option<MilestoneKind>`: `Some(FossilFreeYear)` once revealed
  - `pub fn month_is_clean(report: &MonthlyReport, struck: bool) -> bool`

Rules: in `close_month`, take `strike_pending` once and use it for both the season run and `clean_run` (`+1` if clean, else `0`). In `check`, after the three goals: if all three are reached and `bonus_revealed` is `None`, set it to `month`. Then, if revealed, not yet reached, and `clean_run >= 12`, push `(FossilFreeYear, month)` to `reached` and `newly_reached`. `status(FossilFreeYear)` gives `CleanMonths { months: clean_run, needed: 12 }`.

- [ ] **Step 1: Write the failing tests:**
  - `bonus_hidden_until_three_reached`; `bonus_revealed_in_the_month_the_third_is_reached`
  - `run_counts_before_reveal_and_awards_at_reveal`: 14 clean months, three goals completed at month 14 → reached in month 14
  - `run_of_five_at_reveal_awards_seven_closes_later`
  - `fossil_energy_resets_the_run`: `Ccgt` at `0.5` MWh; `zero_energy_fossil_entries_do_not_reset` (`Coal: 0.0`); `energy_at_epsilon_does_not_reset` (`1e-9`)
  - `strike_resets_the_run`; `frequency_notice_does_not_reset_the_run`
  - `bonus_state_round_trips_at_schema_12`; `schema_11_state_reads_empty_run_unrevealed`
- [ ] **Step 2:** Run `cargo nextest run -p application milestones` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run --workspace` → PASS.
- [ ] **Step 5:** Commit `feat: add the hidden fossil-free year milestone`.

### Task 9: Seed the run for old saves

**Files:**
- Modify: `crates/application/src/projections/milestones.rs` (seeding)
- Modify: `crates/application/src/game.rs:171-184` (`if snapshot.schema < 12 { projections.milestones.upgrade_fossil_free(&projections.monthly_reports, &session) }`, after the `< 10` upgrade)
- Test: milestones tests; `crates/infrastructure/tests/save_snapshots.rs` (older-schema byte tests mirror the upgrade)

**Interfaces:**
- Consumes: Task 8; `MonthlyReportProjection::history()`.
- Produces:
  - `pub fn month_struck(report: &MonthlyReport, strike_threshold: Ratio) -> bool` = `(report.demand.value() > 0.0 && report.unserved.value() / report.demand.value() > strike_threshold.value()) || (report.frequency_failed && !report.frequency_notice)` (mirrors `Regulator::assess_month`, `regulation/mod.rs:161-200`)
  - `pub(crate) fn upgrade_fossil_free(&mut self, reports: &MonthlyReportProjection, session: &GameSession)`: `clean_run` = trailing run of `month_is_clean(r, month_struck(r, session.region().regulator().strike_threshold))` over `history()`; then `check(session.clock().month(), ..)` and truncate `newly_reached` to its prior length (no notification)

- [ ] **Step 1: Write the failing tests:**
  - `month_struck_matches_the_regulator`: proptest over demand, unserved, failed and notice flags, comparing against a `Regulator` fed the same month (fresh regulator, no notice history, so set `frequency_notice` from what it actually issued)
  - `seeded_run_equals_live_run`: play a scripted game (tiny region, fossil plants stopped from month 3, one month with forced unserved over the threshold); save at schema 11 bytes; restore; `clean_run` equals the run tracked live
  - `old_save_with_three_reached_reveals_and_awards_silently`: `newly_reached` unchanged, `reached` contains `FossilFreeYear` at the restored month when the seeded run ≥ 12
  - Update older-schema byte tests to apply the same upgrade before comparing
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run --workspace` → PASS.
- [ ] **Step 5:** Commit `feat: seed the fossil-free run for older saves`.

### Task 10: Milestones panel row and milestone toasts

**Files:**
- Modify: `crates/game/src/hud/milestones.rs:49-67` (rows = `targets()` then `bonus()` if `Some`)
- Create: `crates/game/src/hud/milestone_toasts.rs` (announcer system), register in `crates/game/src/hud/mod.rs`
- Modify: `crates/game/src/hud/toasts.rs:112-138` (`collect_toasts` also reads `MilestoneToast` messages)
- Modify: `crates/game/src/l10n/localize.rs:496-541` (`FossilFreeYear`, `CleanMonths`)
- Modify: `assets/locales/en-US/ui.ftl:44-50`, `crates/game/src/l10n/keys.rs`, `crates/game/tests/game/l10n_coverage.rs:401-425`
- Test: `crates/game/tests/game/` milestone module

**Interfaces:**
- Consumes: Task 8 `bonus()`, `reached()`.
- Produces:
  - `#[derive(Message)] pub struct MilestoneToast(pub MilestoneNotice)`; `pub enum MilestoneNotice { Revealed(MilestoneKind), Reached(MilestoneKind) }`
  - `#[derive(Resource, Default)] struct MilestoneCursor { generation: Option<u64>, reached: usize, revealed: bool }`
  - `fn announce_milestones(session: Option<Res<Session>>, generation: Res<SessionGeneration>, mut cursor: ResMut<MilestoneCursor>, mut out: MessageWriter<MilestoneToast>)`, ordered after `apply_player_commands`. When `generation` differs from the cursor's, it adopts the current counts without emitting; this covers new game, load, resume and replay. Otherwise it emits `Reached` for each new `reached()` entry and `Revealed` when `bonus()` turns `Some`. It never mutates the session.
  - `.ftl`:
    - `milestone-fossil-free-year = Run a full year on clean power, without a strike`
    - `milestone-progress-clean-months = { $months } of { $needed } clean, strike-free months`
    - `toast-milestone-revealed = New milestone: { $name }`
    - `toast-milestone-reached = Milestone reached: { $name }`

- [ ] **Step 1: Write the failing tests:**
  - `bonus_row_appears_only_after_reveal`: the panel has 3 rows before and 4 after
  - `milestone_reached_toasts_once_through_the_real_dispatch` (any regular milestone)
  - `bonus_reveal_and_award_toast_once`
  - `loading_a_save_raises_no_milestone_toasts` (including an old save awarded on restore)
  - `milestones_panel_fits_four_rows_at_min_window_and_max_zoom`
  - l10n coverage for the new kinds and progress
- [ ] **Step 2:** Run `cargo nextest run -p game` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo nextest run --workspace`, clippy, fmt check, `cargo run -p game -- --exit-after-frames 120` → all green.
- [ ] **Step 5:** Commit `feat: show the green milestone and toast milestones`.

---

## Final verification

- [ ] `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`
- [ ] `cargo nextest run -p infrastructure -E 'test(prechange) | test(capture)'` (golden continuations unchanged)
- [ ] `cargo run -p game -- --exit-after-frames 120` exits 0
- [ ] Play one shipped region to completion (or load a completed save created after Task 7): the growth toast appears, cities grow taller, and the outlook rises month over month
