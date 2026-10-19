# Megawatt Mogul Plan 2: Bevy Game Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A playable Milestone 1 game. It runs from the main menu through region choice and a seeded
new game into the full Planning → Operating → Review cycle. Every phase is driven by an egui HUD
over the finished simulation core, and all text comes from Fluent en-US.

**Architecture:** This plan adds the `crates/game` Bevy binary (spec §4.1, §8.1) with four
plugins: `AppStatePlugin`, `SimulationPlugin`, `LocalizationPlugin` and `HudPlugin`.
- The game owns no rules. It reads only the query views and projections, and it writes only by
  sending `PlayerCommand` messages through the existing `Game` pipeline.
- **Presenter pattern.** Each HUD panel is a pure *presenter*: `fn …_model(&Game, &Localizer, &UiState) -> …Model`.
  Its egui drawing function turns the model into widgets and returns `Vec<UiAction>`.
  `UiAction`s map to `PlayerCommand`s in one place. Presenters and the action mapping are
  unit-tested without a window. Plugins are tested in a headless `App`.
- **Selection without the diorama.** Plan 2 gives cell and plant selection a 2D egui map.
  *(Builder's call.)* Plan 3 adds the 3D diorama, overlays, camera, dev tools and 3D picking, which
  writes the same `Selection` resource.

**Tech Stack:**
- Rust 1.98, edition 2024
- Bevy 0.20 (MSRV 1.97.1)
- bevy_egui 0.43.1 (egui 0.36)
- egui_plot 0.37.0. Keep it pinned: 0.38 exists but tracks a newer egui.
- fluent-bundle 0.16 with unic-langid 0.9
- tracing 0.1

All of these were checked to resolve on 2026-10-19.

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`:
- §2 (player experience), §3.1 (scope), §4 (architecture);
- §6.3 (queries/projections), §8.1 (plugins), §8.4 (HUD by phase), §9 (cross-cutting);
- §10.1 (`game/tests/`);
- §12.6, §14.3 and §15.5 (the Planning queries the HUD shows).

Also read `docs/superpowers/plans/2026-10-18-plan-1-followups.md` ("Carry into Plan 2" and the watch
items).

## Global Constraints

**Dependency rule and boundaries**
- `game` depends on `application`, `infrastructure` and `domain`, plus Bevy, bevy_egui, egui_plot,
  fluent-bundle, unic-langid and tracing.
- No other crate gains a Bevy dependency. `cargo test -p domain` and `cargo test -p application`
  must not compile Bevy.
- "The presentation layer never reads aggregate internals." The HUD reads only `GameSession`
  query methods (`plant_views`, `city_views`, `grid_snapshot`, `company_view`, `hourly_forecast`,
  `hydro_schedule`, `minimum_generation_check`, `capacity_shortfall_check`,
  `rate_review_estimate`, `region()`) and `Game::projections()`.
- The HUD writes only by sending `PlayerCommand`s.

**Text and errors**
- "The domain never produces user-facing text." All display text comes from Fluent
  `assets/locales/en-US/*.ftl`. Region data refers to translation keys (`name_key`).
- "A `CommandError` becomes a localised toast. This is the only place errors are mapped for
  display."

**Simulation pacing**
- "`FixedUpdate` runs `AdvanceTicks { n: speed }`." Speeds are pause, 1×, 2×, 4× and 8×.
- "At 1× a day takes about 6 real minutes." So `FixedUpdate` runs at **4 Hz**, and `n` equals the
  speed multiplier (1, 2, 4 or 8).

**Region picker**
- "The region picker is skipped automatically when exactly one region exists. With two regions it
  is shown."

**Determinism**
- The simulation advances only through commands, which are logged.
- Wall-clock time and entropy are allowed in `game` only for UI timing (toasts), the default seed
  and log file names. They never reach the domain.

**Workspace conventions**
- Workspace conventions from Plan 1 apply: `#![forbid(unsafe_code)]`, `license = "AGPL-3.0-only"`,
  shared lints, conventional commits, and nextest, clippy `-D warnings` and `fmt --check` green
  before every commit.
- The `game` crate is excluded from mutation testing (spec §10.4).

**Tests**
- Tests are hermetic: a temp dir per I/O test, no fixed paths, no environment variables, no
  current-directory changes (spec §10.5).
- Paths always reach code as arguments or resources.

## Review Focus

1. **A command queued across a phase change in the same frame.** For example, a target change sent
   in the frame the month closes. The domain rejects it with `WrongPhase`, it becomes one toast, and
   nothing panics or is applied. Test in Task 4.
2. **Game over mid-day or at month close** (bankruptcy, licence revoked). The fixed-step driver stops
   issuing `AdvanceTicks`, so there is no stream of repeated `GameOver` toasts. A game-over modal is
   shown. Test in Task 4.
3. **A broken or missing region file.**
   - The picker lists it as unavailable, with its error, and still offers the good region.
   - An empty or missing regions directory shows a message instead of panicking.
   - Auto-skip happens only when the listing has exactly one entry and that entry is available.

   Test in Tasks 2 and 5.
4. **The command log failing mid-game** (for example, disk full). The game keeps running, and a
   persistent banner says recording has stopped. Test in Tasks 2 and 4.
5. **A missing translation.**
   - The HUD shows the key itself and logs one warning. It never panics.
   - A coverage test proves every key the code uses exists in en-US, including every shipped
     region's city and region `name_key`.

   Test in Task 3.

---

### Task 1: The `game` crate skeleton, app states and CI

**Files:**
- Create: `crates/game/Cargo.toml`, `crates/game/src/main.rs`, `crates/game/src/lib.rs`, `crates/game/src/state.rs`, `crates/game/src/paths.rs`, `crates/game/tests/support/mod.rs`, `crates/game/tests/states.rs`
- Modify: the workspace `Cargo.toml` (members and `[workspace.dependencies]`), `.github/workflows/ci.yml`, `.gitignore` (`logs/`), `README.md` (how to run)
- Test: `crates/game/tests/states.rs`

**Interfaces:**
- Produces:
  - **App states:** `pub enum AppState { #[default] Boot, MainMenu, RegionSelect, InGame }` (Bevy `States`).
  - **Phase sub-state:** `pub enum PhaseState { #[default] Planning, Operating, Review }`, a Bevy
    `SubStates` with source `AppState::InGame`. It mirrors the domain `DayPhase` one way only, and
    Task 4 drives it.
  - **Asset paths:** `pub struct AssetRoot(pub PathBuf)` resource and `pub struct LogDir(pub PathBuf)` resource.
    - `paths::resolve_asset_root(arg: Option<PathBuf>) -> PathBuf` returns the first of: the arg,
      `./assets`, `<exe dir>/assets`, `env!("CARGO_MANIFEST_DIR")/../../assets`.
    - The log dir defaults to `./logs`.
    - Both come from CLI flags `--assets <dir>` and `--log-dir <dir>`, parsed with `std::env::args`.
      Don't add a CLI crate.
  - **`pub struct CorePlugins`** (a `PluginGroup`): `AppStatePlugin`, later `LocalizationPlugin`
    and `SimulationPlugin`. It is headless-safe, so it needs no window or renderer.
  - **`pub struct GamePlugins`:** `CorePlugins` plus `EguiPlugin` and `HudPlugin`. `main.rs` adds
    `DefaultPlugins` (window title "Megawatt Mogul") and `GamePlugins`.
  - **`AppStatePlugin`:** `Boot` moves to `MainMenu` once the boot resources exist. In Task 1 that
    is just `AssetRoot` and `LogDir`; later tasks add to the boot checklist through a
    `BootChecklist` resource.
  - **Test support:** `tests/support/mod.rs` `pub fn headless_app(assets: &Path, logs: &Path) -> App`.
    It uses `MinimalPlugins`, `StatesPlugin` and `CorePlugins`, with the given paths inserted.
- **Bevy features:** set `default-features = false`. Enable only what a windowed egui app needs:
  window/winit, render, core pipeline, state, multi-threaded, and the platform backends
  (X11/Wayland on Linux). No audio, which is out of scope (§3.3).
  - Bevy 0.20 renamed features in past releases, so look up the 0.20 feature list on docs.rs and
    record the chosen set, with one line each, in the crate's `Cargo.toml` comments.
  - bevy_egui uses its default features.
- **CI:** the build job installs Bevy's Linux build dependencies, per Bevy 0.20's `linux_dependencies.md`
  (at least `libudev-dev`, `libwayland-dev`, `libxkbcommon-dev`). Game tests run headless, with no
  display.
- **API note:** buffered events are *messages* in Bevy ≥ 0.17: `#[derive(Message)]`, `add_message`,
  `MessageWriter` and `MessageReader`. egui systems run in bevy_egui's
  `EguiPrimaryContextPass`. Check both against the pinned docs, and record any rename in
  `ARCHITECTURE.md` (Task 10).

- [ ] **Step 1: Write the failing tests:**
  - `boot_moves_to_the_main_menu`: after a few `app.update()`s, `State<AppState>` is `MainMenu`.
  - `the_phase_substate_exists_only_in_game`: `PhaseState` is absent outside `InGame`, and is
    `Planning` on entering `InGame`.
  - `asset_root_prefers_the_explicit_argument`: a temp dir is passed.
  - `asset_root_falls_back_to_the_workspace_assets`: the fallback contains `regions/`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game`. Expected: FAIL (compile).
- [ ] **Step 3: Implement** per Interfaces. `cargo run -p game` opens a window showing the
  (still empty) main menu.
- [ ] **Step 4: Verify.**
  - Run `cargo nextest run -p game`; expect PASS.
  - Run `cargo nextest run -p domain -p application` and check that Bevy is not compiled:
    `cargo tree -p application | grep -c bevy` must be 0.
  - Run `cargo tree -d -p game | grep egui` and check there is a single egui version.
- [ ] **Step 5: Commit**: `feat(game): crate skeleton, app states and CI`

---

### Task 2: Read-side completions in application and infrastructure

These are the carry-overs the HUD needs (follow-ups "Carry into Plan 2"). No rules change:
`SIM_RULES_VERSION` stays 3, and no snapshot may change.

**Files:**
- Modify: `crates/application/src/ports.rs`, `crates/application/src/game.rs`, `crates/application/src/pipeline/{mod.rs,command_log_layer.rs}`, `crates/application/src/projections/{timeline.rs,plant_history.rs}`
- Modify: `crates/infrastructure/src/region/catalog.rs`, `crates/infrastructure/src/command_log/dto.rs`
- Modify: `crates/domain/src/session/views.rs`, `crates/domain/src/grid/power_system.rs` (minimum-generation evaluator), and a line-quote query (where `BuildLine` prices a line)
- Modify: spec §6.3 (one builder's-call line: "price" becomes system marginal cost) and the follow-ups doc (mark each carry-over done)
- Test: inline tests, `crates/infrastructure/tests/region_catalog.rs`

**Interfaces:**
- **Region listing (Review Focus 3).**
  - `RegionCatalog::list(&self) -> Result<Vec<RegionListing>, RegionLoadError>`, where `pub struct RegionListing { pub id: RegionId, pub entry: Result<RegionSummary, RegionLoadError> }`.
  - Directory-level failures stay `Err`.
  - `RonRegionCatalog::list` deserializes each file's DTO only, for `id` and `name_key`. It must
    not run the mapping or rasterisation; `load` still validates fully.
  - `auto_select_region(listing: &[RegionListing]) -> Option<&RegionId>` returns `Some` only when
    there is exactly one entry and it is `Ok`.
- **Command-log health (Review Focus 4).**
  - `build_pipeline(sink) -> (Box<dyn CommandHandler>, LogHealth)`, where `pub struct LogHealth`
    is a cloneable handle (`Arc<Mutex<Option<CommandLogError>>>`) with
    `fn failure(&self) -> Option<CommandLogError>`.
  - `CommandLogLayer` records its **first** append error there and keeps running the game.
  - `Game::log_failure(&self) -> Option<CommandLogError>`.
- **Timeline marginal cost.** `TimelinePoint.marginal_cost: Option<Price>` is the highest
  `marginal_cost` among connected plants producing more than 0 in that tick, or `None`.
  *(Builder's call, recorded in §6.3: under a regulated tariff the per-tick "price" the merit
  order teaches is the system marginal cost.)*
- **Plant revenue.** `PlantHistory.revenue: Money` adds, per closed month, the plant's weighted
  merit-order energy × the tariff in force that month. Round once per plant per month
  (`round_ties_even`). It is a read-side figure and not a ledger entry.
- **Minimum-generation check (follow-ups watch item).**
  - `minimum_generation_check` samples each hour at minute 0 and minute 59 through the A4
    `HourEnd` evaluator, as `capacity_shortfall_check` does.
  - It reports the worse end's `committed_minimum`, `net_load` and `excess`.
  - Update the `MinimumGenerationWarning` docs and §14.3 wording.
- **Line quote.** `GameSession::line_quote(&self, from: NodeId, to: NodeId) -> Result<LineQuote, DomainError>`
  is pure. `pub struct LineQuote { pub cells: u32, pub capital: Money, pub months: u32 }` uses the
  same length and cost rule `BuildLine` charges, and returns the same errors `BuildLine` would
  (unknown node, line to self, duplicate).
- **DTO finiteness.** The command-log DTO → `Command` mapping rejects a non-finite
  `SetDispatchTarget.mw` with a typed error instead of building a NaN `Megawatts`.

- [ ] **Step 1: Write the failing tests:**
  - `a_broken_region_file_is_listed_as_unavailable` (temp dir with one good file and one garbage
    file → two listings, one `Err`);
  - `listing_does_not_rasterise` (a file that parses as a DTO but fails mapping is listed `Ok` and
    fails only at `load`);
  - `auto_select_requires_exactly_one_available_entry` (one ok → Some; one ok + one broken → None;
    one broken → None);
  - `a_failing_sink_is_reported_and_the_game_continues` (a sink whose `append` fails on the 2nd
    call → `log_failure()` is `Some` after it, and later commands still succeed);
  - `timeline_marginal_cost_is_the_most_expensive_producer`;
  - `plant_revenue_is_energy_times_tariff_rounded_once`;
  - `minimum_generation_check_sees_a_trough_at_an_hour_boundary` (a shaped day whose trough sits on
    an hour knot);
  - `line_quote_matches_what_build_line_charges` (quote, then build, then compare capital and months
    with the ledger and the construction event);
  - `non_finite_dispatch_target_is_rejected_in_the_log`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain -p application -p infrastructure`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement** per Interfaces, updating every caller of the changed signatures.
- [ ] **Step 4: Verify.** Run the full `cargo nextest run --workspace -E 'not package(game)'`. It
  must PASS **with no snapshot change**. Then run clippy and fmt.
- [ ] **Step 5: Commit**: `feat: region listing, log health, marginal cost, plant revenue, line quote`

---

### Task 3: Localisation (`LocalizationPlugin`)

**Files:**
- Create: `crates/game/src/l10n/{mod.rs,format.rs,keys.rs}`
- Create: `assets/locales/en-US/{ui.ftl,errors.ftl,events.ftl,lessons.ftl,regions.ftl}`
- Test: inline tests in `l10n`, plus `crates/game/tests/l10n_coverage.rs`

**Interfaces:**
- Produces:
  - **`Localizer` resource.**
    - Loads every `.ftl` under `<AssetRoot>/locales/en-US/` with `std::fs` at Boot. It is
      registered in `BootChecklist`.
    - Uses `fluent_bundle::concurrent::FluentBundle`, which is `Send + Sync` for a Bevy resource.
    - Set `use_isolating(false)`.
    - `pub fn text(&self, key: &str) -> String` and `pub fn args(&self, key: &str, args: &[(&str, FluentArg)]) -> String`,
      where `pub enum FluentArg { Text(String), Number(f64) }`.
    - **Missing key:** return the key and `tracing::warn!` once per key. Never panic.
    - **ftl syntax errors at load:** fail Boot with a visible error screen in Task 5. Don't panic.
  - **`pub trait Localize { fn localize(&self, l10n: &Localizer) -> String; }`.** Implement it for:
    - `CommandError`, including every `DomainError`, `RegionLoadError` and `CommandLogError` variant;
    - `Lesson`;
    - `IncidentKind`;
    - `StrikeReason`;
    - `GameOverReason`;
    - `PlantKind`;
    - `PhaseState`;
    - `Season`;
    - `MilestoneKind`;
    - `SiteError`;
    - the toast-worthy `DomainEvent`s: `Blackout`, `BlackoutRestored`, `RotatingOutageOrdered`,
      `StrikeIssued`, `FrequencyNoticeIssued`, `StrikeExpired`, `LicenseRevoked`, `Bankrupt`,
      `PlantTripped`, `OverFrequencyTrip`, `HydroBudgetExhausted`, `CityConnected`,
      `PlantCommissioned`, `LineCommissioned`, `RateReviewCompleted`.

    Use exhaustive `match`es, with no `_` arm, so a new variant fails to compile until it is
    translated.
  - **Display formatters** in `format.rs`, pure and en-US:
    - `money(Money) -> String` (`$1.23M`, `$45.6k`, `−$2.00B`);
    - `megawatts(Megawatts)` (`1,234 MW`, `12.3 GW` at ≥ 10,000 MW);
    - `megawatt_hours`;
    - `hertz_deviation` (`+0.12 Hz`);
    - `percent(Ratio)` (`99.4%`);
    - `price(Price)` (`$45.00/MWh`);
    - `clock(tick) -> "17:40"`;
    - `month_year(SimMonth) -> "March 2027"`.

    These pass into Fluent as `FluentArg::Text`. *(Builder's call: Fluent's NUMBER needs ICU data
    the crate set doesn't ship.)*
  - **`keys.rs`:** `pub const ALL_UI_KEYS: &[&str]` lists every literal key the HUD uses, so the
    coverage test can check them.
- `LocalizationPlugin` inserts `Localizer` and marks the boot item done.

- [ ] **Step 1: Write the failing tests:**
  - `every_ui_key_exists_in_en_us` (iterates `ALL_UI_KEYS`);
  - `every_error_and_lesson_variant_translates_without_falling_back`. Build one value of each
    variant and assert the result isn't the bare key. Enumerating variants is a test-side table;
    the exhaustive match keeps it honest.
  - `every_shipped_region_and_city_name_key_exists` (reads both shipped RON files through
    `RonRegionCatalog`);
  - `missing_key_returns_the_key`;
  - `ftl_files_parse_without_errors`;
  - formatter tables (`money_formats_scale`, `clock_formats_ticks`, …).
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game l10n`. Expected: FAIL.
- [ ] **Step 3: Implement**, and write the en-US copy: plain, short, teaching-first. Use the spec's
  own phrasing where it exists, e.g. the ramp warning in §6.3.
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): Fluent localisation with en-US coverage`

---

### Task 4: The simulation bridge (`SimulationPlugin`)

**Files:**
- Create: `crates/game/src/sim/{mod.rs,driver.rs,new_game.rs}`
- Test: `crates/game/tests/simulation.rs`

**Interfaces:**
- Consumes: `Game`, `Command`, `RonRegionCatalog`, `FileCommandLog`, `LogHealth`/`Game::log_failure` (Task 2), `AssetRoot`/`LogDir`/`PhaseState` (Task 1).
- Produces:
  - **Session resource.** `pub struct Session(pub Game)`. It exists only in `AppState::InGame`.
    It is removed on `OnExit(InGame)`.
  - **Messages:**
    - `pub struct PlayerCommand(pub Command)`;
    - `pub struct CommandFailed(pub CommandError)`;
    - `pub struct DomainEventMessage(pub DomainEvent)`;
    - `pub struct NewGameRequest { pub region: RegionId, pub seed: u64 }`;
    - `pub struct ReturnToMenu`.
  - **`apply_player_commands`** (in `Update`, in a `SimSet::Apply` system set):
    - Drains `PlayerCommand`s **in order** and calls `Game::execute` for each.
    - On `Ok`, writes one `DomainEventMessage` per event, skipping `TickCompleted`, which the HUD
      reads from projections instead.
    - On `Err`, writes `CommandFailed`.
    - It never panics.
  - **`SimSpeed` resource.** `pub enum SimSpeed { #[default] Paused, X1, X2, X4, X8 }`, with
    `fn ticks(self) -> u32` returning 0, 1, 2, 4 or 8.
    - `Time<Fixed>` is set to 4 Hz on `OnEnter(InGame)`.
    - Entering Operating sets the speed to `X1`. Leaving Operating sets it to `Paused`.
  - **`drive_ticks`** (in `FixedUpdate`):
    - It sends `PlayerCommand(AdvanceTicks { n })` only when every one of these holds:
      - the phase is `Operating`;
      - the speed is not `Paused`;
      - the session status is `Active`;
      - no `AdvanceTicks` is still unapplied (one in flight at most).
    - Once the session is no longer `Active`, it stops for good, so there is no toast stream
      (Review Focus 2).
  - **`mirror_phase`** (after `SimSet::Apply`): sets `NextState<PhaseState>` from
    `game.session().phase()` when the two differ. This is the only writer of `PhaseState`.
  - **`start_new_game`**, on `NewGameRequest`:
    - Loads the region through the `Catalog` resource.
    - Creates `FileCommandLog` at `<LogDir>/<YYYYMMDD-HHMMSS>-<region>-<seed>.log`, making the
      directory if needed. Wall clock is allowed here.
    - Calls `Game::start_new(.., GameSettings::default(), sink)`.
    - On `Ok`, inserts `Session` and enters `InGame`. On `Err`, writes `CommandFailed` and stays in
      the menu.
  - **`ReturnToMenu`** removes `Session` and enters `MainMenu`.
  - **`Catalog` resource.** `pub struct Catalog(pub RonRegionCatalog)`, inserted at Boot from
    `<AssetRoot>/regions` and registered in `BootChecklist`. `start_new_game` and the menu (Task 5)
    use it, so no one else builds a catalog.
  - **`pub fn default_seed() -> u64`:** derived from `SystemTime` nanos, shown and editable in the
    menu.

- [ ] **Step 1: Write the failing tests** (headless app, temp assets dir copying `assets/regions` and `assets/locales`, temp log dir):
  - `a_new_game_request_enters_planning_and_writes_a_log` (the log and `.region.ron` archive exist
    in the temp dir);
  - `commands_apply_in_order_and_re_emit_events`;
  - `a_rejected_command_becomes_one_command_failed`;
  - `commands_queued_across_a_phase_change_are_rejected_cleanly`. In one frame, queue
    `AdvanceTicks { n: 1440 }` then `SetDispatchTarget`. Expect `Review`, one `WrongPhase`
    `CommandFailed`, and no panic (Review Focus 1).
  - `operating_advances_speed_ticks_per_fixed_step`. Drive `Time<Fixed>` manually, or use
    `TimeUpdateStrategy`. At `X4`, k fixed steps advance 4k ticks.
  - `pause_stops_ticks`;
  - `the_month_close_mirrors_review_and_pauses`;
  - `game_over_stops_the_driver` (Review Focus 2). Use an overreach-style fixture, or a session
    driven to bankruptcy or revocation through commands. After game over, further fixed steps send
    no `AdvanceTicks` and produce no `CommandFailed`.
  - `a_failing_log_keeps_the_game_running` (Review Focus 4). Use a read-only log dir after start,
    or inject a failing sink through a test-only constructor. Assert `log_failure().is_some()` and
    that ticks still advance.
  - `return_to_menu_drops_the_session`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game simulation`. Expected: FAIL.
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): simulation bridge with fixed-step batching and phase mirroring`

---

### Task 5: Shell screens, top bar, toasts and banners

**Files:**
- Create: `crates/game/src/hud/{mod.rs,action.rs,selection.rs,menu.rs,top_bar.rs,toasts.rs,game_over.rs}`
- Test: inline presenter tests, plus `crates/game/tests/hud_smoke.rs`

**Interfaces:**
- Consumes: `Localizer`/`Localize`/formatters (Task 3), messages and resources (Task 4), `RegionListing`/`auto_select_region` (Task 2).
- Produces:
  - **`Selection` resource** (`selection.rs`): `pub enum Selection { #[default] None, Cell(CellCoord), Plant(PlantId), City(CityId), LineFrom(NodeId) }`.
    It is reset on `OnEnter(InGame)`. Plan 3's 3D picking writes the same resource.
  - **`pub enum UiAction`.** Its variants are `Command(Command)`, `NewGame { region, seed }`,
    `ReturnToMenu`, `SetSpeed(SimSpeed)`, `Select(Selection)` and `Quit`.
    - `fn dispatch(actions, &mut writers…)` is the single mapping to messages.
    - Every panel returns `Vec<UiAction>`, and none writes messages directly.
  - **`HudPlugin`.** Registers the panel systems in `EguiPrimaryContextPass`, each gated by
    `AppState` and `PhaseState`.
  - **Main menu.** Contains *New game* (seed field prefilled with `default_seed()`) and *Quit*.
    *New game* lists regions with `RegionCatalog::list`:
    - when `auto_select_region` is `Some`, it emits `NewGame` directly;
    - otherwise it enters `RegionSelect`.
  - **Region select.** Each listing is a button showing its localised `name_key`. Unavailable
    entries are disabled and show their localised error. An empty listing or a directory error
    shows a localised message plus *Back* (Review Focus 3).
  - **Top bar** (all `InGame` phases). Shows `month_year`, the season, the phase, cash,
    reliability and satisfaction (from `company_view`), strikes, and a *Menu* button.
    - When `Game::log_failure()` is `Some`, a persistent warning badge reads "Recording stopped:
      …" (Review Focus 4).
  - **Toasts.**
    - `CommandFailed` and the toast-worthy `DomainEventMessage`s from Task 3 become localised toasts.
    - Each toast lasts 4 s of real time. At most 5 are visible, the newest on top.
    - Repeated identical texts collapse into one toast with a count.
  - **Game-over modal.** Appears when `session().status()` is `GameOver`. It shows the localised
    reason, final cash, reliability and satisfaction, and a *Main menu* button.
  - **Boot error screen.** If Boot fails (an ftl parse error, or the asset root is missing), show
    a plain egui screen with the error text and *Quit*, instead of panicking.

- [ ] **Step 1: Write the failing tests:**
  - **Presenter tests:**
    - `menu_auto_skips_with_one_available_region`;
    - `region_select_shows_broken_entries_disabled`;
    - `top_bar_model_formats_company_view`;
    - `top_bar_shows_recording_stopped`;
    - `toasts_collapse_duplicates_and_cap_at_five`;
    - `game_over_model_names_the_reason`.
  - **`hud_smoke`:** for each screen, run its draw function inside a bare
    `egui::Context::run(RawInput::default(), …)`, with no renderer, against a real `Game`. Assert
    it returns without panicking and produces the expected actions when given a scripted click.
    Where scripting clicks is impractical, call the action-producing helper directly.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game hud`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): menu, region select, top bar, toasts and game-over`

---

### Task 6: Planning, part 1: map, cell inspector and build menu

**Files:**
- Create: `crates/game/src/hud/{map.rs,inspector.rs,build.rs}`
- Test: inline presenter tests, plus `hud_smoke` additions

**Interfaces:**
- Consumes: `Selection` (Task 5), `region().terrain()`, `region().site_check`, `region().spec`, `plant_views`, `city_views`, `line_quote` (Task 2).
- Produces:
  - **Map panel** (Planning and Operating). An egui painter draws one rect per terrain cell,
    coloured by the active `MapLayer { Terrain, Irradiance, CoolingWater, HydroPotential, Suitability(PlantKind) }`.
    - It also draws:
      - cities as circles, filled when connected and hollow when not;
      - the substation;
      - plants as kind glyphs;
      - lines as segments.
    - A click selects the topmost object under the pointer (plant > city > cell).
    - Hit-testing is a pure function `fn hit_test(layout: &MapLayout, pos: Pos2, &Game) -> Selection`.
    - The map scales to fit the panel and keeps its aspect ratio.
  - **Cell inspector** for `Selection::Cell`. Shows land, height, irradiance, cooling water and
    hydro potential (`percent`), plus a suitability row per plant kind: ✓, or the localised
    `SiteError`.
  - **Build menu.**
    - **Plant.** For the selected cell, one row per kind showing capacity, capital, build months
      and marginal cost, from `region().spec`. *Build* is enabled only when `site_check` passes,
      and emits `UiAction::Command(BuildPlant { kind, cell })`.
    - **Line.** Choose a node (substation, city or plant) as `LineFrom`, then a target node.
      Preview with `line_quote`, showing cells, capital and months, or the localised error.
      *Build line* emits `BuildLine`.
    - Both are available in Planning only.

- [ ] **Step 1: Write the failing tests:**
  - `hit_test_prefers_plants_over_cities_over_cells`;
  - `map_layout_keeps_aspect_and_fits`;
  - `suitability_row_reports_site_errors`;
  - `build_button_disabled_on_an_unsuitable_cell`;
  - `line_preview_shows_the_quote_or_error`;
  - `build_actions_carry_the_selected_cell_and_kind`.

  Add a `hud_smoke` case drawing the map for both shipped regions.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game hud`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): planning map, cell inspector and build menu`

---

### Task 7: Planning, part 2: commitment grid, forecast, tariff and warnings

**Files:**
- Create: `crates/game/src/hud/{commitment.rs,forecast_chart.rs,tariff.rs,planning_warnings.rs,plant_list.rs}`
- Test: inline presenter tests, plus `hud_smoke` additions

**Interfaces:**
- Consumes: `plant_views`, `hourly_forecast`, `rate_review_estimate`, `company_view`, `minimum_generation_check`, `capacity_shortfall_check`, `projections().rate_review`.
- Produces:
  - **Commitment grid** (§8.4: "the primary planning tool").
    - **Layout:** one row per commissioned dispatchable plant, grouped by kind in merit order, and
      24 hour toggles.
    - **Editing:** a toggle edits a pending `[bool; 24]` held in `UiState`. *Apply*, or leaving the
      row, emits `CommitPlant { plant, hours }`. A rejected pattern (`CommitmentInfeasible`)
      reverts the row and shows the toast.
    - **Shortcuts:** "all day" and "off".
    - **Per-row controls:** an AGC toggle (`SetAutoFollow`) and an initial-target field
      (`SetDispatchTarget`, validated range min..capacity, shown beside the field). Hydro rows also
      show the day's water budget (§2.2).
  - **Forecast chart** (egui_plot). The hourly forecast mean with its uncertainty band (from
    `ForecastPoint`), the expected solar line, and **stacked committed capacity by kind** per hour
    (Σ capacity of plants committed in that hour).
  - **Tariff panel.**
    - Current tariff and allowed return.
    - The projected rate review (`rate_review_estimate`): projected return, reserve margin, capital
      to hold, capital to release, and capital at risk.
    - Months until the December review.
  - **Planning warnings.**
    - `capacity_shortfall_check`, e.g. "18:00 short by 1.2 GW: expect a rotating outage".
    - `minimum_generation_check`, e.g. "03:00: committed minimum exceeds load by 300 MW".
    - Hydro plants whose day budget is 0.
    - Each warning has a *show hour* action that highlights the column in the commitment grid.
  - **Begin Operating** button: emits `BeginOperating`.

- [ ] **Step 1: Write the failing tests:**
  - `commitment_row_emits_commit_plant_on_apply`;
  - `infeasible_commit_reverts_the_pending_row` (the presenter consumes the `CommandFailed` for
    that plant);
  - `stacked_committed_capacity_sums_by_hour_and_kind`;
  - `warnings_list_shortfall_then_min_gen_then_hydro`;
  - `tariff_panel_counts_months_to_december`;
  - `initial_target_outside_bounds_is_not_sent`.

  Add a `hud_smoke` case for Planning with both shipped regions.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game hud`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): commitment grid, forecast, tariff and planning warnings`

---

### Task 8: The Operating HUD

**Files:**
- Create: `crates/game/src/hud/{speed.rs,frequency.rs,supply_chart.rs,plant_panel.rs,ramp_warnings.rs,hydro_panel.rs,incidents.rs}`
- Test: inline presenter tests, plus `hud_smoke` additions

**Interfaces:**
- Consumes: `grid_snapshot`, `plant_views`, `hourly_forecast`, `hydro_schedule`, `projections().{timeline, ramp_warnings, plant_history}`, `SimSpeed`, `Selection`.
- Produces:
  - **Speed controls:** pause, 1×, 2×, 4×, 8×, plus Space to toggle pause. They emit
    `UiAction::SetSpeed`.
  - **Frequency gauge.**
    - Shows the deviation (`hertz_deviation`) on a dial coloured by band, and the current
      imbalance (from `last_tick`).
    - Shows the grid mode (Normal or Blackout).
    - When `outage > 0`, a "Rotating outage: 12% of demand" indicator, which also shows
      `outage_target` while the two differ.
    - Protection's `shed`, labelled as a share of the load (§15.3).
  - **Supply chart** (egui_plot). The timeline as supply stacked by kind in merit order, against
    actual demand and served load, with the forecast line for the day and the **system marginal
    cost** on a secondary axis (Task 2).
  - **Plant list and selected-plant panel.**
    - The list shows every plant with status, output and AGC. Clicking a plant selects it.
    - The panel shows output, target, ramp, marginal cost, energy and revenue so far
      (`plant_history`), the hydro budget, the AGC toggle, start/stop, and a target slider.
    - The slider is bounded min..capacity. Its value is sent on release, not on every frame.
  - **Ramp warnings:** the `RampWarningProjection` entries, localised.
  - **Hydro panel:** per scheduled plant, the water level λ ("hydro covers everything above
    31 GW", §12.6) and the 24 hourly allocations, with elapsed hours shown as frozen (§12.6).
  - **Incident ticker:** the latest localised incidents and outage orders, timestamped with
    `clock(tick)`.

- [ ] **Step 1: Write the failing tests:**
  - `speed_buttons_emit_set_speed`;
  - `space_toggles_pause`;
  - `frequency_model_bands_and_outage_indicator`;
  - `supply_series_stack_in_merit_order`;
  - `target_slider_sends_once_on_release_within_bounds`;
  - `hydro_panel_shows_level_or_no_level`;
  - `incident_ticker_orders_newest_first`.

  Add a `hud_smoke` case that runs a session one hour into Operating, then draws every panel.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game hud`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): operating HUD`

---

### Task 9: The Review modal and milestones

**Files:**
- Create: `crates/game/src/hud/{review.rs,milestones.rs}`
- Test: inline presenter tests, plus `hud_smoke` additions

**Interfaces:**
- Consumes: `projections().{monthly_reports, lessons, milestones, rate_review}`, `MonthClosed` report fields.
- Produces:
  - **Review modal** over the dimmed screen (§8.4). It shows, in this order:
    - **P&L:** revenue, fuel, start-up, fixed O&M, construction and net (`money`), with "× days"
      noted (§2.3).
    - **Merit order used:** plant, kind, marginal cost and energy, cheapest first.
    - **Reliability:** served, unserved (planned rotating outage vs unplanned) and
      `out_of_band_share`.
    - **Incidents timeline:** localised, with `clock(tick)`.
    - **Lessons:** the 2–3 localised `Lesson`s.
    - **Regulatory events:** any notice or strike this month, with its reason.
    - **Rate review:** the result, if this was December.

    A *Continue* button emits `FinishReview`.
  - **Milestones panel** (reachable from the top bar in any phase): every `MilestoneKind`, with
    progress or the achieved date.

- [ ] **Step 1: Write the failing tests:**
  - `review_model_lists_pl_lines_in_order`;
  - `review_model_splits_planned_and_unplanned_unserved`;
  - `review_model_shows_strike_reason_from_events` (the lesson's `notice` flag isn't the source,
    per the Plan 1b note);
  - `continue_emits_finish_review`;
  - `milestones_model_shows_progress`.

  Add a `hud_smoke` case that draws Review after a full day.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p game hud`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(game): review modal and milestones`

---

### Task 10: End-to-end check, docs and playtest checklist

**Files:**
- Create: `crates/game/tests/end_to_end.rs`, `docs/superpowers/plans/2026-10-19-plan-2-playtest.md`
- Modify: `ARCHITECTURE.md` (a `game` crate section covering plugins, presenter pattern, message flow, fixed-step pacing and any Bevy 0.20 API notes from Task 1), `TESTING.md` (game test layers), `AGENTS.md` (game commands and rules), `README.md`, follow-ups doc (mark the carry-overs done)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write `end_to_end.rs`.**
  - **Setup:** a headless app with `CorePlugins` and the presenters' action mapping, no egui. For
    each shipped region, at seed 7:
    1. Send `NewGameRequest`.
    2. Apply the hands-off plan through `UiAction`s: commit all, AGC on.
    3. `BeginOperating`.
    4. `SetSpeed(X8)`.
    5. Step `FixedUpdate` until `PhaseState::Review`, then `FinishReview`.
  - **Assertions:**
    - phase sequence Planning → Operating → Review → Planning;
    - the log file grows with every command;
    - no `CommandFailed`;
    - the month's report exists.
  - **Runtime:** under 20 s in debug. Use `Time<Fixed>` manual stepping, not real time.
- [ ] **Step 2: Run it.** Run: `cargo nextest run -p game end_to_end`. Expected: PASS.
- [ ] **Step 3: Write the playtest checklist** (`…-plan-2-playtest.md`). Number each step and pair it
  with the expected result, so the user can run `cargo run -p game` and tick it off.
  - **Menus:** menu → region picker shows both regions.
  - **Planning:**
    - map layers;
    - building a peaker on an unsuitable cell is disabled;
    - a line quote;
    - an infeasible nuclear commitment gives a toast and the row reverts;
    - warnings.
  - **Operating:**
    - 1×/8×/pause;
    - frequency gauge;
    - drag a target;
    - a forced shortage shows a rotating outage and a toast.
  - **Review:** contents.
  - **Banners:** make the log dir read-only to see the recording-stopped banner.
  - **Game over:** reach game over to see the modal.
- [ ] **Step 4: Update the docs** per Files, matching each document's style.
- [ ] **Step 5: Run the full verification.** Run
  `cargo nextest run --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`.
  Expected: all pass.
- [ ] **Step 6: Commit**: `test(game): end-to-end month through the app; docs: game crate and playtest checklist`
