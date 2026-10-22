# Testing

Development is test-driven: a failing test, then the minimal code to pass it, then refactor. Every
commit leaves `cargo nextest run --workspace`, clippy and `cargo fmt --check` green.

## Layers

| Layer | Where | Tooling |
|---|---|---|
| Unit | Inline `#[cfg(test)]` in `domain` and `application` | std |
| Property-based | `domain`, `application` | `proptest`, `proptest-state-machine` |
| Integration and scenario | `application/tests/`: command scripts against small fixture regions built in code | std, `insta` |
| Adapter | `infrastructure/tests/` (parses both shipped region files) | std |
| Game | Inline presenter tests in `game/src/hud/`; `game/tests/game.rs`, one binary of headless `App` tests (see [The game crate](#the-game-crate)) | std, Bevy `MinimalPlugins`, egui |
| Mutation | `domain`, `application` | `cargo-mutants` |
| Runner | Everything | `cargo-nextest` |

Beyond the inline unit and property tests, the integration suites are
`application/tests/{scenarios,determinism,state_machine,pipeline,rules_version_guard}.rs`, the adapter
suites are `infrastructure/tests/{region_catalog,shipped_regions,command_log}.rs`, the game's are the
modules of `game/tests/game.rs`, and `domain/tests/dependency_rule.rs` enforces the dependency rule
(on the `domain`, `application` and `infrastructure` manifests).

## Running each layer

```sh
cargo nextest run --workspace                  # everything
cargo nextest run -p domain                    # domain only (never compiles Bevy)
cargo nextest run -p application               # unit, property and scenario tests
cargo nextest run -p infrastructure            # adapter tests
cargo nextest run -p game                      # presenter, glue, egui-pass and end-to-end tests
cargo nextest run -p game end_to_end           # one month through the app, per shipped region
cargo nextest run --workspace --profile ci     # what CI runs (no retries)
cargo insta review                             # review snapshot changes
cargo mutants -p domain -p application         # mutation testing
```

Property tests use `proptest`; set `PROPTEST_CASES` to run more cases locally. Note that nextest does
not run doctests; run `cargo test --doc --workspace` for those.

The whole workspace runs in about 23 s in a debug build. Most of that is the three year-long
acceptance runs in `infrastructure::shipped_regions`, `hands_off_plan_survives_year_one`,
`no_commands_survive_year_one` and `shipped_regions_never_order_outages_hands_off` (about 18 s
each, side by side: two regions × eight seeds × twelve months); every other test finishes in under
3 s.

## The game crate

The game's tests sit in six layers, from the cheapest to the widest. None opens a window.

- **Presenter tests**, inline under `#[cfg(test)] mod tests` in each `game/src/hud/<panel>.rs`. They
  call the pure `<panel>_model(..)` on a real `Game` (`hud::test_support`: `shipped_l10n()`,
  `eastern_game(..)`, the shipped Eastern region on seed 7 in Planning, and the `FullDisk` sink)
  and assert on the model's strings and on each `ButtonModel`'s action and `enabled`, comparing
  text against `l10n::format` and `l10n.text(key)`. Each ends with
  `assert_eq!(l10n.problems(), vec![])`, so a missing key fails.
- **Glue tests** (`tests/game/hud.rs`, `simulation.rs`, `states.rs`) run the plugins in a headless
  `App` (`support::headless_app` or `support::hud_app`: `MinimalPlugins`, `StatesPlugin`,
  `CorePlugins`, and `HudPlugin` without `EguiPlugin`) on
  `TimeUpdateStrategy::ManualDuration(ZERO)`. They drive the app with messages and `app.update()`
  and assert on resources. HUD actions go through the real `dispatch` (`run_system_once` with an
  `ActionSink`), never by writing the resources directly, so a broken wire fails. The fixed-step
  driver is stepped with `TimeUpdateStrategy::FixedTimesteps(1)`: each `app.update()` then runs
  `FixedUpdate` exactly once, and nothing depends on real time.
- **Draw tests** (`tests/game/hud_smoke.rs`, `review_smoke.rs`) draw one panel's draw function in
  `support/egui_harness.rs`, inside the container its system uses. Widgets are read back from
  egui's AccessKit tree, and `click`, `click_nth`, `drag` and `press_key` are real pointer and key
  events, so a test passes only if the button is there and clickable.
- **The real egui pass** (`support/egui_pass.rs`). `EguiPass::attach` gives a headless app the
  primary egui context `EguiPlugin` would attach to the window; `run` and `settled` run the
  plugin's own systems in `EguiPrimaryContextPass` with their real run conditions, and `run_with`,
  `click_at` and `drag` feed input events. It catches what the other layers can't: a phase gate
  that draws the wrong panels, and conflicting system parameters (Bevy B0002), which are only
  checked when the pass is first initialised (`the_egui_systems_have_no_conflicting_parameters`).
  The modules that drive whole screens through it:
  - `shortfall_prompt.rs`: *Start the day* opens the prompt only on a shortfall, and its buttons,
    a click outside and the transition frame;
  - `operating_side.rs`: the gauge, the fixed-height details block and the plant list that fills
    the rest, whatever is selected and however the panels are sized;
  - `city_card.rs`: a clicked city's card in Planning's inspector and Operating's details block;
  - `construction.rs`: the Construction tab and the construction card of a plant being built;
  - `map_legend.rs`: every city clickable at 1024×640, 1280×720 and 2560×1440 in both regions and
    phases, with the default legend (open in a corner that hides no city or plant, or collapsed)
    and opening at 1280×720; the legend toggled by the player, and inside the drawn map with every
    panel at its maximum.
- **The 1024×640 fit tests** (`at_the_smallest_window_…`) lay out each screen at the minimum window
  size, for both shipped regions, and check that panels don't overlap and that the controls a
  player needs (*Apply*, *Continue*, the figures of the selected plant) are inside their panel.
- **The panel resize tests** (`tests/game/panel_resize.rs`) drag each outer panel's inner edge
  through the real pass (`EguiPass::drag`) at 1024×640 and 2560×1440: it grows with the drag,
  stops at its opening size and at its maximum, the map's area keeps 320×200, and a selection,
  a tab or a whole day afterwards moves neither a panel nor the map.
- **The end-to-end month** (`tests/game/end_to_end.rs`), for each shipped region on seed 7: a new
  game, which already runs the hands-off plan (spec §16.3), so no planning command is sent;
  *Start the day*, which begins the day directly with no shortfall; 8×, the fixed-step driver to
  the close, then *Continue*. It asserts the phases
  Planning → Operating → Review → Planning, no `CommandFailed`, a log that gains one line per
  command, and the month's report. It asserts nothing about the month's outcome, which the
  `infrastructure` acceptance runs pin. A third run undoes the inherited schedule first
  (`support::empty_start_in`), so *Start the day* opens the shortfall prompt and *Start anyway*
  begins it; the day orders a rotating outage and the close issues a strike: both toasts must be
  on screen by the end of the frame their event arrives in, and every toast must come from an
  event. Each run takes about 0.2 s.

**The empty start.** A new game runs the previous operator's schedule (spec §16.3), so its fleet
is committed all day with AGC on. A test that needs an uncommitted fleet (a shortfall, an outage,
a plant to start by hand) recreates the old empty start explicitly: `empty_start(&mut game)` on a
bare `Game` (`hud::test_support` inline, `support` in `tests/game/`), or `empty_start_in(&mut app)`
through the real `dispatch` in an app. It then asserts the shortfall or state it relies on, as
`short_app` in `shortfall_prompt.rs` asserts `capacity_shortfall_check()` is `Some`, so the test
can't pass for the wrong reason on a healthy fleet.

`tests/game/l10n_coverage.rs` checks the translations: every literal key in `l10n::keys` exists in
en-US, every typed value (errors, events, lessons, kinds) translates through an exhaustive table,
and a broken `.ftl` fails Boot visibly.

**One test binary.** Every game integration test is a module of `tests/game.rs`, under
`tests/game/` (with `#[path]` attributes), and `tests/game/layout.rs` fails if another top-level
test file appears. Each Bevy-linked test binary costs hundreds of MB of disk and its own link step,
so the crate has exactly two: the library's inline tests and `tests/game.rs`. Doctests (one binary
each) are off, and `main.rs` has no tests. Add a module, not a file in `tests/`.

**The manual launch smoke.** No test opens a real window: winit needs the main thread on macOS,
which nextest doesn't give it, and CI has no display. After a change to plugins, systems or their
parameters, run

```sh
cargo run -p game -- --exit-after-frames 120
```

It opens the window, draws the main menu for 120 frames and exits with status 0; a panic on the
first frame (a B0002 conflict, a missing egui schedule) shows here first. It is step 0 of the
[playtest checklist](docs/superpowers/plans/2026-10-19-plan-2-playtest.md), which covers what only a
person can see.

## Save/resume and client preferences

- `application/tests/save_resume.rs` compares full checkpoints, public views and every projection
  after restore and continuation, including retained history after mid-run recording failure,
  year-end regulation, construction and terminal Review. Prefix/continuation invariants use
  property tests; rejected commands leave the capture unchanged.
- `infrastructure/tests/save_{codec,store,compatibility}.rs` cover self-contained files, exact
  payload checksums, explicit size/version failures, saves from another build or platform that
  decode and list with their metadata, build fingerprint inputs, actual disk bytes across
  publication faults, OS-lock contention, revisions and recovery without parent saves/logs/installed
  regions. Rename tests preserve the checkpoint, stamp and date across Unicode names, reject stale
  writers and corrupt saves, and exercise every publication fault and durability retry.
  Directory-sync uncertainty is distinct from rollback.
  `save_store.rs` records which directories a save syncs (`with_directory_sync`): only the root
  when it exists; each new directory's parent when the save creates them; and, on a Retry or the
  next save, what an uncertain publication, a partial sync or a failed creation left unsynced.
- `tests/game/save_resume.rs` covers queued ticks, generation rejection, paused load entry,
  month-close versus Save-and-menu ordering, independent storage errors, Retry and real egui
  dialogs. Rename tests click through the real browser/modal, prefill and cancel edits, preserve the
  running checkpoint, retain a renamed autosave through the next month close and recover from
  conflicts and pre/post-publication failures. The normal HUD contrast checker also inspects the
  save modal's paint output. A failed autosave's top-bar warning is checked to start with the failure (a worker panic included), to
  outlive *Continue without saving*, to clear only on a confirmed save at that boundary or later
  (Retry included) or on leaving the run, and, with recording stopped too, to become one counted
  indicator that fits at 1024×640. A save another build wrote lists by name and resumes paused when
  its replay verifies; when it doesn't, the browser shows the other-version copy at 1024×640, and
  neither the menu nor the file changes.
- `infrastructure/tests/client_preferences.rs` and `tests/game/preferences.rs` cover isolated
  preference files, restart, real sidebar/bottom-panel drags, zoom/speed, effects toggled through
  real buttons and retained across new games/restarts, legacy effects defaults, normal-exit flushing and
  property-tested reachable geometry. Tests never write the player's real preferences.

`tests/game/world.rs` covers the integrated diorama through CPU geometry, headless lifecycle and
real egui input tests; see [3D diorama](#3d-diorama-plan-3) for the rendering and native boundaries.

Focused runs: `cargo nextest run -p application save_resume`,
`cargo nextest run -p infrastructure save_`, `cargo nextest run -p game save_resume`, and
`cargo nextest run -p game preferences`. Native checks must also exercise actual launch, OS close,
load, pause, month close, overwrite and resizing with a separate `--save-dir` and `--log-dir`.

## Property catalogue

The invariants the property tests must cover:

- **Ramping:** for any sequence of targets, the per-tick change in output stays within the ramp rate,
  and online output stays between minimum stable output and capacity.
- **Merit order:** AGC never leaves a cheaper auto-follow plant with headroom while raising a more
  expensive one, and the reverse holds when lowering.
- **Conservation:** each `TickSummary` describes the post-protection state and satisfies
  generation = Σ by_plant = Σ by_kind = served + imbalance (signed),
  served = demand × (1 − outage) × (1 − shed), unserved = demand − served, the planned
  `demand × outage` within unserved, and 0 ≤ served ≤ demand. `curtailed` is solar availability withheld,
  never generated, so it is not part of generation. Unserved energy is shed load only; a residual
  mismatch is absorbed by system inertia and shows up as frequency deviation.
- **Money:** the change in cash always equals the sum of the ledger entries, exactly.
- **Commitment:** validate cyclic off-gaps against shutdown plus start-up, including midnight and
  exact-fit boundaries. Nuclear accepts all-day/all-off only. Initial state follows the pattern;
  physical start-up state does not cross representative months. Operating starts beyond midnight
  fail unchanged; stops/trips suppress automatic actions without mutating the base pattern.
- **Weighting:** daily energy and restarts use `days_in_month`; distinct boundary starts use a
  weight of one. No recurring/boundary duplicate or synthetic-initialisation charge; exactly one
  rounding step per ledger entry.
- **Hydro:** the energy a hydro plant produces in a played day never exceeds that day's budget.
- **Hydro schedule:** water filling conserves water (the allocation sums to the lesser of the budget
  and the usable water, and never exceeds the budget), stays within `[0, capacity]` and gives
  ineligible hours nothing, shaves the highest residuals first, is monotone in the budget and is
  deterministic. In the power system, planning draws no randomness, plants fill in id order against
  the residual, a foreseen fall ramps down at most one ramp per tick, no hour spends more than its
  allocation plus one tick of ramp, the emergency lift holds for the whole restoration, and the
  view's elapsed hours stay frozen.
  A stopped/tripped hydro unit with AGC still on reserves no operating allocation while its
  commitment is suppressed; manual restarts rejoin the plan and next-month previews stay unchanged.
- **Home-city topology:** the anchor is an existing connected starter city's node, with exactly
  one node per city or plant and no self-line or extra anchor asset. Vary the home city, city order,
  connected subset and seed. Reject missing or initially unconnected home cities; verify named,
  unique line endpoints and one ordinary city marker per city.
- **Text contrast:** an independent WCAG luminance calculation checks the HUD palette against
  every text-bearing surface and widget state, including secondary labels, at 4.5:1 or better.
  Disabled labels are checked as the HUD's helpers really paint them, on every surface, and a
  disabled button or slider must paint unlike an enabled one, including inside a disabled `Ui`.
  Property tests exercise arbitrary foreground/background pairs. Inspect actual egui paint output
  for both regions' screens and verify opaque backings behind map labels and bar hover text. Run
  the windowed launch smoke after changing HUD theme systems.
- **Determinism:** the same verified versions, settings, region content, seed and commands produce
  an identical state hash on the same build and platform (libm transcendentals may differ in the
  last bit between targets; see `ARCHITECTURE.md`). A save from another build restores when its
  replay verifies and fails as `OtherVersionDiverged`, installing nothing and leaving no branch
  log, when it doesn't; one that no longer decodes fails as `OtherVersionUnreadable`. Test
  version/fingerprint mismatches before command execution, exact-byte region archival, and
  recovery after shipped data is retuned or removed. Missing/corrupt archives never silently fall
  back to installed data; failed archive/header initialisation prevents new games.
- **Robustness:** any random sequence of commands (state-machine test) never panics, and all
  invariants hold afterwards.
- **Parsing:** malformed region DTOs produce typed errors, never panics.
- **Rate review:** the new tariff x the past year's served energy = the revenue requirement (within
  one cent); the allowed return always stays within the PBR band. Assess reserves against the higher
  of actual and projected coincident peaks, including December connections. First excess review
  holds capital without return; useful portions are released; only portions still excess at a second
  consecutive review are written off, bounded by remaining book value. Held history survives annual
  resets, partial releases preserve the right history, previews never mutate it, and capital
  adjustments never change cash. Held portions continue book depreciation. The return term is linear
  in the months tallied (a partial first year earns `months / 12` of a year's return, within one
  cent), and a full year matches the unprorated formula exactly.
- **Over-frequency protection:** protection never arms at or below `trip_above`, arms after exactly
  `trip_after_ticks` consecutive ticks above it, and disarms at or below `curtail_above`. Every trip
  is preceded by a full window of ticks with a physical surplus, and trips are at least one window
  apart. A trip takes the highest-cost connected online producing dispatchable plant (ties by id),
  never solar or a plant at 0 MW, and waits while the rest of the fleet could not cover it within one
  tick or another plant is losing output AGC cannot reverse.
- **Armed versus unarmed:** over whole days of fleets from the start fleet to three surplus CCGTs,
  with or without hydro, in any month, armed protection never adds shed energy or a blackout to the
  same day with trips out of reach. Hand-picked scheduled hydro falls with a trip due check the same.
- **Frequency performance:** the out-of-band share counts Normal-mode ticks only. The first
  frequency-only failure in 12 months draws a notice, and a second one strikes with reason
  `FrequencyPerformance`. A notice month resets the clean streak, a month failing both tests earns
  one `UnservedEnergy` strike, clean-month expiry needs both tests passed, and three strikes from any
  mix revoke the licence.
- **Minimum-generation warning:** on crafted fleets it reports the hand-computed worst hour and
  excess (earliest hour on ties), returns `None` when the committed minimums fit (exactly fitting
  included), counts only committed hours, and leaves `state_hash` unchanged.
- **Staged shedding (step deficit):** `staged_shedding_arrests_any_step_deficit` takes any step
  deficit below the one-tick blackout line (0.1–29.9% of demand) and any AGC ramp from 0 to 2% of
  demand per tick on a single bus: no blackout, minimum deviation above `blackout_below`, peak shed
  at most `d + 3 × shed_step`, and a rebound below `trip_above`. Its harness, `step_deficit_run`, is
  a port of the single-bus reference model the spec §15.4 bounds were measured with; the two differ
  only exactly on a stage threshold, where the domain's rule is strict. `stages_fire_once_per_event`
  checks each stage sheds at most once per event and re-arms only at or above `−normal_band`,
  against a brute-force count of the stages crossed.
- **Rotating outages:** the target is exactly the worse-end forecast share, 0 on a covered day; the
  day starts at hour 0's target; the outage moves at the paced cut and restore rates, never past the
  target, and never restores a surplus from an unrecoverable fall; a blackout holds it at 0 with
  the target frozen and restores exactly to that target; black start needs generation; a dark grid
  never arms over-frequency protection, including when a large unit comes online into a full
  outage; a tick orders at most one outage; a rotating outage alone keeps the hydro cap; and the
  planned energy is never more than the unserved energy (`check_invariants`).
- **Capacity-shortfall warning:** on crafted fleets it reports the hand-computed worst hour and
  shortfall, returns `None` when the committed fleet covers every hour, and leaves `state_hash`
  unchanged.

## Acceptance checks

`infrastructure/tests/shipped_regions.rs` plays the shipped regions themselves (spec §12.5, §14.5):

- **Hands-off year:** a plan that commits every dispatchable plant all day with AGC on, and issues
  no other command, plays 12 months from each region's start on seeds 0–7. The game must still be
  `Active`. If it fails, retune region data, never the rules. The East passes with a thin margin:
  6 of 8 seeds draw a frequency notice in a shoulder month, and none has more than one strike
  outstanding.
- **No-commands year** (spec §16.5): the same year with no commands at all, on the previous
  operator's schedule a new game inherits (spec §16.3). It must pass too.
- **No shortfall at the start** (spec §16.6): each region, on seeds 0–7, has no
  capacity-shortfall warning on month 1 with no commands.
- **Hydro smoke runs:** a Western January keeps every hydro unit producing through the evening peak
  on seeds 0–7, and a Western October start stays reliable with no evening cliff.
- **No outages hands-off:** the same hands-off plan orders no rotating outage in any of the 192
  region-seed-months (spec §15.8).

`application/tests/scenarios.rs` checks the overreach acceptance pair (spec §15.8) on its July
2027 month, a city connected without the capacity to serve it:

- **`overreach_rotates_outages_instead_of_collapsing`** (seeds 13, 1, 2 and 3, with demand
  noise): no blackout, every committed plant online at the close, reliability above 0, deviation
  above `blackout_below`, the rotating-outage lesson taught, and
  `0.95 ≤ rotating_outage / minute-by-minute forecast shortfall ≤ 1.10`.
  Protection shed from demand noise is allowed. Seed 1 is the closest at 0.978.
- **`overreach_without_demand_noise_rotates_exactly_the_shortfall`**: with noise off,
  `shortfall − ε ≤ rotating_outage ≤ shortfall + shed_step × peak demand × hours with an outage ×
  days in the month`, and protection sheds nothing.

## Snapshots

Canonical scenarios ("East winter peak", "West duck-curve day", "Overreach: city connected without
capacity", and two overbuild cases) snapshot their `MonthlyReport`s with `insta`, so tuning changes
show up as reviewable diffs. The overbuild cases build four surplus CCGTs and snapshot their
regulatory timeline as well:

- `overbuild_overcommitment_loses_licence` commits them all day. Over-frequency draws a notice, then
  three frequency-performance strikes, and the licence is revoked at the October 2029 close. This
  is deliberately a game-over script.
- `overbuild_idle_capacity_disallowance` leaves them uncommitted. The December 2029 reserve review
  holds the excess capital, and the December 2030 review disallows it.

`rules_version_guard` pins a nine-month digest to `SIM_RULES_VERSION`.

1. Run the tests; a changed snapshot fails and writes a `.snap.new` file.
2. Run `cargo insta review` and accept or reject each diff. Without a terminal, run
   `cargo insta test --workspace --unreferenced=reject`, read each pending diff (`cargo insta
   pending-snapshots`, or diff `.snap` against `.snap.new`), then `cargo insta accept`.
3. Commit the accepted `.snap` files with the change that caused them, listing each diff and its
   cause in the commit message.

## Mutation testing

`cargo-mutants` mutates each function in `domain` and `application` (flipping operators, replacing
bodies with defaults) and reruns the tests; a mutant the tests do not fail on is "missed" and points at
an assertion that is missing or too weak. The `game` crate is out of scope. It is never run on every
commit.

**Target:** zero missed mutants in `domain`. The only exceptions are `exclude_re` entries in
`.cargo/mutants.toml`, each with a `#` comment proving the mutant equivalent (or the code untestable).
`application` is held to the same standard.

### Commands

```sh
cargo mutants -p domain -p application -j 4          # the full run (what the weekly job does)
cargo mutants -p domain --list | wc -l               # size the job (about 1,500 mutants for domain)
cargo mutants -p domain -j 4 -F "grid/dispatch.rs"   # one file while fixing survivors
cargo mutants -p domain -j 4 --shard 3/8 --output target/mutants-domain-3   # one slice of eight
```

A full local run takes roughly 20 minutes at `-j 4`. To keep each command short, run the shards
`0/8` through `7/8` one after another, each with its own `--output` directory. Use `-F` (an examine
regex), not `-f`, to narrow to a file: `-f` is merged with the configured `examine_globs` and still
runs everything. Mutants run in scratch copies of the tree, so `cargo mutants` never touches your
working directory; tests are hermetic (see below), which is what makes the parallel runs safe.

`.cargo/mutants.toml` selects nextest and the `mutants` nextest profile (`.config/nextest.toml`). That
profile stops at the first failure and terminates running tests immediately, so a mutant that makes a
loop spin forever fails fast instead of waiting out `timeout_multiplier`.

### Reading `mutants.out/`

| File | Meaning |
|---|---|
| `missed.txt` | Mutants every test passed against. Each one needs a stronger test (or a justified exclusion). |
| `caught.txt` | Mutants at least one test failed against. |
| `timeout.txt` | Mutants whose tests did not finish. They count as detected (an infinite loop is a failure), but check they are not hiding a slow test. |
| `unviable.txt` | Mutants that do not compile; nothing to do. |
| `outcomes.json`, `log/`, `diff/` | Per-mutant detail: the test output and the exact source change for each mutant. |

To kill a survivor, open its file under `diff/` to see the change, then write a test that fails against
it: prefer a property test with boundary values (a ramp exactly at the limit, a reserve margin exactly
at its threshold) over a one-off example. Before excluding a mutant, show that no input can tell it
apart from the original; an operator that is only unreachable at one boundary is usually still
testable with a hand-built state.

### Scheduled job

`.github/workflows/mutants.yml` runs the full `cargo mutants -p domain -p application -j 4` every
Monday at 06:00 UTC and on demand (`workflow_dispatch`), then uploads `mutants.out/` as the
`mutants-out` artifact. The job goes red when anything is missed; download the artifact and read
`missed.txt`. Output directories (`mutants.out*/`) are git-ignored.

### Mutation testing status

Last full run (Plan 1, before the Plan 1a amendments; 8 shards of `domain`, 2 of `application`):

| Crate | Mutants | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|---|
| `domain` | 1,476 | 1,110 | 0 | 0 | 366 |
| `application` | 139 | 101 | 0 | 0 | 38 |

Both crates are at the zero-missed target, so there is no application backlog to record. The first
baseline found 123 missed (and 10 timed-out) mutants in `domain` and 6 in `application`; all were
killed with tests except the equivalent mutants excluded in `.cargo/mutants.toml`. Those are the
exceptions to keep an eye on:

- `GameSession::settings` and `PowerSystem::model` return a value of a type with one inhabitant
  (`NetworkModel::CopperPlate`); they become testable when a second network model exists.
- `HeldSlice::consecutive_excess_reviews` returns a count that is always 1 for any slice that
  survives a review, because a second consecutive review writes the slice off. The counter is
  vestigial state the write-off rule never reads past 1.
- The remaining entries are arithmetic that cannot change a result: expected solar at tick 0 is zero,
  a zero shortfall allocates zero, `% 24` on an hour already reduced modulo 24, a float distance that
  can never equal `1e-9`, a zero-weight allocation, and a zero hydro capacity, which allocates
  nothing whichever branch of `water_fill` runs.

**Plan 1a targeted pass** (after `SIM_RULES_VERSION` 2). `-F` selected the functions Plan 1a added or
changed (water filling, the hydro plan and ceilings, AGC retargeting and redispatch, protection and
the trip gates, the frequency-performance verdict, the rate review, the minimum-generation check and
the report counters), sharded so each run takes about 2 minutes at `-j 4`:

| Run | Mutants | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|---|
| `domain` shard 0/3 | 130 | 105 | 0 | 0 | 25 |
| `domain` shard 1/3 | 130 | 124 | 0 | 0 | 6 |
| `domain` shard 2/3 | 129 | 81 | 0 | 0 | 48 |
| `application` | 44 | 40 | 0 | 0 | 4 |

The first pass missed 12 in `domain` (none in `application`). Eleven were killed with tests; the
twelfth, `cap > 0.0` in `water_fill`, is the zero-capacity exclusion above.

**Plan 1b targeted pass** (after `SIM_RULES_VERSION` 3). `-F` selected all of
`grid/power_system/outage.rs` and the functions Plan 1b added or changed: sizing and deliverable
supply, the paced move and its covers, the tick, the boundary dispatch, `begin_day` and
`begin_operating`, the hydro plan and its preview, staged shedding and restoration, the report
accumulator, the invariants, the capacity-shortfall warning, frequency-parameter validation and
the lessons. It ran as four shards of about 3 minutes each at `-j 3`:

| Run | Mutants | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|---|
| shard 0/4 | 135 | 112 | 0 | 0 | 23 |
| shard 1/4 | 135 | 107 | 0 | 0 | 28 |
| shard 2/4 | 135 | 105 | 0 | 0 | 30 |
| shard 3/4 | 135 | 95 | 0 | 0 | 40 |

That is 499 `domain` mutants (383 caught) and 41 `application` mutants (36 caught). The first pass
had 567 mutants and missed 29, all in `domain`:

- Eight were killed with tests: the second boundary dispatch at `BeginOperating` (three), the
  curtailed solar counted as restore cover, no "outage lifted" order on a covered day, and
  `next_outage` with no demand and on landing exactly on its target (three).
- Thirteen (and two timeouts) were in `stages_crossed`, whose division-based first guess was
  corrected by two loops. Any start gave the same count, so the function is now a plain scan of
  the stage thresholds.
- Seven are equivalent and excluded in `.cargo/mutants.toml`: a re-run of an idempotent boundary
  dispatch, the zero solar at midnight, a served load read only when shed is zero, two
  comparisons whose caller or shortcut rules out a difference, and two assignments of an equal
  value. The eighth, the `now - target` of `next_outage`'s early return, is gone: the early return
  now tests the same `gap` the move uses, so that operator is covered by the paced-move tests. The midnight-solar entry already existed but no longer matched: Plan 1b moved its
  operator to another column, so a column-pinned exclusion can silently stop applying.

## Isolation rules (all I/O tests)

`cargo-mutants` runs build copies in parallel, so tests must be hermetic.

- No fixed paths. Each test gets its own `tempfile::TempDir`; fixtures are copied in, never shared.
- No process-global mutable state: no `static mut`, no environment variables, no current-directory
  changes. Configuration is passed explicitly through ports.
- Reading shipped `assets/` by absolute path from `CARGO_MANIFEST_DIR` is allowed (read-only).
- Any future database follows the same rule: one per test (in-memory or a temp-file SQLite), never a
  shared schema.

## Writing a new test: checklist

- [ ] Write it first and watch it fail for the right reason.
- [ ] It sets up its own state and shares nothing with other tests.
- [ ] Any file I/O goes through a fresh `TempDir`.
- [ ] Randomness comes from an explicit seed.
- [ ] Domain tests use `domain`'s `test-fixtures` feature or in-code fixture builders, not plant
      numbers copied into the test.
- [ ] Invariants get a property test, not only hand-picked examples.
- [ ] Error paths assert both the typed `Err` and that state is unchanged.
- [ ] Snapshot tests have a descriptive name, and the diff was reviewed.
- [ ] A change that alters outcomes for the same inputs changes the rules-version guard snapshot
      (`application/tests/snapshots/rules_version_guard__*.snap`): either it is a bug, or
      `domain::SIM_RULES_VERSION` is bumped in the same commit.

## 3D diorama (Plan 3)

`application/tests/save_resume.rs` checks that completed supply curves retain all 1,440 samples,
their original forecasts and represented months across later days and replayed saves, with
property coverage for seeds and tick batching. A commissioning regression keeps the forecast
from before a city connects at month close. `game/tests/game/review_smoke.rs` opens the completed
curve through the real Review button, revisits earlier months through Planning's History selector,
checks the full-width report/retrospective at 1024 and 1920 pixels with 1× and 1.25× UI zoom,
and opens Operating history without changing the simulation or displaying today's incidents
beside an old curve.

`tests/game/world.rs` covers terrain winding/coverage and both shipped regions, camera fit and
projection/ray round trips, bounded camera gestures, pointer-anchored zoom across camera angles,
terrain/roof heights and distance limits, physical viewport rounding and zero-sized
windows, accelerated ray equivalence to the rendered terrain, object bounds, opaque label fitting,
simulation-clock sunlight from east through south to west, monthly arc height and sunrise/sunset
limits, paused lighting, nighttime ambient visibility and Planning's noon preview,
and uniform copper-plate supply. Invariants use property tests.

Headless `WorldViewPlugin` tests reconcile stable IDs without a renderer, check bounded mesh/material
counts through idle frames, rebuild on new games and free every owned asset on leave. A fixed command
script compares checkpoints and monthly reports with a client lacking the world plugin. Construction
state and identity stay visible. Mid-day save/resume tests include world reconciliation and paused
entry in both regions. Real egui passes exercise projected selection, click/drag separation, dialogs,
Review, legend/panel capture, Shift + left-drag and right/middle-drag panning without orbiting or
selecting, all resource layers and text contrast at 1024×640 / 1280×800 and 1× / 1.25×
zoom (the input rectangle is in logical points, as bevy_egui supplies it). The compact layer
selector is clicked through its real popup to guard against map click-through. The opened legend
scrolls inside narrow maps, and panel scroll inertia cannot zoom the camera. Real wheel events
keep city and plant surfaces under the pointer in both regions and Planning/Operating phases,
without selecting or changing the simulation. A camera configuration
regression checks the transparent full-window primary context and separate HDR camera. Actual compositing and shaders still require the native playtest.

```sh
cargo nextest run -p game world_
cargo build --workspace --features game/dev
cargo nextest run --workspace --features game/dev
cargo clippy --workspace --all-targets --features game/dev -- -D warnings
```

Native acceptance covers both regions/phases, camera gestures, picking/building, effects off,
resource/Control-room toggles, Review/menu/load transitions and F12's filtered inspector. Use isolated
`--log-dir` and `--save-dir` paths and retain an existing unsaved player session.


## Expansion balance (A7)

`infrastructure/tests/shipped_regions.rs` keeps the existing hands-off year-one checks and adds
bounds for tycoon build pacing. A property test plans a first expansion to Cleveland/Vancouver from
the public region/session queries, adds up to three CCGTs and their transmission, schedules the city
connection with commissioning, and plays a full year. It checks total capital versus starting cash,
monthly cash reserves, seasonal reliability, forecasts, connection timing and active status. The
scenario reads tuning from shipped data instead of copying plant numbers into test constants.

Build presenter tests compare both total and approximate monthly payments to the actual instalment
split, including singular one-month line quotes. UI fixtures that need several hydro budgets provide
those units explicitly; generic fleet tests derive expected counts from the region.
