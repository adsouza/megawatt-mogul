# First-Game Tutorial and SGCC Unlock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A first-game tutorial that runs over the live game (a goal crawl, spotlights and
action-gated instructions), plus an SGCC lock that opens after four other regions are played.

**Architecture:** Player progress (the tutorial flag and the regions played) is a new client
record stored beside preferences, never inside saves. A new application projection counts month
closes after the first expansion; it travels in saves under portable state schema 8. The tutorial
lives entirely in the `game` crate: a pure step model chooses what to show from game and UI state,
an overlay pass draws it above every HUD layer, and every change it makes goes through `dispatch`.
It never sends a `PlayerCommand`.

**Tech Stack:** Rust, Bevy, bevy_egui/egui, serde + RON, proptest, cargo-nextest.

**Spec:** `docs/superpowers/specs/2026-10-31-first-game-tutorial-design.md`. Read it first. Section
numbers below (§n) refer to it.

## Global Constraints

- AGENTS.md applies throughout, notably the dependency rule, determinism, the presenter pattern,
  "a panel system reads what `dispatch` writes through `ActionSink`'s getters", one test binary for
  the `game` crate, and TDD.
- The tutorial never sends a `PlayerCommand` and never writes simulation state.
- Every visible tutorial text meets 4.5:1 contrast against its backing (main spec §8.4). Crawl
  text clips at a horizon; it never fades.
- Spotlight hold: 4.0 s, with 0.3 s fades in and out. Any click or Escape ends a spotlight or the
  crawl early. The ending click is consumed.
- No Next button anywhere.
- Player-facing text goes in `assets/locales/en-US/ui.ftl`, and every new literal key also goes in
  `crates/game/src/l10n/keys.rs`.
- Keep `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo fmt --all --check` green at every commit. Conventional commit messages, ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A loaded save, or returning to the menu, mid-tutorial.** The tutorial stops: no overlay in a
   loaded game, and `tutorial_pending` stays true. Test in Task 6.
2. **The minimum window at maximum UI zoom.** Spotlight cards and instructions stay fully on
   screen. Test in Task 8.
3. **A prompt opening under the tutorial** (the shortfall confirmation on *Start the day*, or the
   connection-capacity confirmation on a line). The prompt stays clickable, and Escape goes to the
   prompt, not the tutorial. Test in Task 8.
4. **An unreadable `progress.ron`.** The game runs with defaults and the file is never rewritten.
   Test in Task 4.
5. **A region whose starter fleet already has a battery (Southern China).** The starter battery
   doesn't satisfy "battery queued". Test in Task 6.

## File Map

| File | Responsibility |
|---|---|
| `crates/domain/src/state.rs` | Schema 8 constants; a `since8` arm for `state_record!` |
| `crates/application/src/projections/expansion.rs` (new) | `ExpansionProjection` |
| `crates/application/src/projections/mod.rs`, `game.rs` | Wire the projection; seed it on restoring schema ≤ 7 |
| `crates/application/src/ports.rs` | `RegionSummary.unlock_after_regions` |
| `crates/infrastructure/src/region/{dto,bundled,catalog}.rs`, `assets/regions/sgcc.ron` | `unlock` metadata |
| `crates/infrastructure/src/client_progress.rs` (new), `client_preferences.rs`, `browser.rs`, `browser_storage.js` | Progress codec and storage |
| `crates/game/src/progress.rs` (new) | `ClientProgressStore`, `ProgressPlugin`, played-region recorder, `UnlockAllRegions` |
| `crates/game/src/paths.rs`, `desktop.rs` | `--unlock-all-regions` |
| `crates/game/src/hud/menu.rs` | SGCC lock on the picker, tutorial checkbox, *Play tutorial* |
| `crates/game/src/hud/tutorial/{mod,steps,anchors,overlay,crawl}.rs` (new) | The tutorial |
| `crates/game/src/hud/action.rs` | New `UiAction`s, `ActionSink` fields |
| `crates/game/src/hud/{top_bar,map,build,planning,speed,review}.rs` | Mark anchors; reveal the battery card |
| `crates/game/src/sim/{mod,new_game}.rs` | `NewGameStarted` message |
| `crates/game/tests/game/{tutorial,progress}.rs` (new) | Integration tests (modules of `tests/game.rs`) |

---

### Task 1: `ExpansionProjection` and state schema 8

**Files:**
- Modify: `crates/domain/src/state.rs` (constants at lines 8 and 12; the macro at line ~297; the
  schema test at ~368)
- Create: `crates/application/src/projections/expansion.rs`
- Modify: `crates/application/src/projections/mod.rs`, `crates/application/src/game.rs:125-152`
- Test: unit tests in `expansion.rs`; restore test in `crates/application/tests/` beside the
  existing snapshot tests

**Interfaces:**
- Produces:
  - `domain::state::CURRENT_STATE_SCHEMA == 8` and `SUPPORTED_STATE_SCHEMAS == 1..=8`.
  - A `state_record!` arm `($ty { fields ; since8 { added: default } })`, the same shape as
    `since6`, gated on `schema >= 8`.
  - `application::projections::{ExpansionProjection, PLAYED_MONTHS}`:
    - `pub const PLAYED_MONTHS: u8 = 12;`
    - `pub struct ExpansionProjection { closes_since_expansion: Option<u8> }`, deriving `Debug,
      Clone, PartialEq, Default`.
    - `pub fn new(session: &GameSession) -> Self` returns `Some(0)` if any connected city
      (`session.city_views()`, `.connected`) is not in `session.region().start().connected_cities`,
      and `None` otherwise.
    - `pub fn played(&self) -> bool` is true when the count is `Some(n)` with `n >= PLAYED_MONTHS`.
  - `Projections.expansion: ExpansionProjection`, applied last in `apply_all`.

- [ ] **Step 1: Write the failing tests** in `expansion.rs`, on the `tiny_region_parts` fixture
  session that `milestones.rs` tests use:
  - `starts_unplayed_without_extra_city`: `new` gives `None`, and `played()` is false.
  - `counts_closes_after_first_extra_city`: apply `CityConnected` for a non-starter city, then 11
    `MonthClosed`, then assert `!played()`. One more `MonthClosed`, then assert `played()`.
  - `starter_city_connections_do_not_count`: `CityConnected` for a starter city, then 20 closes,
    then assert `!played()`.
  - `closes_before_expansion_do_not_count`: 30 closes, then the connection, then 11 closes, then
    assert `!played()`.
  - proptest `played_iff_twelve_closes_follow_an_extra_connection` over random event sequences
    built from {starter connect, extra connect, close}. `played()` equals "at least 12 closes
    after the first extra connect".
  - `state_bytes_round_trip_at_schema_8`.
- [ ] **Step 2: Run** `cargo nextest run -p application expansion`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `ExpansionProjection` and its `Projection::apply`:
  - On `DomainEvent::CityConnected { city }` with `city` not a starter city and the count `None`,
    set the count to `Some(0)`.
  - On `DomainEvent::MonthClosed(_)` with `Some(n)`, set it to `Some((n + 1).min(PLAYED_MONTHS))`.
  - Encode it with `domain::state_record!(ExpansionProjection { closes_since_expansion })`.
- [ ] **Step 4: Bump the schema.**
  - Set the constants to 8, add the `since8` macro arm, and update `state.rs`'s schema tests to
    accept 1..=8 and reject 0, 9 and `u32::MAX`.
  - Add `expansion` to `Projections` and `Projections::new`. Its `state_record!` becomes
    `Projections { ...existing...; since8 { expansion: ExpansionProjection::default() } }`.
- [ ] **Step 5: Seed old saves.** In `Game::restore`, after the re-encode check, if
  `snapshot.schema < 8`, set `projections.expansion = ExpansionProjection::new(&session)`.
  - Test `schema_7_snapshot_restores_with_seeded_expansion`: capture a checkpoint after
    connecting an extra city, re-encode its snapshot at schema 7 (`state_bytes_schema(7)`, and
    recompute `state_hash`), restore it, and assert the count is `Some(0)`.
- [ ] **Step 6: Run** `cargo nextest run --workspace`. Expected: PASS. Update any snapshot test that
  pinned schema 7 to assert `CURRENT_STATE_SCHEMA` instead.
- [ ] **Step 7: Commit** `feat: count month closes after the first expansion (state schema 8)`.

### Task 2: `ClientProgress` storage

**Files:**
- Create: `crates/infrastructure/src/client_progress.rs`, and export it from `lib.rs`
- Modify: `crates/infrastructure/src/client_preferences.rs`: extract the atomic publish shared by
  both records
- Modify: `crates/infrastructure/src/browser.rs` and `src/browser_storage.js` (wasm only)
- Test: `crates/infrastructure/tests/client_progress.rs`

**Interfaces:**
- Produces (`infrastructure::client_progress`):
  - `pub struct ClientProgress { pub version: u32, pub tutorial_pending: bool, pub played_regions:
    BTreeSet<String> }`, with `#[serde(default, deny_unknown_fields)]`. Its `Default` is
    `{ version: 1, tutorial_pending: true, played_regions: {} }`.
  - `pub fn read_progress(root: &Path) -> Result<ClientProgress, SaveError>`. A missing
    `progress.ron` gives the default.
  - `pub fn write_progress(root: &Path, value: &ClientProgress) -> Result<(), SaveError>`.
  - `pub fn decode_progress(bytes: &[u8]) -> Result<ClientProgress, SaveError>` and
    `pub fn encode_progress(value: &ClientProgress) -> Result<Vec<u8>, SaveError>`.
  - Wasm: `browser::read_progress().await` and `browser::write_progress(&ClientProgress).await`,
    stored under key `'progress'` in the existing `preferences` object store
    (`progressRead`/`progressWrite` in `browser_storage.js`).
- Validation: `version == 1`, at most 256 played regions, each id 1–64 bytes, and encoded size at
  most 16 KiB. Otherwise `SaveError::Malformed("client progress")` or `SaveTooLarge`.

- [ ] **Step 1: Write the failing tests:**
  - `missing_file_reads_default`, asserting `tutorial_pending` and empty regions.
  - `round_trip`.
  - `absent_keys_take_defaults`: decode `(version: 1)`, then assert `tutorial_pending == true` and
    `played_regions` empty. Per the memory rule, delete each key in turn and assert the default.
  - `unknown_key_is_malformed`.
  - `rejects_bad_version_and_oversized_region_lists`.
  - `write_is_atomic_in_its_own_tempdir`: write, then read back, with no stray temp files.
- [ ] **Step 2: Run** `cargo nextest run -p infrastructure client_progress`. Expected: FAIL.
- [ ] **Step 3: Implement.** Extract `pub(crate) fn publish_atomic(root: &Path, file_name: &str,
  bytes: &[u8], stage: &'static str, sync: &dyn Fn(&Path) -> io::Result<()>)` from
  `write_preferences_with` (keeping its tempfile, fsync, persist and directory-sync sequence), and
  use it for both records. Add the wasm functions, mirroring `read_preferences` and
  `write_preferences` in `browser.rs`.
- [ ] **Step 4: Run** the infrastructure tests, the existing preference tests and
  `cargo build -p infrastructure --target wasm32-unknown-unknown` (use whatever wasm build command
  CI uses, if this target isn't set up). Expected: PASS.
- [ ] **Step 5: Commit** `feat: store player progress in its own client file`.

### Task 3: Region `unlock` metadata

**Files:**
- Modify: `crates/infrastructure/src/region/dto.rs` (the V4 struct near line 418, and
  `ParsedRegionFile` near line 622), `region/bundled.rs:21`, `region/catalog.rs:264`
- Modify: `crates/application/src/ports.rs:33` and every `RegionSummary { .. }` literal (in the
  `game`, `application` and `infrastructure` tests and in `menu.rs` tests)
- Modify: `assets/regions/sgcc.ron` (after the `atlas` line)
- Test: `crates/infrastructure/tests/region_catalog.rs` and `shipped_regions.rs`

**Interfaces:**
- Produces:
  - `RegionSummary { id, name_key, pub unlock_after_regions: Option<u32> }`.
  - In the DTO: `pub unlock: Option<UnlockDto>` with the same serde attributes as `atlas`, where
    `UnlockDto { pub regions_played: u32 }` uses `deny_unknown_fields`.
  - `ParsedRegionFile::unlock_after_regions(&self) -> Option<u32>`. Legacy files return `None`.
  - Validation: `regions_played` must be in `1..=64`. Otherwise
    `RegionError::InvalidShape { field: "unlock.regions_played" }`.

- [ ] **Step 1: Write the failing tests:**
  - `shipped_sgcc_unlocks_after_four_regions`: the shipped catalog listing for `sgcc` has
    `Some(4)`, and every other shipped region has `None`.
  - `unlock_rejects_zero_and_unknown_fields`.
  - `canonical_bundle_round_trips_unlock`, if `bundle.rs` re-serializes V4 files.
- [ ] **Step 2: Run** `cargo nextest run -p infrastructure region`. Expected: FAIL.
- [ ] **Step 3: Implement.** Add `unlock: (regions_played: 4),` to `sgcc.ron`, and set the field in
  both listing readers.
- [ ] **Step 4: Run** `cargo nextest run --workspace`. Expected: PASS. Bundle or fingerprint
  regeneration tests may need regenerated data. Regenerate with the project's tool, never by hand.
- [ ] **Step 5: Commit** `feat: let a region file require played regions before it unlocks`.

### Task 4: Progress in the game: store, plugin, recorder and unlock flag

**Files:**
- Create: `crates/game/src/progress.rs`
- Modify: `crates/game/src/lib.rs` (register `ProgressPlugin` in `GamePlugins` next to
  `PreferencesPlugin`; `HudPlugin` inits `ClientProgressStore`)
- Modify: `crates/game/src/paths.rs` (the args parser near line 44), `crates/game/src/desktop.rs`
- Test: `crates/game/tests/game/progress.rs` (add a `mod` to `tests/game.rs`), `tests/game/cli.rs`

**Interfaces:**
- Consumes: `infrastructure::client_progress::*` from Task 2, and
  `Game::projections().expansion.played()` from Task 1.
- Produces:
  - `#[derive(Resource, Default)] pub struct ClientProgressStore { pub value: ClientProgress, pub
    loaded: bool, /* private: writable, written, root, wasm reader */ }`. Initialised by `HudPlugin`
    with `loaded: false`. Tests use it in memory; no I/O happens without `ProgressPlugin`.
  - `pub struct ProgressPlugin`:
    - Loads once from `SaveDir`. On `Ok`, it sets `value`, `written` and `writable = true`.
    - On `Err` from an existing file, it warns, keeps the defaults, and sets `writable = false`.
      The file is then never written.
    - In `Last`, it writes synchronously when `writable && written != Some(value)`.
    - Wasm uses the same flow through `spawn_local`, mirroring `preferences.rs`.
  - `pub fn record_played_region(session: Option<Res<Session>>, mut progress:
    ResMut<ClientProgressStore>)`, added by `HudPlugin` in `Update` while `InGame`. It inserts
    `session.region().id().as_str()` once `expansion.played()`.
  - `#[derive(Resource)] pub struct UnlockAllRegions;`.
  - `ParsedArgs.unlock_all_regions: bool`, set by the hidden `--unlock-all-regions` flag.
    `desktop.rs` inserts `UnlockAllRegions` when it is set.

- [ ] **Step 1: Write the failing tests:**
  - `progress_loads_and_writes_in_tempdir`.
  - `unreadable_progress_is_never_overwritten`: write garbage to `progress.ron`, run the app,
    change `value`, run frames, and assert the bytes are unchanged and `value.tutorial_pending`
    is true.
  - `played_region_is_recorded_once`: a `hud_app`, `request(.., "western_interconnection")`, then
    drive the session (via `Session` commands, as `player_workflows.rs` does) to connect an extra
    city and close 12 months. Assert the set holds it exactly once.
  - `cli_parses_unlock_all_regions` in `cli.rs`.
- [ ] **Step 2: Run** `cargo nextest run -p game progress cli`. Expected: FAIL.
- [ ] **Step 3: Implement** to the interfaces above.
- [ ] **Step 4: Run** the `game` tests, then
  `cargo run -p game -- --exit-after-frames 120`. Expected: PASS, and exit code 0.
- [ ] **Step 5: Commit** `feat: load, save and record player progress`.

### Task 5: Picker lock, tutorial checkbox and *Play tutorial*

**Files:**
- Modify: `crates/game/src/hud/menu.rs` (`MainMenuModel` at line 100, `region_select_model` at
  364, `region_choice`, `draw_region_select`, `region_select_screen` at 693, and the tests)
- Modify: `crates/game/src/hud/action.rs` (`UiAction`, `ActionSink`, `dispatch`)
- Modify: `assets/locales/en-US/ui.ftl`, `crates/game/src/l10n/keys.rs`
- Test: `menu.rs` unit tests, `tests/game/region_picker.rs`

**Interfaces:**
- Consumes: `ClientProgressStore` and `UnlockAllRegions` (Task 4), and
  `RegionSummary.unlock_after_regions` (Task 3).
- Produces:
  - `pub struct RegionAccess<'a> { pub played: &'a BTreeSet<String>, pub unlock_all: bool }`.
  - `region_select_model(listings, seed, access: &RegionAccess, l10n) -> RegionSelectModel`.
  - `RegionChoice.locked: Option<String>`. A locked card's button is disabled; the existing
    disabled styling also mutes the globe fill.
  - `RegionSelectModel.show_tutorial: TutorialToggle`, where `TutorialToggle { pub label: String,
    pub on: bool }`. It is drawn as an `egui::Checkbox` beside the seed field and emits
    `UiAction::SetTutorialPending(!on)`.
  - `MainMenuModel.play_tutorial: ButtonModel` with `UiAction::PlayTutorial`,
    `enabled_if(available)`, placed after *New game*.
  - `UiAction::SetTutorialPending(bool)` and `UiAction::PlayTutorial`. `dispatch` writes
    `ClientProgressStore.value.tutorial_pending`. `PlayTutorial` sets it to true and then behaves
    like `Navigate(RegionSelect)`.
  - `ActionSink` gains `progress: ResMut<ClientProgressStore>` and
    `pub fn progress(&self) -> &ClientProgress`.
- Copy (`ui.ftl`):

  ```
  menu-play-tutorial = Play tutorial
  tutorial-show = Show tutorial
  region-locked = Play { $needed } other regions to unlock — { $played } of { $needed } so far
  ```

  `$played` counts played regions other than this one, capped at `$needed`.

- [ ] **Step 1: Write the failing tests:**
  - `sgcc_locked_until_four_other_regions`: played {east, west, brazil} gives locked, disabled and
    "3 of 4". Adding "india" unlocks it.
  - `own_id_never_counts`: played {sgcc, east, west, brazil} stays locked.
  - `unlock_all_ignores_threshold`.
  - `locked_card_stays_disabled_after_seed_rebind`. This guards the `enabled` re-binding in
    `draw_region_select`.
  - `checkbox_reflects_and_toggles_pending`, glue-tested through the real `dispatch`.
  - `play_tutorial_arms_and_opens_setup`, glue-tested.
  - Every key is present (the l10n coverage test picks this up through `keys.rs`).
- [ ] **Step 2: Run** `cargo nextest run -p game menu region_picker`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the `game` tests, including the fit test for the region picker at the
  minimum window. Expected: PASS.
- [ ] **Step 5: Commit** `feat: lock SGCC behind played regions and add tutorial toggles`.

### Task 6: Tutorial step model and run lifecycle

**Files:**
- Create: `crates/game/src/hud/tutorial/mod.rs` (`TutorialPlugin`, lifecycle systems),
  `steps.rs` (the table and the pure model), `anchors.rs`
- Modify: `crates/game/src/sim/mod.rs` and `sim/new_game.rs` (both `start_new_game` variants),
  `crates/game/src/hud/mod.rs` (add `TutorialPlugin` to `HudPlugin`, and a `HudSet::Tutorial` after
  `Overlays`)
- Modify: `ui.ftl`, `keys.rs` (all tutorial copy below)
- Test: unit tests in `steps.rs`, `crates/game/tests/game/tutorial.rs` (new module)

**Interfaces:**
- Produces:
  - `pub struct NewGameStarted { pub generation: u64 }` (a Bevy `Message`). It is written right
    after `persistence.enter(generation)` on success.
  - `pub enum TutorialAnchor { Cash, Map, BatteryCard, LineSection, StartDay, SpeedControls,
    ReviewConstruction }`.
  - `anchors.rs`:
    - `pub fn mark_anchor(ctx: &egui::Context, anchor: TutorialAnchor, rect: egui::Rect)`
      stores `(ctx.cumulative_pass_nr(), rect)` as temp data under
      `egui::Id::new(("tutorial-anchor", anchor))`.
    - `pub fn anchor_rect(ctx: &egui::Context, anchor: TutorialAnchor) -> Option<egui::Rect>`
      returns the rect only if it was marked in the current pass.
  - `pub enum TutorialStep { FrameGrid, Crawl, ShowForecast, Cash, PickSite, RevealBattery,
    BatterySpotlight, BuildBattery, ConnectSpotlight, ConnectBattery, StartSpotlight, StartDay,
    Speed, Construction }`, deriving `Ord`.
  - `pub enum StepKind { Move, Crawl, Spotlight, Instruction }` and `pub enum Done { Never,
    CellSelectedOrBattery, BatteryQueued, BatteryLinked }`.
  - `pub struct StepDef { pub step, pub phase: DayPhase, pub kind, pub anchor:
    Option<TutorialAnchor>, pub text_key: Option<&'static str>, pub done: Done }` and
    `pub const TUTORIAL_STEPS: [StepDef; 14]`, in exactly this order:

    | step | phase | kind | anchor | text key | done |
    |---|---|---|---|---|---|
    | FrameGrid | Planning | Move | – | – | Never |
    | Crawl | Planning | Crawl | – | `tutorial-crawl` | Never |
    | ShowForecast | Planning | Move | – | – | Never |
    | Cash | Planning | Spotlight | Cash | `tutorial-cash` | Never |
    | PickSite | Planning | Instruction | Map | `tutorial-pick-site` | CellSelectedOrBattery |
    | RevealBattery | Planning | Move | – | – | BatteryQueued |
    | BatterySpotlight | Planning | Spotlight | BatteryCard | `tutorial-battery-spotlight` | BatteryQueued |
    | BuildBattery | Planning | Instruction | BatteryCard | `tutorial-battery-build` | BatteryQueued |
    | ConnectSpotlight | Planning | Spotlight | LineSection | `tutorial-connect-spotlight` | BatteryLinked |
    | ConnectBattery | Planning | Instruction | LineSection | `tutorial-connect-build` | BatteryLinked |
    | StartSpotlight | Planning | Spotlight | StartDay | `tutorial-start` | Never |
    | StartDay | Planning | Instruction | StartDay | `tutorial-start` | Never |
    | Speed | Operating | Spotlight | SpeedControls | `tutorial-speed` | Never |
    | Construction | Review | Spotlight | ReviewConstruction | chosen, see below | Never |

  - `#[derive(Resource, Default)] pub struct TutorialRun(pub Option<TutorialState>)`.
  - `pub struct TutorialState { pub generation: u64, pub starter_batteries: BTreeSet<PlantId>,
    pub played: BTreeSet<TutorialStep>, pub seen_review: bool }`.
    `TutorialState::new(generation, &Game)` captures the battery plant ids that exist at start.
  - `pub fn active_step(state: &TutorialState, game: &Game, selection: Selection) ->
    Option<&'static StepDef>`. It returns the first `TUTORIAL_STEPS` entry whose `phase` equals
    the session's current phase, that is not in `played`, and whose `done` predicate is false.
    One-shot kinds (Move, Crawl, Spotlight) join `played` when they finish. Instructions never
    join `played`.
  - Predicates:
    - **BatteryQueued:** some `plant_views()` entry with `kind == Battery` whose id is not in
      `starter_batteries`.
    - **BatteryLinked:** some such battery whose `node` is an endpoint of a `line_views()` entry
      (any status).
    - **CellSelectedOrBattery:** `Selection::Cell(_)`, or BatteryQueued.
  - `pub fn construction_key(state, game) -> &'static str` returns:
    - `"tutorial-construction-general"` when there is no tutorial battery and no line;
    - `"tutorial-construction-paying"` when any tutorial battery or its line is still under
      construction;
    - `"tutorial-construction-paid"` otherwise.
  - Lifecycle systems (in `TutorialPlugin`):
    - On `NewGameStarted`, if `progress.loaded && progress.value.tutorial_pending`, set the run to
      `Some(TutorialState::new(..))`. Otherwise set it to `None`.
    - Each frame, clear the run if `SessionGeneration` differs from `state.generation`, or on
      `OnExit(AppState::InGame)`.
    - Set `seen_review` when the phase is Review. When `seen_review` is set and the phase becomes
      Planning, finish: set `tutorial_pending = false` and clear the run.
- Copy (`ui.ftl`):

  ```
  tutorial-crawl = You run a power company.
      Connect more cities to sell more electricity and make more money.
      But every customer you connect needs power, every minute of every day.
      Keep supply equal to demand, or the lights go out.
  tutorial-cash = This is your money. Grow it by selling power, without blacking out your customers.
  tutorial-pick-site = Click a square on the map to see what you can build there.
  tutorial-battery-spotlight = Batteries store power when it's cheap and release it when demand spikes.
  tutorial-battery-build = Build a battery here.
  tutorial-connect-spotlight = A power plant only helps once a line connects it to your grid.
  tutorial-connect-build = Build a line from your battery to your grid.
  tutorial-start = Ready? Start the day.
  tutorial-speed = Things move slowly at first. Speed up the day here if you like.
  tutorial-construction-paying = This is your first payment for the battery and its power line. You'll pay a share every month until they're built.
  tutorial-construction-paid = This is your payment for the battery and its power line. They're paid off.
  tutorial-construction-general = Construction is what you pay each month for things you've ordered.
  tutorial-end = End tutorial
  ```

- [ ] **Step 1: Write the failing unit tests** (`steps.rs`), on a real `Game` started in
  `western_interconnection`. Commands are executed directly on the `Game`:
  - `first_step_is_frame_grid`.
  - `played_one_shots_are_skipped`.
  - `pick_site_done_by_cell_selection`.
  - `battery_queued_before_asked_skips_battery_steps`: queue a battery at once, then mark
    FrameGrid..Cash played. The active step is ConnectSpotlight.
  - `deselecting_returns_to_pick_site`: after RevealBattery and BatterySpotlight are played, with
    `Selection::None`, PickSite is active.
  - `starting_the_day_drops_planning_steps`: execute `BeginOperating` at PickSite. The active step
    is Speed.
  - `southern_china_starter_battery_does_not_count`.
  - `construction_key_*` for all three outcomes.
- [ ] **Step 2: Write the failing integration tests** (`tests/game/tutorial.rs`):
  - `new_game_with_pending_progress_starts_tutorial`: in a `hud_app`, set
    `ClientProgressStore { loaded: true, .. }` with pending true, then `request(..)`. Assert the
    run is `Some`.
  - `not_pending_means_no_tutorial`.
  - `loading_a_save_mid_tutorial_stops_it` (Review Focus 1). Use the save/load helpers from
    `save_resume.rs`, assert the run is `None`, and assert `tutorial_pending` is still true.
  - `returning_to_menu_clears_the_run`.
  - `leaving_first_review_finishes`: drive Planning, Operating and Review into the next Planning.
    Assert `tutorial_pending == false`.
- [ ] **Step 3: Run** `cargo nextest run -p game tutorial`. Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** the `game` tests. Expected: PASS.
- [ ] **Step 6: Commit** `feat: tutorial step model and lifecycle`.

### Task 7: Tutorial actions through `dispatch`, anchors, and the battery-card reveal

**Files:**
- Modify: `crates/game/src/hud/action.rs`, `ui_state.rs` (add the `reveal_plant_card` field)
- Modify: `top_bar.rs` (the cash stat in `draw_scores`), `map.rs` (the map viewport rect), `build.rs`
  (the battery `PlantRow` in `draw_plant_section`; `draw_line_section`), `planning.rs` (the
  *Start the day* button), `speed.rs` (`draw_speed`'s whole row), `review.rs` (the
  `review-construction` row in `draw_stats`): each calls `mark_anchor` with the widget's rect
- Test: `tests/game/tutorial.rs`

**Interfaces:**
- Consumes: Task 6's types.
- Produces:
  - `UiAction::Tutorial(TutorialAction)`, where `pub enum TutorialAction { Play(TutorialStep),
    Dismiss(TutorialStep), RevealDone, End }`. `dispatch` handles them as follows:
    - `Play(FrameGrid)` sets `world.focus = Some(CameraFocus::Zone(session.anchor_node()))`.
    - `Play(ShowForecast)` calls `ui.apply(&UiAction::SetPlanningTab(PlanningTab::Forecast))`.
    - `Play(RevealBattery)` sets `ui.reveal_plant_card = Some(PlantKind::Battery)`.
    - Every `Play` also inserts the step into `played`.
    - `Dismiss(step)` inserts the step into `played`.
    - `RevealDone` clears `ui.reveal_plant_card`.
    - `End` sets `tutorial_pending = false` and clears the run.
    - With no run, each of these is a no-op.
  - `ActionSink` gains `tutorial: ResMut<TutorialRun>` and `pub fn tutorial(&self) ->
    Option<&TutorialState>`.
  - Build panel: when `reveal_plant_card == Some(kind)` and that row is drawn, call
    `response.scroll_to_me(Some(egui::Align::Center))` on the row and emit
    `UiAction::Tutorial(TutorialAction::RevealDone)`.

- [ ] **Step 1: Write the failing tests:**
  - Glue tests, one per `TutorialAction` variant through the real `dispatch`, each asserting the
    write above.
  - proptest `tutorial_actions_never_send_player_commands`: dispatch an arbitrary
    `Vec<TutorialAction>` (up to 32 entries) and assert no `PlayerCommand` message was written.
  - `anchors_are_marked_in_planning`: a full HUD frame in Planning with a cell selected. Assert
    `anchor_rect` is `Some` for Cash, Map, BatteryCard, LineSection and StartDay.
  - `speed_and_review_anchors_are_marked`.
  - `reveal_scrolls_battery_card_into_view`: at the minimum window, the battery card's rect lies
    within the build panel's clip rect after reveal.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the `game` tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat: route tutorial moves through dispatch and mark anchors`.

### Task 8: The overlay: crawl, spotlight and instructions

**Files:**
- Create: `crates/game/src/hud/tutorial/overlay.rs`, `crawl.rs`
- Modify: `crates/game/src/hud/tutorial/mod.rs` (register the overlay system in `HudSet::Tutorial`)
- Modify: `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`: add a short §8.4 bullet,
  *"First-game tutorial: see 2026-10-31-first-game-tutorial-design.md"*, and a §28.10 note on
  locked regions
- Test: `tests/game/tutorial.rs`, `tests/game/contrast.rs`

**Interfaces:**
- Consumes: `active_step`, `construction_key`, `anchor_rect`, `TutorialAction`, and
  `ActionSink::tutorial()`.
- Produces:
  - `pub enum TutorialView { Move(TutorialStep), Crawl { text: String, progress: f32 },
    Spotlight { step: TutorialStep, anchor: TutorialAnchor, text: String, opacity: f32 },
    Instruction { anchor: TutorialAnchor, text: String, end: ButtonModel } }`.
  - `pub fn tutorial_view(step: &StepDef, state: &TutorialState, game: &Game, l10n: &Localizer,
    shown_for: f32) -> TutorialView`. It is pure. Opacity ramps 0→1 over 0.3 s, holds to 4.3 s,
    then ramps to 0 by 4.6 s. Crawl progress is `shown_for / CRAWL_SECONDS`, with
    `CRAWL_SECONDS = 45.0`.
  - The overlay system `tutorial_overlay(contexts, time: Res<Time<Real>>, l10n, mut shown:
    Local<Option<(TutorialStep, f64)>>, mut sink: ActionSink)`:
    - `Move` dispatches `Play(step)`.
    - A spotlight or crawl that has run its course dispatches `Dismiss(step)`.
    - Drawing happens in an `egui::Area` at `Order::Tooltip`, above every HUD layer.
  - Spotlight drawing:
    - A full-screen `Sense::click()` area paints black at `0.65 * opacity` alpha everywhere
      except a rounded rect around the anchor, inflated by 6 px.
    - The note sits on an opaque palette card beside the cut-out, clamped inside the screen.
    - A click or Escape dispatches `Dismiss(step)`. The click is consumed by the area.
    - If the anchor isn't marked this pass, nothing is drawn and the timer doesn't start.
  - Instruction drawing:
    - A non-interactive card beside the anchor holds the text and a `tutorial-end` link button
      (`End`).
    - If the anchor is absent, the card sits at the top centre of the map anchor, or at the
      screen's top centre.
    - The card never blocks input outside itself.
  - Crawl drawing (`crawl.rs`):
    - A full-screen backdrop of black at 0.8 alpha.
    - The text is laid out as a galley, tessellated into a mesh, and transformed by a perspective
      that shrinks it towards a horizon at 25% of the screen height.
    - Glyphs above the horizon are clipped, never faded.
    - The colour is the palette's accent yellow (choose one with at least 4.5:1 against the
      backdrop composited over white).
    - The AccessKit label is the full text.
    - A click or Escape dismisses it.
  - Escape is handled only when no prompt is open (`sink.open_prompt().0.is_none()`) and no
    popup is open.

- [ ] **Step 1: Write the failing tests:**
  - `tutorial_view_opacity_curve` at 0, 0.15, 2, 4.45 and 4.7 s.
  - `spotlight_click_is_consumed`: with the StartSpotlight active, a pointer click at the
    *Start the day* rect leaves the phase in Planning and marks the step played.
  - `spotlight_ends_by_itself_after_hold`.
  - `escape_goes_to_open_prompt_not_tutorial` (Review Focus 3): with a prompt open and an
    instruction showing, Escape closes the prompt and the tutorial run is unchanged.
  - `prompt_stays_clickable_under_instruction`.
  - `cards_fit_minimum_window_at_max_zoom` (Review Focus 2): every spotlight note and instruction
    rect lies within the screen at the minimum window size and maximum UI zoom.
  - In `contrast.rs`, `crawl_text_meets_contrast_over_white`, and the note text against its card.
  - `crawl_has_full_accessible_name`.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo nextest run --workspace`, clippy, `fmt --check`, and
  `cargo run -p game -- --exit-after-frames 120`. Expected: all PASS, and exit code 0.
- [ ] **Step 5: Play it** with `cargo run -p game` on an empty temp save directory. Walk the
  tutorial once and take screenshots of the crawl, a spotlight and an instruction.
- [ ] **Step 6: Commit** `feat: draw the first-game tutorial overlay`.
