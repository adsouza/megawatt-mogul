# Megawatt Mogul Plan 1a: Spec Amendments A1–A3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the three spec amendments on top of the finished Plan 1 simulation core, before
Plan 2:
- A1: a hydro peak-shaving schedule;
- A2: a prorated partial-year rate review;
- A3: over-frequency trips, a minimum-generation warning, and frequency-performance strikes with a
  notice first.

**Architecture:** Everything stays inside the existing Clean Architecture layers.
- **Domain rules:** `grid` (schedule, AGC ceiling, protection arming), `regulation` (proration,
  frequency strikes and notices) and `session` (accounting, reports, views).
- **Read side:** `application` projections and lessons.
- **Data:** `infrastructure` DTOs and the shipped RON files.
- **Versioning:** one `SIM_RULES_VERSION` bump to 2 at the end, with every snapshot regenerated once
  and reviewed.

**Tech Stack:** unchanged from Plan 1 (Rust 1.98, edition 2024; no new crates).

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`, specifically §12 (A1), §13
(A2) and §14 (A3), plus the §2.5, §3.2, §5.4 and §6.4 pointers.

## Global Constraints

- Every Plan 1 Global Constraint still applies (`docs/superpowers/plans/2026-10-18-plan-1-simulation-core.md`):
  - dependency limits;
  - determinism (BTreeMap only, no wall clock, all randomness through `SimRng`, fixed RNG draws
    per tick and per day);
  - exact money (priced once per ledger entry at month close);
  - `Err` leaves state unchanged;
  - no player-facing strings in domain or application;
  - hermetic I/O tests;
  - TDD.
- **New behaviour must not draw from the RNG.** The hydro schedule, the minimum-generation check and
  the protection arming are all deterministic functions of state.
- **Unserved energy is shed load only,** and `imbalance` is signed (Ruling R16).
- **`SIM_RULES_VERSION` changes exactly once, from 1 to 2, in Task 9.** Until then, snapshot changes
  are expected and are *not* accepted task by task: each task verifies with
  `INSTA_UPDATE=no cargo nextest run --workspace -E 'not (binary(scenarios) | binary(rules_version_guard))'`
  plus its own tests.
  Task 9 regenerates and reviews every snapshot together.
- **New region fields** live only in region data (RON) and the test fixture, never as code
  constants. Defaults:
  - `frequency.trip_above = +1.5 Hz` and `frequency.trip_after_ticks = 5`;
  - `regulator.frequency_band = 0.2 Hz` and `regulator.frequency_violation_share = 0.05`.
- **When a smoke or acceptance run misses its target, retune region data, never the rules** (spec
  §12.5, §14.5).

## Review Focus

1. **A falling hydro allocation at an hour boundary.** Output must ramp down at the ramp rate, never
   drop in one tick (§12.4). Test in Task 2.
2. **Emergency override flapping.** The cap stays lifted for the whole restoration while `shed > 0`,
   and returns only when shed is 0 and the band isn't `Shedding`. Test in Task 2.
3. **A hydro plant with every hour uncommitted, or with no budget.** The schedule gives `λ = None`
   and all-zero allocations, with no panic and no division by zero. Test in Task 1.
4. **A month that fails both strike tests.** One strike, reason `UnservedEnergy`, and the notice is
   not used up. Test in Task 6.
5. **A game starting in April.** The first December review prorates the return by 9/12, and its
   tariff matches a January-start equivalent within one cent. Test in Task 3.

---

### Task 1: Water-filling allocation (pure function)

**Files:**
- Create: `crates/domain/src/generation/hydro_schedule.rs`; Modify: `generation/mod.rs` (re-export)
- Test: inline

**Interfaces:**
- Produces:
  - **`pub struct HydroFill { pub level: Option<Megawatts>, pub hourly: [Megawatts; 24] }`**.
  - **`pub fn water_fill(residual: &[Megawatts; 24], eligible: &[bool; 24], capacity: Megawatts, budget: MegawattHours) -> HydroFill`**.
    - Hours with `eligible[h] == false` get 0.
    - Otherwise allocation_h = `clamp(residual_h − λ, 0, capacity)`, choosing λ so that
      Σ allocation_h × 1 h = budget.
    - If even λ = min_eligible(residual) − capacity can't use the whole budget, allocate
      `min(capacity, max(0, residual_h))` and set `level = None` (the rest spills).
    - `level = None` also when the budget ≤ 0 or no hour is eligible.
- **λ by bisection,** with exactly 100 iterations between `lo = min_eligible(residual) − capacity`
  and `hi = max_eligible(residual)`. The iteration count is fixed so the result is deterministic.
  Don't rescale the result to hit the budget exactly. Accept a bisection residual of
  ≤ 1e-6 × budget, and assert it in tests.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn fills_the_peak_first() {
    // residual 1000 MW everywhere except hours 17..=20 at 1500; capacity 800; budget 1200 MWh
    // → λ = 1200; hours 17..=20 get 300 each, other hours 0
}
#[test] fn caps_at_capacity_and_spills_excess_water() {
    // capacity 100, budget 10_000 MWh, residual 500 everywhere → every eligible hour 100, level None
}
#[test] fn ineligible_hours_get_nothing() { /* eligible only 0..12 → hours 12..24 are 0 */ }
#[test] fn no_budget_or_no_eligible_hours_gives_no_level() { /* level None, all zero, no panic */ }
proptest! {
    #[test] fn conserves_water(res in prop::array::uniform24(0.0f64..5000.0), cap in 1.0f64..2000.0, budget in 0.0f64..40_000.0, mask in any::<[bool;24]>()) {
        // Σ alloc ≤ budget + 1e-6·budget, and Σ alloc ≈ min(budget, Σ_{eligible} min(cap, max(0,res_h))) within 1e-6·max(1,budget)
    }
    #[test] fn bounded(...) { /* 0 ≤ alloc_h ≤ cap; ineligible → 0 */ }
    #[test] fn water_filling_optimal(...) { /* eligible i,j: alloc_i < cap − ε && alloc_j > ε ⇒ res_i − alloc_i ≤ res_j − alloc_j + 1e-6 */ }
    #[test] fn monotone(...) { /* eligible i,j: res_i ≥ res_j ⇒ alloc_i ≥ alloc_j − 1e-9 */ }
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain hydro_schedule`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `water_fill`** as specified.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain hydro_schedule`. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): water-filling hydro allocation`

---

### Task 2: Hydro schedule in `PowerSystem` (A1 §12.3–§12.6)

**Files:**
- Modify: `crates/domain/src/grid/power_system.rs`, `crates/domain/src/session/views.rs`
- Test: `crates/domain/src/grid/power_system/tests.rs`, `session/tests.rs`

**Interfaces:**
- Consumes: `water_fill`, `HydroFill` (Task 1); `Plant::{commitment, auto_follow, hydro_budget, spec}`;
  `connected_demand_forecast`; `expected_solar`.
- Produces:
  - **`PowerSystem` state:** `hydro_plan: BTreeMap<PlantId, PlantHydroPlan>`, where
    `PlantHydroPlan { level: Option<Megawatts>, hourly: [Megawatts; 24], frozen: [Option<Megawatts>; 24] }`
    (`frozen[h]` records hour h's allocation when hour h began).
  - **`fn plan_hydro(&mut self, region: &RegionDefinition, month: SimMonth, from_hour: u8)`** (private).
    - Residual = forecast net load at minute 30 of each hour h ≥ `from_hour`: connected demand
      forecast minus expected solar, with hours before `from_hour` set to 0.
    - It walks scheduled plants in id order. Scheduled means connected, commissioned, hydro,
      `auto_follow`, with a budget > 0.
    - Each plant's `eligible[h]` is `h ≥ from_hour && commitment.is_committed(h)`.
    - It calls `water_fill(residual, eligible, capacity, remaining_budget)` and subtracts the
      allocation from the residual before the next plant.
  - **`pub(crate) fn hourly_net_load(&self, region: &RegionDefinition, month: SimMonth) -> [Megawatts; 24]`**
    returns connected demand forecast minus expected solar at minute 30 of each hour. Both
    `plan_hydro` and Task 7 use it.
  - **Call sites:** `plan_hydro(.., 0)` in `begin_day` (after pattern initialisation, before
    `balance_boundary`), and `plan_hydro(.., hour)` at every tick with `tick % 60 == 0 && tick > 0`.
    Do the hourly re-plan before the tick's availability step. When hour h begins, set `frozen[h]`.
  - **AGC ceiling (not physical):** in `retarget_agc`, a scheduled hydro plant's entry in the
    `available` map becomes `min(availability, hourly[current_hour])`.
  - **Ramp-down rule:** if a scheduled plant's output is above its allocation, set
    `target = max(allocation, output − ramp)` before AGC runs. `Plant::step` keeps only the budget
    limit.
  - **Emergency lift:** no ceiling and no ramp-down while `protection.shed > 0` or the frequency
    band is `Shedding`.
  - **Unplanned plants** (hydro with AGC on but no `hydro_plan` entry) are not capped.
  - **`balance_boundary`** caps scheduled hydro at `hourly[0]`.
  - **`pub fn hydro_schedule(&self) -> HydroScheduleView`** on `PowerSystem`, and
    **`GameSession::hydro_schedule()`** delegating to it, where
    `HydroScheduleView { plants: BTreeMap<PlantId, PlantHydroScheduleView { level: Option<Megawatts>, hourly: [Megawatts; 24], budget_remaining: MegawattHours }> }`.
    Hours before the current hour report `frozen[h]` (0 if `None`); later hours report the latest
    plan.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn hydro_is_spent_on_the_evening_peak() { /* West-like fixture: one hydro (budget < cap·24) + thermal, auto-follow all; after the day, hydro output during the peak net-load hour > 0 and its energy before 06:00 < energy during 17:00–21:00 */ }
#[test] fn falling_allocation_ramps_down_not_cliffs() { /* allocation 800 → 0 at an hour boundary: output decreases ≤ ramp per tick */ }
#[test] fn unused_water_is_replanned_at_the_next_hour() { /* force AGC to hold hydro below allocation for an hour; next hour's plan sums to the new remaining budget */ }
#[test] fn emergency_lifts_the_cap_for_the_whole_restoration() { /* induce shedding: hydro exceeds allocation while shed > 0, cap returns once shed == 0 and band != Shedding; no on/off alternation tick-to-tick while shed > 0 */ }
#[test] fn manual_and_unplanned_hydro_are_uncapped() { /* auto_follow false → no plan entry, follows target; plan entry missing mid-hour → no ceiling */ }
#[test] fn uncommitted_hours_get_zero_allocation() { /* commitment 6..=21 only */ }
#[test] fn hydro_schedule_view_freezes_elapsed_hours() { /* after hour 3 re-plans, view hours 0..3 equal what was planned when each began */ }
#[test] fn planning_draws_no_rng() { /* RNG after begin_day + 120 ticks == a reference SimRng advanced by 1 + 120 standard_normal draws */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain power_system`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement** per Interfaces. Document the cap-versus-physical distinction in a doc comment on `retarget_agc`.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain`, excluding the snapshot tests per Global Constraints. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): hydro peak-shaving schedule with ramp-aware AGC ceiling`

---

### Task 3: Prorated partial-year rate review (A2 §13)

**Files:**
- Modify: `crates/domain/src/regulation/mod.rs` (`YearTally`, `record_month`, `apply_rate_review`) and `regulation/rate_review.rs` (`compute_rate_review`)
- Test: inline, plus `session/month_close/tests.rs`

**Interfaces:**
- Produces:
  - **`YearTally.months: u8`.** `record_month` increments it (saturating at 12) and
    `apply_rate_review`'s reset zeroes it.
  - **The return term** in `compute_rate_review` becomes
    `round_ties_even(return × eligible_rate_base × months ÷ 12)`. When `months == 0` the term is 0.
    Nothing else changes.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn full_year_return_is_unchanged() { /* months 12 → identical RateReviewOutcome to the pre-change formula on the same inputs */ }
proptest! { #[test] fn return_term_is_linear_in_months(months in 1u8..=12, rb in 0i64..1e12 as i64) { /* RR(months) − (operating + depreciation) == round(return·rb·months/12) ± 1 cent */ } }
#[test] fn zero_months_earns_no_return() { /* no panic; return term 0 */ }
#[test] fn april_start_does_not_over_recover() { // session-level
    // two sessions with steady identical monthly figures: start April vs start January; first December tariff equal within one cent
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain regulation month_close`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): prorate the partial-year rate review return`

---

### Task 4: New region parameters and validation (A3 §14.2, §14.4, §14.5)

**Files:**
- Modify: `crates/domain/src/region/params.rs`, `region/mod.rs` (validation), `region/fixtures.rs`
- Modify: `crates/infrastructure/src/region/dto.rs` (`FrequencyDto`, `RegulatorDto`), `region/mapping.rs`
- Modify: `crates/infrastructure/tests/support/minimal_region.ron`, `assets/regions/eastern_interconnection.ron`, `assets/regions/western_interconnection.ron`
- Test: inline region tests, `infrastructure/tests/region_catalog.rs`

**Interfaces:**
- Produces:
  - `FrequencyParams { …, trip_above: Hertz, trip_after_ticks: u32 }`.
  - `RegulatorParams { …, frequency_band: Hertz, frequency_violation_share: Ratio }`.
  - DTO fields `trip_above_hz: f64`, `trip_after_ticks: u32`, `frequency_band_hz: f64`,
    `frequency_violation_share: f64`.
  - Validation (`InvalidBand`/`InvalidShape` with field names):
    - `frequency.trip_above > curtail_above`;
    - `frequency.trip_after_ticks ≥ 1`;
    - `regulator.frequency_band > 0` and finite.
  - All three RON files and the fixture use the Global Constraints defaults.

- [ ] **Step 1: Write the failing tests.** Extend `out_of_range_numbers_are_rejected` / `range_boundaries_are_accepted` with one negative and one boundary case per new field, and assert the fixture and both shipped regions load with the defaults.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain region && cargo nextest run -p infrastructure`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat: region parameters for over-frequency trips and frequency performance`

---

### Task 5: Over-frequency trips (A3 §14.2)

**Files:**
- Modify: `crates/domain/src/grid/protection.rs`, `grid/power_system.rs` (`apply_protection`), `events.rs`, `session/report.rs` (`IncidentKind`)
- Test: inline protection tests, `power_system/tests.rs`

**Interfaces:**
- Consumes: `FrequencyParams.trip_above` and `trip_after_ticks` (Task 4).
- Produces:
  - **`ProtectionState { …, over_ticks: u32, over_armed: bool }`** with the arming rule exactly as in
    spec §14.2:
    - not armed: count ticks with `dev > trip_above`, and reset the count at or below it;
    - arm on reaching `trip_after_ticks`;
    - while armed: count ticks with `dev > curtail_above`, and reset after each trip;
    - disarm at or below `curtail_above`.
  - **`ProtectionOutcome::OverFrequencyTrip`.** Normal mode only. It is evaluated after the existing
    Curtail logic, in the same call: the outcome is `OverFrequencyTrip` if the trip condition fires
    that tick, otherwise `Curtail` as today.
  - **`PowerSystem` trips** the connected, online, dispatchable plant with output > 0 and the highest
    marginal cost (ties broken by plant id): `trip()` plus `suppress_commitment_for_day()`. It emits
    `DomainEvent::OverFrequencyTrip { plant }`. When no plant qualifies, nothing happens.
  - **`IncidentKind::OverFrequencyTrip { plant: PlantId }`**, recorded by `MonthAccumulator`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn never_arms_at_or_below_trip_above() { /* 1_000 ticks at dev = trip_above → no trip */ }
#[test] fn arms_after_exactly_trip_after_ticks_then_trips_once_per_window() { /* dev 2.0 sustained: trips at ticks 5,10,15… while > curtail_above */ }
#[test] fn disarms_at_curtail_above() { /* armed, dev drops to 0.5 → disarmed, counters 0 */ }
#[test] fn trips_most_expensive_producer_and_never_solar() { /* peaker+ccgt+coal+solar online → peaker first, then ccgt; solar untouched */ }
#[test] fn overbuild_surplus_clears_and_stays_cleared() { /* fixture: extra committed CCGTs, all auto-follow, night surplus → deviation ≤ curtail_above within bounded ticks and for the rest of the day */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain protection power_system`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same command. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): over-frequency generator trips with arming`

---

### Task 6: Frequency-performance strikes with a notice first (A3 §14.4)

**Files:**
- Modify: `crates/domain/src/session/report.rs` (`MonthAccumulator`), `regulation/mod.rs`, `session/month_close.rs`, `events.rs`
- Modify: `crates/application/src/projections/milestones.rs` (adapt to the new event shape)
- Test: inline regulation tests, `month_close/tests.rs`, milestone tests

**Interfaces:**
- Consumes: `RegulatorParams.frequency_band` and `frequency_violation_share` (Task 4).
- Produces:
  - **`MonthAccumulator`** counts `normal_ticks` and `out_of_band_ticks`, the Normal-mode ticks with
    `|dev| > frequency_band`.
  - **`pub enum StrikeReason { UnservedEnergy, FrequencyPerformance }`.**
    `DomainEvent::StrikeIssued { strikes: u8, reason: StrikeReason }`, and the new
    `DomainEvent::FrequencyNoticeIssued { month: SimMonth }`.
  - **`Regulator.last_frequency_notice: Option<SimMonth>`.**
  - **`Regulator::assess_month(&mut self, month: SimMonth, demand: MegawattHours, unserved: MegawattHours, out_of_band: Ratio, p: &RegulatorParams) -> Vec<DomainEvent>`**
    replaces `assess_month_reliability`, and `close_month` calls it. Rules exactly as in spec §14.4:
    - **Unserved failure** (with or without a frequency failure): one strike, reason
      `UnservedEnergy`, and the notice is untouched.
    - **Frequency-only failure:**
      - if no notice in the last 12 months (`None`, or `months_since ≥ 12`): issue a notice, set
        `last_frequency_notice`, reset the clean streak, and add no strike;
      - otherwise: a strike with reason `FrequencyPerformance`.
    - **Clean month:** both tests pass. Expiry and revocation logic is otherwise unchanged.
  - **`out_of_band`** = `out_of_band_ticks / normal_ticks`, or 0 when `normal_ticks == 0`.
  - **`MonthlyReport.out_of_band_share: Ratio`** and **`MonthlyReport.frequency_notice: bool`.**

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn first_frequency_only_failure_draws_a_notice_not_a_strike() {}
#[test] fn second_frequency_failure_within_twelve_months_strikes() { /* reason FrequencyPerformance */ }
#[test] fn a_notice_is_due_again_after_twelve_months() {}
#[test] fn both_failing_gives_one_unserved_strike_and_keeps_the_notice() {}
#[test] fn notice_month_resets_the_clean_streak() {}
#[test] fn three_strikes_from_any_mix_revoke() {}
#[test] fn out_of_band_counts_normal_mode_ticks_only() { /* blackout ticks excluded */ }
#[test] fn full_season_milestone_ignores_notices() { /* application */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain regulation month_close report && cargo nextest run -p application milestones`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same commands. Expected: PASS.
- [ ] **Step 5: Commit**: `feat(domain): frequency-performance strikes with a notice first`

---

### Task 7: Minimum-generation warning and new lessons (A3 §14.3)

**Files:**
- Modify: `crates/domain/src/session/views.rs`, `session/report.rs` (`MonthlyReport`, accumulator), `session/month_close.rs`
- Modify: `crates/application/src/projections/lessons.rs`
- Test: `session/tests.rs`, inline lessons tests

**Interfaces:**
- Consumes: `PowerSystem::hourly_net_load` (Task 2).
- Produces:
  - **`pub struct MinimumGenerationWarning { pub hour: u8, pub committed_minimum: Megawatts, pub net_load: Megawatts, pub excess: Megawatts }`**
    and **`GameSession::minimum_generation_check(&self) -> Option<MinimumGenerationWarning>`**.
    - Per hour: sum over connected, commissioned, dispatchable plants committed in that hour. A plant
      with AGC on contributes `min_output`; a manual plant contributes `initial_target`.
    - Excess = that sum − net load. Report the hour with the largest positive excess, earliest on
      ties, or `None` if there is none.
    - Pure.
  - **`MonthlyReport.overgeneration: MegawattHours`** is the weighted Σ max(0, imbalance) per tick.
    **`MonthlyReport.over_frequency_trips: u32`** comes from incidents.
  - **New lessons** in priority order, inserted after `UnservedEnergy`:
    - `Lesson::Overgeneration { energy: MegawattHours, trips: u32 }`, when overgeneration > 2% of
      served energy or trips ≥ 1;
    - then `Lesson::FrequencyPerformance { out_of_band: Ratio, notice: bool }`, when the month failed
      frequency performance.

    The cap of three lessons and the `[AllClear]` fallback are unchanged.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn minimum_generation_check_reports_the_worst_hour() { /* crafted fleet: committed minimum 900 MW vs 03:00 net load 600 → hour 3, excess 300 */ }
#[test] fn minimum_generation_check_is_none_when_minimums_fit() {}
#[test] fn minimum_generation_check_counts_only_committed_hours_and_is_pure() { /* state_hash unchanged */ }
#[test] fn overgeneration_lesson_fires_on_surplus_or_trips() {}
#[test] fn frequency_lesson_reports_notice_flag() {}
#[test] fn lesson_priority_places_new_lessons_after_unserved() {}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain session && cargo nextest run -p application lessons`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them to verify they pass.** Run the same commands. Expected: PASS.
- [ ] **Step 5: Commit**: `feat: minimum-generation warning and overgeneration/frequency lessons`

---

### Task 8: Western hydrograph, late-year smoke run and beginner acceptance (A1 §12.5, A3 §14.5)

**Files:**
- Modify: `assets/regions/western_interconnection.ron` (all 12 `hydro.monthly_flow` values, with a
  comment describing the Pacific Northwest hydrograph). Modify region data further only if the
  acceptance check fails, and say so.
- Test: `crates/infrastructure/tests/shipped_regions.rs`

**Interfaces:**
- Consumes: everything above, and the hands-off helper already in `shipped_regions.rs` (commit every
  dispatchable plant all day, AGC on).
- **Flows (Jan..Dec):** about `[0.85, 0.85, 0.95, 1.15, 1.45, 1.55, 1.2, 0.85, 0.65, 0.65, 0.75, 0.8]`,
  a spring freshet in May and June with an autumn low. Small adjustments are allowed. Winter must not
  exceed 0.9.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn west_october_smoke_run_is_reliable() { /* start month overridden via parts/dto to October; reliability ≥ 0.99, Active */ }
#[test] fn west_january_hydro_has_no_evening_cliff() { /* realistic flows; hydro output > 0 throughout 20:00–21:00 whenever it began the day with water */ }
#[test] fn hands_off_plan_survives_year_one() {
    // both shipped regions × seeds 0..8: 12 months of the hands-off plan (no other commands) → status Active after month 12
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p infrastructure --test shipped_regions`. Expected: the January-cliff test fails with today's data, or passes only because the data is inflated. Record which.
- [ ] **Step 3: Re-author the flows,** then iterate on region data only until all three tests pass. Record every data change and its reason in the commit message.
- [ ] **Step 4: Run the tests to verify they pass.** Run: `cargo nextest run -p infrastructure --test shipped_regions`. Expected: PASS. Report the total runtime; it must stay under about 60 s in debug.
- [ ] **Step 5: Commit**: `feat(data): realistic Western hydrograph; late-year smoke and hands-off acceptance`

---

### Task 9: Version bump, snapshots, docs and mutation pass

**Files:**
- Modify: `crates/domain/src/lib.rs` (`SIM_RULES_VERSION = 2`), `crates/application/tests/snapshots/*` (regenerated)
- Modify: `ARCHITECTURE.md`, `TESTING.md` and `docs/superpowers/plans/2026-10-18-plan-1-followups.md`
- Modify: `.cargo/mutants.toml`, only for justified new equivalences

**Interfaces:** none new.

- [ ] **Step 1: Bump the version.** Set `SIM_RULES_VERSION = 2`.
- [ ] **Step 2: Regenerate and review the snapshots.** Run `cargo insta test --workspace --review`.
  For each changed snapshot, check that the diff is explained by A1, A2 or A3:
  - hydro shifts toward peak hours;
  - April-start returns are prorated;
  - the overbuild scenario now shows trips and frequency strikes or notices;
  - the guard's first line reads `SIM_RULES_VERSION=2`.

  Accept only explained diffs, and list them in the commit message.
- [ ] **Step 3: Update the docs.**
  - `ARCHITECTURE.md`: the tick order gains the hourly hydro re-plan and the over-frequency trip; the
    determinism notes are unchanged.
  - `TESTING.md`: add the new properties and the acceptance check.
  - Follow-ups doc: mark Q2 and Q3 and the first-year review as **implemented**.
- [ ] **Step 4: Run targeted mutation testing.** Run
  `cargo mutants -p domain -F 'water_fill|plan_hydro|retarget_agc|balance_boundary|evaluate|assess_month|minimum_generation_check|compute_rate_review' -j 4`,
  sharded so that each run takes under 10 minutes. Expected: 0 missed. Kill any survivors with
  tests.
- [ ] **Step 5: Run the full verification.** Run
  `cargo nextest run --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`.
  Expected: all pass.
- [ ] **Step 6: Commit**: `chore: SIM_RULES_VERSION 2 — regenerate snapshots for amendments A1–A3`
