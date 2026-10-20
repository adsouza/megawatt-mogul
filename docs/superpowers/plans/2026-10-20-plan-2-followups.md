# Plan 2 (game shell): rulings, open questions and follow-ups

Plan 2 ran task by task. Each task had one implementer subagent and one reviewer, and the plan
ended with a whole-branch review and a single fix wave. This file records what the run decided
and what it left open, so that Plan 3 can start from it. The playtest checklist is in
[`2026-10-19-plan-2-playtest.md`](2026-10-19-plan-2-playtest.md).

## Rulings made during execution

Each ruling ends with what it costs if it turns out to be wrong.

### Process and build

- **P2: one integration-test binary.** `crates/game/tests/game.rs` holds every integration test,
  with modules under `tests/game/`. This overrides the per-file test paths in the plan. Each
  Bevy-linked test binary costs hundreds of MB and link time. If wrong: that one binary rebuilds on
  any test edit.
- **R1: test harnesses.** The lib unit-test harness is on, so inline presenter tests can run. The
  bin harness is off. If wrong: one extra Bevy-linked binary.
- **R2: dev profile.** Dependencies build at opt-level 3 with `debug = false`, and our own crates
  keep debug info. If wrong: stack traces inside dependencies lack debug info.
- **P3: plugin skeleton.** Task 1 created an empty `HudPlugin`, so `GamePlugins` was complete from
  the start. If wrong: none.
- **P4: boot errors.** A boot failure is a `BootError(String)` resource. The game stays in
  `AppState::Boot` and gets no new state variant. If wrong: a small refactor.

### Read side (domain and application)

- **Snapshot change.** The `overbuild_overcommitment_loses_licence` snapshot change is accepted.
  The minimum-generation warning now checks both hour ends. It is a read-side change, so
  `SIM_RULES_VERSION` stays 3. If wrong: none.
- **`line_quote` has no phase check.** It is a read-only quote; `WrongPhase` stays with
  `BuildLine`. If wrong: the HUD may show quotes outside Planning, which is harmless.
- **Accepted query additions.** These are narrow, read-only queries:
  - `line_views`;
  - `PlantView.hydro_day_budget` and `initial_target`;
  - `incidents_today`;
  - `plant_energy_today`;
  - `RegulatoryProjection`;
  - `MilestoneProjection::status(kind, now)`.

  If wrong: a few small query methods.
- **The strike-free milestone resets each season.** It shows 0 when its run is from an earlier
  season. If wrong: none.

### Localisation and copy

- **Names in messages.** Toasts and incidents name plants and cities through a `Names` context,
  never bare ids. If wrong: a small API change.
- **Unit threshold.** Figures switch from MW to GW at 10,000 MW, as the plan says, although the
  spec's example reads "6.2 GW". If wrong: mid-size figures read in MW.

### Simulation bridge

- **One `AdvanceTicks` batch in flight at most.** A frame slower than 250 ms slows the game rather
  than catching up. If wrong: slow machines run below nominal speed.
- **Session insertion.** `Session` is inserted one frame before the game enters `InGame`. If wrong:
  none.
- **Speed.** Each day starts at 1×, as the plan says. Prompts don't pause the simulation. If wrong:
  playtest feedback.

### HUD

- **Presenter pattern.** UI follows the presenter pattern: model, then presenter, then draw, then
  system, with `dispatch` as the single writer. Every `UiAction` is glue-tested through the real
  `dispatch`. See ARCHITECTURE.md. If wrong: none.
- **Leave prompt.** Menu asks for confirmation before leaving a game, because there is no
  save/load yet. Strikes show against the limit, for example "1/3". If wrong: an extra click.
- **Minimum window and map shapes.**
  - The minimum window is 1024×640.
  - Map markers hit-test the shapes as drawn.
  - Kind letters appear only at 9 pt or larger, and a colour legend explains the markers. By
    default it shows in the first corner of the map (top right, bottom right, bottom left, top
    left) that covers no city or plant, and is collapsed only when no corner is clear; a *Legend*
    toggle lets the player show or hide it (Plan 2a final fix wave).

  If wrong: kinds are read by colour at small sizes.
- **Line ends.** They are chosen in the build menu, not by clicking the map. If wrong: an extra
  click path for players.
- **Fixed panel sizes.** Panels have fixed sizes so the map never moves, with placeholders when
  nothing is selected. If wrong: less map in Planning.
- **Phase gating.** Panels gate on the domain's phase (`in_phase`), not on `PhaseState`, so there
  is no empty frame when the phase changes. If wrong: none.
- **Operating layout.**
  - The speed bar sits under the top bar.
  - The right panel holds the frequency gauge, the plant list and the selected plant.
  - The bottom panel has Supply and Hydro tabs, with ramp warnings and incidents beside them.

  If wrong: a layout reshuffle after playtest.
- **Review modal.**
  - It is drawn over the dimmed map, and the backdrop and modal are one layer.
  - Milestones opens from the top bar in any phase and stays above the modal.
  - Toasts draw at `Order::Tooltip`.
  - Game over takes precedence over Review.

  If wrong: see the open question below.
- **Recording-stopped banner.** It is truncated beside Menu and Milestones, with the full reason
  in a tooltip. If wrong: none.
- **Selected-plant figures.**
  - The panel shows "Energy today", plus one "Closed months" row with energy and revenue.
  - There is no revenue-today figure, because nothing is priced per tick.

  If wrong: the player lacks a revenue-today estimate.

### End to end and docs

- **Launch smoke.** `--exit-after-frames <N>` is a manual launch smoke. It can't run under
  nextest, because winit needs the main thread on macOS and CI has no display. If wrong: one small
  CLI flag.
- **Recording-stopped playtest step.** The step fills a 2 MB disk image. A read-only log directory
  doesn't stop writes to a file the game already has open. If wrong: none.

## Questions settled by the user

- **Game over shows the final month on request.** A player who loses at month close can open that
  month's review, with its lessons and regulator section, from a *See the month* button on the
  game-over modal. *Back* returns to the game-over modal.
- **Closing the window doesn't confirm.** The OS close button quits without asking. Only the
  in-game Menu confirms.

## Follow-ups

### For Plan 3

- **Click-through.** Verify that egui's pointer-over-HUD check works with per-system root Uis, so
  3D picking doesn't click through panels.
- **Drawing lines on the map.** Consider drawing lines on the map once picking exists. Today line
  ends come from the build menu.
- **Map polish.**
  - Map labels can run past the right edge when the map is full width (Review). Flip the label
    anchors near the edge.
  - Orange, red and yellow are close. The colour test checks only that RGB values are unique.

### Infrastructure and CI

- **CI build cache.** There is none yet, and Bevy makes CI slow.
- **CI dependencies.** CI installs `libasound2-dev` needlessly.
- **`.gitignore`.** The `logs/` pattern matches at any depth.
- **Log-file race.** An exclusive-create race in `create_log` returns an error instead of trying
  the next suffix.

### Tests

- **Translation keys.** `ALL_UI_KEYS` is a hand-kept list. A source-scanning check would remove the
  step every task has to remember.
- **Game-over driver test.** `game_over_stops_the_driver` can't tell the status gate from the
  phase gate. Assert Review plus Paused.
- **Review Focus 1.** Its test doesn't assert that nothing was applied. Compare `state_hash`.
- **Log-failure test.** It fails from the first append, not mid-game. Fail after N appends
  instead.
- **Minimum-generation tie test.** It uses a flat load, so it can't see the tie rule.
- **Asset-root fallback test.** It depends on the ambient working directory. Prefer a pure
  `first_existing` over `TempDir`s.
- **Missing checklist.** `finish_boot` treats a missing `BootChecklist` as ready. Fail loudly
  instead.
- **CLI edge cases.** `--assets --log-dir` and `--flag=value` are untested.
- **Variant-table guards.** They are partial for `Season`, `DayPhase`, `PhaseState`,
  `StrikeReason`, `GameOverReason` and `PlantKind`.
- **Fit check slack.** The figure-row fit check compares against the panel's outer edge, not the
  scroll clip edge, which leaves a few px of slack.
- **Hand-composed Planning smoke screens.** They remain. Migrate them onto `EguiPass` with
  `run_with`.
- **Vacuous western check.** The western no-toast-covers-Continue check can pass with no toasts.
- **End-to-end failure message.** "Shown a frame late" also covers "never shown".
- **Rate review on a real December.** It is tested only through a hand-built report.

### Copy

- **Unserved energy.** `UnservedEnergy` at tick 0 prints "first at 00:00".
- **Shrinking outages.** The rotating-outage toast says "ordered" even when the outage shrinks.
- **Notice copy.** It hard-codes "within a year".
- **Small fixes:**
  - the outage-target fragment;
  - "Fuel cost" vs "Marginal cost";
  - a capitalised phase name mid-sentence;
  - cooling water %;
  - unprefixed node ids.
- **Rounding.** At formatter edges, 9,999.6 MW reads "10,000 MW".

### Code

- **Repeated endpoint checks.** `line_quote` repeats endpoint checks.
- **Unknown plants.** `system_marginal_cost` silently skips them.
- **Uncommented check order.** The `add_line` check order changed (`DuplicateLine` before
  `DuplicateLineId`) without a comment.
- **Private rustdoc link.** A public doc links the `pub(crate)` `HourEnd`, which raises a rustdoc
  warning.
- **`Names::new`.** It computes connectivity and is built several times per frame. That is cheap
  at shipped sizes.
- **Committed state in charts.**
  - The forecast chart and the planning warnings show committed state, not the unapplied draft.
  - Displayed bounds are rounded differently from the bounds the parser accepts.
  - There is no click-drag painting across hours.
- **Warnings popup.**
  - It is drawn inside `planning_panel`'s system rather than its own overlay system.
  - Its open state survives a tab switch.
- **Hydro shading.** `hydro_shade` repeats after four plants.
- **Supply chart.** The cost-axis ticks aren't round numbers. This is a playtest item.

## Plan 2a (Amendment A5 and playtest fixes)

Plan 2a started as Amendment A5 (the inherited schedule and the shortfall confirmation). Three
playtest-driven tasks and a colour fix were added to it. It ran the same way as Plan 2, with two
whole-branch reviews and one fix wave.

### Rulings

- **The start fleet.** A new game commits every dispatchable start-fleet plant all day with AGC on
  (`SIM_RULES_VERSION` 4).
  - Tests that need an uncommitted fleet call `empty_start` and assert the shortfall they rely on.
  - The rules-version guard's month 1 depends on the inherited schedule, so the guard can see it.
  - If wrong: none.
- **The shortfall confirmation.**
  - It is Planning-only. *Start anyway* sends `BeginOperating`.
  - The prompt closes when the phase changes, both in the presenter and after `SimSet::Apply`.
  - It keeps the figures it opened with.
  - If wrong: none.
- **Window and panels.**
  - The window opens at 80% of the monitor.
  - The outer panels resize from their old size, which is now their minimum. Side panels go up to
    40% of the width and bottom panels up to 50% of the height.
  - The map's *area* keeps at least 320×200.
  - The Operating bottom panel reserves the side panel's minimum height (gauge, selected-object
    block and three list rows).
  - If wrong: at 1024×640 the Operating bottom panel barely grows.
- **The Operating side panel.**
  - The order is gauge, then a fixed-height selected-object block (plant, city or construction
    card), then a plant list that fills the rest.
  - Every selectable object must fit the block.
  - If wrong: a layout swap.
- **The city card.** It shows the system figures scaled by the city's base-load share. An
  unconnected city shows what it would add, or when a line under construction will connect it.
  If wrong: a card layout change.
- **Construction progress.**
  - The in-service month is the first month the project can be used. The commissioning toast
    comes in the Review of the month before.
  - `line_build_months` shares `line_quote`'s months rule.
  - If wrong: none.
- **The legend.**
  - It is collapsible.
  - Until the player chooses, it opens in the first corner that covers no city or plant centre,
    and otherwise stays collapsed.
  - If wrong: the legend moves between corners until the player chooses.
- **The served line.** It is green (the gauge's in-band colour) and kept more than 80 RGB away
  from every stack colour. If wrong: none.

### Follow-ups

- **A 1-column legend.** Try it before collapsing. It would fit the empty margin beside the West's
  narrow terrain at 1280×720, where every corner of the full legend hits a city.
- **The layer-button row wraps** to as many as six rows on a narrow map, which costs the drawn map
  height. Try a single non-wrapping row or a dropdown.
- **The city card.**
  - Planning has no Close button (add it for Planning only, if asked).
  - The name uses a heading, while plant details use bold text.
- **Duplicate AccessKit names.** A city and a plant with the same name would share one.
- **The legend toggle** is a raw `egui::Button` drawn with `ui.put`, not the shared
  `toggle_button` helper.
- **The legend's measurement has no runtime check.** A future label that wraps could make the
  measurement drift from the drawn size.
- **No test catches a dropped months-left map label.** The plant letters have the same gap.
- **Not checked in a real window yet:**
  - the confirmation prompt's look;
  - dragging a panel by hand;
  - the legend toggle and its corner.

  Run the playtest checklist's steps 2.1a, 2.8, 3.6 and 7–9.

