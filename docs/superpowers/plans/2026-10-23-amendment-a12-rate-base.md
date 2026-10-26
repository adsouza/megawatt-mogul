# Amendment A12: Rate-Base Need Tests Implementation Plan

**Completion reconciliation (2026-10-26):** Implemented.

- [x] Duration-based battery accreditation, satisfaction-adjusted returns and the line need test are implemented.
- [x] Region validation, HUD copy, compatibility and loophole regressions are implemented.
- [x] Completion is recorded under rules 13 in [spec §26](../specs/2026-10-18-megawatt-mogul-design.md#26-amendment-a12-close-the-rate-base-loophole-and-make-satisfaction-count-2026-10-23); later amendments supersede the historical rules-11 instructions below.

The original execution recipe is retained below as plain steps. This reconciliation records
landed deliverables, without claiming a fresh run of each historical RED test, command or commit
sequence. Native checks without a complete receipt remain unchecked.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. The step sequence below is the historical execution recipe.

**Goal:** Every asset that earns a regulated return passes a need test. Batteries are accredited by
duration, redundant lines are refused when ordered, and satisfaction moves the allowed return.

**Architecture:**
- New regulator parameters arrive through a versioned region schema, with frozen defaults for
  archived regions.
- Battery firm MW comes from one domain derivation that every reserve consumer shares. December
  refreshes the stored rate-base profiles from it.
- The line rule lives in `GameSession::line_quote`, so the HUD's existing quote/error path picks it
  up.
- The satisfaction term joins `compute_rate_review`. Its display is a direct query, so no persisted
  type changes.

**Tech Stack:** Rust workspace (`domain`, `application`, `infrastructure`, `game`), serde/RON,
proptest, insta, Bevy 0.20 + egui 0.36 for the HUD.

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md` §26 (Amendment A12). Also
read §2.5 (rate review, holding), §20.6 (battery regulation history) and §21 (portable saves).

## Global Constraints

- `SIM_RULES_VERSION` goes from **10 to 11**, at the first behaviour change (Task 2). Plan 5 later
  takes 12.
- Portable state schema stays **1**. No new fields on any persisted type. That includes
  `RateReviewEstimate`, `RateReviewOutcome`, `MonthlyReport`, `YearTally`, `AssetProfile`, and every
  projection.
- A restored checkpoint must equal the saved checkpoint byte for byte (§21, §26.6). Nothing may
  refresh state during restore.
- Shipped storage ladder: 2 h → 0.65, 4 h → 0.85, 6 h → 0.93, 8 h → 0.96. Shipped
  `satisfaction_target = 90` and `return_per_satisfaction_point = 0.0005`. These go in both region
  RON files, `infrastructure/tests/support/minimal_region.ron` and the domain `test-fixtures`
  fixture.
- Region schema marker: `region_schema: 2`. Shipped regions and fixtures declare it. Files without
  it are schema 1 and get the frozen A12 defaults, which equal the shipped values.
- AGENTS.md rules apply:
  - the domain depends only on `rand_chacha` and `thiserror`;
  - no user-facing strings in the domain or application;
  - `BTreeMap`/`BTreeSet` only;
  - every I/O test gets its own `TempDir`;
  - new copy goes in `assets/locales/en-US/*.ftl`, and new literal keys in `crates/game/src/l10n/keys.rs`;
  - game integration tests are modules under `crates/game/tests/game/`;
  - the presenter → draw → `UiAction` → `dispatch` pattern.
- Before every commit, `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo fmt --all --check` must be green. Never use `--no-verify`.
- Commit trailer: `Co-Authored-By:` naming your model.

## Review Focus

1. **A legacy region file that contains a new field but no `region_schema` marker** must fail with
   a typed error, never be defaulted silently (Task 1, `a_schema1_file_with_a12_fields_is_rejected`).
2. **A rules-10 save resumed in a December Review** must not run the annual review a second time
   when the player continues (Task 6, `a_resumed_december_review_is_not_reviewed_again`).
3. **Batteries under construction contribute no firm MW. Disabled batteries count once
   commissioned,** because accreditation follows the asset, not its toggle (Task 2,
   `firm_capacity_counts_commissioned_batteries_only`).
4. **Cancel and reorder.** Cancelling an unfinished line frees its route, so a faster route can then
   be ordered (Task 4, `cancelling_an_unfinished_line_frees_its_route`).
5. **Copy.** A joined but non-duplicate endpoint shows the need-test copy, not the
   `DuplicateLine` copy (Task 5, `a_joined_target_shows_the_need_test_reason`).

---

### Task 1: Region schema 2 and the A12 regulator parameters

No outcome changes. This task only plumbs the data and its validation.

**Files:**
- Modify: `crates/domain/src/region/params.rs`: add `StorageAccreditation` and three
  `RegulatorParams` fields.
- Modify: `crates/domain/src/region/mod.rs`: add two `RegionError` variants.
- Modify: `crates/domain/src/region/fixtures.rs` (`regulator()`): add the shipped values.
- Modify: `crates/infrastructure/src/region/dto.rs`: add `RegionFile.region_schema` and the three
  `RegulatorDto` options.
- Modify: `crates/infrastructure/src/region/mapping.rs` (`map_region`, `map_regulator`).
- Create: `crates/infrastructure/src/region/legacy.rs`, for the frozen A12 defaults, and declare
  it in `crates/infrastructure/src/region/mod.rs`.
- Modify: `assets/regions/eastern_interconnection.ron`, `assets/regions/western_interconnection.ron`
  and `crates/infrastructure/tests/support/minimal_region.ron`.
- Modify: `crates/game/src/l10n/localize.rs`, `assets/locales/en-US/errors.ftl` and
  `crates/game/src/l10n/keys.rs`, for the copy of the new `RegionError` variants.
- Test: inline in `params.rs`, and `crates/infrastructure/tests/region_catalog.rs`.

**Interfaces:**
- Produces:
  - `domain::region::StorageAccreditation`, with:
    - `pub fn new(points: Vec<(f64, Ratio)>) -> Result<Self, RegionError>`;
    - `pub fn credit(&self, hours: f64) -> Ratio`;
    - `pub fn points(&self) -> &[(f64, Ratio)]`.
  - `RegulatorParams` gains:
    - `pub storage_accreditation: StorageAccreditation`;
    - `pub satisfaction_target: f64`;
    - `pub return_per_satisfaction_point: f64`.
  - `RegionError::MissingField { field: &'static str }` and
    `RegionError::UnsupportedRegionSchema(u32)`.
  - `infrastructure::region::CURRENT_REGION_SCHEMA: u32 = 2`.
  - `infrastructure::region::legacy::{A12_STORAGE_ACCREDITATION: [(f64, f64); 4], A12_SATISFACTION_TARGET: f64, A12_RETURN_PER_SATISFACTION_POINT: f64}`.

- **Step 1: Write the failing ladder tests** (inline `#[cfg(test)]` in `params.rs`).

```rust
fn shipped() -> StorageAccreditation {
    StorageAccreditation::new(vec![(2.0, r(0.65)), (4.0, r(0.85)), (6.0, r(0.93)), (8.0, r(0.96))]).unwrap()
}
#[test]
fn credit_is_exact_at_points_linear_between_and_flat_beyond() {
    let l = shipped();
    assert_eq!(l.credit(4.0), r(0.85));
    assert!((l.credit(3.0).value() - 0.75).abs() < 1e-12);
    assert!((l.credit(1.0).value() - 0.325).abs() < 1e-12); // linear from (0, 0)
    assert_eq!(l.credit(0.0), Ratio::ZERO);
    assert_eq!(l.credit(10.0), r(0.96));
}
#[test]
fn malformed_ladders_are_rejected() {
    let bad = |pts: Vec<(f64, Ratio)>| StorageAccreditation::new(pts).is_err();
    assert!(bad(vec![]));
    assert!(bad(vec![(0.0, r(0.5))]));                    // hours must be > 0
    assert!(bad(vec![(4.0, r(0.8)), (2.0, r(0.9))]));     // hours strictly increasing
    assert!(bad(vec![(2.0, r(0.8)), (2.0, r(0.9))]));
    assert!(bad(vec![(2.0, r(0.9)), (4.0, r(0.8))]));     // credit non-decreasing
    assert!(bad(vec![(f64::NAN, r(0.5))]));
}
proptest! {
    #[test]
    fn credit_is_monotone_in_duration(a in 0.0f64..20.0, b in 0.0f64..20.0) {
        let l = shipped();
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(l.credit(lo) <= l.credit(hi));
    }
}
```

- **Step 2: Run them and see them fail.** Run `cargo nextest run -p domain storage_accreditation credit_is malformed_ladders`.
  Expected: compile error, because `StorageAccreditation` does not exist yet.

- **Step 3: Implement `StorageAccreditation`** in `params.rs` and re-export it from
  `domain::region`. Rejections return `RegionError::InvalidShape { field: "regulator.storage_accreditation" }`.
  `credit` clamps a negative or NaN `hours` to `Ratio::ZERO`. Add the three `RegulatorParams`
  fields. Add the shipped values to `fixtures.rs` `regulator()`. Add the two new `RegionError`
  variants, with `thiserror` messages in the existing style.

- **Step 4: Write the failing mapping tests** in `crates/infrastructure/tests/region_catalog.rs`.
  Each builds its RON in its own `TempDir` from `support/minimal_region.ron`, edited as text.
  - `a_schema2_region_maps_the_a12_fields`: the minimal region (now `region_schema: 2`) maps
    `storage_accreditation.credit(4.0) == 0.85`, `satisfaction_target == 90.0` and
    `return_per_satisfaction_point == 0.0005`.
  - `a_schema2_region_missing_an_a12_field_names_it`: remove `satisfaction_target`. Loading fails
    with `RegionError::MissingField { field: "regulator.satisfaction_target" }`. Repeat for the
    other two fields.
  - `a_schema1_region_gets_the_frozen_a12_defaults`: remove `region_schema` and the three fields.
    It maps to exactly the legacy constants.
  - `a_schema1_file_with_a12_fields_is_rejected`: remove only `region_schema`. Loading fails with
    `MissingField { field: "region_schema" }`.
  - `an_unknown_region_schema_is_rejected`: `region_schema: 9` fails with `UnsupportedRegionSchema(9)`.
  - `satisfaction_parameters_are_validated`: a target outside `0..=100`, or a non-finite value,
    fails with `InvalidShape` naming the field.

- **Step 5: Run them and see them fail.** Run `cargo nextest run -p infrastructure region_catalog`.
  Expected: they fail, because the fields are unknown (`deny_unknown_fields`).

- **Step 6: Implement the DTO and mapping.**
  - `RegionFile` gains `#[serde(default)] pub region_schema: Option<u32>`.
  - `RegulatorDto` gains `#[serde(default)]` options:
    - `storage_accreditation: Option<Vec<(f64, f64)>>`;
    - `satisfaction_target: Option<f64>`;
    - `return_per_satisfaction_point: Option<f64>`.
  - In `map_region`, match `region_schema`:
    - `None`: all three fields absent → legacy constants; any present → `MissingField { field: "region_schema" }`.
    - `Some(2)`: all three required, each missing one → `MissingField { field: "regulator.<name>" }`.
    - `Some(n)`: `UnsupportedRegionSchema(n)`.
  - Doc-comment `legacy.rs` as frozen compatibility defaults for archives that predate A12
    (spec §26.6). They are never read from shipped files.
  - Author the new fields in the three RON files:
    - `region_schema: 2` at the top level;
    - `storage_accreditation: [(2.0, 0.65), (4.0, 0.85), (6.0, 0.93), (8.0, 0.96)]`;
    - `satisfaction_target: 90.0`;
    - `return_per_satisfaction_point: 0.0005` inside `regulator`.

- **Step 7: Add localisation for the two new `RegionError` variants.**
  - `region-error-missing-field = The region file is missing { $field }.`
  - `region-error-unsupported-schema = The region file uses schema { $schema }, which this version can't read.`

  Add their match arms in `localize.rs` and their keys in `keys.rs`.

- **Step 8: Run the whole workspace.** Run `cargo nextest run --workspace`. Expected: PASS,
  including `save_snapshots` (the golden saves' schema-1 regions take the legacy path) and the
  unchanged rules-version guard. Shipped fingerprints change, and so do any tests that pin them:
  update those pins deliberately.

- **Step 9: Commit.** `feat: add region schema 2 with storage accreditation and satisfaction-return parameters`

---

### Task 2: Battery accreditation by duration, with rules 11

**Files:**
- Modify: `crates/domain/src/region/plant_spec.rs`: add `PlantSpec::firm_capacity`.
- Modify: `crates/domain/src/grid/power_system.rs`: change `firm_capacity` (near line 1222) to
  take the ladder.
- Modify: `crates/domain/src/session/mod.rs` (start fleet, near line 184) and
  `crates/domain/src/session/month_close.rs` (commissioning near line 249, `review_inputs`,
  `annual_review`).
- Modify: `crates/domain/src/company/rate_base.rs`: add `refresh_plant_firm`.
- Modify: `crates/domain/src/session/views.rs` (`rate_review_estimate`).
- Modify: `crates/domain/src/lib.rs`: `SIM_RULES_VERSION = 11`.
- Test: inline `month_close/tests.rs`, `rate_base/tests.rs` and `power_system/tests.rs`; a
  property test in `crates/domain/src/session/tests.rs`.
- Regenerate: `crates/application/tests/snapshots/*.snap` (the rules guard and scenarios).

**Interfaces:**
- Consumes: `StorageAccreditation::credit` and `RegulatorParams::storage_accreditation` (Task 1).
- Produces:
  - `PlantSpec::firm_capacity(&self, accreditation: &StorageAccreditation) -> Megawatts`. For
    generating kinds it is `capacity_credit.of(capacity)`. For Battery it is
    `capacity × credit(energy_capacity × discharge_efficiency / capacity)`.
  - `PowerSystem::firm_capacity(&self, accreditation: &StorageAccreditation) -> Megawatts`, the sum
    over commissioned plants.
  - `RateBase::refresh_plant_firm(&mut self, firm_of: impl Fn(PlantId) -> Option<Megawatts>)`. It
    overwrites `AssetProfile::Plant.firm` for every plant asset where `firm_of` returns `Some`.

- **Step 1: Write the failing tests.**
  - `the_shipped_battery_is_accredited_at_170_mw` (`plant_spec.rs`): the fixture battery spec's
    `firm_capacity(&shipped ladder)` equals `Megawatts::new(170.0)`. Every generating spec equals
    `capacity_credit.of(capacity)`.
  - `firm_capacity_counts_commissioned_batteries_only` (`power_system/tests.rs`). A battery under
    construction adds 0. A commissioned one adds 170 MW, enabled or disabled.
  - `one_firm_derivation_everywhere` (proptest in `session/tests.rs`). Over generated build
    sequences in `tiny_region`, including batteries, each commissioned plant's stored rate-base
    firm equals its `PlantSpec::firm_capacity`. `PowerSystem::firm_capacity` equals their sum, and
    the estimate's reserve margin uses that sum.
  - `excess_batteries_are_held_then_written_off` (`month_close/tests.rs`). In a fixture whose
    generating fleet already exceeds the reserve margin, two batteries commissioned before
    December are held at that December review (`CapitalHeld` for both), and written off at the
    next one.
  - `a_battery_that_fills_a_reserve_gap_is_not_held`: the same fixture with the fleet just below
    the margin. The battery is not held.
  - `refresh_rewrites_only_plant_firm` (`rate_base/tests.rs`). Give an asset with `firm = 0` and
    `firm_of` returning 170. Afterwards `firm == 170`, and line assets and held slices are
    unchanged.

- **Step 2: Run them and see them fail.** Run `cargo nextest run -p domain`. Expected:
  compile errors or failures in exactly these tests.

- **Step 3: Implement.**
  - Replace the three `capacity_credit.of(capacity)` sites with `spec.firm_capacity(&region.regulator().storage_accreditation)`.
  - In `annual_review`, call `refresh_plant_firm` with each commissioned plant's derived firm before
    `apply_reserve_review`. Do it inside the closing command, so a restored boundary is never
    touched.
  - In `rate_review_estimate`, clone the rate base, refresh the clone, then
    `preview_reserve_review`. The estimate stays pure.
  - `YearTally::firm_added` keeps its current recording, now with the derived value, and nothing
    reads it.
  - Set `SIM_RULES_VERSION = 11`.

- **Step 4: Run them and see them pass.** Run `cargo nextest run -p domain`. Expected: PASS.

- **Step 5: Regenerate the snapshots deliberately.** Run `cargo nextest run -p application`,
  then `cargo insta review`. Expected changes: the rules guard records version 11. Scenario digests
  change only where batteries exist. If a storage-free scenario's digest changes, stop: that is a
  bug.

- **Step 6: Run the whole workspace.** Run `cargo nextest run --workspace`. Expected: PASS.
  The golden saves (rules 8) still restore byte-identically, because restore refreshes nothing.

- **Step 7: Commit.** `feat: accredit batteries by duration in the reserve review (rules 11)`

---

### Task 3: Satisfaction as a performance-based return term

**Files:**
- Modify: `crates/domain/src/regulation/rate_review.rs`: add `satisfaction` to `RateReviewInputs`,
  the term in `compute_rate_review`, and `satisfaction_adjustment`.
- Modify: `crates/domain/src/regulation/mod.rs`: the re-export.
- Modify: `crates/domain/src/session/month_close.rs` (`review_inputs`).
- Modify: `crates/domain/src/session/views.rs`: add `satisfaction_return_adjustment`.
- Regenerate: the application snapshots.

**Interfaces:**
- Consumes: `RegulatorParams::{satisfaction_target, return_per_satisfaction_point}` (Task 1).
- Produces:
  - `domain::regulation::satisfaction_adjustment(satisfaction: f64, p: &RegulatorParams) -> f64`,
    which is `p.return_per_satisfaction_point × (satisfaction − p.satisfaction_target)`, unclamped.
  - `RateReviewInputs.satisfaction: f64`. It is not persisted, so adding it is allowed.
  - `GameSession::satisfaction_return_adjustment(&self) -> f64`: the adjustment for the company's
    current satisfaction.

- **Step 1: Write the failing tests** in `rate_review.rs`, using the fixture regulator. In each,
  reliability sits at the target and there is no previous cost, so only the base and the
  satisfaction term apply.

```rust
#[test]
fn satisfaction_moves_the_allowed_return() {
    let p = tiny_region().regulator().clone();
    let at = |s: f64| compute_rate_review(&RateReviewInputs { satisfaction: s, ..base_inputs() }, &p).allowed_return.value();
    assert!((at(90.0) - 0.09).abs() < 1e-12);
    assert!((at(100.0) - 0.095).abs() < 1e-12);
    assert!((at(80.0) - 0.085).abs() < 1e-12);
}
proptest! {
    #[test]
    fn return_is_the_clamped_sum_of_its_terms(s in 0.0f64..=100.0, served in 0.0f64..1e9, demand in 0.0f64..1e9) {
        // hand-compute base + reliability + satisfaction terms, clamp to [min_return, max_return],
        // and compare with compute_rate_review's allowed_return (within 1e-12)
    }
}
```

  Also add `a_session_reports_its_satisfaction_adjustment` in `session/tests.rs`: a new game at the
  starting satisfaction of 70 reports `0.0005 × (70 − 90) = −0.01`.

- **Step 2: Run them and see them fail.** Run `cargo nextest run -p domain satisfaction`. Expected: FAIL or a compile error.

- **Step 3: Implement.** Add the term before `clamp_return`. Set `satisfaction` from
  `self.company.satisfaction()` in `review_inputs`. Month close updates satisfaction at step 5,
  before the review at step 6, so December's review sees December's value. The estimate inherits
  the term through `review_inputs`. Update any existing test literals of `RateReviewInputs`.

- **Step 4: Run them and see them pass.** Run `cargo nextest run -p domain`. Expected: PASS. All
  the existing A2 proration and band properties still pass.

- **Step 5: Regenerate the snapshots deliberately.** Run `cargo insta review`. Every scenario
  that reaches a December review changes its allowed return and tariff. The version stays 11:
  it is unreleased, so a second regeneration under the same bump is the plan's ruling, recorded in
  the ledger.

- **Step 6: Run the beginner checks.** Run `cargo nextest run -p infrastructure shipped_regions`.
  Expected: PASS. If a year-one run fails, retune the shipped satisfaction values in region data,
  never the rule, and say so in the commit body.

- **Step 7: Commit.** `feat: let satisfaction adjust the performance-based allowed return`

---

### Task 4: Refuse lines whose ends are already joined

**Files:**
- Modify: `crates/domain/src/error.rs`: add `DomainError::LineNotNeeded { from: NodeId, to: NodeId }`.
- Modify: `crates/domain/src/grid/network.rs`: add `Network::joined`.
- Modify: `crates/domain/src/session/commands.rs` (`line_quote`).
- Modify: `crates/game/src/l10n/localize.rs`, `assets/locales/en-US/errors.ftl` and
  `crates/game/src/l10n/keys.rs`.
- Modify: existing tests that order a redundant line, such as the "second, slower line" test in
  `crates/game/src/hud/city_card.rs` near line 305. Re-aim each one at an endpoint that is not
  joined, keeping what the test was written to show.
- Test: inline `network.rs` and `session/commands/tests.rs`.

**Interfaces:**
- Produces:
  - `Network::joined(&self, a: NodeId, b: NodeId) -> bool`: whether a path of lines in any status
    (in service or under construction) joins `a` and `b`. It is a deterministic BFS over a
    `BTreeMap` adjacency.
  - `line_quote` returns `Err(DomainError::LineNotNeeded { from, to })` when `joined`, after the
    existing `check_line`, so an exact duplicate still returns `DuplicateLine`. `build_line`
    inherits the rule through `line_quote`.

- **Step 1: Write the failing tests** (`session/commands/tests.rs`, using `tiny_region`).
  - `a_line_between_joined_nodes_is_refused`: a starter city to another starter city returns
    `LineNotNeeded`. The session's `state_hash` and the ID counter are unchanged.
  - `an_exact_duplicate_still_reports_duplicate_line`.
  - `an_ordered_line_already_joins_its_ends`. Order home→X with X a new plant. Then any line
    between X and another starter node is refused.
  - `islands_are_joined_too`. Join two unconnected cities to each other. A second line between
    them is refused, and a line from either to the home network is allowed.
  - `cancelling_an_unfinished_line_frees_its_route` (Review Focus 4). Order A→X, cancel it, then
    order B→X successfully.
  - `accepted_orders_never_close_a_cycle` (proptest). Over random sequences of `BuildLine` and
    cancellation on `tiny_region`, take the in-service plus under-construction graph after each
    step. It has `lines == nodes_touched − components`, so it is a forest. Each `BuildLine`
    succeeds exactly when `!joined(from, to)` held before it, and the endpoints are distinct,
    known and not duplicates.
  - `grandfathered_cycles_still_work`. Add a redundant line directly with
    `power.network_mut().add_line`, the way a pre-A12 save could hold one; `add_line` keeps
    accepting it because the rule lives in `line_quote`. Play to the next month close: the line
    commissions, enters the rate base as before, and connectivity is unchanged.

- **Step 2: Run them and see them fail.** Run `cargo nextest run -p domain line`. Expected: FAIL.

- **Step 3: Implement `Network::joined`, the error variant and the `line_quote` check.** Add
  the copy:
  - `error-line-not-needed = { $from } and { $to } are already joined by lines in service or under construction, so the regulator won't approve another line between them.`

  Add its key, and its match arm (with the `from` and `to` names, like `DuplicateLine`).

- **Step 4: Run the workspace.** Run `cargo nextest run --workspace`. Fix the tests that relied on
  redundant lines, as listed under Files. If a scenario's script ordered a redundant line,
  re-aim the script and regenerate its snapshot deliberately.

- **Step 5: Commit.** `feat: refuse transmission lines whose ends are already joined`

---

### Task 5: HUD: the satisfaction adjustment and the need-test copy

**Files:**
- Modify: `crates/game/src/l10n/format.rs`: add `signed_points`.
- Modify: `crates/game/src/hud/tariff.rs` (`tariff_model` and the Planning/Operating variants near
  lines 31 and 149).
- Modify: `crates/game/src/hud/review.rs` (the rate-review section near line 240).
- Modify: `assets/locales/en-US/ui.ftl` and `crates/game/src/l10n/keys.rs`.
- Test: inline presenter tests in `tariff.rs`, `review.rs` and `build.rs`; a real egui pass in
  `crates/game/tests/game/hud_smoke.rs`.

**Interfaces:**
- Consumes: `GameSession::satisfaction_return_adjustment()` (Task 3) and `DomainError::LineNotNeeded` (Task 4).
- Produces: `format::signed_points(fraction: f64) -> String`. For example `0.005` → `+0.50 pp`,
  `−0.01` → `−1.00 pp` using the existing `MINUS`, and a value that rounds to zero shows `0.00 pp`.

- **Step 1: Write the failing tests.**
  - `signed_points_formats_percentage_points` (`format.rs`), with the three cases above.
  - `the_tariff_panel_shows_the_satisfaction_adjustment` (`tariff.rs`). On a new game, the review
    stats contain `("Satisfaction adjustment", "−1.00 pp")`, directly after the projected return.
    It is read from the direct query, not the projection.
  - `a_december_review_shows_its_satisfaction_adjustment` (`review.rs`). While in the Review of a
    December close, the rate-review section has a second line, `Satisfaction moved your allowed
    return by { $points }.`, using the session's current adjustment. Other months, and archived
    reports viewed later, show no such line.
  - `a_joined_target_shows_the_need_test_reason` (`build.rs`, Review Focus 5). With the home city
    as origin, a target joined only through other lines has `build.enabled == false`, and its
    preview is the localised `error-line-not-needed` text.
  - In `hud_smoke.rs`, through the real egui pass at 1280×640, the tariff panel's new row paints
    whole: assert on painted glyphs (`contrast::painted_glyphs`), not AccessKit. It meets 4.5:1
    against its backing, and `l10n.problems()` is empty.

- **Step 2: Run them and see them fail.** Run `cargo nextest run -p game satisfaction_adjustment signed_points joined_target`. Expected: FAIL.

- **Step 3: Implement.** Add the copy:
  - `tariff-satisfaction-adjustment = Satisfaction adjustment`
  - `review-satisfaction-adjustment = Satisfaction moved your allowed return by { $points }.`

  Add both keys. The Review line shows when `report.rate_review.is_some()`, the phase is Review,
  and `report.month` is the month the session just closed.

- **Step 4: Run the game tests.** Run `cargo nextest run -p game`. Expected: PASS.

- **Step 5: Run the launch smoke.** Run `cargo run -p game -- --exit-after-frames 120`.
  Expected: exit 0.

- **Step 6: Commit.** `feat: show the satisfaction adjustment and the line need test in the HUD`

---

### Task 6: Compatibility and acceptance

**Files:**
- Test: `crates/infrastructure/tests/save_snapshots.rs` and `crates/infrastructure/tests/shipped_regions.rs`.
- Modify: `ARCHITECTURE.md` and `TESTING.md`, for the region schema marker and the legacy-defaults
  rule. Modify `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md` too: set the §26
  status line to implemented, and update the header's amendment list.

**Interfaces:**
- Consumes: everything above. Golden saves live in
  `crates/infrastructure/tests/support/portable-saves/`. Use the `restored(bytes)` helper.

- **Step 1: Write the compatibility tests** in `save_snapshots.rs`.
  - `an_old_battery_is_accredited_without_rewriting_the_save`, using
    `schema1-charged-battery.mgsave`:
    1. After restore, `game.checkpoint()` equals the decoded checkpoint, with only the header's
       rules version raised (as the existing test asserts).
    2. The battery's rate-base profile firm is still 0.
    3. `session.rate_review_estimate().reserve_margin` uses firm capacity that includes 170 MW per
       commissioned battery.
    4. Play on, with `BeginOperating`, `AdvanceTicks` to midnight and `FinishReview`, until a
       December close. The profile firm is then 170.
  - `a_resumed_december_review_is_not_reviewed_again` (Review Focus 2). The golden saves are all
    January boundaries. So restore `schema1-migrated-review.mgsave`, play it to the December close,
    then `encode_save` its checkpoint into a `TempDir` and restore that. Across `FinishReview` and
    the next Planning commands, count `RateReviewCompleted` events: exactly 0. The December
    report's `rate_review` is equal before and after.

- **Step 2: Write the loophole acceptance** in `shipped_regions.rs`.
  `gold_plated_batteries_do_not_raise_cash_over_three_reviews` runs for both regions, seeds 0–7,
  over 36 months from the start:
  1. Compare the hands-off game with the same game plus 10 batteries.
  2. Order the batteries in month 1 Planning, on the 10 nearest buildable land cells to the home
     city (ties by cell coordinate). Order one `BuildLine` from the home city's node to each.
  3. Assert the gold-plated game's cash at the final Review is ≤ the hands-off cash.
  4. Assert at least one `CapitalHeld` for a battery at the first December after commissioning.
  5. Assert that ordering a second line from any battery to another starter node returns
     `LineNotNeeded`.

- **Step 3: Run them.** Run `cargo nextest run -p infrastructure save_snapshots shipped_regions`.
  Expected: PASS. A failure of the loophole acceptance means the rules are wrong; fix them, not
  the test.

- **Step 4: Run every beginner check.** Run `cargo nextest run --workspace`. This includes
  `hands_off_plan_survives_year_one`, `no_commands_survive_year_one`,
  `shipped_regions_never_order_outages_hands_off`, `shipped_projects_fit_tycoon_pacing` and the
  §18.3 early-expansion acceptance. Expected: PASS. Run clippy with `-D warnings` and
  `fmt --check` too.

- **Step 5: Run the launch smoke and update the docs.** Run
  `cargo run -p game -- --exit-after-frames 120`, expecting exit 0. Then update the docs listed
  under Files.

- **Step 6: Commit.** `test: pin A12 compatibility and the closed rate-base loophole` and
  `docs: mark Amendment A12 implemented`.
