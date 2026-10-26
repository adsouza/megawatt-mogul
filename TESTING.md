# Testing

Development is test-driven: a failing test, then the minimal code to pass it, then refactor. Every
commit leaves `cargo nextest run --workspace`, clippy and `cargo fmt --check` green.

## Layers

| Layer | Where | Tooling |
|---|---|---|
| Unit | Inline `#[cfg(test)]` in `domain` and `application` | std |
| Property-based | `domain`, `application` | `proptest`, `proptest-state-machine` |
| Integration and scenario | `application/tests/`: command scripts against small fixture regions built in code | std, `insta` |
| Adapter | `infrastructure/tests/` (parses every shipped region file) | std |
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
CARGO_PROFILE_DEV_OPT_LEVEL=3 CARGO_PROFILE_DEV_DEBUG=0 \
  cargo nextest run -p game --run-ignored ignored-only -E 'test(max_grid_frame_budgets)'
                                                 # 256 × 128 frame budgets (timing, ignored by default)
```

`max_grid_frame_budgets` (`tests/game/world.rs`) times terrain build, picking, the resource overlay
and the region picker's paint on the 256 × 128 maximum grid. It is `#[ignore]`, so ordinary runs skip
it. The command above runs it with the workspace crates optimised, and is a proxy for `--release`
that spares the disk. A plain-profile run compiles the workspace crates unoptimised, and is
expected to miss the overlay and picker budgets.

Property tests use `proptest`; set `PROPTEST_CASES` to run more cases locally. Note that nextest does
not run doctests; run `cargo test --doc --workspace` for those.

Test builds optimize the `domain` crate at level 2, including when downstream integration tests
exercise it. Debug assertions and integer overflow checks remain enabled. Ordinary development
builds keep their existing settings; this does not switch tests to the release profile.

### Behavioral contracts across features

Line coverage, replay equality and pinned outcomes do not establish physical correctness. Keep
independent expectations alongside tests that compare consumers of a shared calculation:

- `domain/grid/power_system/tests/transition_properties.rs` constructs valid cyclic commitments
  with shutdown **plus** startup in every off-gap. Every generated day must start with online,
  offline, starting and stopping plants; it must actually complete starts and stops. It varies
  manual targets, AGC, connectivity, scarce hydro water and the placement of mixed runs. Every
  minute checks physical output transitions through the whole controller, reported generation,
  disconnected supply and water consumption. Only an explicit plant-trip event permits an
  instantaneous loss; resource caps and startup minimum output have separate explicit rules.
  A second property checks that extra fixed generation displaces the same amount of midnight AGC.
- `application/tests/state_machine.rs` mixes constructively valid recurring runs with hostile
  patterns and wrong-phase commands. A separate property requires generated mixed commitments
  to be accepted; increasing random seeds must not substitute for reaching valid schedules.
- `domain/session/forecast_contract_tests.rs` derives thermal supply from commitments, manual
  targets and eligibility independently of `supply_forecast()`. It checks every minute, including
  connected construction, and verifies warning magnitude/hour. Mixed hydro/solar/storage cases
  calculate solar output, cumulative water use and stored energy with losses independently,
  including disabled, disconnected and unfinished batteries. A balanced, noise-free manual
  fleet compares forecast against every actual operating minute. This exact comparison is
  intentionally restricted: the general forecast is an optimistic capacity estimate, not a
  simulation of every ramp, protection action or random demand error.
- `game/tests/game/player_workflows.rs` uses actual widgets at minimum window size (including
  enlarged UI zoom): scroll to a plant, edit and apply a valid commitment, inspect the forecast,
  dismiss/accept the shortfall prompt, operate at 32×, open the retrospective, continue and revisit
  unchanged history. A refused infeasible schedule must restore an editable plan and leave
  Start usable. Check command multiplicity, simulation state and control visibility as well as
  the presenter data; setup commands and phase advancement alone do not prove a player task works.
  `game/tests/game/battery.rs` also takes a charged battery through disabling, naming a real disk
  save with Enter, leaving, selecting Load/Resume, and enabling it again. The full checkpoint,
  paused clock, enabled state and stored energy must survive, and evening discharge must work.

When adding a regression, ensure it fails for the faulty behavior independently of other fixes.
For example, removing a midnight dispatch error must not hide unsafe surplus protection. Prefer
small physical oracles and explicit transition coverage over blessing changed snapshot numbers.

Optional economic zones have independent arithmetic and compatibility checks:

- `domain/region/economics_tests.rs` covers inheritance, all-default identity, exact boundary
  sampling, repeated crossings on a descending diagonal, water, endpoint reversal, rate-LCM and
  quote overflow, and typed map validation. Session properties check cent-exact instalments and
  unchanged state/IDs on rejection.
- `infrastructure/tests/economic_construction.rs` follows two local plants and a cross-zone line
  through payments, cancellation, commissioning and original rate-base valuation; save/resume
  continues identically even after the installed source changes. Forged effective specs and
  changed archived economics are rejected. `region_catalog.rs` checks strict DTOs and public
  list/load/outline consistency, including typed rate-LCM overflow.
- `infrastructure/tests/support/economic-baseline/` is the immutable rules-16 capture taken at
  `2009acc` before this implementation. Never regenerate it. Tests verify its original source,
  canonical state/projections, boundary hash and construction continuation. Literal dynamic hashes
  retain the macOS/aarch64 origin-platform caveat.
- `game/tests/game/economics.rs` uses real widgets and the real egui pass for local previews,
  orders, construction/installed details, exact route breakdown, minimum-window enlarged-zoom fit
  and text contrast. Its fictional profiles and names are fixtures, not national price estimates.
  [Plan 6a](docs/superpowers/plans/2026-10-25-plan-6a-economic-zones.md#implementation-and-validation-record)
  records the isolated native save/resume-through-commissioning acceptance.

The new-game globe's pure tests in `hud/region_globe.rs` pin its 22° tilt, 24-second rotation,
bidirectional drag and two-second idle hold, with projection round-trip properties. Real widget
tests in `game/tests/game/region_picker.rs` verify that dragging never selects a region, rotation
and picking agree, and the existing seed validation, exclusions, keyboard cards, small-window fit
and text contrast remain intact. They also cover responsive one/two-column cards, row-major
keyboard access, reversed automatic rotation, and hover titles painted after every region fill.
Card-body tests cover selection from the hidden hemisphere, midpoint alignment and a two-second
hold before automatic rotation resumes; pure motion properties pin shortest arcs, frame partition
independence and manual interruption. Rendering clips the far hemisphere before triangulation.

The year-one checks in `infrastructure::shipped_regions` run one test per region and mode, so
nextest schedules them in parallel and names a failure: `year_one_hands_off_<id>` and
`year_one_no_commands_<id>` for each of the seven shipped regions, each playing eight seeds ×
twelve months. `year_one_cases_cover_every_shipped_region` fails if a shipped region lacks its
pair. `shipped_regions_never_order_outages_hands_off` covers every shipped region and seed too.
A12's three-review battery comparison has sixteen independent region/seed tests, each playing two
companies for 36 months. They use `GameSession` through the real core command handler and invariant
checks, without application projections or recording, and disable added batteries immediately after
commissioning to isolate idle-capital abuse. They retain the assets' firm capacity and capital and
assert that storage flows stay zero. Two additional seed-7 cases, one per region, run the full
application pipeline with enabled batteries and require actual storage operation. A state-byte
comparison through construction, commissioning and the first battery operating month checks the
streamlined driver against the application. Nextest can schedule the cases in parallel; use a
focused filter when changing an unrelated rule.

These scenarios check every added battery and its dedicated line at the first review, including
the line's matching held fraction. Focused domain tests use a fully excess fixture to check exact
book values for both asset kinds at holding and write-off. A fast property test independently
calculates cent-exact partial holdings, eligible returns and subsequent depreciation across
generated costs, lives, first-review lengths and one to four associated lines; it also checks that
capital adjustments are non-cash and written-off value is not recovered again.

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
  - `planning_performance.rs`: idle passes and view changes reuse cached analysis, while an applied
    plan edit refreshes the Start-the-day shortfall confirmation before it is drawn;
  - `operating_side.rs`: the gauge, the fixed-height details block and the plant list that fills
    the rest, whatever is selected and however the panels are sized;
  - `city_card.rs`: a clicked city's card in Planning's inspector and Operating's details block;
  - `construction.rs`: the Construction tab and the construction card of a plant being built;
  - `map_legend.rs`: every city clickable at 1024×640, 1280×720 and 2560×1440 in the Eastern and Western regions and
    phases, with the default legend (open in a corner that hides no city or plant, or collapsed)
    and opening at 1280×720; the legend toggled by the player, and inside the drawn map with every
    panel at its maximum.
- **The 1024×640 fit tests** (`at_the_smallest_window_…`) lay out each screen at the minimum window
  size, for the Eastern and Western regions, and check that panels don't overlap and that the controls a
  player needs (*Apply*, *Continue*, the figures of the selected plant) are inside their panel.
- **The panel resize tests** (`tests/game/panel_resize.rs`) drag each outer panel's inner edge
  through the real pass (`EguiPass::drag`) at 1024×640 and 2560×1440: it grows with the drag,
  stops at its opening size and at its maximum, the map's area keeps 320×200, and a selection,
  a tab or a whole day afterwards moves neither a panel nor the map.
- **The end-to-end month** (`tests/game/end_to_end.rs`), for the Eastern and Western regions on seed 7: a new
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
  preference files, restart, real sidebar/bottom-panel drags, zoom/speed (including 16×/32× across pause and restart), effects toggled through
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
  for the Eastern and Western regions' screens and verify opaque backings behind map labels and bar hover text. Run
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
- **No outages hands-off:** the same hands-off plan orders no rotating outage in any of the 672
  region-seed-months (seven regions × eight seeds × twelve months; spec §15.8).

The shipped-region catalog is also checked for provenance and geography (Plan 7):

- **Research-row provenance:** `new_region_entries_cite_research_rows` requires every plant entry,
  `lines:`, `starting_tariff_per_mwh:`, `cash:` and economic-zone profile of the five Plan 7
  regions to have a `[P7-…]` citation in the comment block directly above it, and every cited ID
  to be a row of `docs/research/2026-10-25-plan-7-regional-economics.md`. Its parser tests pin what
  counts as a row and as a citation. Tuning numbers stay in region RON; the note holds sources.
- **Authoring reproducibility:** `europe_maghreb_zone_cells_are_reproducible` regenerates the
  Europe–Maghreb `economic_zones.cells` from `crates/infrastructure/authoring/` and compares them
  with the shipped file. The Python tools under `tools/region_authoring/` (Natural Earth polygon
  extraction, the research-map extent check) have their own offline tests:
  `python3 -I tools/region_authoring/test_region_authoring.py`. See that directory's `README.md`.

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
  three frequency-performance strikes, and the rules-14 trace revokes the licence at the November
  2029 close (strikes in August, October and November; September stays below the threshold).
  This is deliberately a game-over script.
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

`tests/game/world.rs` covers terrain winding/coverage and the Eastern and Western regions, camera fit and
projection/ray round trips, bounded camera gestures, pointer-anchored zoom across camera angles,
terrain/roof heights and distance limits, physical viewport rounding and zero-sized
windows, accelerated ray equivalence to the rendered terrain, object bounds, opaque label fitting,
simulation-clock sunlight from east through south to west, monthly arc height and sunrise/sunset
limits, paused lighting, nighttime ambient visibility and Planning's noon preview,
and uniform copper-plate supply. Flow-route tests cover generation reversals, transit cities,
stable loop routing, opposing transfers, disconnected/unfinished routes and chain cut balances.
Dot tests check spherical shape, route reversal/path bounds, animation/pause, overlays, blackout
hiding, asset reuse and checkpoint preservation, including bright cyan dots against deeper blue
energized lines independent of sunlight.
The Western starter network regression checks that the overlapping Seattle–Grand Coulee and
Seattle–Spokane connections show one net direction, and zero transfer hides their dots.
Generated overlapping intervals vary direction, slope, weights and endpoint order and require
one stream per shared stretch with the algebraic net; example checks cover cancellation,
partial overlap and construction exclusions.
Window tests check facade placement, demand-scaled transforms, lit/dark groups at full, half and
zero supply, disconnected cities, overlay hiding and lit walls without whole-building emission.
A property test checks proportional window lighting and monotonic darkening as supply falls.
Invariants use property tests.

Headless `WorldViewPlugin` tests reconcile stable IDs without a renderer, check bounded mesh/material
counts through idle frames, rebuild on new games and free every owned asset on leave. A fixed command
script compares checkpoints and monthly reports with a client lacking the world plugin. Construction
state and identity stay visible. Mid-day save/resume tests include world reconciliation and paused
entry in every shipped region. Real egui passes exercise projected selection, click/drag separation, dialogs,
Review, legend/panel capture, Shift + left-drag and right/middle-drag panning without orbiting or
selecting, all resource layers and text contrast at 1024×640 / 1280×800 and 1× / 1.25×
zoom (the input rectangle is in logical points, as bevy_egui supplies it). The compact layer
selector is clicked through its real popup to guard against map click-through. The opened legend
scrolls inside narrow maps, and panel scroll inertia cannot zoom the camera. Real wheel events
keep city and plant surfaces under the pointer in the Eastern and Western regions and Planning/Operating phases,
without selecting or changing the simulation. A camera configuration
regression checks the transparent full-window primary context and separate HDR camera. Actual compositing and shaders still require the native playtest.

```sh
cargo nextest run -p game world_
cargo build --workspace --features game/dev
cargo nextest run --workspace --features game/dev
cargo clippy --workspace --all-targets --features game/dev -- -D warnings
```

Native acceptance covers the Eastern and Western regions/phases, camera gestures, picking/building, effects off,
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


## Battery storage (Plan 4)

Domain tests exercise 00:00–18:00 grid charging, evening gap support, efficiency and capacity limits,
blackout/islanding, shared-grid accounting, daily inventory close and monthly weighting. Property
tests cover energy conservation, schedule bounds and arbitrary save boundaries. Application tests
compare tick batches, preserved inventory, reports and completed curves after portable restore.
Game tests use real enabled controls and graph layouts, with and without connected batteries,
at the 1280×640 minimum window size. Client preferences remain separate from simulation state.

## Midnight dispatch and surplus protection (May 2033)

`domain::region::fixtures::may_2033_parts` reduces the incident to its five hydro units, four
CCGTs and 3,552.12 MW midnight demand without retaining a personal save. Domain tests check that
the evening CCGT's shutdown tail is counted exactly once, and that a separate injected surplus
engages protection while respecting ramps and preserving service. Properties cover arbitrary
evening runs and manual/AGC tails, and forbid curtailment from replacing dispatch outputs/targets.
A resource-limited hydro tail must be capped before it displaces other midnight supply.
The application scenario checks the full May day, reports and operating-boundary snapshot restore.
Rules-8 portable golden snapshots must retain state/history through the rules-9 upgrade; original
fixture files are never regenerated. Actual player saves may be verified read-only with temporary
in-memory continuations, preserving source bytes and historical reports.

## Save migrations and portable state (A10)

`domain/tests/portable_state.rs` checks complete state/RNG continuation at arbitrary tick boundaries
and rejects hostile bytes without panicking. `application/tests/save_resume.rs` covers canonical
state/projection hashes, the complete audit journal, schema/rules compatibility and battery inventory
across restoration. Domain remains free of serde and I/O.

`infrastructure/tests/support/portable-saves/` freezes format-2/schema-1 examples in every phase,
including a charged battery. Never regenerate old fixtures to clear a compatibility failure; retain
readers or add explicit schema upcasters. CI must restore, continue and re-save all of them.
`infrastructure/tests/save_snapshots.rs` also checks historical-rule refusal, original-slot write
protection, provenance deduplication, unknown future schemas and snapshot-rooted diagnostic replay.
`game/tests/game/save_snapshots.rs` loads converted saves with the normal Resume buttons and checks
read-only preload/activation: loading creates no new player save or recovery prompt.

Historical engines, the converter, twelve pre-battery fixtures, migration tests and supporting
sources are retained only in `tools/archive/save-recovery-2026-10-23/recovery-source.tar.gz`, outside
the Cargo workspace. Verify its checksum and extracted source hashes, build it in an isolated
checkout/target directory and run its nine migration tests. The shipped game's Cargo dependency
tree must contain no `legacy-sim`; do not reintroduce the engine to fix a snapshot reader.

Delayed-worker game regressions prove monthly autosaves show no modal, keep Review/retrospective
and Continue usable, preserve exact closing boundaries while later play proceeds, serialize queued
months and never pause a later day on receipt. A failed earlier save remains retryable with newer
boundaries queued; cancelling a manual dialog does not cancel the background save. Existing fault
and durability tests retain warnings and original checkpoints. Every writable fixture uses its own
TempDir. Native acceptance uses isolated save/log directories; real saves/backups are hash-checked
before and after work, never used as writable test fixtures.


## Fast save-library metadata

`infrastructure/tests/save_store.rs` asserts that cards can be extracted without constructing a
playable region or decoding command history, while selected reads still reject invalid content.
An injected listing reader proves warm scans do not read unchanged files. Examples cover external
rename/add/delete/recreation, same-size edits with restored mtime, atomic-replacement races and
transient read errors. Restart tests prove unchanged files cause no source reads; changed/deleted
sources refresh/evict individually. Missing, truncated, corrupt, oversized, obsolete, foreign-build
and unwritable disk indexes fall back without changing saves. Unavailable cards persist; transient
I/O errors do not. A property test compares memory, restarted disk and independent fresh scans after
arbitrary slot changes.
The listing projector has property coverage for arbitrary escaped strings, nested decoys and hostile
UTF-8, plus normal-parser fallback for noncanonical RON. Existing integrity/version/provenance tests
remain binding. `game/tests/game/save_snapshots.rs` paints the real browser before polling its
refresh worker and verifies that cached cards appear without Working/Loading text.

`cargo run -p infrastructure --example save_library_timing -- <save-directory>` measures cold and
warm and process-restart scans (only disposable metadata is written). Full validation remains separate:
`cargo run -p infrastructure --example verify_saves -- <save-directory>` restores every visible
snapshot. Native acceptance opens the library from the menu and loads a selected game with isolated
save/log directories. Hash-check actual player files and backups before/after the work.


## Prepared Resume activation

`application/tests/save_resume.rs` covers the ownership boundary that freezes a game and its
captured checkpoint until recording activation. Infrastructure tests bind precompressed recording
bytes to the exact checkpoint, reject mismatches/cancellation before publishing sidecars, and
verify replay of the prepared base plus newly accepted commands. Existing game tests require the
opening menu to paint before preload begins, remain interactive, create no speculative recordings,
reuse one in-flight worker, discard prepared memory on other choices and reject replaced sources.
`cargo run -p infrastructure --example resume_timing -- <save-directory>` separates source decoding,
restoration, in-memory preparation and explicit activation using temporary recording files.


## Lossless tick RLE (save envelope 3)

`infrastructure/tests/save_codec.rs` verifies adjacent-only runs, changed speed/action boundaries,
zero-tick batches, malformed/oversized counts and arbitrary batch-size/repeat/action round trips.
The frozen `tests/support/tick-rle-format1.mgsave` was captured with the pre-RLE writer. Its
rules-8 command-only state remains version-fenced. Recompute its batch pattern with current rules
and its exact archived region in a TempDir, then verify format-1 load, RLE re-encoding, per-command
events, projections and diagnostic replay. Existing format-2/schema-1 golden snapshots remain immutable and
must restore/continue/re-save as format 3. Unit tests cover combined expansion limits and
cancellation during expansion. Counts and audit hashes always cover original expanded commands.

`cargo run -p infrastructure --example snapshot_rle_size -- <save-directory>` compares the latest
automatic snapshot's original bytes with current encoding in memory, verifies full restoration,
and leaves source snapshots untouched (only the disposable metadata index may refresh).


## Monthly binary chart directories (format 4)

`application/tests/save_resume.rs` checks exact chart split/join across Planning/Operating/Review
and arbitrary tick boundaries; compare canonical bytes, not just visual output. Keep all live
buffer/forecast and completed-month samples. `infrastructure/tests/monthly_charts.rs` covers binary
monthly files and the compressed text manifest, exact restoration, every publication fault for
new/overwritten snapshots, cancellation, durability retries, immutable-file reuse and orphan cleanup,
rename/delete conflicts, deletion retries with recreated slots, cached restart metadata, corrupted/
missing parts, path traversal and symlinks. Production game save/resume asserts directory output and
compares the complete saved state/projections after reloading it. Diagnostic replay tests exercise
the same binary layout in resumed recording bases.

Personal conversion is one-time local work: ignored tooling verifies every source/converted
checkpoint before publishing the library, then verifies all originals in an external backup.
No migration prompt or converter is linked into the game. `/tools/archive/` is Git-ignored; do not
commit archived tools, backups, migration receipts or staging saves. Native acceptance uses isolated
copies to Resume, inspect retrospective curves, save and reload directory snapshots.

`tests/support/monthly-directory-v4/` retains a permanent binary-format compatibility fixture,
paired with the older charged-battery format-2 save. Its original bytes are immutable; tests load,
compare exact canonical state, continue, and re-save a temporary copy. This fixture is part of the
normal tests and is distinct from Git-ignored personal recovery tools and backups.

## Rate-base need tests (A12, rules 13)

Region-catalog tests distinguish schema 2's required accreditation and satisfaction fields from
schema 1's frozen compatibility defaults. They reject markerless A12 fields, unsupported versions,
malformed ladders and invalid satisfaction parameters, and cover DTO round trips. The duration
ladder and allowed-return formula have property tests; session tests compare accreditation across
commissioning, stored profiles, total firm MW and pure estimates. Excess batteries are held and
then written off; useful storage is retained.

Generated line build/cancel sequences use an independent component model to check the forest
invariant and exact refusal conditions. Rejections preserve the complete application checkpoint
and audit journal; cancelling an unfinished line frees its route. Directly restored cycles remain
valid and commission. HUD checks distinguish the need-test reason from an exact duplicate and
paint the satisfaction row whole with 4.5:1 contrast at 1280×640. Only a live December Review shows
its current satisfaction contribution.

The frozen schema-1 battery save remains byte-equivalent after restore apart from the rules
header: a preview uses new accreditation without changing saved profiles, which refresh in
December. A saved December Review continues without repeating the annual review. The shipped
acceptance compares ten extra batteries against a baseline for the Eastern and Western regions, seeds 0–7, across
36 months: excess storage is held at its first review and final cash never beats the baseline.

`grid::power_system::tests::battery_planning` compares cached forecasts and complete charge plans
bit for bit with the original uncached calculations across generated months, seeds, live/preview
modes, thermal states, manual targets, disabled storage and inventory overrides. It also checks
that schedule-only projection produces the same schedule bytes as collecting forecast points.
Planning scratch data lives only for an immutable query; it is never persisted or reused across
simulation ticks. Keep the full 16-case, three-review shipped acceptance when optimizing this path.

## Wind farms and revised charging (Plan 5, rules 14)

`domain/grid/power_system/tests/wind.rs` independently checks 120 MW from a 300 MW farm at 80%
site potential and 50% resource throughout a whole day, shared RNG across farm counts, correlated
forecast bands, proportional Solar/Wind curtailment, a sudden lull with finite storage, and the
17:00–18:00 overlap/18:00 cutoff. `generation/wind.rs` property-tests the bounded AR(1) formula.
`session/forecast_contract_tests.rs` includes wind in the independent mixed hydro/solar/storage
oracle. `session/wind_tests.rs` refuses thermal controls atomically and compares continuation/RNG
at midnight, hourly, overlap, cutoff and Review save boundaries, including next-month Planning.

Region adapters require schema-3 wind data and cutoff fields; frozen legacy extension RON payloads
are pinned by independent SHA-256 values. The released schema-1 golden saves stay immutable and
are verified against the schema-1 canonical encoder before upcasting. Tests compare old fields,
RNG, inventory and projections rather than requiring new-schema hashes to equal old-schema hashes.
Monthly/flat save tests round-trip wind/battery states across policy boundaries and compare the
next command. Existing schema-1 compressed monthly charts still join to their original bytes.

The single game integration binary includes `game/wind.rs`: actual build-widget clicks flow through
dispatch, construction, a needed connection and commissioning; the forecast, world model and real
minimum-size plant card show wind with no thermal/storage controls. Existing battery toggle, Enter
save, resume, history and chart-layout workflows remain covered. Windowed smoke/native QA are
recorded in Plan 5. Focused checks do not grant permission to run the full local workspace suite.

### Rules-14 RNG and source-schema regression checks

`grid::power_system::tests` independently pins solar then wind at day start, demand then wind per
minute, and reuse of the initial wind sample at minute zero. Planning/outage sizing must add no
extra draws. Hydro cap/re-plan and final-hour tests use controlled noise-free fixtures: the
final-hour oracle is 384 MW from the authored water budget, and recovered unplanned hydro targets
the whole served load. Noisy emergency cases separately pin one shedding episode, no blackout,
no shedding after restoration and reviewed rules-14 energy limits. The over-frequency settling
oracle integrates the three known surplus windows instead of assuming a 3 Hz peak.

Game `save_snapshots` tests compare restored schema-3 captures to the immutable schema-1 fixtures
using source-schema canonical gameplay/projection bytes. They also check upgraded battery cutoff,
header rules, journal, command count, summary, archived region, provenance and unchanged disk bytes.
A new capture's schema/hash may change; elapsed history and original save files may not.
