# Megawatt Mogul Plan 1b: Spec Amendment A4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Amendment A4 on top of Plans 1 and 1a:
- forecast-based rotating outages;
- staged under-frequency load shedding;
- a capacity-shortfall warning for Planning;
- the matching lessons.

Together these mean a capacity shortage costs the player the load they couldn't serve, not the
whole grid (open question Q1).

**Architecture:** Everything stays inside the existing layers.
- **Domain rules:**
  - `grid::protection`: the stages;
  - `grid::power_system`: outage sizing and application, and the pure hydro preview;
  - `session`: accounting, reports and the query.
- **Read side:** `application` lessons.
- **Data:** `infrastructure` DTOs and the shipped RON files.
- **Versioning:** one `SIM_RULES_VERSION` bump to 3 at the end, with every snapshot regenerated once
  and reviewed.

**Tech Stack:** unchanged (Rust 1.98, edition 2024; no new crates).

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md` §15 (A4), plus the §5.4 table
rows that point to it. §12.4 (hydro lift and recovery) and §14.2 (trip gate headroom) define helpers
this plan reuses.

## Global Constraints

- Every Plan 1 and Plan 1a Global Constraint still applies:
  - dependency limits;
  - determinism: BTreeMap only, no wall clock, all randomness through `SimRng`, and a fixed number
    of RNG draws per tick and per day;
  - exact money;
  - `Err` leaves state unchanged;
  - no player-facing strings in domain or application;
  - hermetic I/O tests;
  - TDD.
- **New behaviour must not draw from the RNG.** Outage sizing, the stages and the shortfall check
  are deterministic functions of state.
- **Unserved energy is shed load only** (R16):
  - `served = demand × (1 − outage) × (1 − shed)` and `unserved = demand − served` (§15.3);
  - `imbalance` stays signed.
- **`SIM_RULES_VERSION` changes exactly once, from 2 to 3, in Task 8.**
  - Until then, snapshot changes are expected and are not accepted task by task.
  - Each task verifies with
    `INSTA_UPDATE=no cargo nextest run --workspace -E 'not (binary(scenarios) | binary(rules_version_guard))'`
    plus its own tests.
- **The new region field is data only:** `frequency.shed_stage_spacing = 0.2 Hz` in the fixture and
  all three RON files, never as a code constant.
- **Reuse, don't re-derive.** One-tick headroom is `PowerSystem::agc_headroom` (shared with the
  §14.2 trip gate and §12.4 recovery). Water filling is the `plan_hydro` code path. The hourly
  forecast is the same `connected_demand_forecast`/`expected_solar` pair as `hourly_net_load`.
- **When an acceptance run misses, retune region data, never the rules** (§14.5, §15.7).

## Review Focus

1. **A stage that fires twice in one event, or never re-arms.** Stages fire once per event and
   re-arm only at or above `−normal_band`. Test in Task 2.
2. **Outage sizing at an hour whose demand rises steeply.** The target covers the worse of minute 0
   and minute 59, so protection doesn't shed inside the hour on a noise-free forecast. Test in
   Task 3.
3. **Restoring outage load faster than the fleet can ramp.** The outage holds while one-tick
   headroom is below one `shed_step` of demand. Test in Task 3.
4. **A blackout on a shortage day.** Sizing freezes, `outage` stays 0 and restoration completes at
   the frozen target. Afterwards Normal mode resumes with that outage, and the dark load counts as
   unplanned. Test in Task 3.
5. **A planned outage lifting the hydro cap.** With `shed == 0` and `outage > 0` the cap stays in
   force, so water isn't spent early. Test in Task 3.

---

### Task 1: The `shed_stage_spacing` region parameter

**Files:**
- Modify: `crates/domain/src/region/params.rs` (`FrequencyParams`), `region/mod.rs` (validation), `region/fixtures.rs`
- Modify: `crates/infrastructure/src/region/dto.rs` (`FrequencyDto`), `region/mapping.rs`
- Modify: `crates/infrastructure/tests/support/minimal_region.ron`, `assets/regions/eastern_interconnection.ron`, `assets/regions/western_interconnection.ron`
- Modify: `crates/infrastructure/tests/region_catalog.rs` (`MINIMAL_SHA256`)
- Test: inline region tests, `infrastructure/tests/region_catalog.rs`, `infrastructure/tests/shipped_regions.rs`

**Interfaces:**
- Produces:
  - `FrequencyParams.shed_stage_spacing: Hertz`;
  - DTO field `shed_stage_spacing_hz: f64`;
  - validation `InvalidBand { field: "frequency.shed_stage_spacing" }` unless the value is finite
    and > 0.

- [ ] **Step 1: Write the failing tests.** Extend `out_of_range_numbers_are_rejected` with 0,
  a negative, NaN and ∞. Extend `range_boundaries_are_accepted` with a small positive value. Assert
  that the fixture and both shipped regions load with 0.2 Hz.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain region && cargo nextest run -p infrastructure`. Expected: FAIL.
- [ ] **Step 3: Implement.** Follow the Task 4 pattern from Plan 1a, and recompute `MINIMAL_SHA256`.
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat: region parameter for staged under-frequency shedding`

---

### Task 2: Staged under-frequency shedding (§15.4)

**Files:**
- Modify: `crates/domain/src/grid/protection.rs` (`ProtectionState`, `evaluate`)
- Test: inline protection tests; `crates/domain/src/grid/power_system/tests.rs`

**Interfaces:**
- Consumes: `FrequencyParams.shed_stage_spacing` (Task 1).
- Produces:
  - **`ProtectionState.stages_fired: u8`**, which counts stages fired in the current
    under-frequency event.
  - **The rule in `evaluate`,** on a Normal-mode tick with `dev < shed_below`:
    - `crossed = 1 + floor((shed_below − dev) / shed_stage_spacing)`, saturated to `u8`;
    - `new = crossed.saturating_sub(stages_fired)`, then `stages_fired = max(stages_fired, crossed)`;
    - shed rises by `shed_step × new` if `new > 0`, and by one `shed_step` otherwise (the slow
      stage);
    - the result is clamped to 1 and snapped.
  - **Re-arming.** `stages_fired` resets to 0 on every branch where `shed_ticks` resets at
    `dev ≥ −normal_band`. That covers the Restored branch and the in-band None branch.
  - The blackout conditions and the order of branches are unchanged.
  - A test-support helper, `fn step_deficit_run(d: f64, ramp: f64, p: &FrequencyParams) -> StepRun`
    inside `#[cfg(test)]`. It runs `next_deviation` + `evaluate` on a single bus: supply
    `1 − d`, demand 1, and supply following served load by at most `ramp` per tick without
    exceeding `1 − d`. It runs 240 ticks and returns `{ blackout, min_dev, max_dev, peak_shed }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn stages_crossed_in_one_tick_fire_together() { /* from 0 shed, dev −1.45 with spacing 0.2 → crossed 5 → shed 0.25 */ }
#[test] fn a_stage_fires_once_per_event() { /* dev −0.75 for 3 ticks → stages 0..=1 once (0.10), then slow stage +0.05 per tick */ }
#[test] fn stages_re_arm_at_normal_band() { /* event, recover to −0.05 → stages_fired 0; next dip fires stage 0 again */ }
#[test] fn stage_boundary_is_exclusive() { /* dev exactly shed_below − spacing crosses 1 stage, not 2 */ }
proptest! {
    #[test] fn staged_shedding_arrests_any_step_deficit(d in 0.001f64..0.299, ramp in 0.0f64..0.02) {
        // fixture params: !blackout, min_dev > blackout_below, peak_shed ≤ d + 3·shed_step + 1e-9, max_dev < trip_above
    }
}
#[test] fn a_sudden_quarter_of_supply_lost_does_not_black_out() { // power_system level
    // fixture fleet balanced at noon; trip plants totalling ≈ 25% of served load at once → no Blackout for the rest of the hour
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain protection power_system`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement** per Interfaces. Document the stage rule and its measured bounds (spec
  §15.4 builder's call) on `evaluate`.
- [ ] **Step 4: Run them to verify they pass.** Run the same command, plus the fast workspace suite
  from Global Constraints. Expected: PASS. Existing protection tests that assumed one step per tick
  at depth must change only where a dip now crosses extra stages; list each one you change.
- [ ] **Step 5: Commit**: `feat(domain): staged under-frequency load shedding`

---

### Task 3: Rotating outages in `PowerSystem` (§15.3)

**Files:**
- Modify: `crates/domain/src/grid/power_system.rs` (state, `begin_day`, `balance_boundary`, `tick`), `grid/protection.rs` (restoration floor), `grid/mod.rs` (`TickSummary`), `events.rs`
- Possibly create: `crates/domain/src/grid/power_system/outage.rs` (a sizing helper module, like `hydro_plan.rs`)
- Test: `crates/domain/src/grid/power_system/tests.rs`

**Interfaces:**
- Consumes: `agc_headroom`, `agc_ceilings`, `connected_demand_forecast`, `expected_solar`,
  `hydro_plan` (allocations), `plan_hydro`.
- Produces:
  - **State:** `PowerSystem.outage: Ratio` and `PowerSystem.outage_target: Ratio`.
  - **`fn size_outage(&self, region: &RegionDefinition, month: SimMonth, hour: u8) -> Ratio`**
    (pure).
    - It evaluates `share_m` at ticks `hour × 60` and `hour × 60 + 59`, and returns
      `round_up_to(shed_step, max(share_0, share_59))`, clamped to 0–1.
    - Deliverable supply is as listed in §15.3: online non-hydro plants ramp-limited to `m`, or
      at target if manual; scheduled hydro at `hourly[hour]`; manual hydro at its target limited
      by its budget; solar at `m`. Everything else counts 0.
    - Use a `1e-9` tolerance before rounding up, so an exact multiple doesn't gain a step (memory:
      float threshold tolerance).
  - **Call sites:**
    - In `begin_day`, after `plan_hydro(.., 0)`: `outage_target = size_outage(.., 0)` and
      `outage = outage_target`. `balance_boundary` then fills `forecast × (1 − outage) − solar − manual`.
    - In `tick`, at each hour boundary after `plan_hydro`, re-size, but only in Normal mode;
      during a blackout the target is frozen.
    - When the target changes, emit `DomainEvent::RotatingOutageOrdered { hour, share }`, and also
      at `begin_day` when hour 0's target is not 0. `begin_day` returns its events through
      `DayInitialisation` or the session's existing path; follow how boundary starts are surfaced.
  - **The tick's load.** In `tick`, `load = demand × (1 − outage)`. Use `load` in place of `demand`
    for the redispatch requirement, the imbalance, `next_deviation`'s system size and
    `evaluate`'s demand argument. The summary carries `demand` (full),
    `served = load × (1 − shed)`, `unserved = demand − served` and `outage`.
  - **Moving `outage`,** once per Normal-mode tick, after protection:
    - if `outage < outage_target`, add one `shed_step`;
    - if `outage > outage_target` and `agc_headroom(available, agc_ceilings(tick + 1), None) ≥ shed_step × demand`,
      subtract one `shed_step`;
    - otherwise hold.
    - In Blackout mode, `outage = 0`.
  - **Blackout entry** (the `Blackout` outcome) sets `outage = 0` and keeps `outage_target`.
  - **Restoration floor.** `evaluate` gains a parameter `restore_to: Ratio`, which is the frozen
    `outage_target`.
    - In Blackout mode, restoration completes, returning `BlackoutRestored`, when shed reaches
      `restore_to` (snapped) instead of 0.
    - `PowerSystem` then sets `outage = restore_to` and protection's `shed = 0`.
    - Every other caller passes `Ratio::ZERO`.
  - **`TickSummary.outage: Ratio`.**
  - **Hydro lift.** `hydro_cap_lifted` is unchanged: it already keys on protection's `shed` and the
    band. Add a doc line saying a rotating outage alone doesn't lift the cap (§15.3).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn covered_days_have_no_outage() { /* tiny fixture, normal day: outage_target 0 every hour; summaries identical in outage == 0 */ }
#[test] fn outage_sizes_to_the_worse_end_of_the_hour() { /* crafted fleet & rising hour: target == round_up(max(share_0, share_59)); hand-computed */ }
#[test] fn outage_target_is_a_minimal_multiple_of_the_shed_step() { /* exact-multiple shortfall gets no extra step (1e-9 tolerance) */ }
#[test] fn the_day_starts_at_hour_zeros_outage() { /* begin_day with a 50% shortfall: outage == target, boundary dispatch balanced against reduced load, RotatingOutageOrdered emitted */ }
#[test] fn a_noise_free_shortage_day_sheds_nothing_unplanned() { /* demand noise σ = 0 region variant, steep-hour shortage: protection shed == 0 every tick */ }
#[test] fn restoring_outage_load_waits_for_headroom() { /* target falls with slow fleet: outage steps down only on ticks where headroom ≥ shed_step × demand; no protection shed */ }
#[test] fn a_blackout_freezes_the_outage_and_restores_to_it() { /* force blackout mid-shortage: outage 0 while dark, target unchanged across an hour boundary, BlackoutRestored when shed reaches target, then outage == target and shed == 0 */ }
#[test] fn a_rotating_outage_alone_keeps_the_hydro_cap() { /* west-like fleet with shortage: shed == 0, outage > 0 → AGC hydro target never above its ceiling */ }
#[test] fn sizing_draws_no_rng() { /* RNG after begin_day + 120 ticks equals reference advanced by 1 + 120 draws, as in Plan 1a */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain power_system protection`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement** per Interfaces. Document the tick-order changes in `PowerSystem::tick`'s
  doc comment.
- [ ] **Step 4: Run them to verify they pass.** Run the same command, plus the fast workspace suite.
  Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): forecast-based rotating outages`

---

### Task 4: Outage accounting and incidents (§15.3, §15.6 report fields)

**Files:**
- Modify: `crates/domain/src/session/report.rs` (`MonthAccumulator`, `MonthlyReport`, `IncidentKind`), `session/month_close.rs`
- Test: inline report tests, `session/month_close/tests.rs`

**Interfaces:**
- Consumes: `TickSummary.outage` and `RotatingOutageOrdered` (Task 3).
- Produces:
  - **`MonthlyReport.rotating_outage: MegawattHours`:** the weighted Σ of `demand × outage` per
    tick, using the same `minute()` helper and month weight as `unserved`. It is part of
    `unserved`, never added to it.
  - **`MonthlyReport.peak_outage_share: Ratio`:** the month's largest ordered share.
  - **`IncidentKind::RotatingOutage { share: Ratio }`,** recorded when a non-zero target is ordered.
  - **Invariant:** add the check `rotating_outage ≤ unserved + 1e-6 × demand` to
    `GameSession::check_invariants`, or to the report's own consistency checks, whichever the
    codebase uses for `served + unserved = demand`.

- [ ] **Step 1: Write the failing tests:**
  - `rotating_outage_is_part_of_unserved`;
  - `peak_outage_share_is_the_largest_ordered`;
  - `an_ordered_outage_is_an_incident`;
  - `blackout_dark_load_is_not_counted_as_planned`, which drives a blackout on a shortage day and
    checks that the blackout ticks add to `unserved` but not to `rotating_outage`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain report month_close`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command, plus the fast workspace suite. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): rotating-outage accounting`

---

### Task 5: Capacity-shortfall warning and the pure hydro preview (§15.5)

**Files:**
- Modify: `crates/domain/src/grid/power_system/hydro_plan.rs` (factor out the pure plan computation), `grid/power_system.rs` (factor out the day-budget formula from `begin_day`), `session/views.rs`, `session/mod.rs` (re-export)
- Test: `crates/domain/src/session/tests.rs`, `grid/power_system/tests.rs`

**Interfaces:**
- Consumes: `hourly_net_load`, `water_fill`, the commitment patterns.
- Produces:
  - **`fn day_hydro_budget(region: &RegionDefinition, month: SimMonth, plant: &Plant) -> MegawattHours`**,
    the formula `begin_day` uses today (capacity × 24 × cell potential × monthly flow). `begin_day`
    calls it.
  - **`fn compute_hydro_plan(&self, region, month, from_hour: u8, budget: impl Fn(&Plant) -> MegawattHours) -> BTreeMap<PlantId, HydroFill>`**,
    which is pure. `plan_hydro` calls it with each plant's remaining budget and keeps its
    frozen-hour bookkeeping. The preview calls it from hour 0 with `day_hydro_budget`.
  - **`pub struct CapacityShortfallWarning { pub hour: u8, pub demand: Megawatts, pub deliverable: Megawatts, pub shortfall: Megawatts }`**
    and **`GameSession::capacity_shortfall_check(&self) -> Option<CapacityShortfallWarning>`**.
    - **Per hour,** sum over connected, commissioned plants committed in `h` by their pattern:
      - AGC non-hydro: capacity × availability. Use the same `available` resource rule AGC uses
        for that kind at minute 30 of `h`, so wind and other resource-limited kinds count their
        resource.
      - manual: `initial_target`;
      - scheduled hydro: the preview's `hourly[h]`.
    - Add expected solar at minute 30.
    - `demand` is the connected forecast at minute 30.
    - The result is the hour with the largest positive `demand − deliverable`, earliest on ties,
      or `None`. It is pure.
  - **No change to `hydro_schedule()`.** The preview is `pub(crate)` (§15.5).

- [ ] **Step 1: Write the failing tests:**
  - `capacity_shortfall_check_reports_the_worst_hour` (crafted fleet, hand-computed);
  - `capacity_shortfall_check_is_none_when_covered`;
  - `capacity_shortfall_check_counts_hydro_from_the_preview` (equals what `BeginOperating` then
    plans for hour 0);
  - `capacity_shortfall_check_is_pure` (`state_hash` unchanged);
  - `begin_day_budgets_are_unchanged_by_the_refactor` (an equality oracle against the old
    formula).
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain session power_system`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command, plus the fast workspace suite. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): capacity-shortfall warning with a pure hydro preview`

---

### Task 6: Lessons (§15.6)

**Files:**
- Modify: `crates/application/src/projections/lessons.rs`
- Test: inline lessons tests

**Interfaces:**
- Consumes: `MonthlyReport.{rotating_outage, peak_outage_share, unserved}` (Task 4).
- Produces:
  - **`Lesson::RotatingOutages { energy: MegawattHours, peak_share: Ratio }`.** It fires when
    `rotating_outage > 0`, with priority directly before `UnservedEnergy`.
  - **`Lesson::UnservedEnergy`** now fires only when `unserved − rotating_outage > 0` and reports
    that amount. `first_incident_tick` is the first incident that isn't a `RotatingOutage`, or 0.
  - The cap of three and the `[AllClear]` fallback are unchanged.

- [ ] **Step 1: Write the failing tests:**
  - `rotating_outages_lesson_fires_on_planned_energy`;
  - `unserved_lesson_counts_only_unplanned_energy`;
  - `an_outage_only_month_has_no_unserved_lesson`;
  - `lesson_priority_places_outages_before_unserved`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p application lessons`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command, plus the fast workspace suite. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(application): rotating-outage lesson; unserved lesson counts unplanned energy`

---

### Task 7: Overreach acceptance and shipped-region checks (§15.7, §15.8)

**Files:**
- Test: `crates/application/tests/scenarios.rs` (assertions only, no snapshot acceptance yet), `crates/infrastructure/tests/shipped_regions.rs`
- Modify region data only if an acceptance run fails, and say so (§15.7)

**Interfaces:**
- Consumes: everything above, the overreach scenario setup in `application/tests/support`, and the
  hands-off helper in `shipped_regions.rs`.

- [ ] **Step 1: Write the failing tests:**
  - `overreach_rotates_outages_instead_of_collapsing` (application). In month 4 of the overreach
    script, check that:
    - there is no `Blackout` incident;
    - every committed plant is online at the month close's last tick;
    - `reliability > 0`;
    - min deviation is above `blackout_below`;
    - `rotating_outage` lies within [Σ minute-by-minute forecast shortfall, that + one `shed_step`
      × demand per hour]. Compute the bounds in the test from `connected_demand_forecast` and the
      fleet's deliverable, minute by minute;
    - `Lesson::RotatingOutages` is in the month's lessons.
  - `hands_off_plan_survives_year_one`: the existing test must still pass. Re-run it, don't
    rewrite it.
  - `shipped_regions_never_order_outages_hands_off` (infrastructure): over the hands-off year,
    both regions and seeds 0–7, `rotating_outage == 0` every month. The shipped fleets cover their
    forecasts; if this fails, report the hours and shortfall before touching data.
- [ ] **Step 2: Run them.** Run: `cargo nextest run -p application -E 'binary(scenarios) and test(overreach)' && cargo nextest run -p infrastructure --test shipped_regions`. Expected: the overreach test PASSES once Tasks 2–6 are in, or fails with numbers. Record which. The snapshot assertion in the existing overreach test will mismatch; ignore it until Task 8.
- [ ] **Step 3: If an acceptance check fails,** diagnose first (hours, shortfall, which plants).
  Retune region data only, and list every change and its reason in the commit.
- [ ] **Step 4: Run the tests to verify they pass.** Run the same commands. Expected: PASS, apart from snapshot mismatches. Report the runtime of `shipped_regions`; it must stay under about 60 s in debug.
- [ ] **Step 5: Commit**: `test: overreach rotates outages; shipped regions need none hands-off`

---

### Task 8: Version bump, snapshots, docs and mutation pass

**Files:**
- Modify: `crates/domain/src/lib.rs` (`SIM_RULES_VERSION = 3`), `crates/application/tests/snapshots/*` (regenerated)
- Modify: `ARCHITECTURE.md`, `TESTING.md`, `docs/superpowers/plans/2026-10-18-plan-1-followups.md`
- Modify: `.cargo/mutants.toml`, only for justified new equivalences

**Interfaces:** none new.

- [ ] **Step 1: Bump the version.** Set `SIM_RULES_VERSION = 3`.
- [ ] **Step 2: Regenerate and review the snapshots.** Run `cargo insta test --workspace`, inspect
  every pending diff, then `cargo insta accept`. Each diff must be explained by A4:
  - `overreach_city_without_capacity` changes from a month-long blackout to a rotating outage with
    reliability above 0;
  - every snapshot gains `rotating_outage` and `peak_outage_share`;
  - a scenario with shedding may change because of the stages;
  - the guard's first line reads `SIM_RULES_VERSION=3`.

  Accept only explained diffs, and list them in the commit message.
- [ ] **Step 3: Update the docs.**
  - `ARCHITECTURE.md`: the tick order gains outage sizing after the hourly hydro re-plan, the
    `load = demand × (1 − outage)` substitution, and outage movement after protection. The
    protection step gains the stages.
  - `TESTING.md`: add the new properties, among them the step-deficit proptest and the overreach
    acceptance check.
  - Follow-ups doc: mark Q1 as **implemented (Plan 1b, A4)**.
- [ ] **Step 4: Run targeted mutation testing.** Run
  `cargo mutants -p domain -p application -F 'evaluate|size_outage|compute_hydro_plan|day_hydro_budget|capacity_shortfall_check|lessons_for|tick|begin_day|balance_boundary'`.
  Shard it so each run takes under 10 minutes, and run it in the background. Expected: 0 missed.
  Kill any survivors with tests.
- [ ] **Step 5: Run the full verification.** Run
  `cargo nextest run --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`.
  Expected: all pass.
- [ ] **Step 6: Commit**: `chore: SIM_RULES_VERSION 3 — regenerate snapshots for amendment A4`
