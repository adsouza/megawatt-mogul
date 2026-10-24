# Plan 5: Wind farms and midnight-to-18:00 battery charging

- **Date:** 2026-10-23
- **Status:** Implemented (2026-10-24); verification recorded below.
- **Depends on:** Implemented Plans 1–4, portable saves (§21), residual-gap battery support
  (§23, rules 10), and Planning forecast reuse (§24).
- **Binding spec:** [Amendment A11, §25](../specs/2026-10-18-megawatt-mogul-design.md#25-amendment-a11-wind-farms-and-midnight-to-1800-battery-charging-2026-10-23).
- **Goal:** Build/connect onshore wind, use its correlated availability throughout dispatch and
  planning, and allow empty batteries to charge immediately from midnight until **18:00**.
  Keep 17:00–24:00 deficit support and exactly-once midnight inventory reporting/reset.

## Decisions and boundaries

The user's final charging cutoff is 18:00. There is no sunrise gate, sunset cutoff or charging
after 18:00. During 17:00–18:00 choose charging on real surplus or discharge on a real deficit;
never both in the same minute or one battery feeding another. Charging is subordinate to all
customer load and protection. Eligibility at 00:00 does not create energy before generators
produce it. Keep the existing inverter ratings/efficiencies and A12 duration-based battery accreditation (§26).

Wind is buildable onshore generation with site potential and a single shared regional process,
not dispatchable thermal capacity. No starter fleet/cash or existing generator tuning changes.
Use the initial farm/availability tuning in §25.2; author maps in RON, not code constants.
Wind's seasonal forecast is a central scenario with an indicative range, not guaranteed output.
No offshore assets, storms, turbine wakes/failures, wind-speed model or manual storage editor.

Coal and CCGT already need off-gaps long enough for shutdown plus startup. Shipped coal requires
600/6 + 480 = 580 minutes (10 hours on the hourly grid); CCGT requires 500/25 + 120 = 140 minutes
(three hours on the hourly grid). These include shutdown/restart rather than an extra fully-offline
dwell period. The documentation correction does not introduce minimum-on/off dwell timers.
Preserve existing direct regressions and fill only missing coverage; do not alter commitment
semantics.

Implementation was authorised on 2026-10-24. The tasks below follow
TDD: establish a focused failing test, implement the minimum behavior, pass and refactor. Keep
intermediate work compilable; do not expose new-rule gameplay until the matching save reader and
upcaster are available. No new domain dependency, user-facing domain/application strings, Bevy
outside game, global test state, unchecked player-input panic or HashMap iteration is permitted.

## Inspected starting points

Inspected at `5064032` on 2026-10-23; re-check the checkout before implementation.
Paths below are relative to the repository root.

| Current owner | Required change |
|---|---|
| `crates/domain/src/lib.rs` | Inspected baseline was rules 10; implementation rebased on A12 rules 13 and advances to 14 |
| `region/plant_spec.rs` | Seven asset kinds; append Wind/tag 7; separate variable generation/storage classifiers; add charge-end hour |
| `region/{terrain,params,mod,fixtures}.rs` | No wind potential/parameters; seven-kind validation and battery daylight validation must evolve |
| `generation/{plant,commitment,solar}.rs` | Shared non-dispatchable branches require explicit storage exclusions; wind uses resource availability |
| `grid/power_system.rs` | Solar-specific availability, curtailment, midnight dispatch and reconnect helpers |
| `grid/power_system/{battery,hydro_plan,outage}.rs` | Expected solar is subtracted directly; storage predicate excludes evening; charging `if/else if` suppresses overlap discharge |
| `session/views.rs` | Solar-only net-load projection and daylight battery-window view |
| `application/src/projections/` | By-kind histories, events, reports and chart archive encodings must carry wind without rewriting past values |
| `application/src/{game,checkpoint}.rs` | Portable snapshot schema is hardcoded to 1; original canonical bytes are verified before installation |
| `infrastructure/src/region/{dto,mapping,raster}.rs` | Geographic resource fields and catalog DTOs lack wind |
| `infrastructure/src/save/{codec,listing,monthly}.rs` | Several schema gates and monthly binary split/join paths must support schema 2 while retaining schema 1 |
| `game/src/hud/`, `game/src/world/` | Kind lists, cards, static suitability maps, charts, turbine geometry and charging copy |

Domain-only paths in the table are under `crates/domain/src/`; the application/infrastructure/game
paths without a `crates/` prefix are under `crates/`. The tiny region fixture is
`crates/domain/src/region/fixtures.rs`. Game integration tests stay modules under
`crates/game/tests/game/`, registered in `crates/game/tests/game.rs`.

## Task 1: Freeze compatibility contracts and add wind/battery data types

**Modify:** `crates/domain/src/region/{plant_spec,terrain,params,mod,fixtures}.rs`,
`crates/domain/src/generation/{mod,plant,commitment}.rs`,
`crates/infrastructure/src/region/{dto,mapping,raster}.rs`,
`assets/regions/{eastern_interconnection,western_interconnection}.ron`,
`crates/infrastructure/tests/support/minimal_region.ron` and relevant fixture constructors.
**Add:** versioned schema-1 wire readers/encoders in small domain/application compatibility
modules before editing positional state records; frozen pre-wind region extension data under
infrastructure compatibility fixtures/data, outside simulation constants.

- [x] Freeze current schema-1 golden bytes/readers and tests before changing `state_record!`
  layouts. Cover Planning, Operating with stored charge, blackout and Review, including existing
  compressed monthly chart payloads. Keep old fixtures immutable and capture source hashes.
- [x] Add failing validation/tag/classification tests: Wind appended as tag 7, all eight kinds
  represented, old tags/order stable, Wind never storage/thermal, Battery never variable generator.
- [x] Add `WindParams`, cell potential and `BatterySpec.charge_end_hour`. Require wind parameters
  and a Wind catalog entry for current-format regions; validate all §25.2 constraints and
  `1 <= discharge_start_hour <= charge_end_hour <= 24`. Remove storage's daylight-interval
  validation and retain solar's own daylight validation. Both shipped charge-end values are 18.
- [x] Rasterise an explicitly authored wind field using the existing Gaussian-field mechanism.
  Pin exact field payloads and representative high/low/zero-potential sites in both shipped
  maps. Zero potential is legal on land; no water/occupancy exception is added. Tests must show
  solar, cooling, hydro, terrain and existing city/plant locations are unchanged.
- [x] Add the §25.2 catalog/seasonality values and fixture values. Freeze this initial region
  extension for legacy saves. Do not use whatever shipped tuning happens to exist at load time.
- [x] Add pure region compatibility mapping for old archived content with a versioned frozen
  extension. Keep new-content validation strict; distinguish legacy missing fields from malformed
  authored fields. Preserve exact archived source bytes and existing tuning for hashing/provenance.
- [x] Add/preserve coal's 9-hour rejection/10-hour acceptance and CCGT's two-hour rejection/
  three-hour acceptance, including midnight-wrapping off-gaps. Retain existing CCGT commitment
  regressions rather than duplicating them. Document shutdown/start-up feasibility separately
  from independent dwell timers, and keep command refusal atomicity.

**Focused checks:** region/commitment tests in domain; region parsing/raster/catalog tests in
infrastructure. No changed live behavior is released in this task.

## Task 2: Pure wind process, resource output and forecast contracts

**Add:** `crates/domain/src/generation/wind.rs`.
**Modify:** `generation/mod.rs`, `grid/power_system.rs`, `session/mod.rs`, typed view/forecast
records and explicit state encodings; use a small `grid/power_system/variable_generation.rs`
module for shared Solar/Wind availability and forecast plumbing if it keeps owners clear.

- [x] Write independent formula tests for bounded AR(1) evolution and capacity × potential ×
  factor output. Cover zero noise, bounds/clipping, low/high sites and midnight/night generation.
- [x] Implement the exact §25.3 draw contract: initial wind draw after solar, then one normal
  after demand and before generation for each minute 1–1439. Minute 0 reuses the initial sample.
  All days consume the same wind draws regardless of fleet/weather/protection state. No draw
  occurs in a query, paused UI frame, declined command or restore. Test RNG positions explicitly.
- [x] Store the regional factor and its day/minute initialisation boundary. Multiple farms share
  it and scale only by capacity/site. Commissioning/building/connecting a farm never rerolls wind.
- [x] Add pure central/range forecasts: monthly nominal factors in Planning, current conditioned
  factor in Operating, clipped indicative bands and per-kind totals. Do not sample future weather
  or claim exact clipped-process means/statistical coverage. Shared errors never diversify away.
- [x] Expose typed plant/grid wind views and shared variable net-load queries. Add tests that
  repeated query/chart passes preserve full state/RNG and use the same per-kind arithmetic.

**Focused checks:** wind/RNG/forecast tests in domain. Review hand-computed expected values rather
than generating expectations with the production forecast helper.

## Task 3: Schema 2 and ordinary-load migration before gameplay cutover

**Modify:** domain/application compatibility/state owners,
`crates/application/src/{game,checkpoint}.rs`,
`crates/application/src/projections/{mod,timeline,chart_archive,monthly_report}.rs`,
`crates/infrastructure/src/save/{codec,directory,listing,listing_cache,monthly}.rs`, command-log checkpoint
sidecars and their tests. Audit every schema check with `rg`, not only the primary restore API.

- [x] Introduce schema-2 readers/writers for added region/state/spec/forecast/curtailment fields.
  Preserve explicit kind tags, scalar widths and collection ordering. Directory 4, envelope 3
  and diagnostic log 2 stay unless an independently demonstrated encoding need requires a bump.
- [x] Verify schema-1 canonical session/projection bytes, archived region, journal hash, boundary
  and integrity checks using frozen schema-1 readers/encoders **before** in-memory upcasting.
  Never hash schema-2 bytes against the source schema-1 boundary. No historical gameplay engine
  or rerun of old commands is required to restore the snapshot.
- [x] Upcast using the frozen wind extension, charge end 18, zero historical wind fields and
  neutral current-month factor without consuming RNG. Preserve all old state/history/assets,
  especially charged inventory, water, commitments, cash, construction, reports and provenance.
  Mark future plans stale for regeneration on the next tick/Planning view; keep elapsed entries.
- [x] Keep old projection/event variants readable, including solar-only curtailment. New history
  can carry typed per-kind records; old chart samples/reports remain semantically identical.
  Schema-aware chart split/join must reconstruct source-schema bytes before source verification.
- [x] Retain read-only preload and existing revision/atomic-save safeguards. A later explicit or
  monthly save writes schema 2; simply loading never rewrites original slots or backups. Preserve
  protected converted originals and metadata-only listing without constructing sessions.
- [x] Test schema-1 charged saves before/after 18:00, corrupted/future schemas, cancelled loads,
  duplicate source boundaries, old region tuning, flat/directory snapshots and diagnostic bases.
  Compare old source bytes and every preserved gameplay field independently of the new hash.

**Focused checks:** portable/save/restore/checkpoint/chart-archive tests in application and
infrastructure; temporary directories per I/O test. This is a prerequisite for Task 4's rules bump.

## Task 4: Wind dispatch, protection, economics and rules 14

**Modify:** `crates/domain/src/lib.rs`, `generation/plant.rs`,
`grid/{dispatch,power_system,summary,protection}.rs`, `events.rs`,
`session/{commands,invariants,report,month_close}.rs`, company/regulation tests,
application event/report projections and `crates/application/tests/rules_version_guard.rs`.

- [x] Start with failing scenarios: wind-only nighttime supply, calm farms, construction/isolation,
  two farms in the same lull, surplus curtailed across Solar/Wind and trip/reconnection/blackout.
- [x] Wire resource availability into generator stepping and initial midnight dispatch. Explicitly
  exclude Wind from thermal commands, commitment, minima, startup counts, AGC targets and thermal
  restart candidates; command errors leave complete session/history state unchanged. Reuse the
  established renewable reconnect policy, never the battery or thermal branch by accident.
- [x] Generalise standing curtailment to the same fraction of available Solar/Wind. Retain actual
  next-tick/release timing, record total and per-kind withheld MW once, preserve historical solar
  events and introduce typed new records. Test rising/falling availability and zero-output sites.
- [x] Update headroom/trip gates and post-protection reconciliation without rewriting physical
  generator outputs or charging/discharging a provisional flow twice. Include Wind in by-kind/
  by-plant generation and invariants; no curtailed MW is delivered energy or revenue.
- [x] Apply ordinary wind construction/O&M/rate-base/book-life/credit rules. Test zero startup/fuel,
  month weighting and wind/battery shared-line capital attribution. Firm credit is regulatory
  only, never free current MW or a wind forecast substitute.
- [x] Advance `SIM_RULES_VERSION` 13 → 14 (A12 and its interaction fixes are already implemented) when behavior is enabled with Task 3 complete. Audit
  expected digest changes from new draws and outcomes, and keep old command-only logs fenced.
  Old portable saves continue through the schema upcaster; do not reject them on rules alone.

**Focused checks:** dispatch/protection/wind/economics domain tests; application rules guard and
selected replay/continuation cases. Review, rather than blanket-accept, affected snapshots.

## Task 5: Shared forecasts and 00:00–18:00 storage control

**Modify:** `crates/domain/src/grid/power_system/{battery,hydro_plan,outage}.rs`,
`generation/battery_schedule.rs`, `session/{views,forecast_contract_tests,battery_tests}.rs`,
`grid/power_system/tests/` and application ramp-warning projections.

- [x] Replace direct solar subtraction with shared Solar + Wind forecasts in all planning,
  boundary, hydro, outage, ramp, restoration and headroom consumers. Retain visible per-kind data.
  Assess every minute for wind/storage fleets, even when there is no enabled battery. Preserve
  reach-by-hour-end dispatchable generation for outage sizing and deterministic earliest ties.
- [x] Write boundary tests before changing predicates: 00:00, pre-sunrise, sunset before 18:00,
  16:59, 17:00, 17:59, 18:00 and 23:59. Both shipped regions charge before sunrise/after an early
  sunset and never at/after 18:00. No thermal charge receipt arrives merely because it was requested.
- [x] Replace daylight charging throughout schedule/live/preview/view/headroom validation with
  the charge-end field. Remove sunset tapering and taper against 18:00 plus free-room limits.
  Keep request timing, thermal ramp-tail reservation and safe handling of renewable surplus.
- [x] Fix overlap mode selection explicitly: actual eligible deficit chooses discharge; actual
  surplus after full customer demand permits charge. A true charge-window predicate cannot mask
  discharge. Choose one shared bus sign and allocate once by PlantId, preventing cross-battery
  cycling, charging while load is shed and storage-fed charge requests.
- [x] Replace any one-fill-then-evening projection with a chronological inventory evaluator that
  can charge/discharge/recharge before 18:00. Rolling valley-fill requests use current free room;
  do not borrow future discharge room. Re-plan after feasible storage charge/discharge direction transitions in the overlapping window
  and at the §25.4 boundaries; the preview must model the same re-plan boundaries, not an
  unconstrained whole-day optimiser. Keep fixed numerical tolerances/iteration counts.
- [x] Planning may project feasible receipts before later delivery. Operating warnings/outages/
  restoration use only currently stored energy and no hypothetical future wind receipts. All
  consumers share the transition limits, while independent tests derive energy and shortages.
- [x] Preserve §23 generator targets at full served customer demand plus feasible charge requests.
  Batteries bridge deficits and never replace already available scheduled hydro/thermal output.
  Hydro uses variable net load but no battery subtraction; charging never requests extra hydro.
- [x] Prove customer priority, charging cancellation and safe 18:00/full-inventory thermal taper.
  A persistent/abrupt wind lull must engage real finite-energy support and ordinary protection;
  no fictitious nameplate generation, sustained charging after cutoff or hidden starter retuning.
- [x] Re-check final 23:59 settlement, disabled/disconnected inventory and midnight report/reset:
  exact once-only close, no 00:00 free refill and charge input = delivered + losses + unused.

**Focused checks:** independent domain forecast contracts, wind/battery/overlap/cutoff properties,
hydro/protection transitions; selected application forecast and ramp-warning tests. Use noise-free
fixtures for exact forecast/live comparison and separate noisy lull tests for protection behavior.

## Task 6: Construction, HUD, forecasts and localisation

**Modify:** `crates/game/src/hud/{action,build,plant_list,plant_panel,planning,planning_cache,
planning_warnings,operating,forecast_chart,supply_chart,supply_history,review,hydro_panel}.rs`,
localisation owners and `assets/locales/en-US/*.ftl`.
**Tests:** inline presenters plus `crates/game/tests/game/{battery,player_workflows,l10n_coverage}.rs`
and a `wind.rs` module registered in the existing test binary.

- [x] Add Wind to every build/kind list, quote/card/icon/colour/legend, selection and report path.
  Show potential and expected output before purchase, and live available/delivered/curtailed
  values afterwards. Omit all thermal/battery controls on a wind card.
- [x] Show wind central/range forecasts with labels that explain their assumptions. Keep shared
  chart/shortfall analysis and §24 cache invalidation; view-only actions do not rebuild analysis,
  while region/session/load/month/command changes refresh it before a Planning panel draws.
- [x] Replace daylight storage copy with **00:00–18:00 charging / 17:00–24:00 gap support /
  midnight reset**. Show correct overlap/full/empty/disabled/disconnected modes and distinguish
  expected receipts from actual stored energy. Keep separate MW/MWh axes and customer-only demand.
- [x] Extend retrospective generation/curtailment reporting and event translation for Wind while
  preserving old solar labels and past reports. Keep battery throughput/losses/unused inventory
  separate from customer revenue and primary energy.
- [x] Use pure presenter → draw → UiAction → dispatch; panel systems read ActionSink getters.
  Add Fluent keys to `l10n/keys.rs` and coverage. Real-widget build/select/connect paths cover
  Enter once-only submission, disabled/busy states and dialog Escape/popup priority as applicable.
- [x] Verify minimum-size/zoom plots, external legends and all controls remain usable. Preserve
  existing scroll/layout/preferences behavior; no new Bevy-linked integration binary.

**Focused checks:** affected presenter tests, actual egui workflow modules and l10n coverage in game.

## Task 7: Wind resource overlay and turbine diorama

**Modify:** `crates/game/src/world/{model,render,overlay,ui,plugins,flow}.rs`, map/build presentation
and existing world test modules after locating the actual geometry/view-sync owners.

- [x] Start with pure model/suitability tests: wind potential legend, buildable versus water/
  occupied cells, selected/unfinished/disconnected model treatment, fitting and picking.
- [x] Add a small recognisable turbine group within the 0.8-cell footprint. Resource overlay
  displays static site quality, not stochastic current weather. Reuse existing connection/source
  routes without representing them as power-flow physics or proving the source of stored energy.
- [x] If rotors animate, derive animation from view data and visual-effects settings only. Do not
  change simulation state/RNG, wake static screens unnecessarily or defeat idle redraw behavior.
- [x] Verify through the real ViewSync/plugin path and launch smoke after plugin/system changes.
  Specifically catch resource borrowing conflicts that appear only on the first windowed frame.

**Focused checks:** relevant headless world/model tests; `cargo run -p game -- --exit-after-frames 120`.

## Task 8: Independent contracts, compatibility and native acceptance

- [x] Add hand-computed fixtures for nighttime wind, shared regional lulls, predictable mean wind
  loss, wind + hydro, mixed renewable curtailment, charging at midnight and 17:00–18:00 mode changes.
  Check physical output/net load and exact shortage minutes independently of shared helpers.
- [x] Property-test bounds/conservation, multiple batteries, mode switches and deterministic ID
  order; vary construction/connectivity, manual/AGC thermal states, commitment tails and water.
  Include zero/noise and sustained-shortage cases; success is not merely replay equality.
- [x] Test new-rule batched versus one-tick runs, different speeds, repeated queries and save/
  resume before/after wind draws, during overlap charging/discharge, at 18:00, blackout and Review.
  Compare full state, RNG, chart/report history and audit boundaries.
- [x] Load all retained schema-1 golden saves, verify source bytes unchanged, continue and save
  schema 2. Round-trip schema-2 wind states through flat/directory/diagnostic formats and monthly
  chart split/join. Re-test malformed/future schemas, cancellation, provenance and atomic writes.
- [x] Run relevant focused crate checks sequentially; build and clippy/fmt as appropriate. Full
  workspace tests run in CI. Obtain the user's express permission before any full local suite;
  inspect push hooks because pre-push invokes it. Do not bypass repository hooks. No commit or
  push is included in this implementation task.
- [x] Native QA in isolated temporary save/log directories: both regions; build/connect a farm
  and battery; observe pre-sunrise charging, daytime variable output, overlap deficit support,
  hard 18:00 stop, evening gap coverage and midnight reset/report; save/resume mid-day and Review.
  Inspect visuals at minimum supported size/zoom and ensure idle screens retain their behavior.
- [x] Update `ARCHITECTURE.md`, `TESTING.md`, spec/plan status and actual verification results
  after implementation. Replace historical daylight assumptions in active docs/help; retain
  clearly marked original Plan 4 decisions as history. Record observed results separately from implementation checkboxes.

## Validation commands and completion gate

Run the smallest relevant filters after each TDD step; examples (separate, sequential commands):

```sh
cargo nextest run -p domain --features test-fixtures -E 'test(wind) | test(battery) | test(commitment)'
cargo nextest run -p application -E 'test(forecast) | test(portable) | test(rules_version)'
cargo nextest run -p infrastructure -E 'test(region) | test(save) | test(portable)'
cargo nextest run -p game -E 'test(wind) | test(battery) | test(player_workflows) | test(l10n)'
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo run -p game -- --exit-after-frames 120
```

Inspect discovered test names; an empty filter does not establish a pass. Broaden to relevant
crate suites when needed, without treating several invocations that collectively run everything
as a workaround for the user's full-suite restriction. CI must keep workspace tests/lint/format
green. Native behavior, saved-game compatibility and independent physics contracts are required
alongside unit tests; mutation testing remains on demand. Completion requires every task above
and recorded results, not just successful compilation or updated outcome snapshots.

## Implementation and verification record — 2026-10-24

Implemented on the existing checkout starting at `84baa04`. A12 had already advanced the rules
through 13, so this change uses **rules 14**, **portable schema 2**, and **region schema 3**.
Directory/flat/log envelope versions remain 4/3/2. The original schema-1 golden files are unchanged;
source-schema canonical verification precedes the in-memory upgrade. Frozen East/West/fallback
wind extension payloads are checked by SHA-256 and never read current shipped tuning.

Wind now participates in construction, dispatch, correlated regional forecasts, hydro/net-load
planning, minute shortage assessment, protection, generation history and per-kind curtailment.
Both authored regions include the resource field and catalog entry. Wind cards omit thermal
controls; the resource overlay, forecast range, review rows and static two-turbine diorama are
available through the existing presenter/ViewSync paths. Rotor animation was optional and is not
added.

Storage charges from midnight until 18:00, with deficit-first discharge during 17:00–18:00 and
finite-inventory recharging when room becomes available. Hourly and command-driven replanning
remain; extra intra-hour replanning is limited to feasible storage flow reversals during overlap.
This avoids repeated full-day projections on harmless noisy bus-sign changes in large fleets.
The reviewed rules-14 snapshot captures the new RNG sequence and revised charging/fuel results;
A12 accreditation, customer priority, starter fleets/cash and generator tuning are preserved.

Final focused checks (all passed; counts are executions per invocation, not disjoint totals):

| Check | Result |
|---|---|
| Domain: wind, battery, forecast contracts, commitment, curtailment, over-frequency | 88 passed |
| Application: rules outcome digest, ramp warning, portable state and chart archive | 10 passed |
| Infrastructure: save snapshots, monthly charts, region catalog and shipped regions | 84 passed |
| Game: map/build/plant/forecast/world/wind/localisation/battery/performance | 152 passed |
| Application/infrastructure: corruption, cancellation, provenance, preload and atomic publication | 15 passed |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo build --workspace` | Passed |
| Native launch with `--exit-after-frames 120` and isolated assets/saves/logs | Exit 0 |

Domain filters used `--features test-fixtures`. The new resume contract compares complete state
and histories at ticks 0, 1, 60, 1019, 1020, 1079, 1080, 1439 and 1440, and next-month Planning.
Adapter tests round-trip Wind/Battery through flat and directory saves at policy boundaries.
Independent arithmetic/property checks cover shared wind draws, bounded output, correlated bands,
proportional curtailment, discharge/recharge, no charging after 18:00 and finite cover in a lull.
Retained coal/CCGT cyclic-gap regressions pass without adding a separate dwell timer.

Native acceptance used a temporary copy of the assets with a commissioned Wind/Battery pair in
each QA starter fleet, plus separate save/log/preferences directories. These additions were made
only to the temporary copy. Both regions completed January at 100% reliability, with Wind in the
merit table and battery losses/unused-at-midnight accounting visible. West saved and resumed at
00:36 with 53 MWh stored and 72 MW of wind; East resumed its January Review unchanged. At the
1280×640 minimum content size, East showed 76 MW of wind and immediate charging at 00:04;
continuing to February showed zero stored energy. At 19:12 the battery held 794 MWh with zero
charging. Exact 17:59/18:00 and overlap reversals are additionally asserted by automated tests.
The compact forecast legend, Wind card without thermal controls, scrollable storage card and
Review remained usable. A further native purchase created Wind farm 75 and the quoted line 77
from Columbus, both visible in Construction; commissioning/connectivity is covered through the
real HUD/dispatch/ViewSync integration test. The QA app was closed after inspection.

The full local test suite, CI and mutation testing were **not run**. Full local suites still need
express user permission; mutation testing remains on demand. No commit or push was made.

### Follow-up after the full-suite report — 2026-10-24

The user's full-suite run exposed 18 failures outside the original focused filters. A focused
reproduction found ten domain assertions, five scenario tests (five snapshots once the early
licence-date assertion was corrected), and three game snapshot/load tests.

Updated the explicit RNG contract to two day-start normals, one demand draw at minute zero and
demand then wind thereafter, including fleets without wind. Replaced seed-sensitive hydro
margins with controlled fixtures and exact cap/served-load/final-hour oracles. The surplus-trip
settling check now integrates the known surplus windows; noisy emergency references were reviewed
and updated to the rules-14 traces while retaining one-episode/no-blackout/no-re-shedding checks.
Reviewed the five scenario diffs with their conservation, economics and regulator assertions still
enabled. The overcommitment trace now asserts August/October/November strikes, no September
strike, and November 2029 revocation.

Game load tests now assert schema 2 plus exact preservation of source-schema gameplay/RNG and
projection bytes, journal, summary, region archive and provenance; disk originals remain unchanged.
No simulation behavior or tuning changed in this follow-up.

Final follow-up validation: 148 domain grid tests, nine application scenarios and three game save
tests pass (160 focused tests). Formatting and workspace clippy pass. The full workspace suite
was not rerun locally; the original full-suite restriction still applies. No commit/push was made.
