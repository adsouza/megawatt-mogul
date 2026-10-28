# Amendment A19: Geothermal plants with reservoir decline

- **Date:** 2026-10-27
- **Status:** Implemented locally on 2026-10-27; focused native/browser validation passed. CI and publication are not claimed.
- **Binding spec:** [Amendment A19, §33](../specs/2026-10-18-megawatt-mogul-design.md#33-amendment-a19-geothermal-plants-with-reservoir-decline-2026-10-27).
- **Inspected baseline:** `c1de9d8`; simulation rules **27**, portable state schema **5**,
  region schema **4**, eight plant kinds. Captured before implementation.
- **Target:** Rules **29**, portable state schema **6**, region schema **5**. A19's original
  rules-24/state-5 reservation has been overtaken by other changes. Initial A19 used rules 28;
  rules 29 narrows dry-site exclusion to geothermal after user feedback. If another change lands
  first, allocate the next unused versions and update the spec and this plan together.
- **Goal:** Build firm, fuel-free geothermal at authored hydrothermal sites; reveal bounded
  exploration risk before major capital spending; model field decline and diminishing makeup
  drilling; explain the consequences in paused Planning/Review and on the plant card.

The user subsequently authorized implementation. Checked tasks below are implemented; the
validation record at the end describes actual evidence and its limits. No commit, push or release
has been performed.

## Contract and implementation decisions

Follow TDD for each behavior: focused failing test, minimal implementation, pass, refactor.
Keep dependency direction, typed domain/application values, deterministic ID order, integer-cent
accounting and the presenter's action funnel. New integration modules for the game belong under
`crates/game/tests/game/` and are registered in the existing `game.rs` binary.

- Append `PlantKind::Geothermal` with portable tag **8**; append the geothermal siting tag without
  changing existing tags. Both are readable only in portable schema **6 or later**. Geothermal is
  dispatchable, neither variable generation nor storage. `PlantSpec` remains immutable per plant.
- Every current region declares geothermal parameters, a catalog entry and a resource field.
  Six regions have an explicitly zero field. Only Western Interconnection and Continental
  Europe/Maghreb initially have sites. No starter assets, cash, tariffs, other plant tuning,
  economic-zone boundaries or electrical-area boundaries change.
- Keep stage/field/makeup/notice state on the plant, explored-dry cells in an ordered session
  collection, and the pre-geothermal availability marker in persisted region/session data.
  A dry cell refuses later **geothermal** orders; other kinds use ordinary siting/occupancy rules.
  Keep the dry result and original project identity when a different plant reuses the cell.
- Quote and order use the same site-specific economic terms. Exploration takes the configured
  share of the effective capital; the remaining capital is queued only after success. Split
  each stage into cent-exact instalments, retaining the final remainder. Initial total lead time
  is 12 played months, including exploration; validate the 24-month cap after zone overrides.
- Add pure plant-level available-capacity, minimum-output and firm-capacity accessors. Use these
  wherever a live plant is assessed; catalog nameplate, fixed O&M, marginal cost, ramp and
  startup terms stay unchanged. Do not mutate the stored spec to simulate decline.
- Preserve old source bytes, hashes, journals and historical projections before upcasting.
  Archived region schemas through **4** receive the same frozen unavailable stub, regardless
  of ID; portable schemas through **5** contain no geothermal state. These are different
  version axes. Old games continue without geothermal even after saving under schema 6.
- No new exploration RNG draw occurs until an exploration actually completes. Make exactly
  one draw per completing plant, in plant-ID order. Preview, UI, forecasts, loading and makeup
  drilling never consume this draw. No-geothermal gameplay preserves its existing RNG stream.
- Makeup capital needs independently dated/depreciated capital components inside a plant's
  rate-base entry. Do not replace that entry, restart its original life, erase old holdings or
  count all original plant capital as new again. Keep aggregate plant/ledger identities stable.
- Protect exploring nodes from connections that could outlive a dry result. Apply the typed
  endpoint refusal to AC line quotes/orders and HVDC terminal quotes/orders, including the
  explicit converter-station path added since A19 was drafted. This extends the same invariant
  to the current grid model; it does not add a new connection feature.
- No EGS, shared reservoirs, reinjection, cancellation of plant orders, carbon price or new
  gameplay randomness. Steam animation, if added, is view-only.

The month-close sequence is explicit: snapshot which plants were already commissioned; book
that month's operating costs and due instalments; progress construction and resolve completing
explorations in ID order before commissioning; decline the previously commissioned fields;
complete makeup after that decline and add its capital; evaluate crossings against the final
field; then run depreciation and the existing regulatory/reporting steps. A restoration in the
same close therefore does not create a transient decline warning. Preserve the existing event
order when none of these geothermal operations occurs, and test December reviews against this
same order. No new field value retroactively changes the operating day just accounted for.

Paths below are relative to the repository root.

## Current owners

| Owner | Work |
|---|---|
| `crates/domain/src/region/{plant_spec,terrain,params,mod,fixtures,economics}.rs` | Kind/siting, resource, parameters, validation, effective terms and fixtures |
| `crates/domain/src/generation/{plant,commitment}.rs` | Plant state, capacity limits and lifecycle; add a small geothermal module |
| `crates/domain/src/session/{commands,month_close,views,invariants,mod,links}.rs` | Orders, queues, dry sites, monthly lifecycle, views, scoped-island copies and wire state |
| `crates/domain/src/grid/{network,power_system,dispatch}.rs`, `grid/power_system/` | Safe node removal; physical, forecast, AGC, protection and island consumers |
| `crates/domain/src/company/{ledger,rate_base}.rs`, `regulation/` | Cash classification, capital components, depreciation, A12 assessment and review preview |
| `crates/domain/src/{state,events,error,lib}.rs` | Schema/rules, typed errors/events and explicit portable tags |
| `crates/application/src/{command,checkpoint,game}.rs`, `pipeline/core.rs`, `projections/` | Command dispatch, snapshots, persistent lessons/history and reports |
| `crates/infrastructure/src/region/` | Strict schema-5 DTO/parser/mapping, rasterization, frozen extension, bundles and outlines |
| `crates/infrastructure/src/{save,command_log}/` | All schema gates, command DTOs, chart splitting/joining, flat/directory/browser restore |
| `crates/game/src/hud/`, `l10n/`, `world/` | Presenters, build/map/card/notices, localization, diorama and selection cleanup |
| `assets/regions/`, `assets/locales/en-US/` | All shipped region declarations, sourced site maps and player-facing text |

## Task 1: Freeze the current compatibility boundary

**Modify:** domain state tests, `crates/domain/tests/portable_state.rs`, application checkpoint/
save tests, infrastructure save/region tests and the relevant support fixtures.

- [x] Before changing positional records, preserve schema-5 canonical reader/encoder behavior
  alongside schemas 1–4. Capture small rules-27/schema-5 fixtures in Planning, mid-Operating
  and Review, including a multi-island game with HVDC and monthly charts. Use isolated temporary
  libraries; never overwrite personal saves or regenerate existing golden fixtures.
- [x] Pin original source bundles, portable session/projection bytes, hashes, journal and RNG.
  Keep source-schema verification before any in-memory additions, including the region stub.
- [x] Add failing version/tag tests for the target schema and rejection of kind tag 8/new siting
  tags at schemas 1–5. Keep all old enum tags and collection order stable.
- [x] Inventory every literal schema gate, `state_record!`, explicit encoder and save format
  envelope. Portable schema evolution does not by itself require changing directory, flat,
  monthly or command-log envelope versions; change an envelope only if its actual framing changes.
- [x] Establish a no-geothermal continuation oracle against the baseline, comparing gameplay,
  money, events, histories and RNG independently of expected new rules/schema headers.

**Gate:** old fixtures verify exactly; new failure cases expose the missing migration/tag support.

## Task 2: Add geothermal data, strict region schema 5 and legacy extension

**Modify:** domain region/generation types and fixtures; infrastructure region
`{dto,catalog,mapping,raster,outline,bundled,legacy}.rs`; relevant region support fixtures.
**Add:** `crates/domain/src/generation/geothermal.rs` and
`crates/infrastructure/src/region/geothermal_unavailable_v1.ron`.

- [x] Add kind/classifiers, `GeothermalSite { min }`, `NoGeothermalSite`, cell potential,
  `GeothermalParams`, typed availability and plant geothermal state. Default in-memory additions
  for old plants must carry no geothermal state; do not create a geothermal starter plant.
- [x] Add linear interpolation for success odds and initial field quality between the siting
  minimum and full potential. Define the full-potential/minimum coincidence safely rather than
  dividing by zero. Validate finite values, positive initial field, bounds and monotonic endpoints.
- [x] Validate positive exploration/makeup duration, exploration shorter than effective build
  time, positive book life, build time at most 24, valid capital share, bounded decline,
  positive ceiling step and worthwhile gain, and nonnegative/representable costs. Reject invalid
  economic-zone overrides and date/quote overflow before allocating IDs or changing queues.
- [x] Add an explicit schema-5 DTO/parse/mapping path with required geothermal fields. Preserve
  schema-4 parsing, including its fast path and optional solar-calibration behavior; update
  region listing, loading, outlines and bundled catalogs consistently. Missing/malformed schema-5
  content is an error, never a request for compatibility defaults.
- [x] Rasterize the new base-plus-blobs field without changing existing terrain/resources,
  zone cells, city/plant coordinates, geographic bounds or source-bundle framing.
- [x] Load the immutable unavailable stub for every pre-geothermal archive, not just named
  shipped regions. Apply the frozen wind extension first for pre-wind sources. The stub has
  zero potential, valid placeholder spec/parameters and an unavailable marker; it never consults
  current assets. Pin its payload/hash in a regression test.
- [x] Add properties for interpolation bounds/monotonicity and data validation. Test zero-resource
  versus legacy-unavailable reasons separately, including unknown region IDs.

**Gate:** new-format fixtures parse strictly; all retained old sources map with unchanged original
bytes and existing gameplay fields. The new kind is not exposed as playable before Task 3.

## Task 3: Implement schema 6 across session, projections and adapters

**Modify:** domain state implementations; application `{checkpoint,game,command}.rs`, projections;
infrastructure command DTOs, save codec/listing/monthly/directory/journal and browser paths.

- [x] Encode geothermal stage, initial/current field, completed rounds, active makeup payments,
  notice state, dry cells, historical project identity and availability marker explicitly.
  Add capital-component wire support alongside Task 6; do not silently discard those records.
- [x] Preserve schema-5 and older canonical writers/readers. Verify old bytes/hashes first, then
  upgrade to empty geothermal collections and unavailable gameplay. Persist the marker in new
  saves so the next load no longer depends on the compiled stub.
- [x] Append event/command/lesson tags with schema gates. Add `DrillMakeupWells` and
  `DismissFieldNotice` through application commands, core handler, command-log DTOs and journals.
  Preserve old command-log replay policy; accepting an old checkpoint does not imply replaying
  a historical command-only log under new rules.
- [x] Update all schema-range checks, chart archive codecs and monthly split/join paths for 6.
  Cover flat and directory saves, diagnostic export/import and browser storage with the existing
  atomic publication, corruption limits, provenance and cancellation protections.
- [x] Reject malformed states: mismatched kind/state, impossible stage/payment counters,
  out-of-range/nonfinite field, duplicate dry identities or dry cells containing cities/geothermal,
  inconsistent makeup capital,
  invalid notice thresholds and unsupported future schemas. Validate without panicking.
- [x] Advance rules to 29 only with the corresponding state/region support in place. Keep the
  schema-5 decoder and baseline fixtures; review rules-digest changes instead of blessing broad
  snapshot updates. Later tasks must add their own checkpoint cases to this boundary.

**Gate:** all old formats still verify; a schema-6 empty-geothermal session round-trips through
each adapter. Feature commits remain compilable and saveable as lifecycle behavior is added.

## Task 4: Build orders, exploration and safe dry-project removal

**Modify:** session commands/month close/invariants, plant construction, network and links;
application projections/history as needed. Add focused domain/session and adapter tests.

- [x] Quote total capital/time/nameplate, exploration months/capital at risk, site odds and
  expected initial MW from shared pure domain terms. Siting rejects water, occupied cells,
  below-minimum potential, legacy-unavailable games and any geothermal-explored-dry cell.
  A dry result must not exclude other plant kinds from that cell.
- [x] Queue only the exploration share initially. At the close ending exploration, book the
  final exploration instalment, progress construction, then resolve the single RNG draw in
  ascending plant-ID order before commissioning. On success queue remaining capital over
  remaining build months, beginning payments next close: no extra payment or delay month.
- [x] On dry, emit `ExplorationDry` with exact capital lost, retain immutable historical identity,
  remove unpaid queues, live plant and grid node, mark the cell dry and recompute affected grid
  views. Never reuse IDs or delete ledger/history records. Report/history translation must work
  after the plant no longer exists; no presenter may assume every historical plant is live.
- [x] Refuse AC/HVDC connections to either exploring endpoint in pure quotes and actual orders.
  On success those endpoints become eligible while construction continues. Assert dry removal
  cannot leave a line, converter station, link, rate-base asset or dangling selection/reference.
- [x] Successful commissioning adds full successful project capital once and uses the existing
  all-day commitment/AGC default and notification for dispatchable plants. Dry exploration
  spending stays cash construction loss: no depreciation, rate base or recoverable operating
  expense in revenue requirement, including the annual review preview.
- [x] Cover both outcomes, simultaneous completions, boundary months/December, insufficient cash
  without a new order gate, and all rejection paths. Compare whole state/IDs/RNG on `Err`.

**Gate:** three exploration closes plus nine construction closes commission initial tuning at
close 12; dry ends at close 3 with exactly the exploration spending lost and no orphan assets.

## Task 5: Monthly decline and every live-capacity consumer

**Modify:** plant/commitment, power-system dispatch/AGC/protection/islands/planners, session views,
rate-review physical capacity and application ramp/forecast projections.

- [x] Apply decline once per month close to geothermal commissioned before that close, including
  offline/disconnected plants. Newly commissioned plants retain their initial field that close.
  Use multiplication by `(1 - monthly_rate)`, with no simulation `powf`, `exp` or `ln`; retain a
  strictly positive field for valid long continuations and test numerical boundary behavior.
- [x] Make live available capacity `nameplate × field`, minimum stable output its configured
  fraction, and firm capacity credit times available MW. Keep nameplate/capital/O&M/ramp/startup
  unchanged. Refresh live regulatory inputs before preview and actual review.
- [x] Audit every direct `spec.capacity`, `spec.min_output()` and `spec.firm_capacity()` use for
  live plants: manual target validation, initial target, recurring commitment and off-gaps,
  starting/stopping, AGC headroom, outage/protection, midnight initialization, supply/minimum/
  ramp warnings, hydro/storage residual forecasts, island activation/support and HVDC terminals.
  Leave catalog-only nameplate calculations alone. Revalidate/clamp inherited targets at the
  month boundary so a previously valid target cannot exceed newly available capacity.
- [x] Add independent arithmetic/physical tests with controlled noise, not only comparisons
  between consumers of one helper. Cover manual and AGC plants, starts/stops, mixed fleets,
  disconnected/construction plants and island scopes. Update scoped-session copies for new state.
- [x] Property-test decline bounds/monotonicity and exact capacity/minimum/firm scaling; show
  unchanged behavior for every pre-existing kind and unchanged no-geothermal RNG.

**Gate:** actual output and forecasts observe the same resource limit, with independent expected
MW; declining capacity cannot become hidden dispatch headroom or inflated regulatory capacity.

## Task 6: Makeup orders, capital components and A12 treatment

**Modify:** geothermal state/commands/month close, company rate base, review preview and projections.

- [x] Add a pure makeup quote shared by UI and command validation: next round, fixed round capital
  from regional data/effective construction economics, duration, current/ceiling MW and gain.
  Define and test the economic-zone rule explicitly: use the same local capital multiplier as
  the installed plant's construction terms, without inventing another zone or tuning constant.
- [x] Restore round `k` to `initial_field × (1 - k × ceiling_step)`, never below current field.
  Refuse unfinished/non-geothermal plants, wrong phase, active drilling, exhausted/nonpositive
  ceiling or gain below the configured minimum. Exactly-minimum gain is accepted. No cash gate,
  cancellation or exploration draw is introduced. Validate everything before mutation.
- [x] Keep one project per plant with cent-exact monthly instalments independent of original
  construction queues. The plant runs and declines during the project. At completion, first
  apply that close's ordinary decline, then restore to the round ceiling and emit actual MW
  restored. Do not decline the restored field a second time in that close.
- [x] Keep the original rate-base entry and add an internal capital component per completed
  round, with completion date, own money/depreciation/held history and remaining original plant
  book life. Aggregate values remain under `AssetRef::Plant`; retain deterministic component
  ordering. Existing non-geothermal entries stay on their unchanged arithmetic path.
- [x] Recover each component straight-line to the plant's original book-life end, including
  exact final cents and existing completion-month depreciation convention. Handle a plant at
  or beyond its book-life end explicitly: immediate depreciation of the addition, no division
  by zero and no extension of original capital's life.
- [x] For A12, assess the new round's capital in its completion year against accredited restored
  MW, not whole nameplate. Keep original/new/held components distinct when refreshing physical
  firm capacity: attribution across components must not count the restored MW twice. Record the
  restored increment and subsequent decline for the component; allocate the remaining current
  firm MW to the original component. Later rounds must preserve the sum at current plant firm MW.
- [x] Preserve A12 hold/release/second-review write-off, cent-exact aggregate conservation and
  existing associated-line treatment; a retrofit alone must not reclassify all old line capital
  as new. Preview and actual review use the same assessment. Keep total physical firm MW separate
  from capital eligibility so a write-off cannot remove operating capacity.
- [x] Test two rounds on an old plant with original capital already depreciated/held, multiple
  completions in one year, December completion, partial excess, release/write-off, end of life,
  save/resume mid-round and no effect on non-geothermal A12 cases. Add properties for money
  conservation and decreasing positive ceilings; never replace checks with snapshot-only oracles.

**Gate:** one successful round restores the expected MW and adds only its own capital as new;
old capital/life/holdings survive and annual assessment never double-counts physical firm MW.

## Task 7: Durable notices, lessons and removed-project history

**Modify:** geothermal/session state and typed events; application lessons, timeline, plant history,
monthly reports and rate-review projections; persistence implementations from Task 3.

- [x] Track crossings below 90%, 80%, 70% and subsequent deciles of **commissioned** output,
  with strict boundary tests. Do not compare with nameplate. Re-arm a threshold only after
  restoration above it; a crossing fires once while remaining below. Keep numeric thresholds
  and acknowledged/first-lesson state portable.
- [x] Add `ExplorationSucceeded`, `ExplorationDry`, `FieldDeclineThreshold`,
  `MakeupDrillingOrdered` and `MakeupDrillingCompleted` as typed events with sufficient data for
  history after project removal. Persist first-decline teaching state per plant.
- [x] Crossing creates a persistent Planning warning; ordering makeup acknowledges the current
  warning, restoration updates it, and `DismissFieldNotice` records Not now in the command
  journal. Dismissal hides that warning until the next crossing, across month changes and reload.
  Dismissal without an active warning is refused atomically.
- [x] Integrate success/dry outcomes and first/later decline lessons into the existing priority
  order and three-lessons limit. Test collisions with blackout/reliability/regulatory lessons;
  capacity and loss information must still be accessible in history/card if a lesson is displaced.
- [x] Preserve dry-project names/kind/identity in reports, ledger/event translation, timeline and
  plant history. Record losses once, not both as construction cash and another cash charge.
- [x] Add mixed-threshold, makeup/re-arm, dismissed/reload and several-plants tests. Notices may
  surface at 32× but must not depend on transient toast timing or UI queries for persistence.

**Gate:** no silent decline, monthly nagging or resurrected dismissed notice; dry outcomes remain
understandable after their live plant has been removed.

## Task 8: Shipped resource maps, tuning and lifetime balance test

**Modify:** all eight top-level region RON files, fixture data, relevant region-authoring support,
`crates/infrastructure/tests/region_catalog.rs` and `shipped_regions.rs`.
**Add:** `crates/infrastructure/tests/geothermal_balance.rs` for the adapter-level economic oracle.

- [x] Before final tuning, verify the cited exploration-success and annual-decline figures
  against primary publications. Record precise source links and units in the data/test notes;
  distinguish physical evidence from compressed gameplay economics. Resolve monthly decline
  offline and store its literal value in RON/fixtures, never compute transcendental rates in ticks.
- [x] Author sourced blobs for all §33.5 named Western fields (including Baja California) and
  Tuscany/western Turkey. Test representative cells, resource bounds and absence elsewhere.
  Each blob comment names the field and source. Keep Italy and Turkey in existing zone 0.
- [x] Add the §33.5 initial plant/parameter values to every region, explicit all-zero resource
  maps in the other six, then retune geothermal values only as the balance oracle requires.
  All final tuning belongs in RON and the test fixture, never hardcoded simulation constants.
- [x] Build a deterministic economic oracle reading shipped data separately for Western
  Interconnection, Italy and Turkey. Select country candidates from documented geographic masks,
  same-area reachable land sites near a city; select the highest potential, tie-break by cell
  then city ID. Document and pin the modest distance cutoff and real line quote treatment in
  the test before tuning; fail if any country has no eligible good site.
- [x] Compare geothermal with CCGT, coal and nuclear at 90% baseload duty and the same city/line
  distance. Include regional base-return capital recovery, depreciation, fixed O&M, marginal
  cost, actual construction timing, lifetime declining output and worthwhile makeup at notices.
  Normalize expected exploration loss per successful plant as
  `((1 - odds) / odds) × exploration_capital`; avoid an extra or missing probability factor.
- [x] Construct the marginal comparison at the siting minimum with the same connection distance
  and local terms, even if the raster lacks an exact-minimum cell. Assert good-site geothermal
  beats all three comparators and marginal geothermal loses to CCGT. Report site, odds, quality,
  lifetime costs/energy, makeup rounds and all margins, so retuning cannot conceal a fragile pass.
- [x] Independently check a short hand-calculated economic case and cash/energy units. Do not
  use regulatory allowed return as company energy revenue or recover failed drilling through
  customer operating expense. Re-run relevant starter service cases to prove unchanged fleets.

**Gate:** all three geographic comparisons pass both sides of §33.6, every named field is sourced,
and the six no-resource regions expose a valid disabled geothermal option.

## Task 9: Build/map/plant/notices HUD and localization

**Modify:** game HUD `{action,build,construction,map,plant_panel,plant_list,planning_warnings,
planning_cache,review,toasts,supply_chart,supply_history,chart_style,selection}.rs`, associated
presenters, localization keys/names/translations and `assets/locales/en-US/*.ftl`.

- [x] Add the ninth build row, differentiating no-resource regions from pre-geothermal games.
  Show total/at-risk capital, staged lead times, odds and site output; exploration construction
  cards show remaining stage time. After success show normal remaining construction.
- [x] Add geothermal resource overlay/legend, hover/selected-site values and distinct dry markers
  with tooltips. Do not silently reuse hydro potential or color alone to convey dry sites.
- [x] Add field percentage relative to commissioned output and approximate annual decline,
  completed rounds, progress and the Planning makeup quote with restored from/to MW. Show clear
  disabled reasons outside Planning, during a project and below worthwhile gain. Operating
  shows the same field information read-only with the existing thermal controls.
- [x] Add persistent Planning warning actions Show plant and Not now, wired through `UiAction`
  and real `dispatch`. Selection opens the correct plant card; dry-result removal clears stale
  live selection while preserving the historical outcome. Refresh cached warnings/quotes after
  orders, dismissal, phase changes and restoration.
- [x] Integrate events/toasts/lessons, supply stack and chart colors, reports, kind filters,
  histories, plant lists and accessible names. Add every literal Fluent key to `l10n/keys.rs`;
  translations of removed projects must not dereference a missing live plant.
- [x] Follow presenter → draw → action → dispatch, use ActionSink getters for resources it writes,
  and cover each new action through the real dispatcher. If adding a form/dialog, cover same-pass
  current-text Enter, enabled/busy gates, exactly-once submission and active-dialog Escape.
- [x] Use actual widget/egui-pass tests at minimum supported window size and enlarged zoom for
  nine build rows, the widest makeup quote and expanded warnings. Verify scrolling, clickable
  controls, contrast and no overlaps, along with read-only Operating and persistent Review.

**Gate:** a player can discover, order, inspect, connect after success, drill and dismiss through
real controls; all reasons remain visible and usable at the minimum size.

## Task 10: Geothermal diorama and map/world synchronization

**Modify:** game world `{model,render,overlay,picking,plugins}.rs` and relevant ViewSync owners.

- [x] Add a recognizable cooling-tower/steam/well-pad model within the existing plant footprint.
  Preserve construction, selection, picking and disconnected treatment. Dry removal removes
  the plant/node representation and reveals the dry marker without stale entities.
- [x] Keep resource overlay and suitability consistent with domain siting and legacy availability.
  Optional steam is purely visual, respects visual-effects/idle redraw settings and consumes
  no simulation RNG or wall-clock input to domain state.
- [x] Test geometry/footprint and actual plugin/ViewSync create-update-remove paths, selection,
  dry markers and overlays. Run isolated native launch smoke after plugin/system changes to
  catch first-frame Bevy resource conflicts that headless presenter tests cannot establish.

**Gate:** successful and dry projects render/pick correctly with no stale scene objects or
simulation changes caused by animation.

## Task 11: Full feature acceptance and documentation

- [x] Test deterministic outcomes across tick batching, supported speeds and repeated queries,
  with multiple completing explorations in ID order. Compare state/RNG/events/money/history,
  including save/resume immediately before/after outcome, mid-build, mid-makeup, after dry,
  just before thresholds, after dismissal and after re-arming.
- [x] Restore schema-4 golden saves in every phase and retained schemas 1–3, plus Task 1's
  rules-27/schema-5 fixtures. Verify original bytes/hashes before upgrade, continue under new
  rules and save schema 6. A restored Western game must refuse geothermal everywhere, display
  the legacy reason and retain the unavailable marker after another save/load.
- [x] Exercise the real command/application/UI flow: successful and dry builds, blocked premature
  connection, post-success connection and commissioning, manual/AGC operation, makeup payment/
  completion, loss history, Planning dismissal and Review lessons. Include hostile commands
  and property-based invariants, not just happy-path replay or regenerated digests.
- [x] Run focused checks sequentially, workspace clippy and formatting; use CI for full workspace
  tests. Obtain express advance permission for any full local suite, including one invoked by
  pre-push hooks. Inspect hooks before any later commit/push; do not bypass them. Mutation
  testing remains on demand. Do not start several filtered commands that collectively evade
  the full-suite restriction.
- [x] Native acceptance uses isolated assets/save/log directories: Western at 32×, a dry result,
  a successful project through commissioning, two decline notices and one makeup round. Confirm
  paused-phase visibility, minimum-window/zoom fit and save/resume of an active project. If a
  temporary accelerated QA fixture is needed, label it and separately verify shipped decline;
  never modify personal saves or ship the accelerated fixture as region tuning.
- [x] Run the browser build/smoke locally and preserve the existing CI checks; compare a
  native/browser geothermal continuation at exploration and decline boundaries. Record actual
  platform evidence; do not claim unsupported cross-platform float identity from a native pass.
- [x] Update spec §3.2's plant table, capacity credits/book lives and siting description, plus
  any active seven/eight-kind wording. Update `ARCHITECTURE.md`, `TESTING.md`, A19 status and
  this plan with the chosen month-close order, wire versions, capital-component treatment,
  source/tuning notes and actual results. Keep historical amendment versions clearly historical.

## Focused validation commands and completion gate

Discover actual test names as tasks add them; an empty filter does not establish a pass. Run
commands separately and sequentially, choosing the smallest relevant set after each TDD step:

```sh
cargo nextest run -p domain --features test-fixtures -E 'test(geothermal) | test(makeup) | test(rate_base)'
cargo nextest run -p application -E 'test(geothermal) | test(save_resume) | test(rules_version)'
cargo nextest run -p infrastructure -E 'binary(geothermal_balance) | test(geothermal) | test(portable)'
cargo nextest run -p game -E 'test(geothermal) | test(l10n) | test(player_workflows)'
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
```

Run the existing isolated launch-smoke procedure with `--exit-after-frames 120`; confirm its
assets, saves and logs resolve into temporary directories before opening the window. Broaden
focused filters for region parsing, A12, dispatch/forecast, save codecs/charts and shipped starter
scenarios when their owners change. Preserve CI's full workspace, development-tools and browser
checks. The completed local checks are recorded below; the full local suite remains unrun.

Completion requires every task above, §33.9 acceptance and recorded results. Fill in counts,
commands/filters, compatibility fixtures, all three balance margins, native/browser evidence and
any remaining limitations after implementation. Passing compilation alone, unchanged snapshots
or a single happy-path playthrough does not complete A19.


## Implementation and validation record — 2026-10-27

Initial implementation: rules **28**, portable state **6** (read **1–6**), region schema **5**.
The dry-site correction below advances rules to **29**. Save envelopes keep
existing versions. The frozen unavailable region extension and original rules-27 fixtures retain
old source bytes and no-geothermal continuations; the rules digest changed only its version header.
Capture provenance and SHA-256 values live beside the fixtures in `crates/infrastructure/tests/support/pre-geothermal*/README.md`.
The one-time capture example refuses current rules and must run against the immutable old tree.

The implementation follows the month-close order above. The live plant accessors feed dispatch,
AGC, forecasts, outages, reserve assessment and UI controls. Makeup adds dated components to the
original plant identity, with independent holds/write-offs and its remaining original life. At
end of book life a component is recovered in one close without division by zero. Dry spending
remains construction cash loss and never enters rate base or recoverable operating expense.

Final geothermal-only tuning is **$150M** capital, **$300k/month** fixed O&M, **$5/MWh** variable
O&M, **150 MW**, 12 months including 3 exploration months, 12% at risk, 45–80% odds,
15–100% initial quality, 3% annual decline, $15M/3-month makeup, 5-point ceiling steps and
3-point minimum gain. The original $225M/$750k draft failed the required lifetime comparisons;
other technologies, starter fleets and starting cash are unchanged. Field/odds/decline choices
are game tuning informed by the sources in §33.5, not claims of uniform real-world performance.
The shipped Gaussian fields are sourced per blob; the other six regions explicitly have zero.

The independent economic oracle uses an eight-cell city cutoff, documented country masks,
staged construction financing, calendar-month energy, fixed/variable O&M, depreciation/return,
expected failed exploration per successful project, connection capital and worthwhile makeup.
It computes decline and notice crossings independently of production helpers. A hand-calculated
staged-finance/energy case checks cents, MWh and timing. Final levelized costs ($/MWh):

| Geography | Good geothermal | Marginal geothermal | CCGT | Coal | Nuclear | Good margin vs gas/coal/nuclear |
|---|---:|---:|---:|---:|---:|---|
| Western | 39.9873 | 219.8434 | 55.6009 | 47.6031 | 46.1655 | 15.6137 / 7.6158 / 6.1782 |
| Italy | 27.4344 | 137.6483 | 97.8780 | 41.5066 | 50.3744 | 70.4437 / 14.0722 / 22.9401 |
| Turkey | 26.5409 | 131.7976 | 97.7021 | 41.3756 | 50.3030 | 71.1612 / 14.8347 / 23.7621 |

Each good field has potential 1, odds 0.8 and initial quality 1; each marginal comparison uses
potential 0.3, odds 0.45 and quality 0.15. Good cases generate 28,661,437 MWh over the plant
life with five worthwhile makeup rounds. Selected site/city cells are Western (24,67)/(18,64),
Italy (68,54)/(72,59), Turkey (111,70)/(109,71).

Local checks, run sequentially by owner:

- Focused domain geothermal/makeup (**16 passed**), interpolation/decline properties, hostile snapshots, exact
  live capacity and A12 capital-component tests pass. Properties found a subnormal-field floor
  bug; its regression seed is retained. Two-round and end-of-life cases pass.
- Application geothermal batching/query determinism and journal/mid-makeup restore: **2 passed**.
- Infrastructure geothermal region/wire/golden continuation cases: **6 passed**. Original schema-4
  and schema-5 Planning/Operating/Review payloads verify before upgrade, including multi-area HVDC.
  Focused existing portable-state, save, chart, dispatch/forecast and rate-base regressions passed.
- Economic oracle and hand-calculated units: **4 passed**. All eight shipped-region hands-off
  year-one scenarios (each covers seeds 0–7): **8 passed**.
- Focused game geothermal, localization, build/map, commitment/plant presenters, world geometry
  and Bevy parameter checks: **141 passed**. Real egui controls exercise Show/Not now/Drill,
  32× fixed-step operation, journal restore and the selected card at 1024×640 and 1.25× zoom.
- `cargo fmt --all --check`, `git diff --check`, workspace clippy with `-D warnings` and
  `cargo build --workspace` pass.
- Isolated native `--exit-after-frames 120` launch exits **0**. A temporary app bundle made
  the standalone executable available to native UI automation. The accelerated Western save was
  loaded and played at **32×** through eight months: wet/dry lessons, removed dry marker,
  successful commissioning, a 90% decline warning, real makeup order, three payments while
  operating, restoration to 95%, and the same plant's later 90% warning/re-arm and dismissal.
  Production decline is separately covered by domain and balance tests. The QA region uses
  4% monthly decline and 1+3 construction months only in temporary assets.
- `geothermal_acceptance` restores every monthly boundary through split-chart archives and
  native directory saves, including active makeup and dry history. The browser probe also
  restores each target-local checkpoint.
- Rust Trunk **0.22.1** build with the installed Rust **1.98.1** WASM toolchain passes.
  Focused Chromium Playwright `geothermal.spec.js`: **1 passed**; all eight native/WASM
  exploration/field/round/payment/notice traces match exactly. This establishes geothermal
  state agreement for that fixture, not general cross-platform floating-point physics identity.

The full local workspace suite and mutation testing were not run, respecting the user's standing
preference. CI/full browser-suite results are pending a later commit/push; existing CI remains
retained; the browser job now also builds the geothermal native probe before Playwright.
No personal save directories were used. Native/browser acceptance tooling writes
only caller-specified fresh temporary directories.


### Dry-site reuse correction — rules 29

The user clarified that a dry geothermal field does not make land unsuitable for other plant
kinds. The former all-plant restriction is superseded. Only geothermal receives `ExploredDry`;
other kinds retain their normal land, resource and occupancy checks. Snapshot validation allows
an ordinary plant to share a cell with its historical geothermal dry result while still rejecting
cities/geothermal on that cell and reuse of the failed project's plant/node IDs. No portable or
region schema changes are needed. Tooltip and lesson text now state the geothermal-only scope.

The regression covers every kind's siting result, a peaker build on the dry cell, stable dry-well
identity/loss, unique replacement IDs, occupancy after reuse and deterministic save/resume.
The infrastructure pipeline also covers flat and split-chart archive saves and continued operation
after reuse. Rules advance from 28 to 29 because the same non-geothermal order now succeeds;
the existing rules digest changes only its version header.


Correction validation: **16** focused domain geothermal tests, **6** infrastructure geothermal
cases (including cell reuse through flat/archive saves), the rules-version digest, and **62**
focused game geothermal/build/localization checks pass. The digest's simulation outcomes are
unchanged; only the rules header advances. Workspace clippy, formatting and build also pass.
Full local tests and browser acceptance were not rerun for this correction.


### Reported regression follow-up — 2026-10-27

The user's full run found ten failures outside the earlier focused validation. All ten were
reproduced. The geothermal catalog now cites `P7-GEO-001`, an explicitly labelled internal
A19 balance override in the regional-economics note, in every region; the existing provenance
checker remains strict. No tuning values changed.

Two catalog-validation tests now remove Battery explicitly instead of assuming it is the last
entry. The firm-capacity property shortens only ordinary construction, leaving geothermal's
exploration/build duration valid; the provided regression seed is retained. Inspector and Review
expectations include geothermal potential and the expanded fuel/variable-O&M label.

The three short-panel chart failures exposed a real layout regression from the ninth legend
entry. Compact keys keep their font, labels and hit targets while reducing padding. In short
supply panels the hour caption shares the key, leaving more room for the curve and both battery
layouts; normal hourly tick labels remain. The existing minimum-curve-height, key bounds,
keyboard controls and inventory visibility assertions remain, with a new check that the relocated
hour caption stays visible outside the curve.

All ten originally failing tests now pass in focused runs. Nine shipped-region provenance/ramp/
pacing checks and 37 supply/forecast/chart-style checks pass. The full local suite was not rerun.
