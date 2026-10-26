# Plan 6a: Optional Construction Economic Zones (Milestone 2)

**Status:** Implemented 2026-10-25 under rules 17, following user authorization. Region schema
remains 4 and portable state schema remains 3. Tasks 1–6 and 8 are complete. Task 6 was done by
[Plan 7](2026-10-25-plan-7-single-area-regions.md) (Tasks 1, 2, 8 and 9, shipped under rules 18):
Europe–Maghreb carries sourced country profiles. Task 7 remains the mandatory Plan 8 HVDC
handoff; HVDC has not shipped.

**Goal:** Let a region optionally author country or subnational plant and transmission
construction costs and build times. Europe–Maghreb will use this to distinguish construction
markets within one synchronous grid; IPS/UPS and other regions may opt in. Omission is an exact
identity operation. Technical plant characteristics and operating economics remain regional.

**Authority:** [Design spec §28.7.1](../specs/2026-10-18-megawatt-mogul-design.md#2871-optional-construction-economic-zones-specified-2026-10-25-plan-6a).
The spec's currency, route arithmetic, default behavior and compatibility contracts are binding.

**Sequence:** Completed Plan 6 / rules 16 → **Plan 6a** → Plan 7's five single-area regions →
Plan 8's multi-area/HVDC work. Plan 6a bumps rules once (16→17),
keeps region schema 4 and portable state schema 3. Plans 7/8 bump from their actual predecessors;
Plan 8 retains its own planned state-schema change. Task 6 is now scheduled in
[Plan 7](2026-10-25-plan-7-single-area-regions.md) (its Tasks 1, 2 and 9). Plan 8's detailed plan
does not exist yet; Task 7 below is a mandatory handoff for when it is written. Do not re-execute or reopen Plan 6's completed checklist.

## Architecture and constraints

- Domain owns `EconomicZoneId`, validated profiles/map and pure construction-term resolution.
  No new dependency, I/O, user-facing strings, RNG draws or mutable/global price service.
- Infrastructure owns strict optional schema-4 DTOs and row-major mapping. Schemas 1–3 reject
  these new fields; schema 4 without the block preserves all old behavior and source bytes.
- Game presenters show the same terms an order uses. No UI-side multiplication, independent
  duration calculation, or country lookup from a city's civil time/climate/electrical area.
- Profiles override only plant `capital_cost` / `build_months` and AC `cost_per_cell` /
  `cells_per_month`. Missing members inherit the corresponding baseline. ID 0 is the implicit
  regional default; nonzero IDs and translation keys are authored. No wage, GDP or blanket
  currency-conversion multiplier is part of the simulation.
- `cells` has exactly width×height entries, including default-0 water/non-playable cells. Zone
  assignment is explicit region data, independent of country-border rendering or sovereignty.
- All amounts are final region-currency cents after the existing input conversion. Checked
  wide-integer capital/work calculations return typed errors before mutation. Equal monthly
  instalments sum to exactly the quote; build months are positive whole game months.
- Preserve command shape: `BuildPlant { kind, cell }` / `BuildLine { from, to }`. The source and
  locations determine economic terms. Do not add quoted prices supplied by an untrusted client.
- Keep one game integration binary under `crates/game/tests/game.rs`; I/O tests own a TempDir.
  Use property tests for identity, route reversal, map validation and instalment conservation.
- All tuning stays in RON / test-fixtures. Examples below are test fixtures, not economic facts.
  Local source quotes and offline normalization/pacing decisions must be documented before new
  regional data is accepted; do not invent an Indian or Maghreb cost discount from wages alone.
- Implementation follows TDD, then workspace nextest, clippy `-D warnings`, fmt and appropriate
  native checks before its scoped commits. Respect current user permission for broad checks;
  never bypass hooks. See the implementation record below for completed validation.

## Preflight: immutable construction baseline

- [x] Before editing implementation paths, capture a rules-16/schema-4 save with an in-flight
  plant and line. Record source bytes, schema/hash/canonical bytes, instalments, commission dates
  and eventual original rate-base values. Keep the existing released/NA archive fixtures too.
  This is a pre-change oracle, never a fixture regenerated after the new resolver is written.
  Captured at `2009acc` in `crates/infrastructure/tests/support/economic-baseline/`.

## Task 1: Domain economic profiles and deterministic construction terms

**Files:** add `crates/domain/src/region/economics.rs` and focused/property tests; update
`region/mod.rs`, `region/fixtures.rs`, `error.rs` and existing IDs style as needed.

**Interfaces:** distinct `EconomicZoneId`; `EconomicZone` (name key, per-kind partial construction
overrides, partial AC terms); an immutable per-cell assignment; pure cell/route resolvers.
Keep `region.spec(kind)` as the regional baseline for technical/operating fields. Add a fallible
cell-aware effective-spec resolver and a route-terms resolver independent of network eligibility.
They must be usable for already-owned assets as well as a prospective order. Return typed invalid
cell/zone/overflow errors; never silently choose the base profile for invalid input.

- [x] RED: omitted map, explicit all-0 map and differently named but identical profiles produce
  exactly the old plant and line terms; two cells can change capital and duration independently.
- [x] RED: map/profile validation covers unknown/duplicate/reserved IDs, length, non-playable
  assignments, duplicate kinds, bad amounts/durations and unsupported override fields.
- [x] Implement optional profile resolution; no operation may overwrite capacity, siting,
  efficiency, startup, O&M, marginal cost, life or other non-construction spec fields.
- [x] Implement the spec's AC work-unit sampling: existing billable N, canonical endpoint-cell
  order, exact rational midpoint indices and declared boundary tie rule. Do not use the visual
  water supercover as a billable-cell count or add per-country rounding/fees.
- [x] Capital is the checked cents sum. Validate the LCM of all relevant AC rates fits u64;
  accumulate exact scaled work in u128 and ceil once to a positive u32 month count. Reject an
  unrepresentable rate set at load/validation; reject a quote whose money or completion date
  cannot be represented before any state change. No float work sum followed by ceil.
- [x] GREEN/properties: reverse endpoints, repeated zone crossings, exact boundary hits, water,
  diagonals, odd-cent capital, no extra RNG use, uniform terms across distinct IDs, and overflow.
  Pin a hand-computed case where rounding work separately per zone gives the wrong duration.

## Task 2: Strict optional schema-4 authoring

**Files:** `crates/infrastructure/src/region/{dto,mapping,raster,catalog}.rs`,
`crates/infrastructure/tests/region_catalog.rs`, isolated support fixture; no shipped-region
retuning in this task.

**Wire contract:** optional top-level `economic_zones: (zones: [...], cells: [...])`. Each zone
has `id`, `name_key`, optional `plants` entries keyed by existing `PlantKind`, and optional
`ac_lines`. Override members are optional values with documented RON syntax; omitted members
inherit. Unknown fields remain errors. Do not accept reserved future `hvdc` overrides before
Plan 8 implements its typed contract. Existing anonymous and explicit public RON struct names
must keep loading; do not impose a private schema-probe/helper name on source files.

- [x] RED: legacy schemas reject the extension; old v4 absence succeeds unchanged; explicit
  default and a two-country fixture map to the expected typed domain profiles and cells.
- [x] RED: every invalid map/profile case produces a typed field/ID diagnostic, including a
  malformed present block and an overflowing work-rate LCM. No partial silent defaults.
- [x] Add strict DTOs/mapping, preserving the discriminator-before-versioned-DTO boundary and
  Gaussian/cells irradiance support. Validate starter asset construction valuations before a
  `GameSession` can be constructed, including any unrepresentable startup rate-base value.
- [x] GREEN: public list/load, outline and archived-source loading take the same validated
  interpretation; wrong names/unknown fields still fail. Round-trip supported named RON forms.
- [x] Check no new domain dependency or serialization derives leak through the boundary.

## Task 3: Quotes, ordering, accounting and frozen construction

**Files:** `crates/domain/src/session/{commands,mod,month_close,views}.rs`,
`generation/plant.rs`, `company` consumers if necessary; their existing test modules.

**Interfaces:** add a pure `GameSession::plant_quote(kind, cell)` analogous to `line_quote`, with
capital, months and economic-zone identity for the UI. Queries do not require Planning or mutate
state; orders still require Planning and all existing siting/network/need checks. The effective
spec and pure region route resolver are the shared source for terms, not this eligibility quote
when valuing an already-owned asset (which is necessarily occupied/already connected).

- [x] RED: two same-kind plant orders in different zones charge their displayed totals, use
  different months, and commission in exactly their quoted month. Cover independent overrides,
  inherited members and cancellation's existing paid/future-instalment behavior.
- [x] RED: a cross-zone line quote/order/display/cancellation/commissioning has the same total
  and duration; reversing endpoint selection cannot buy the same line cheaper or faster.
- [x] Orders resolve before IDs or session mutation. `build_plant` stores the effective spec
  in the plant's existing state; `build_line` queues the route quote's instalments/months.
- [x] Audit **all** value consumers: starter plant/line original rate base, commissioning,
  `line_build_months`, progress and estimated completion, monthly instalments, capital-held and
  depreciation. In particular, month-close `line_cost(region.lines(), length)` and base-catalog
  construction-card lookups are no longer correct for opt-in regions. Resolve stored line
  geometry against the immutable archived map; use stored plant specs for owned plants.
- [x] Preserve technical/operating fields, line need/siting rules and water-crossing ability.
  No economic-zone boundary creates an electrical area or a transfer limit.
- [x] GREEN/properties: sum of instalments equals quoted cents, one/two/many month completion,
  starters are immediately in service with correctly resolved original value, no mutation on
  invalid/overflow quotes/orders, and unused capital handling still obeys A12.

## Task 4: Save compatibility and replay before UI/data rollout

**Files:** `session/mod.rs` snapshot validation, `application/src/{game,checkpoint}.rs` as
required by existing gates, infrastructure save tests, `tests/game/save_snapshots.rs`.

- [x] RED: save a new two-zone game mid-construction with a plant's effective spec different
  from its base catalog; restore must not reject it as a forged spec. Include a cross-zone line.
- [x] Compare stored plant specs with the cell-aware resolver in `validate_snapshot`; do not
  replace the equality check with accepting arbitrary stored prices or durations.
- [x] Keep state schema 3 and its field layout: effective `PlantSpec` fields, line geometry,
  status and remaining instalments already hold the required history. No persisted zone-ID field
  is necessary. Do not add one and then omit it from hashes. Plan 8 handles its separate changes.
- [x] Verify source-schema hash/canonical bytes before any existing upcast. Omitted/default
  economic data must preserve archived schemas 1–4 regions and supported state schemas exactly.
  Keep the origin-platform dynamic-literal caveat; never regenerate old golden bytes to pass.
- [x] GREEN: uninterrupted versus saved/resumed local construction has identical checkpoints,
  payments, commission dates and original rate-base values. Change the *installed* region file
  after saving: archived source still controls the project; no in-flight repricing occurs.
- [x] Pin source-economics/map tamper rejection through fingerprints/hash checks and exact
  effective-spec validation. Verify existing command-only rules-version fencing remains intact.

## Task 5: Present the local quote and actual construction history

**Files:** `crates/game/src/hud/{build,inspector,construction,plant_panel}.rs`, public view/query
types if needed, `assets/locales/en-US/*.ftl`, `l10n/keys.rs`, existing game integration modules.

- [x] RED real-egui tests: selecting otherwise comparable sites in two countries shows each
  localized zone name, different capital/duration and correct monthly payment. Default/legacy
  sites preserve their prior numbers. Base generic catalog information must not masquerade as
  a final site-specific quote.
- [x] Build preview and confirmation share the order quote; under-construction/installed cards
  use that asset's effective terms, not `region.spec(kind)`. Show months-left of the correct total
  and the actual entry-into-service month. Link breakdown units/cents/exact work sum to the quote.
- [x] Keep presenters pure, actions through dispatch and resource reads through ActionSink
  getters. Localized names/errors/Regional default get Fluent/literal-key coverage; money uses
  the existing regional MoneyStyle. No national currency selector or new map overlay is required.
- [x] GREEN: keyboard submission, disabled invalid builds, minimum window/zoom and 4.5:1 text
  contrast. Native isolated playtest compares two sites, orders one, saves during construction,
  resumes and verifies the displayed cost/time through commissioning. Do not use user saves.

## Task 6: Region-authoring handoff to Plan 7

This is required scope of Plan 7, not new live country-price data fabricated by Plan 6a.

- [x] In the future detailed Plan 7, make its Europe–Maghreb task depend on Tasks 1–5. Author
  country assignments over playable cells; record any unsupported finer attribution as explicit
  regional-default cells. Respect the spec's grid-operation/disputed-territory rules.
  Done by Plan 7 Task 9: the zone polygons in
  [`continental_europe_maghreb_economic_zones.ron`](../../../crates/infrastructure/authoring/continental_europe_maghreb_economic_zones.ron)
  rasterise into the `economic_zones.cells` of
  [`continental_europe_maghreb.ron`](../../../assets/regions/continental_europe_maghreb.ron), whose
  header lists the regional-default countries and the grid-operation rulings;
  `europe_maghreb_zone_cells_are_reproducible` and `europe_maghreb_landmarks` in
  [`shipped_regions.rs`](../../../crates/infrastructure/tests/shipped_regions.rs) check them.
- [x] Use locally sourced whole-project estimates for the relevant technologies, normalize into
  EUR with source/date/currency assumptions, and document compressed build-duration judgments.
  Shared profiles are allowed where justified; the Europe/Maghreb comparison must demonstrate
  meaningful **cost and duration** differences for at least one same-kind plant. No blanket
  Maghreb wage discount or unsupported country-specific number is acceptable evidence.
  Done by Plan 7 Tasks 1 and 9: the rows are in the
  [Plan 7 research note](../../research/2026-10-25-plan-7-regional-economics.md), the Spain and
  Morocco profiles cite them in the region file's `economic_zones` block, and
  `spain_and_morocco_differ_in_cost_and_duration` checks onshore wind (17.0% dearer, 3 against 7
  game months).
- [x] Include the Seville/Málaga/Tangier starter territory and Gibraltar route valuation. Country
  profiles do not change AC connectivity, shared EUR accounting, tariff/regulator or link limits.
  Done by Plan 7 Task 9: `gibraltar_starts_served_named_and_valued_by_route` in
  [`shipped_regions.rs`](../../../crates/infrastructure/tests/shipped_regions.rs).
- [x] IPS/UPS explicitly records whether country/subnational economics is used; if used, author
  it with the same provenance, independently of civil zones and electrical areas. Other regions
  can omit the block and use locally sourced baseline catalogs with exact uniform arithmetic.
  Done by Plan 7 Task 8: [`ips_ups.ron`](../../../assets/regions/ips_ups.ron) records
  "economic_zones omitted by authoring choice", following the research note's
  [IPS/UPS decision](../../research/2026-10-25-plan-7-regional-economics.md#ipsups-economic-zones-decision);
  the other Plan 7 regions omit the block.
- [x] New-region acceptance covers no-command/hands-off year one and early expansion, seeds
  0–7, from the shipped starter territory. Add compact construction scenarios covering each
  distinct authored profile with its actual terms; this does not require a remote country to
  be reachable from Seville within the first-expansion window. Retune data/cash/starter fleet if
  needed; never relax accounting, compatibility or quote invariants.
  Done by Plan 7 Task 9: the `year_one_cases!` entries and
  `shipped_first_expansion_acceptance_seeds_zero_through_seven` in
  [`shipped_regions.rs`](../../../crates/infrastructure/tests/shipped_regions.rs), and
  `shipped_europe_maghreb_profiles_quote_pay_and_commission_by_their_terms` in
  [`economic_construction.rs`](../../../crates/infrastructure/tests/economic_construction.rs).

## Task 7: Mandatory Plan 8 HVDC integration handoff

Plan 6a deferred HVDC to Plan 8. Reconciled 2026-10-26 against the implemented link terms in
[`areas.rs`](../../../crates/domain/src/region/areas.rs), route resolver in
[`economics.rs`](../../../crates/domain/src/region/economics.rs), and link/save regressions:

- [x] Extend the zone DTO/resolver with the spec's optional HVDC construction overrides only
  when implementing links. Resolve terminals independently. Inherit a base combined converter
  cost by exact even splitting, with the odd cent assigned by canonical endpoint-cell order;
  do not charge the combined base at each end or floor away its remainder.
- [x] Resolve route cost from HVDC `cost_per_cell` and route speed from regional line
  `cells_per_month` unless the zone's HVDC terms override them. Quote duration is the maximum
  of both terminal durations and exact route months. Validate a separate HVDC work-rate LCM.
- [x] Cost canonicalization must not swap the link's stored `from`/`to` or scheduled-flow sign.
  Preserve link losses, physics, need checks and synchronous-island boundaries unchanged.
- [x] Cover distinct terminal zones, odd combined converter cents, cross-zone route work,
  endpoint reversal and all-default equivalence in `region/areas.rs`; cover link payments,
  cancellation, commissioning/rate base and snapshot restore in `session/links.rs` and the
  infrastructure save-codec regressions.
- [x] Complete the partial-override and zoned-lifecycle regression matrix: explicitly test only
  one terminal overriding converter terms and authored zones whose HVDC overrides are absent,
  then carry zone-derived totals through payments, cancellation, commissioning and save/resume.
  Completed 2026-10-26: two `region::areas` properties exercise every field-presence mask,
  odd converter cents, zero-price overrides, exact route work and endpoint reversal. The
  `zoned_hvdc_partial_overrides_survive_payments_cancellation_commissioning_and_resume` test in
  [`economic_construction.rs`](../../../crates/infrastructure/tests/economic_construction.rs)
  covers absent, west-only, east-only and both-terminal overrides in both endpoint orders,
  with exact instalments, cancellation after payment, commission dates/rate base and repeated
  standalone save/resume after replacing the installed region source.
- [x] Retain Plan 8's own rules/state changes; do not give economic zones another independent
  state bump or overwrite earlier archived source/canonical history.

## Task 8: Release verification and version/status update

- [x] Run targeted domain/application/infrastructure/game tests through each RED/GREEN stage.
  Verify identity behavior against the immutable preflight construction capture and existing
  released fixtures; do not generate replacement baselines after implementation.
- [x] Bump `SIM_RULES_VERSION` exactly once for Plan 6a from its implementation base (16→17), regenerate/review the rules guard and relevant scenario snapshots. Annotate every
  intended output difference; the only existing snapshot change is the rules-version line.
- [x] Run full workspace nextest, clippy/all-targets `-D warnings`, fmt; native launch and the
  Task 5 isolated workflow. No new extra Bevy integration binary or broad suite repetition without
  a source change/failure/risk justifying it.
- [x] Mark Plan 6a implemented only after its code/validation actually lands. Keep Plan 6's
  historical rules-16 completion and Plans 7/8 status truthful. Update architecture/testing
  guidance for cell-resolved construction; do not claim country tariffs/FX/operating costs exist.
- [x] Commit reviewed scoped changes conventionally, with hooks. Do not push/merge without
  the user's integration choice.

## Review focus

1. Local specs must pass snapshot validation without admitting arbitrary forged stored specs.
2. Quotes, paid totals, progress and original rate base must use the same resolved terms.
3. Cross-country lines must be direction independent and round total work once, with exact
   all-default behavior; water rendering and billable units are different concepts.
4. Profiles must not mutate physics, currency, tariff or regulator behavior by accident.
5. Europe–Maghreb must actually author country differences; optional infrastructure alone does not
   fulfill its data handoff. Absence remains a supported and tested identity for other regions.

## Implementation and validation record

Implemented from clean base `2009acc` in `codex/m2-global-foundations`. Optional strict RON
profiles resolve only construction capital/duration. Cell-aware plant specs, canonical sampled AC
route terms and typed checked arithmetic feed orders, starter valuation, instalments, progress,
commissioning, original rate base and HUD details. No command fields or portable-state fields were
added. Existing physics, operating prices and currency remain regional. Old released fixtures and
the immutable preflight source/save/canonical bytes remain unchanged.

Validation on macOS/aarch64:

- Focused RED/GREEN domain, loader, accounting/save and real-egui tests passed. Review added
  explicit repeated-crossing/descending-diagonal and typed loader-LCM checks. The old rules guard
  failed only on 16→17; its reviewed snapshot changes only that line. No legacy golden was updated.
- Final full workspace nextest: **1,658 passed, 0 skipped** (185.791 seconds). Clippy with all
  targets and `-D warnings`, `cargo fmt --all --check`, `git diff --check` and all 13 local Markdown
  targets/heading anchors passed. The native 120-frame launch smoke exited 0 with isolated
  save/log directories. The only runtime warnings were the existing Metal bindless-mode fallback
  and a winit window-destroy notification; no panic or system-parameter conflict occurred.
- Native isolated fixture, seed `1792941545537596000`: west `(4,4)` Wind preview showed €1.00M,
  about €500k/month and 2 months; east `(10,4)` showed €2.00M, about €667k/month and 3 months.
  Built eastern Wind 13 in April 2027. Enter submitted the named save “Economic checkpoint”.
  Left the session, resumed that disk save in a fresh native process, then played all three
  representative days at 32×. May/June showed 2/1 of 3 months left, always July 2027 entry and
  €2.00M original capital. July Planning showed the commissioned plant, eastern market, €2.00M
  capital and 3-month original build time. It remained disconnected, as ordered.
- The native run exposed long market-name clipping. A failing minimum-window/1.25×-zoom widget
  regression established the overflow; bounded grid columns fixed it. The resumed native cards
  visibly wrapped the full market name. Operating's fixed-height details remain scrollable.
- The fictional fixture's inherited manual starter plan generated frequency notices/strikes;
  this run validates construction and persistence, not acceptance of future country balance data.
  Automated shipped-region/year-one checks remain part of the passing workspace suite.
- Native assets, logs, preferences and saves used an isolated temporary directory. No user saves
  were used. The durable regression fixtures and tests are in `region/economics_tests.rs`,
  `infrastructure/tests/{region_catalog,economic_construction}.rs` and
  `game/tests/game/economics.rs`; scratch native files are not an acceptance dependency.

No real country estimates were fabricated and no shipped region was retuned by Plan 6a. Task 6
was subsequently completed in Plan 7. Task 7's HVDC implementation landed in Plan 8; its remaining
partial-override and zoned-lifecycle regression matrix was completed on 2026-10-26 above.
Plan 6a's original completion record covers its own infrastructure and validation, not execution
of those later plans.

Follow-up regression validation (2026-10-26): 21 focused domain tests, 12 infrastructure
economics/codec/loader tests and the application rules guard passed. Controlled faults that
double-charge default converters or discard zoned capital at commissioning made the new tests
fail; both production mutations were restored. The eight hook-selection tests, workspace
Clippy with warnings denied, formatting, diff checks and added Markdown links/anchors passed.
This follow-up changes tests and documentation only; simulation rules remain 20.
