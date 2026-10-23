# Plan 4: Daily-cycle battery storage

- **Date:** 2026-10-23
- **Status:** Implemented in the current working tree; automated checks, windowed smoke and native cycle/save-resume QA on both regions pass.
- **Depends on:** Implemented Plans 1, 2, 2b and 3.
- **Binding spec:** [Amendment A9, §20](../specs/2026-10-18-megawatt-mogul-design.md#20-amendment-a9-daily-cycle-battery-storage-2026-10-23).
- **Goal:** Buildable batteries start empty each representative day, charge during daylight
  (morning or afternoon), shave the evening peak, and report then clear unused energy at midnight.

## Decisions to preserve

The user selected grid charging, not only otherwise-curtailed solar. Charging can require more
thermal generation and is always subordinate to customer service. Prefer low forecast net-load
minutes across daylight, including mornings; do not hardcode an afternoon-only start or claim
stored electricity came exclusively from solar. The default evening window begins at 17:00.

Use separate charging MW, discharge MW and internal MWh, with one inventory and explicit losses.
Initial tuning is 200 MW in, 200 MW out, 800 MWh, 90% charge/100% discharge efficiency, $100M capital,
two months construction, $100k monthly O&M and 180-month life, all in region data/fixtures. Reuse
plant identity, connections and construction, with dedicated storage behavior and an enabled toggle.
No battery joins thermal AGC, start-up, commitment or the generator merit-order table. No manual
storage targets or schedule editor in this release.

Daily input = delivered energy + conversion losses + unused internal energy at midnight. Account
for the final minute, capture the remainder, clear inventory once, then close the month/autosave.
Weight energy flows and discarded inventory by calendar days; never weight live MW/MWh capacity.
Revenue and reliability remain city-only. Battery regulatory firm credit is zero initially;
normal capital/depreciation/O&M rules still apply. Save/log envelopes remain version 1 and rules
advance from 7 to 8. The later save-migration plan and spec §21 supersede the initial rejection
policy: rules-6/7 saves are verified by the matching historical engine and copied into portable
snapshots; their originals remain intact.

## Verified starting points and gaps

Inspected before implementation on 2026-10-23. This table records the original seams and required
changes; the implementation record below states the current verification status.

| Current code | Gap / required treatment |
|---|---|
| `domain/src/region/plant_spec.rs`: six `PlantKind`s; `is_dispatchable()` excludes only Solar | Add Battery and explicit generating/thermal/storage classification; audit every kind loop/match |
| `domain/src/generation/plant.rs`: positive output, thermal lifecycle, hydro water | Keep discharge output nonnegative; store charging and SOC separately; bypass thermal stepping |
| `domain/src/grid/power_system.rs`: plant step precedes AGC; targets affect next tick | Coordinate charge requests, actual receipts and evening discharge without phantom current-tick generation |
| `domain/src/grid/power_system/hydro_plan.rs`: sequential hydro water filling | Allocate battery plans first, then hydro against residual customer load |
| `domain/src/grid/power_system/outage.rs`: hour-end forecast sizing | Finite energy needs chronological minute evaluation, including intra-hour exhaustion |
| `domain/src/grid/summary.rs`, `session/invariants.rs`: generation = served + imbalance | Add charging load, battery flow/energy records and storage conservation invariants |
| `domain/src/session/{report,month_close}.rs`: daily accumulation, monthly pricing/weighting | Separate throughput/losses/unused inventory; midnight close before construction/report/autosave |
| `domain/src/company/rate_base.rs`: positive-firm assets share excess capacity | Zero-firm battery has no reserve credit; prevent a battery endpoint shielding a shared generator line |
| `application/src/projections/`: timeline/history/report/warnings | Preserve storage data through live, archived and replayed views; no throughput-based extra revenue |
| `infrastructure/src/region/{dto,mapping}.rs`: six-kind catalog and validating map | Add authored battery DTOs, validate all seven kinds, update shipped/minimal/test regions |
| `infrastructure/src/command_log/dto.rs`, `save/codec.rs`: command-sourced saves | Add enabled command DTO; retain early rules checks and exact archived region replay |
| `game/src/hud/`, `world/`: six-kind cards, charts, models and source/city routes | Storage card, build choice, charts, review summary, model and charging route sinks |

Paths in this table are beneath `crates/`. Domain tests mostly live inline. The `test-fixtures`
region is `crates/domain/src/region/fixtures.rs`, not a separate fixture crate.

## Ownership and implementation shape

- **Domain:** validated `BatterySpec`, `BatteryState`, typed modes/flows, pure scheduling and horizon
  projection, command validation, grid balance, midnight settlement and monthly storage totals.
  Add small modules rather than growing `power_system.rs` into a second monolith. No serde/I/O/Bevy
  or new domain dependency. Keep the stored-energy transition a single authority.
- **Application:** command routing, query/projection plumbing, checkpoint/replay coverage. No display
  strings. Project actual tick records; do not read a reset live inventory to reconstruct past SOC.
- **Infrastructure:** region/command DTOs and validation mapping, exact source archival and save
  compatibility. Existing atomic publication, revision guards and preload semantics remain in use.
- **Game:** pure presenter → draw → `UiAction` → `dispatch`, `ActionSink` getters, Fluent text,
  geometry and illustrative routes. Game never calculates battery energy or writes the domain.

Suggested new modules: `generation/battery.rs`, `generation/battery_schedule.rs`, and
`grid/power_system/battery.rs`. Introduce a shared pure minute-horizon evaluator where it fits the
existing grid forecast code. Names may change; physics must not be duplicated between UI, warning,
outage and operating paths. Typed results distinguish expected future charging from energy already
stored. Numeric methods use stable IDs and fixed convergence rules and draw no RNG.

Each task starts with the stated failing tests, runs them to establish the failure, then implements
the minimum change and refactors. Keep all intermediate dependencies compilable. The rules bump
lands with the first outcome-changing behavior, not as a way to bless unexplained snapshot diffs.

## Task 1: Validated battery model and region data

**Modify:** `crates/domain/src/region/{plant_spec,mod,fixtures}.rs`,
`crates/domain/src/generation/{mod,plant}.rs`, unit types only if necessary;
`crates/infrastructure/src/region/{dto,mapping}.rs`, `assets/regions/*.ron`, and fixture constructors.
**Add:** `crates/domain/src/generation/battery.rs`.

- Write unit/property tests for finite positive ratings, efficiency bounds, optional spec-kind
  consistency, daylight-window validation for all months, and seven-kind catalog completeness.
  Pin the zero start/minimum/fuel/firm-credit constraints and initial shipped tuning.
- Add Battery and explicit classifiers. Audit `PlantKind::ALL`, enum casts/indexing, sort keys,
  `is_dispatchable`, fixture helpers, exhaustive translations, availability and lifecycle matches.
  No boolean shortcut may accidentally treat batteries as thermal generators.
- Add typed internal state with exclusive charge/discharge and exact one-minute input/output
  caps. Test arbitrary valid states/requests, both non-unit efficiencies, full/empty inventory,
  fractional final ticks, conservation and repeated no-op transitions.
- Reuse `PlantId`, `NodeKind::Plant`, existing construction and rate-base asset IDs. Commission
  empty and enabled; no minimum output, commitment, daily/boundary starts or startup money.
- Add battery DTOs and data in both shipped regions and minimal RON fixtures. Keep six-generator
  tuning, starter fleets/cash and topology unchanged. Update existing `PlantSpec` literals/fixtures
  with absent battery data; retain no new dependency in domain.
- Run `cargo nextest run -p domain` and `cargo nextest run -p infrastructure`.

## Task 2: Commands, lifecycle boundaries and query types

**Modify:** `crates/domain/src/session/{commands,views,mod}.rs`,
`crates/domain/src/grid/power_system.rs`, domain error/event definitions;
`crates/application/src/{command,error}.rs`, `pipeline/core.rs`;
`crates/infrastructure/src/command_log/dto.rs`.

- Write failing tests for `SetBatteryEnabled` in valid/invalid phases, all wrong-kind thermal
  commands, unknown ID, construction and game-over cases. Compare the complete state/checkpoint
  before/after a refusal, including command history and projections.
- Implement the enabled command and DTO round trips. Default enabled at commissioning; persist
  its choice across months. Disable future flows without discarding inventory. Same-value is a no-op.
- Exclude batteries from thermal commitment initialization, plant stepping, minimum-generation
  sums, auto-follow target loops, restart/trip candidate lists and start counters. Add an explicit
  zero-inventory day initialization; never call it on resume.
- Expose typed battery details and mode in plant/grid views, with current flows and MW/MWh
  ratings. Keep output as delivered MW only and separate charging load; readiness is not a thermal
  Starting/Stopping transition. Add pure schedule-view types for the following task.
- Advance `SIM_RULES_VERSION` from 7 to 8 when these changes first affect simulation outcomes;
  update the rules guard deliberately and review the digest, rather than accepting all snapshots.
- Run `cargo nextest run -p domain -p application -p infrastructure`.

## Task 3: Daylight charging, evening scheduling and hydro coordination

**Add:** pure scheduling helpers and `grid/power_system/battery.rs`.
**Modify:** `generation/hydro_schedule.rs` if sharing bounded fill machinery,
`grid/power_system/{hydro_plan,battery}.rs`, `session/views.rs`.

- Write hand-computed charge/discharge tests and properties for energy/power bounds, deterministic
  ID order, no simultaneous flows, valley-filling/peak-shaving optimality under unconstrained
  headroom, and reduced feasible allocation under generator headroom limits.
- Use shared regional sunrise/sunset, including fractional-hour boundaries, intersected with
  time before the configured evening start. Test mornings, afternoons, winter/summer, clouds,
  no installed solar, and exact sunrise/sunset/17:00 boundaries. Weather changes expected solar,
  not the clock eligibility for grid charging.
- Plan charging over remaining daylight minutes from actual remaining room. Forecast net load
  includes earlier batteries' requests; conservatively cap requests by expected solar plus
  reachable committed non-hydro generation after cities/earlier charge requests. Do not request
  extra hydro to fill batteries; actual surplus can still be absorbed. This keeps charge-request
  planning independent of the hydro plan. Request more generation only through existing AGC target
  timing. Unfulfilled requests are not receipts.
- Plan evening delivery from actual stored energy, then allocate hydro on the residual. In
  Planning only, project feasible expected charging from empty to preview evening coverage; label
  that optimism explicitly. Operating firm/outage paths do not spend future expected receipts.
- Re-plan at day/hour/evening boundaries and the next tick after enable changes/restoration.
  Freeze elapsed allocations; commands/queries themselves advance no time or RNG.
- Test multiple batteries plus scheduled/manual/stopped hydro, including a stopped hydro with
  AGC still enabled, so storage work preserves the previous eligibility regression.
- Run focused domain tests, then `cargo nextest run -p domain -p application`.

## Task 4: Tick integration, finite-energy forecasts and protection

**Modify:** `crates/domain/src/grid/{dispatch,summary,power_system}.rs`,
`grid/power_system/{outage,hydro_plan,battery}.rs`,
`crates/domain/src/session/invariants.rs`, warning queries and recovery helpers.

- Start with failing scenarios: charging surplus, extra thermal output arriving one tick after
  a request, full battery, evening peaker replacement, empty battery, finite energy exhausted
  mid-hour, under-frequency charging cancellation, blackout and restoration.
- Stage provisional flows after generators step; subtract next-tick intended discharge and add
  feasible charge requests when AGC retargets. Actual charge is capped by surplus after full city
  demand, power and room. Actual discharge fills the deficit, never an already oversupplied bus.
- Anticipate full-charge/daylight cutoff and evening depletion with generator ramp capability.
  Taper charge requests early enough to absorb the resulting generator ramp-down tail, reserving
  room for that tail before opportunistic extra absorption. Pre-position replacement generation
  ahead of discharge falls. A forecast must warn/size real shortages, not invent energy.
- Cancel charging before customer shedding, while frequency is below normal, and throughout
  blackout/restoration. Reduce discharge before over-frequency trips. Allow evening emergency
  delivery above its plan within actual energy/power bounds; no morning emergency discharge.
- Re-evaluate final flows after protection and settle inventory exactly once. Summary, counters
  and SOC use the same post-protection record. Test protection that trips generation after a
  provisional charge and that sheds load after a provisional discharge; no stale or double debit.
- Preserve enabled/SOC through blackout without thermal restarts or free refill; exclude storage
  as a black-start source. Only resume after generating plants complete re-energisation/restoration.
- Update every balance/invariant/headroom/surplus calculation for
  `generation = served + battery_charge + imbalance`, including by-kind/by-plant output sums.
  Keep customer-only demand, unserved energy and reliability denominators. Preserve frequency
  system size as city load after rotating outage, before protection shedding; exclude charging.
- Replace endpoint-only sizing for battery fleets with chronological minute-horizon evaluation.
  Spend each projected inventory once; cover depletion inside an hour, window changes and reachable
  generator/hydro output. Operating uses stored energy only; Planning can model feasible expected
  daytime receipts. Preserve the generator reach-by-hour-end convention that avoids outage
  ratcheting; storage never gets that unlimited-source treatment. Keep minimum-generation
  warnings conservative about optional charging.
- Add properties for pure forecasts, no RNG draws, no energy double allocation, deterministic
  multi-battery priority, speed/batch equivalence and flow bounds across protection transitions.
- Run `cargo nextest run -p domain -p application`, preserving storage-free scenario intent.

## Task 5: Midnight close, money and reserve assessment

**Modify:** `crates/domain/src/session/{mod,report,month_close}.rs`,
`session/month_close/tests.rs`, `crates/domain/src/company/rate_base.rs`, domain events;
`crates/application/src/projections/{monthly_report,plant_history,lessons}.rs`.

- Write failing final-minute tests with hand-computed balances: empty/full/partial/disabled/
  isolated inventory; every month length including leap February; construction finishing at close;
  a batch that reaches or passes midnight; repeated Review/capture/resume/FinishReview calls.
- After the last tick's settlement, capture remaining internal MWh, emit one `BatteryDayClosed`
  per commissioned battery in ID order, clear inventory/flows once, then run existing month close.
  No later commissioning/connection changes the just-finished day's totals.
- Extend daily accumulators and monthly reports with per-plant/fleet charging input, delivery,
  two loss totals and unused-at-midnight. Weight exactly once; assert the full energy identity.
  Preserve raw day records for graphs, including the pre-reset inventory close marker. Tick/output
  invariants must respect the reset boundary without rewriting the final tick or losing its energy.
- Keep city-served revenue unchanged, count fuel at primary generators once, exclude storage
  throughput from generator revenue/merit-order entries and startup costs. Test ledger cents and
  cash identity, fixed O&M, instalments, depreciation and ordinary eligible capital.
- Validate zero battery firm credit and reserve sums. Zero-firm batteries/lines with only a
  battery plant endpoint are not held by the MW excess assessment. When a line also has a new
  positive-firm generating endpoint, use that endpoint for association, with existing ID ordering
  among positive-firm candidates; test both ID orders so a battery cannot shield generator capital.
- Add a typed unused-energy lesson only if it conveys the daily reset without displacing
  reliability priority; keep the existing three-lesson cap. The report itself is always present.
- Run `cargo nextest run -p domain -p application` and relevant rate-review scenarios.

## Task 6: Projections and save/replay continuation

**Modify:** `crates/application/src/projections/{timeline,plant_history,ramp_warning,monthly_report}.rs`,
`crates/application/tests/{save_resume,determinism,state_machine,scenarios}.rs`,
`crates/infrastructure/tests/{command_log,save_codec,save_compatibility,shipped_regions}.rs`.

- Extend tick/history records to carry actual charging/discharge/SOC/loss data plus midnight
  marker. Projections replay against each command's resulting session as today; do not derive past
  charge from live post-midnight zero state. Archive the original forecast assumptions. Exclude
  battery output from the highest-fuel-cost producer calculation; it has no independent fuel price.
- Save/restore mid-charge, mid-discharge, disabled-with-charge, during blackout, in midnight
  Review, after game over and in next Planning. Compare state hash, every public view/projection,
  reports and uninterrupted continuation, including RNG-driven values and command batch shapes.
- Add property-generated valid prefix/continuation cases and failure atomicity. Save a resumed
  run again, remove its parent/log/shipped region in isolated TempDirs, then restore independently.
- Keep save/log format versions 1; add command DTO support without aggregate serialization.
  Prove rules-7 files are rejected before decoding their old region catalog or executing commands,
  with clear existing error mapping and unavailable-library entries. Preserve source bytes.
- Confirm exact archived battery data survives retuning/deleting installed regions. No load,
  preload or autosave replay triggers a second inventory close, historical toast or startup charge.
- Re-pin the rules-8 outcome digest and inspect every scenario snapshot diff. Run all backend
  tests with `cargo nextest run -p domain -p application -p infrastructure`.

## Task 7: Build flow, battery cards and 3D/line indicators

**Modify:** `crates/game/src/hud/{build,plant_panel,plant_list,commitment,inspector,action}.rs`,
`crates/game/src/world/{model,render,ui,overlay,flow}.rs`,
`crates/game/src/l10n/{localize,keys}.rs`, `assets/locales/en-US/*.ftl`.

- Add presenter tests first, then actual-dispatch and egui-pass tests as modules under the
  existing `crates/game/tests/game.rs`. No additional Bevy-linked integration-test binary.
- Add Battery to build/quote/site tools; show charge MW, discharge MW, capacity MWh, costs and
  lead time. Commission/connect through ordinary commands; no storage suitability overlay.
- Add dedicated commissioned and construction cards, level/mode/flows/efficiencies/window,
  today's energy and enabled control. Hide thermal target/AGC/start/commitment controls and keep
  batteries out of commitment and generator merit-order groups.
- Add a fitted battery model/marker (0.8-cell footprint), picking/ViewSync, construction labels
  and restored-session reconciliation. Charging is a route sink, discharge a source; extend routes
  across city-to-city paths with net opposing flows and no physical line-MW/source attribution claim.
- Add Fluent keys and exhaustive kind/error/event translations; retain 4.5:1 text contrast and
  opaque chart/map text backings. Use ActionSink getters and the single dispatch funnel.
- Run `cargo nextest run -p game`, including l10n, real-egui parameter conflicts, picking,
  route direction and minimum-window/zoom controls.

## Task 8: Graphs, monthly review and end-to-end acceptance

**Modify:** `crates/game/src/hud/{forecast_chart,supply_chart,supply_history,review}.rs` and related
presenters; add tests as modules of `crates/game/tests/game.rs`. Update architecture/testing notes
where storage changes their balance/tick descriptions.

- Add positive storage-delivery supply, separate charging-load/total-load presentation, original
  customer/served curves and a labelled MWh SOC trace. Keep legends outside plot data and preserve
  show/hide/hover behavior. Distinguish forecast charge assumptions from actual inventory.
- Add monthly fleet/per-battery throughput, losses and unused-at-midnight summary, with units
  and calendar weighting; also show the daily midnight remainder beside the monthly figure.
  Generator merit order stays on the right. Explain reset and grid charging
  without implying extra revenue; show explicit zero remainder when storage was present.
- Verify live, completed-month History and Supply demand retrospective, including midnight
  marker, old archived months and restore. View switching never mutates or advances simulation.
- Run a crafted end-to-end month through build/commission/connect/enable, daylight charge,
  evening discharge and midnight Review, plus mid-day save/resume. Compare a no-battery control:
  lower peaker MWh on the crafted feasible day, no claim that all fleets always save money.
- Preserve existing East/West hands-off/no-command year-one acceptance on seeds 0–7 with no
  storage in starter fleets. Retain the early-expansion affordability tests and scenario meanings.
- Complete the checks and native playtest below; fix any dispatch/forecast/accounting discrepancy
  in its owning layer, not by changing snapshots or adding free starting energy.

## Final checks and native playtest

Run after implementation, not merely after writing this documentation:

```sh
cargo fmt --all --check
cargo build --workspace
cargo nextest run --workspace
cargo test --doc --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p game -- --exit-after-frames 120
```

Use an isolated save/log directory for native QA. On both regions, build/connect a battery, inspect
MW/MWh and controls, observe a morning or afternoon charge, evening delivery, final unused-energy
report and next-day empty state. Save/resume mid-cycle and verify paused exact SOC, graph history
and future operation. Exercise disabling a charged battery and a blackout without refilling it.
Check line directions, Review layout, minimum window and increased zoom. Smoke launch alone cannot
prove this full cycle. Record the actual checks and limitations in the implementation handoff.

Mutation testing is on demand, not a gate for this documentation task. If requested for implementation,
target the new domain/application energy, settlement and command paths and inspect survivors.

## Implementation record (2026-10-23)

Implemented the domain model and deterministic minute scheduler, typed enabled command, battery-aware
AGC/protection/finite-energy forecasts, midnight settlement, monthly accounting, region/command DTOs,
save/replay continuation, build/cards/models/line directions, and forecast/live/history/review views.
The only operator control is enabled/disabled. Both shipped catalogs use the agreed $100M, two-month,
200 MW / 800 MWh tuning; their starter fleets are unchanged. Charging waits for cheaper generation
to ramp, then tapers around inventory/window boundaries. Hydro is allocated after battery plans.
The optional unused-energy lesson was not added: the report and explicit midnight marker explain
the reset without taking a reliability lesson slot.

Verification completed:

- `cargo nextest run --workspace --status-level fail`: **1,295 passed, zero skipped**. This includes
  storage model/scheduler properties, refusal atomicity, charging/protection/depletion regressions,
  midnight/calendar accounting, two-month instalments, peaker replacement, stopped-hydro eligibility,
  multi-battery continuation and tick-batch equivalence, archived tuning/rules rejection, real egui
  storage controls, full-day line direction changes, history/Review and codec restore. Existing
  East/West no-command acceptance remains green.
- `cargo fmt --all --check`, `cargo build --workspace`, `cargo test --doc --workspace`, and
  `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo run -p game -- --exit-after-frames 120` with explicit shipped assets and isolated temporary
  save/log directories: exited 0. The Metal bindless fallback warning is unchanged.
- Reviewed snapshot changes: existing storage-free outcomes remain unchanged; empty storage report
  fields and rules version 8 are explicit. The rules guard additionally pins a battery-specific
  energy/fuel outcome. Save/log envelopes remain version 1; rules-7 games require the old engine.

Native acceptance completed after the Mac was unlocked, using only isolated temporary saves,
logs and preferences (no player games modified):

- Western region: ordered a 200 MW / 800 MWh battery and ordinary two-month connection through
  the native build UI, advanced both construction months, and verified commissioned/connected
  empty storage. Native daylight charging showed 200 MWh input / 180 MWh stored (rounded), then
  evening delivery reached 191 MW. Disabling at 18:40 preserved 474 MWh; Review showed 14.7 GWh
  monthly unused / 474 MWh per day, plus the retrospective's explicit midnight marker. Next
  Planning showed zero inventory and the disabled choice persisted.
- Eastern region: built storage and a longer three-month line, verified zero throughput while
  disconnected, then observed daylight charging and 200 MW evening delivery. The completed cycle
  reported 26.7 GWh input, 24.0 GWh delivered, 2,667 MWh charging losses and zero unused energy
  (display rounding); next Planning showed empty storage.
- Manual save/resume in Western daylight (08:36, approximately 180 MWh) and Eastern evening
  (17:44, approximately 662 MWh) preserved the exact boundary, paused state, inventory and graph
  history. The codec/continuation tests separately compare full state and projections.
- Minimum 1024×640 window at 125% UI zoom: storage controls remained reachable through the sidebar
  scroll regions. Inspected the fitted enclosure model, inbound/outbound illustrative routes,
  review columns and retrospective. Native QA exposed a graph-height defect: stacking both plots
  in the retained 200-point bottom panel left roughly six pixels of usable supply data. The plots
  now share the available height side by side, with 2:1 supply-to-inventory pane widths. A
  “Combine graphs” toggle overlays inventory on an independent MWh axis and is retained in client
  preferences across live/history/retrospective views, new games and restarts. Regression tests
  check readable plot frames without overlap, the combined axis and physical MWh hover values,
  series visibility and real UI clicks that leave the simulation checkpoint unchanged.
- Blackout: generated a separate QA checkpoint from the real charged Western save through normal
  commands (disable storage, stop generators, advance into blackout, re-enable storage). The
  command probe asserted exact inventory retention and zero flows. Loaded it in the native app:
  the card showed Grid suspended, retained approximately 180 MWh and zero charging/discharge as
  native ticks advanced through the afternoon. Storage did not black-start the grid.

Desktop capture timed out intermittently; reconnecting/restarting the isolated QA app recovered
verification. No simulation panic was logged. The final workspace test run includes the chart
regression; clippy and build passed after the layout change. Mutation testing and a fresh coverage
report were not requested or run.

Chart layout follow-up: verified both the 2:1 panes and combined view in the native Western live
chart and Eastern retrospective using isolated saved games at 1024×640 / 125% zoom. Short live
panels now use MW, $/MWh and MWh axis titles to avoid rotated text reaching into the legend; larger
charts keep descriptive titles. All 1,301 workspace tests passed; after the compact-axis polish,
all 634 game/infrastructure tests, workspace build, clippy and formatting passed again.
