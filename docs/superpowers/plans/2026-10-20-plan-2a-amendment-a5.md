# Megawatt Mogul Plan 2a: Spec Amendment A5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new game starts on the previous operator's schedule: every dispatchable start-fleet
plant is committed all day with AGC on. Start the day asks for confirmation when the forecast shows
a shortfall. The window opens at about 80% of the monitor, and the side and bottom panels can be
resized.

**Architecture:**
- The inherited schedule is a domain rule in `GameSession::new`, shipped with a
  `SIM_RULES_VERSION` bump to 4 and reviewed snapshots.
- The confirmation is a `game` presenter decision. It reuses `capacity_shortfall_check()` (§15.5)
  and the existing `OpenPrompt` mechanism. The domain is unchanged by Task 2.

**Tech Stack:** unchanged (Rust 1.98, Bevy 0.20, bevy_egui 0.43.1; no new crates).

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md` §16 (A5), with §16.7 for Task 3. It builds on §14.5
(the hands-off plan) and §15.5 (the capacity-shortfall check).

## Global Constraints

- **Dependency rule:**
  - `game` depends on `application`, `infrastructure` and `domain`.
  - `infrastructure` depends on `application` and `domain`.
  - `domain` depends only on `rand_chacha` and `thiserror`.
  - No Bevy or egui outside `game`.
- **Strings:** no user-facing strings in `domain` or `application`. New copy goes in
  `assets/locales/en-US/ui.ftl` and `ALL_UI_KEYS`, and l10n coverage stays green.
- **Determinism:** `BTreeMap`/`BTreeSet` only, and no wall-clock time.
- **Errors:** a command that returns `Err` leaves the session unchanged.
- **Tests:**
  - TDD: show RED for every behaviour change.
  - `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and
    `cargo fmt --all --check` must be green before every commit.
  - Never use `--no-verify`.
- **Test layout:** all game integration tests live in the one test binary,
  `crates/game/tests/game.rs`, with `#[path]` modules under `tests/game/`.
- **UI pattern:**
  - Follow the presenter pattern (ARCHITECTURE.md, game section). `dispatch` is the single writer.
  - Every new `UiAction` gets a glue test through the real `dispatch`.
  - Everything fits at 1024×640.
  - Mutually exclusive overlays decide once per frame from a frame-start snapshot, never from
    state that `dispatch` writes mid-pass.

## Review Focus

1. **Tests that now pass for the wrong reason.** Many tests and scenarios assumed an empty start.
   A failure-path test, such as overbuild, overreach, nothing committed, or a shortfall or outage
   test, must still reach its failure. Otherwise it would pass silently on a now-healthy fleet.
   Each one either recreates the empty start explicitly or is shown to still fail as written.
   Test in Task 1, and in Task 2 for the game tests.
2. **The hands-off plan is now a no-op.** Applying it to a new game changes no `state_hash`. Test
   in Task 1.
3. **No boundary start in month 1.** The inherited all-day pattern must not trigger a
   `BoundaryStart::Charged`. Test in Task 1.
4. **The confirmation is shortfall-only.** A minimum-generation or dry-hydro warning alone opens
   no prompt, and the prompt never appears in Operating. Test in Task 2.
5. **Prompt and click in the same frame.** *Start anyway* sends exactly one `BeginOperating`.
   Neither the prompt nor the Planning panels draw in an inconsistent combination during the
   transition frame. Test in Task 2, with the real pass and unsettled passes.

---

### Task 1: The inherited schedule (domain), rules version 4

**Files:**
- Modify: `crates/domain/src/session/mod.rs` (`GameSession::new`), `crates/domain/src/lib.rs`
  (`SIM_RULES_VERSION`)
- Test: `crates/domain/src/session/tests.rs`, `crates/infrastructure/tests/shipped_regions.rs`,
  `crates/application/tests/scenarios.rs`, the rules-version guard
- Modify as needed: every test or snapshot that assumed an empty start, game crate tests included
- Docs: spec §14.5 builder's notes only if a figure they quote changes

**Interfaces:**
- Consumes: `CommitmentSchedule::all_day()`, `Plant::set_commitment`, `Plant::set_auto_follow`,
  `PlantKind::is_dispatchable`.
- Produces: a new game whose `plant_views()` show `commitment == [true; 24]` and
  `auto_follow == true` for every dispatchable start-fleet plant. `SIM_RULES_VERSION == 4`.

- [ ] **Step 1: Write the failing tests.**
  - `a_new_game_runs_the_previous_operators_schedule`: on the fixture, every dispatchable
    start-fleet plant has the all-day pattern and AGC on. Solar's commitment and AGC are
    unchanged.
  - `the_hands_off_plan_changes_nothing_on_a_new_game`: compare `state_hash` before and after
    `CommitPlant { hours: [true; 24] }` plus `SetAutoFollow { on: true }` on every dispatchable
    plant.
  - `the_inherited_schedule_charges_no_boundary_start`: begin month 1's day; the report's
    boundary start-ups are zero.
  - `shipped_regions_start_without_a_shortfall` (infrastructure): for both regions at seeds 0–7,
    `capacity_shortfall_check()` is `None` on month 1 with no commands.
  - The beginner acceptance check runs a year with **no** commands, alongside the existing
    hands-off variant.
- [ ] **Step 2: Run them and see them fail.** Run: `cargo nextest run --workspace`. Expected:
  the new tests fail.
- [ ] **Step 3: Implement.**
  - In `GameSession::new`'s start-fleet loop, set the all-day pattern and `auto_follow = true` on
    every dispatchable plant, and update the method's doc comment.
  - Bump `SIM_RULES_VERSION` to 4.
- [ ] **Step 4: Repair the tests that assumed an empty start, keeping each one's intent.** For
  every test that now fails, or that now passes for a different reason (Review Focus 1):
  - recreate the empty start explicitly with `StopPlant` in Planning plus
    `SetAutoFollow { on: false }`, using a shared helper per crate such as `empty_start(&mut game)`;
  - or show it still fails or behaves as written.

  List each test changed, and why, in the report.
- [ ] **Step 5: Regenerate the snapshots once** (`cargo insta test --review` or `cargo insta accept`
  after reading each diff). Explain each diff in the report. Scenarios that were written to fail
  must still fail.
- [ ] **Step 6: Run everything.** Run: `cargo nextest run --workspace`, then clippy and the format
  check. Expected: green.
- [ ] **Step 7: Commit**: `feat(domain): a new game runs the previous operator's schedule (rules v4)`.

---

### Task 2: Confirm Start the day on a forecast shortfall (game)

**Files:**
- Modify: `crates/game/src/hud/planning.rs` (the Start the day button model),
  `crates/game/src/hud/action.rs` (a new `Prompt` variant and its handling),
  `crates/game/src/hud/prompt.rs` (the prompt's model and draw), `assets/locales/en-US/ui.ftl`,
  `crates/game/src/l10n/keys.rs`
- Test: inline presenter tests, `crates/game/tests/game/hud.rs` (glue),
  `tests/game/hud_smoke.rs` or a new `#[path]` module (real pass), `tests/game/end_to_end.rs`
- Docs: `docs/superpowers/plans/2026-10-19-plan-2-playtest.md`, `README.md` if it describes setup

**Interfaces:**
- Consumes: `GameSession::capacity_shortfall_check() -> Option<CapacityShortfallWarning { hour, demand, deliverable, shortfall }>`;
  `OpenPrompt`, `Prompt` and the prompt overlay (the leave prompt's pattern);
  `l10n::format::{clock, megawatts}`.
- Produces:
  - `Prompt::ShortfallStart { hour: u8, shortfall: Megawatts }`, or an equivalent shape carrying
    the warning's figures;
  - Start the day's action is `UiAction::Command(Command::BeginOperating)` when there is no
    shortfall, and an open-prompt action for that prompt otherwise.

- [ ] **Step 1: Write the failing tests.**
  - `start_the_day_begins_directly_without_a_shortfall`: the presenter's button action is
    `BeginOperating`.
  - `start_the_day_asks_first_on_a_shortfall`: the action opens the shortfall prompt with the
    warning's hour and amount.
  - `the_shortfall_prompt_offers_start_anyway_and_keep_planning`:
    - the model's confirm button sends `BeginOperating`;
    - dismiss and outside-click close the prompt and send nothing;
    - the copy names the hour (`clock`) and the amount.
  - `min_gen_or_dry_hydro_alone_never_asks`: a warning that isn't a shortfall leaves the button
    as `BeginOperating`.
  - Glue test through the real `dispatch`: opening the prompt sets `OpenPrompt`; confirming writes
    one `PlayerCommand(BeginOperating)` and closes it. Show that deleting either arm fails a test.
  - Real-pass test at 1024×640, both regions, on a game with an empty start:
    1. click Start the day: the prompt is visible and fits, and no command is sent;
    2. click Keep planning: the game is still in Planning;
    3. click Start the day, then Start anyway: the phase is Operating, with exactly one
       `BeginOperating`.

    Assert on unsettled passes that the prompt and the Operating panels never draw in the same
    pass.
  - `end_to_end`: the nothing-committed month now starts from an explicit empty start and confirms
    through the prompt. The hands-off months run with no planning commands at all.
- [ ] **Step 2: Run them and see them fail.** Run: `cargo nextest run -p game`. Expected: the new
  tests fail.
- [ ] **Step 3: Implement.** The presenter decides the button's action from
  `capacity_shortfall_check()`. The prompt follows the existing `LeaveGame` prompt: model, its own
  overlay system, and `dismiss` as a `UiAction`.
- [ ] **Step 4: Update the playtest checklist.**
  - The Planning steps that told the player to commit plants now expect the grid already filled
    with every plant on AGC.
  - Add a step: clear a large plant's hours → Start the day → the confirmation names the hour and
    shortfall → Keep planning / Start anyway behave as described.
- [ ] **Step 5: Run everything.** Run: `cargo nextest run --workspace`, then clippy and the format
  check. Expected: green.
- [ ] **Step 6: Commit**: `feat(game): confirm Start the day when the forecast is short`.

---

### Task 3: Window opening size and resizable panels (game)

Spec §16.7. Added after playtest feedback.

**Files:**
- Modify: `crates/game/src/main.rs` (the window's opening size)
- Modify: the panel systems in `crates/game/src/hud/{inspector,build,planning,operating}.rs`
- Create: a small pure helper, for example `crates/game/src/window.rs`, with
  `opening_size(monitor: Vec2) -> Option<Vec2>`, and a pure panel-limits helper
- Test: inline unit tests, plus real-pass drag tests in a new `#[path]` module such as
  `tests/game/panel_resize.rs`
- Docs: the playtest checklist (a resize step); TESTING.md only if it lists the fit tests' sizes

**Interfaces:**
- Consumes:
  - Bevy 0.20's `Monitor` and `PrimaryMonitor` (logical size = physical / scale factor);
  - `WindowPosition::Centered(MonitorSelection::Primary)`;
  - egui `Panel::{left,right,bottom}` with `.resizable(true)`, `.default_size`, `.min_size` and
    `.max_size`;
  - the existing size constants `INSPECTOR_PANEL_WIDTH`, `BUILD_PANEL_WIDTH`,
    `PLANNING_PANEL_HEIGHT`, `OPERATING_PANEL_HEIGHT` and the Operating side width;
  - `EguiPass::run_with`, and the `drag` helper in `support/egui_harness.rs`.
- Produces:
  - `opening_size`: 80% of the monitor's logical size, clamped to between 1024×640 and the
    monitor, or `None` with no monitor;
  - each outer panel resizable from its constant (its default and minimum) up to its maximum:
    - side panels: at most 40% of the screen width;
    - bottom panels: at most 50% of the screen height;
    - the map always keeps at least 320×200.

- [ ] **Step 1: Write the failing tests.**
  - `opening_size_is_80_percent_of_the_monitor_clamped`. Use the 2560×1440, 1280×800, 1024×640
    and 800×600 monitors, checking each is clamped up to the minimum and down to the monitor. With
    no monitor it returns `None`.
  - `panel_limits_keep_the_map_usable`. A pure test of the max sizes at 1024×640 and 2560×1440.
    The maximum is never below the minimum.
  - Real pass, at both 1024×640 and 2560×1440, for each resizable panel in Planning and Operating:
    1. drag its inner edge outward by 150 px: the panel grows by 150 and the map shrinks by 150;
    2. drag it inward past its minimum: it stops at its constant;
    3. drag it far outward: it stops at its maximum, and the map keeps at least 320×200.
  - The map never moves on its own. After a drag, a selection change, a tab change and the
    Planning → Operating → Planning round trip each leave the panel sizes and the map rect
    unchanged.
  - The existing 1024×640 fit tests and map-stability tests pass unchanged at the default sizes.
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Implement.**
  - Set the opening size once the primary monitor is known, because monitors appear after the
    event loop starts. Use a system that runs until it has applied the size once, then never
    again. A user resize afterwards always wins.
  - Make the panels resizable with the limits above.
  - Keep the `--exit-after-frames` launch smoke working.
- [ ] **Step 4: Check the real window by hand.**
  1. Run `cargo run -p game -- --exit-after-frames 120` and record that it exits cleanly.
  2. Run the game once and resize a panel. Take a game-window capture only, never the desktop.
- [ ] **Step 5: Update the playtest checklist.** Add the opening size on a large monitor, and
  dragging a panel's edge with the minimum and maximum limits.
- [ ] **Step 6: Run everything.** `cargo nextest run --workspace`, clippy and the format check.
- [ ] **Step 7: Commit**: `feat(game): open at 80% of the monitor; resizable side and bottom panels`.

