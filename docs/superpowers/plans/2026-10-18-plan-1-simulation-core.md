# Megawatt Mogul Plan 1: Simulation Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the complete headless simulation of Milestone 1: the `domain`, `application` and
`infrastructure` crates, plus both shipped region files. A full game can then be played, replayed
and tested entirely through `cargo test`, with no Bevy.

**Architecture:** A Cargo workspace with the Clean Architecture layering
`application → domain` and `infrastructure → application + domain`. The domain is pure Rust (no
Bevy, no serde, no I/O). It owns all rules, including the fixed 12-step tick inside
`GameSession`. The application layer wraps it in a CQRS command pipeline (tracing →
invariant check → command log → core handler) with read-side projections fed by domain events.
Infrastructure parses RON region files into DTOs, rasterises them and maps them through the
domain's validating constructors.

**Tech Stack:** Rust 1.98 (edition 2024), rand_chacha 0.10, thiserror 2.0, tracing 0.1, serde 1.0,
ron 0.12, proptest 1.12, proptest-state-machine 0.9, insta 1.49, tempfile 3.27, cargo-nextest 0.9,
cargo-mutants 27.

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`

**Plan series:** This is Plan 1 of 3. Plan 2 (Bevy game shell: app states, `SimulationPlugin`, egui
HUD, Fluent localisation) and Plan 3 (diorama world view, overlays, camera, dev tools) will each
build on this plan's public API.

## Global Constraints

- Workspace: `resolver = "3"`, `edition = "2024"`, `rust-version = "1.98"`,
  `license = "AGPL-3.0-only"`. Every crate sets `#![forbid(unsafe_code)]`.
- `domain` dependencies are `rand_chacha 0.10` and `thiserror 2.0` only. It has no Bevy, no serde
  and no I/O. Its dev-dependencies are `proptest 1.12` and `insta 1.49`.
- `application` dependencies are `domain`, `thiserror`, `tracing 0.1`. Its dev-dependencies are
  `proptest`, `proptest-state-machine 0.9`, `insta`, and `domain` with the `test-fixtures` feature.
- `infrastructure` dependencies are `application`, `domain`, `serde 1.0` (derive), `ron 0.12`,
  `thiserror`, `tracing`. Its dev-dependencies are `tempfile 3.27`, `proptest`, and `domain` with
  `test-fixtures`.
- **Time:** 1 tick = 1 game minute; `TICKS_PER_HOUR = 60`; `TICKS_PER_DAY = 1_440`. One played day
  is the representative day of a calendar month, and a game year is 12 played days.
- **Money:** `Money` is `i64` cents. Nothing is priced per tick. Physical totals are priced once per
  ledger entry at month close: `price × energy × days_in_month`, rounded once with
  `f64::round_ties_even` to whole cents.
- **Determinism:** use `BTreeMap`/`BTreeSet` (never iterate a `HashMap`), no wall-clock time, all
  randomness through `SimRng`, a single-threaded tick, and tie-breaks by ID.
- **Domain errors:** the domain never panics on player input; it returns `DomainError`. A command
  that returns `Err` leaves the session unchanged. Panics are only for bugs (invariant violations).
- **No user-facing strings** in `domain` or `application`: only typed values and `name_key`
  translation keys.
- **Plant numbers live only in data:** the plant tuning values from spec §3.2 appear only in region
  RON files and the `test-fixtures` fixture, never as code constants.
- **Test isolation:** every I/O test uses its own `tempfile::TempDir`. Tests never use process-global
  mutable state, environment variables or current-directory changes. Reading shipped `assets/` by
  absolute path from `CARGO_MANIFEST_DIR` is allowed (read-only).
- **Process:** TDD for every task (failing test → minimal code → pass → commit). Commit messages
  use a conventional prefix (`feat:`, `test:`, `docs:`, `chore:`) and end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **`AdvanceTicks { n }` with `n` larger than the ticks left in the day.** It must stop at month
   close (phase becomes `Review`) and never run on into the next month. The test lives in Task 17.
2. **A rate review with zero served energy** (a blackout all year, or no connected cities). This
   must not divide by zero; the tariff stays at its previous value. The test lives in Task 14.
3. **Commands after game over** (bankrupt or licence revoked). They return
   `DomainError::GameOver`, never panic, and leave `state_hash` unchanged. The test lives in Task 16.
4. **A commitment whose start-up time exceeds the time left before the committed hour** (e.g.
   nuclear committed from hour 10, with a 48 h start-up). The plant starts immediately and comes
   online late; this is not an error. The test lives in Task 7.
5. **Building on an invalid cell** (out of bounds, water, too little cooling water, no hydro site).
   This returns `InvalidSite` with a typed `SiteError` reason. The test lives in Task 15.

## Clarifications of the spec made by this plan

- **`overgeneration`:** the spec's conservation rule (*generation = served + curtailed*) is extended
  to *generation = served + curtailed + overgeneration*. Overgeneration is surplus that raises the
  frequency, and it is never paid for.
- **Blackout restoration:** in `GridMode::Blackout`, frequency is held at nominal while load
  reconnects in `shed_step` increments, but only while supply covers the next step. This avoids an
  endless blackout loop and models real black-start restoration.
- **Milestones are read-side:** they are tracked by `MilestoneProjection`, not emitted as domain
  events. They are game meta, not grid rules.
- **Ledger = cash only:** depreciation and disallowance are non-cash adjustments to the rate base.
  `CapitalDisallowed` is an event, not a ledger entry.
- **Plant keys:** plants are keyed by `PlantKind` (one spec per kind per region), so the spec's
  `PlantSpecId` becomes `PlantKind`.
- **Lines need a book life too:** `LineParams.book_life_months` is region data.
- **Stopping a committed plant:** `StopPlant` on a committed plant clears its commitment for the
  rest of the day, so automatic commitment never fights the player.
- **Mutation-testing skips** are `exclude_re` entries with justification comments in
  `.cargo/mutants.toml`. A `mutants` attribute crate would break the domain's dependency limits.

---

### Task 1: Workspace scaffold, tooling and project docs

**Files:**
- Create: `Cargo.toml`, `crates/domain/Cargo.toml`, `crates/domain/src/lib.rs`,
  `crates/application/Cargo.toml`, `crates/application/src/lib.rs`,
  `crates/infrastructure/Cargo.toml`, `crates/infrastructure/src/lib.rs`
- Create: `.config/nextest.toml`, `.github/workflows/ci.yml`
- Create: `ARCHITECTURE.md`, `TESTING.md`, `AGENTS.md`; Modify: `README.md`
- Test: `crates/domain/tests/dependency_rule.rs`

**Interfaces:**
- Produces: the workspace layout and the `[workspace.dependencies]` table that every later task
  uses with `{ workspace = true }`. Also the domain feature `test-fixtures = []`.

- [ ] **Step 1: Install the test tooling**

Run: `cargo install --locked cargo-nextest cargo-mutants cargo-insta`
Expected: `cargo nextest --version` prints 0.9.x, `cargo mutants --version` prints 27.x, and
`cargo insta --version` prints 1.x.

- [ ] **Step 2: Write the failing dependency-rule test**

```rust
// crates/domain/tests/dependency_rule.rs
#[test]
fn domain_manifest_has_no_framework_or_io_dependencies() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let deps = manifest.split("[dependencies]").nth(1).unwrap().split("\n[").next().unwrap();
    for forbidden in ["bevy", "serde", "ron", "tokio", "tracing"] {
        assert!(!deps.contains(forbidden), "domain must not depend on {forbidden}");
    }
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo nextest run -p domain`
Expected: FAIL, because the workspace and crate don't exist yet.

- [ ] **Step 4: Create the workspace**

- Root `Cargo.toml`: `members = ["crates/domain", "crates/application", "crates/infrastructure"]`.
- `[workspace.package]` holds the values from Global Constraints.
- `[workspace.dependencies]` pins every crate in Tech Stack, plus `domain = { path = "crates/domain" }`
  and `application = { path = "crates/application" }`.
- `[workspace.lints.rust] unsafe_code = "forbid"`.
- `[workspace.lints.clippy] all = { level = "warn", priority = -1 }`.
- Each crate uses `[lints] workspace = true`.
- `.config/nextest.toml`: a `default` profile with `fail-fast = false`, and a `ci` profile with
  `retries = 0`.
- `ci.yml` runs on push and pull request, using `dtolnay/rust-toolchain@master` with
  `toolchain: "1.98.1"` and the components `clippy, rustfmt`, plus `taiki-e/install-action@v2`
  with `tool: cargo-nextest`. Its steps are `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo nextest run --workspace --profile ci`.

- [ ] **Step 5: Write the four docs**

- **`README.md`**: pitch (from spec §1); the three goals; the two regions; status ("Plan 1:
  headless simulation core; the Bevy client is coming in Plans 2–3"); build and test commands
  (`cargo nextest run --workspace`, `cargo mutants -p domain -p application`); a link to the spec;
  licence: AGPL-3.0.
- **`ARCHITECTURE.md`**:
  - The layer diagram and dependency rule (spec §4.1–4.2).
  - A "where things go" table: value objects → `domain::units`; game rules → domain modules;
    use cases and projections → `application`; file formats → `infrastructure`.
  - The command pipeline diagram (spec §6.2).
  - Write side vs. read side (CQRS) and domain events.
  - Command sourcing (seed + command log, not event sourcing, and why).
  - The determinism rules (copied from Global Constraints).
  - The 12-step tick order (spec §6.4).
  - The settings seams (`NetworkModel`, `SimulationFidelity`).
  - A link to the spec.
- **`TESTING.md`**: the test-layer table (spec §10.1); how to run each layer; the property catalogue
  (spec §10.2); snapshot workflow (`cargo insta review`); mutation testing commands and the target;
  the isolation rules (spec §10.5); a "writing a new test" checklist.
- **`AGENTS.md`**, at most about 60 lines, under these headings:
  - *Project*: two lines.
  - *Commands*: build, test, clippy, fmt, mutants.
  - *Architecture rules*: the dependency rule; no strings in the domain; determinism rules;
    `Err` leaves state unchanged.
  - *Workflow*: TDD; specs in `docs/superpowers/specs/` and plans in `docs/superpowers/plans/`;
    conventional commits.
  - *Testing rules*: isolation; property tests for invariants.
  - *Don'ts*: no `HashMap` iteration in the domain, no `unwrap` on player input, no Bevy outside
    `game`.

- [ ] **Step 6: Run the verification**

Run: `cargo nextest run --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 1 test passes, with no clippy warnings and no fmt diff.

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "chore: scaffold workspace, CI and project docs"
```

---

### Task 2: Value objects (`domain::units`) and `DomainError`

**Files:**
- Create: `crates/domain/src/units.rs`, `crates/domain/src/error.rs`; Modify: `crates/domain/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in `units.rs`

**Interfaces:**
- Produces, all `#[derive(Debug, Clone, Copy, PartialEq)]`, with `PartialOrd` on the float types and
  `Eq, Ord, Hash` on `Money`:
  - **`Megawatts`**: `new(f64)` (`debug_assert!` that the value is finite), `ZERO`,
    `value() -> f64`. Implements `Add, Sub, Neg, AddAssign, SubAssign, Mul<f64>`,
    `Div<Megawatts, Output = f64>` and `Sum`. Also `min, max, clamp(lo, hi), abs`, and
    `energy_over_ticks(ticks: u32) -> MegawattHours` (mw × ticks / 60).
  - **`MegawattHours`**: `new, ZERO, value`, `Add, Sub, AddAssign, Mul<f64>, Sum`.
  - **`Hertz`**: `new, ZERO, value`, `Add, Sub, Neg`.
  - **`Money`**: `from_cents(i64)`, `from_dollars(i64)`, `ZERO`, `cents() -> i64`, `is_negative()`,
    `Add, Sub, Neg, AddAssign, SubAssign, Mul<i64>, Sum`, and
    `split_even(self, parts: u32) -> Vec<Money>` (equal parts with the remainder on the last part;
    panics if `parts == 0`).
  - **`Price`** ($/MWh): `per_mwh(f64)`, `value()`, and
    `cost_of(self, energy: MegawattHours, weight: u32) -> Money`, which computes
    `round_ties_even(price × energy × weight × 100)` cents. This is the single rounding step.
  - **`RampRate`**: `mw_per_minute(f64)`, `per_tick() -> Megawatts`, and
    `over_ticks(ticks: u32) -> Megawatts`.
  - **`Ratio`**: `new(f64) -> Result<Ratio, DomainError>` (accepts `0.0..=1.0` and rejects NaN),
    `ZERO`, `ONE`, `value()`, `of(self, Megawatts) -> Megawatts`.
  - **`DomainError`** (`thiserror::Error`, `Debug, Clone, PartialEq`) with the first variant
    `InvalidRatio { value: f64 }`. Later tasks add more variants.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn money_from_dollars_is_cents() { assert_eq!(Money::from_dollars(3).cents(), 300); }
#[test] fn cost_rounds_half_even_once() {
    let one = MegawattHours::new(1.0);
    assert_eq!(Price::per_mwh(0.125).cost_of(one, 1), Money::from_cents(12));
    assert_eq!(Price::per_mwh(0.375).cost_of(one, 1), Money::from_cents(38));
}
#[test] fn cost_weights_before_rounding() {
    // per-day rounding would give 0 × 31 = 0; weighting first gives 3.1 → 3 cents
    assert_eq!(Price::per_mwh(0.001).cost_of(MegawattHours::new(1.0), 31), Money::from_cents(3));
}
#[test] fn energy_over_ticks_is_minutes() {
    assert_eq!(Megawatts::new(120.0).energy_over_ticks(30), MegawattHours::new(60.0));
}
#[test] fn ramp_per_tick_equals_per_minute() {
    assert_eq!(RampRate::mw_per_minute(5.0).per_tick(), Megawatts::new(5.0));
}
proptest! {
    #[test] fn split_even_sums_exactly(cents in 0i64..1_000_000_000_000, parts in 1u32..=120) {
        let split = Money::from_cents(cents).split_even(parts);
        prop_assert_eq!(split.len() as u32, parts);
        prop_assert_eq!(split.iter().copied().sum::<Money>(), Money::from_cents(cents));
        prop_assert!(split[..split.len() - 1].iter().all(|p| *p == split[0]));
    }
    #[test] fn ratio_accepts_exactly_unit_interval(v in -10.0f64..10.0) {
        prop_assert_eq!(Ratio::new(v).is_ok(), (0.0..=1.0).contains(&v));
    }
}
#[test] fn ratio_rejects_nan() { assert!(Ratio::new(f64::NAN).is_err()); }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo nextest run -p domain units`
Expected: FAIL to compile (`Money` not found).

- [ ] **Step 3: Implement `units.rs` and `error.rs`** with the signatures above. Use `f64::round_ties_even` and cast the result to `i64`.

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo nextest run -p domain units`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**: `git commit -am "feat(domain): value objects with exact money"`

---

### Task 3: Calendar, clock and IDs

**Files:**
- Create: `crates/domain/src/time.rs`, `crates/domain/src/ids.rs`; Modify: `lib.rs`, `error.rs`
- Test: inline in `time.rs` and `ids.rs`

**Interfaces:**
- Produces:
  - `pub const TICKS_PER_HOUR: u32 = 60; pub const TICKS_PER_DAY: u32 = 1_440;`
  - **`SimDuration`**: `from_ticks(u32), from_minutes(u32), from_hours(u32), ticks() -> u32`.
  - **`enum Season { Winter, Spring, Summer, Autumn }`**: December–February is winter,
    March–May spring, June–August summer, September–November autumn.
  - **`SimMonth { year: i32, month: u8 }`** (`Copy, Ord, Hash`):
    `new(year, month) -> Result<SimMonth, DomainError>`, `year(), month(), next()`,
    `days_in_month() -> u32` (Gregorian), `season()`, `index0() -> usize`,
    `months_since(self, earlier: SimMonth) -> i32`.
  - **`SimClock`**: `new(SimMonth)`, `month()`, `tick() -> u32`, `hour() -> u8`,
    `advance(&mut self) -> bool` (true when the tick reaches `TICKS_PER_DAY`), `next_month(&mut self)`
    (moves to the next month and sets tick to 0).
  - **`PlantId(u32)`, `CityId(u32)`, `NodeId(u32)`, `LineId(u32)`** (`Copy, Ord, Hash`, each with
    `From<u32>` and `.0`); **`RegionId(String)`** with `new(&str)` and `as_str()`;
    **`CellCoord { x: u16, y: u16 }`** (`Copy, Ord, Hash`).
  - **`IdAllocator { next: u32 }`**: `Default`, plus `fn next<T: From<u32>>(&mut self) -> T`. One
    shared counter serves every ID type.
  - New `DomainError::InvalidMonth { month: u8 }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn leap_years_follow_gregorian_rules() {
    let feb = |y| SimMonth::new(y, 2).unwrap().days_in_month();
    assert_eq!((feb(2028), feb(2100), feb(2000), feb(2027)), (29, 28, 29, 28));
    assert_eq!(SimMonth::new(2027, 4).unwrap().days_in_month(), 30);
}
#[test] fn december_rolls_into_january() {
    assert_eq!(SimMonth::new(2027, 12).unwrap().next(), SimMonth::new(2028, 1).unwrap());
}
#[test] fn seasons_follow_meteorological_months() {
    let s = |m| SimMonth::new(2027, m).unwrap().season();
    assert_eq!([s(12), s(2), s(3), s(6), s(9)], [Season::Winter, Season::Winter, Season::Spring, Season::Summer, Season::Autumn]);
}
#[test] fn rejects_month_13() { assert_eq!(SimMonth::new(2027, 13), Err(DomainError::InvalidMonth { month: 13 })); }
#[test] fn months_since_counts_across_years() {
    assert_eq!(SimMonth::new(2029, 1).unwrap().months_since(SimMonth::new(2027, 3).unwrap()), 22);
}
#[test] fn clock_reports_day_end_on_tick_1440() {
    let mut c = SimClock::new(SimMonth::new(2027, 1).unwrap());
    let ends: Vec<bool> = (0..TICKS_PER_DAY).map(|_| c.advance()).collect();
    assert!(ends[..1439].iter().all(|e| !e) && ends[1439]);
}
proptest! { #[test] fn year_has_365_or_366_days(y in 1900i32..2400) {
    let total: u32 = (1..=12).map(|m| SimMonth::new(y, m).unwrap().days_in_month()).sum();
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    prop_assert_eq!(total, if leap { 366 } else { 365 });
}}
#[test] fn allocator_is_shared_and_monotonic() {
    let mut a = IdAllocator::default();
    let p: PlantId = a.next(); let n: NodeId = a.next();
    assert_eq!((p.0, n.0), (0, 1));
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain time ids`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `time.rs` and `ids.rs`** with the signatures above.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain`. Expected: PASS.
- [ ] **Step 5: Commit**: `git commit -am "feat(domain): calendar, clock and ids"`

---

### Task 4: Deterministic RNG (`SimRng`)

**Files:**
- Create: `crates/domain/src/rng.rs`; Modify: `lib.rs`, `crates/domain/Cargo.toml` (add `rand_chacha`)
- Test: inline, plus the snapshot `crates/domain/src/snapshots/`

**Interfaces:**
- Produces **`SimRng`**, a wrapper around `rand_chacha::ChaCha8Rng` that implements `Debug`, `Clone`
  and `PartialEq` (by comparing the word position and seed). Its methods: `from_seed(u64)`,
  `next_u64()`, `next_f64() -> f64` in `[0, 1)` (`(next_u64 >> 11) as f64 * 2^-53`),
  `standard_normal() -> f64` (Box–Muller from two uniforms, with `u1` mapped to `(0, 1]`), and
  `normal(mean, sd) -> f64`.
- Use only the `rand_core` that `rand_chacha` re-exports (`rand_chacha::rand_core`), with its
  `SeedableRng::seed_from_u64` and its u64 generation method. Do **not** add the `rand` crate:
  distribution algorithms can change between `rand` versions, which would silently change saved
  games.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn same_seed_same_stream() {
    let (mut a, mut b) = (SimRng::from_seed(7), SimRng::from_seed(7));
    assert!((0..5).all(|_| a.next_u64() == b.next_u64()));
}
#[test] fn different_seeds_differ() { assert_ne!(SimRng::from_seed(1).next_u64(), SimRng::from_seed(2).next_u64()); }
#[test] fn uniform_in_unit_interval() {
    let mut r = SimRng::from_seed(3);
    assert!((0..10_000).all(|_| { let x = r.next_f64(); (0.0..1.0).contains(&x) }));
}
#[test] fn standard_normal_moments() {
    let mut r = SimRng::from_seed(4);
    let xs: Vec<f64> = (0..20_000).map(|_| r.standard_normal()).collect();
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let sd = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
    assert!(mean.abs() < 0.05 && (sd - 1.0).abs() < 0.05);
}
#[test] fn stream_is_stable_across_dependency_upgrades() {
    let mut r = SimRng::from_seed(42);
    insta::assert_debug_snapshot!([r.next_u64(), r.next_u64(), r.next_u64()]);
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain rng`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `rng.rs`.**
- [ ] **Step 4: Run them, accept the snapshot, then re-run.** Run: `cargo insta test -p domain --accept && cargo nextest run -p domain rng`. Expected: PASS, with the snapshot file created.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): deterministic SimRng"`

---

### Task 5: Region model and validation (`domain::region`)

**Files:**
- Create: `crates/domain/src/region/{mod.rs, plant_spec.rs, terrain.rs, params.rs, fixtures.rs}`
- Modify: `lib.rs`; `Cargo.toml` (`[features] test-fixtures = []`)
- Test: inline in `region/mod.rs`

**Interfaces:**
- Produces, all with public fields unless stated otherwise:
  - **`enum PlantKind { Nuclear, Coal, Ccgt, Peaker, Solar, Hydro }`**, with `ALL: [PlantKind; 6]`
    and `is_dispatchable()` (false only for `Solar`).
  - **`enum SitingRule { Anywhere, CoolingWater { min: Ratio }, HydroSite { min: Ratio } }`**.
  - **`PlantSpec`**:
    `{ kind, capacity: Megawatts, min_stable: Ratio, ramp: RampRate, startup: SimDuration, startup_cost: Money, marginal_cost: Price, fixed_om_per_month: Money, capital_cost: Money, build_months: u32, capacity_credit: Ratio, book_life_months: u32, siting: SitingRule }`,
    with the method `min_output() -> Megawatts`.
  - **`Cell`**: `{ land: bool, height: Ratio, irradiance: Ratio, cooling_water: Ratio, hydro_potential: Ratio }`.
  - **`TerrainGrid`** (private fields):
    `new(width: u16, height: u16, cells: Vec<Cell>) -> Result<_, RegionError>`, plus `get(CellCoord) -> Option<&Cell>`, `width()`, `height()`. Cells are stored row-major (`y * width + x`).
  - **`CityDef`**: `{ id: CityId, name_key: String, cell: CellCoord, base_load: Megawatts }`.
  - **`DemandShape`**:
    `{ winter: [f64; 24], summer: [f64; 24], shoulder: [f64; 24], monthly: [f64; 12], forecast_error: Ratio, error_persistence: f64 }`.
    Spring and autumn use `shoulder`.
  - **`SolarParams`**: `{ daylight: [(f64, f64); 12], clearness_mean: [f64; 12], clearness_sd: f64 }`,
    where `daylight` holds the sunrise and sunset hour.
  - **`HydroParams`**: `{ monthly_flow: [f64; 12] }`.
  - **`FrequencyParams`**:
    `{ nominal: Hertz, k_inertia: f64, k_damping: f64, normal_band: Hertz, shed_below: Hertz, blackout_below: Hertz, blackout_after_shed_ticks: u32, curtail_above: Hertz, shed_step: Ratio }`.
  - **`RegulatorParams`**:
    `{ starting_tariff: Price, reference_tariff: Price, base_return: Ratio, min_return: Ratio, max_return: Ratio, reliability_target: Ratio, return_per_reliability: f64, return_per_cost_improvement: f64, reserve_margin: Ratio, strike_threshold: Ratio, strikes_to_revoke: u8, clean_months_to_expire: u8 }`.
  - **`SatisfactionParams`**:
    `{ starting: f64, reliability_floor: Ratio, price_sensitivity: f64, smoothing: Ratio }`.
  - **`LineParams`**: `{ cost_per_cell: Money, cells_per_month: u32, book_life_months: u32 }`.
  - **`StartPlantDef { kind, cell }`** and
    **`StartDef { month: SimMonth, cash: Money, substation: CellCoord, connected_cities: Vec<CityId>, fleet: Vec<StartPlantDef> }`**.
  - **`RegionDefinitionParts`**:
    `{ id: RegionId, name_key: String, terrain, catalog: Vec<PlantSpec>, cities: Vec<CityDef>, demand, solar, hydro, frequency, regulator, satisfaction, lines, start }`.
  - **`RegionDefinition`** (private fields), with `impl TryFrom<RegionDefinitionParts, Error = RegionError>`.
    Getters for every part, plus `spec(kind) -> &PlantSpec`, `city(id) -> Option<&CityDef>`, and
    `site_check(kind, cell) -> Result<(), SiteError>`.
  - **`enum SiteError { OutOfBounds, NotLand, InsufficientCoolingWater { have: Ratio, need: Ratio }, NoHydroSite }`**.
  - **`enum RegionError`**: `GridSizeMismatch { expected: usize, actual: usize }`,
    `MissingPlantSpec(PlantKind)`, `DuplicatePlantSpec(PlantKind)`, `CityOutOfBounds(CityId)`,
    `CityNotOnLand(CityId)`, `DuplicateCity(CityId)`, `UnknownStartCity(CityId)`,
    `InvalidStartSite { kind, cell, reason: SiteError }`, `SubstationNotOnLand`,
    `InvalidShape { field: &'static str }` (non-finite or non-positive demand, solar or hydro
    factors), `InvalidBand { field: &'static str }` (`min_return > max_return`, or `base_return`
    outside it).
  - **`fixtures::tiny_region_parts() -> RegionDefinitionParts`** and
    **`fixtures::tiny_region() -> RegionDefinition`**, compiled under
    `#[cfg(any(test, feature = "test-fixtures"))]`.
- **Fixture values:**
  - `id` is `RegionId::new("tiny")` and `name_key` is `"region-tiny"`.
  - A 12 × 8 grid. Column `x = 0` is water and every other cell is land. Cells with `x ≥ 6` have
    `cooling_water = 1.0` (all others 0.2). Cell (10, 1) has `hydro_potential = 0.8` (all others 0).
    Every cell has `irradiance = 0.8` and `height = 0.3`.
  - The catalog uses spec §3.2 exactly. Fixed O&M per month: nuclear $11,000,000, coal $2,000,000,
    CCGT $600,000, peaker $170,000, solar $500,000, hydro $2,700,000. Siting: nuclear and coal
    `CoolingWater { min: 0.5 }`, hydro `HydroSite { min: 0.5 }`, the rest `Anywhere`.
  - Cities: id 1 at (6, 4) with 600 MW, id 2 at (8, 6) with 400 MW, id 3 at (3, 2) with 300 MW.
  - Start: month 2027-04, cash $2,000,000,000, substation at (7, 4), connected cities [1, 2], fleet
    of coal at (7, 3), CCGT at (8, 4) and peaker at (6, 5).
  - Demand: `shoulder = [0.70,0.66,0.64,0.63,0.64,0.68,0.76,0.85,0.90,0.92,0.93,0.94,0.94,0.94,0.95,0.96,0.98,1.00,0.99,0.96,0.91,0.85,0.78,0.73]`.
    `summer` is `shoulder` × 1.15 for hours 10–19 (otherwise unchanged). `winter` is `shoulder` ×
    1.1 for hours 6–9 and 17–21 (otherwise unchanged).
    `monthly = [1.10,1.05,0.95,0.90,0.95,1.05,1.15,1.15,1.00,0.92,0.98,1.08]`,
    `forecast_error = 0.03`, `error_persistence = 0.995`.
  - Solar: `daylight` runs from (7.5, 17.0) in January up to (5.5, 20.5) in June and back
    symmetrically. `clearness_mean` = 0.75 for every month; `clearness_sd = 0.15`.
  - Hydro: `monthly_flow = [0.6,0.6,0.9,1.2,1.4,1.3,1.0,0.8,0.7,0.6,0.6,0.6]`.
  - Frequency: nominal 60 Hz, `k_inertia 5.0`, `k_damping 0.2`, `normal_band 0.05`,
    `shed_below −0.5`, `blackout_below −1.5`, `blackout_after_shed_ticks 30`, `curtail_above 0.5`,
    `shed_step 0.05`.
  - Regulator: starting tariff $95, reference tariff $100, returns 0.09 (min 0.07, max 0.11),
    `reliability_target 0.999`, `return_per_reliability 2.0`, `return_per_cost_improvement 0.05`,
    `reserve_margin 0.20`, `strike_threshold 0.005`, `strikes_to_revoke 3`,
    `clean_months_to_expire 3`.
  - Satisfaction: `starting 70.0`, `reliability_floor 0.95`, `price_sensitivity 2.0`,
    `smoothing 0.2`.
  - Lines: $40,000,000 per cell, 2 cells per month, book life 600 months.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn tiny_region_is_valid() { assert!(RegionDefinition::try_from(fixtures::tiny_region_parts()).is_ok()); }
#[test] fn rejects_each_invalid_part() {
    let cases: Vec<(fn(&mut RegionDefinitionParts), RegionError)> = vec![
        (|p| { p.catalog.pop(); }, RegionError::MissingPlantSpec(PlantKind::Hydro)),
        (|p| { let c = p.catalog[0].clone(); p.catalog.push(c); }, RegionError::DuplicatePlantSpec(PlantKind::Nuclear)),
        (|p| p.cities[0].cell = CellCoord { x: 0, y: 4 }, RegionError::CityNotOnLand(CityId(1))),
        (|p| p.cities[0].cell = CellCoord { x: 50, y: 4 }, RegionError::CityOutOfBounds(CityId(1))),
        (|p| p.start.connected_cities.push(CityId(99)), RegionError::UnknownStartCity(CityId(99))),
        (|p| p.start.substation = CellCoord { x: 0, y: 0 }, RegionError::SubstationNotOnLand),
        (|p| p.demand.monthly[3] = 0.0, RegionError::InvalidShape { field: "demand.monthly" }),
        (|p| p.regulator.min_return = Ratio::new(0.2).unwrap(), RegionError::InvalidBand { field: "regulator.return" }),
    ];
    for (mutate, expected) in cases {
        let mut parts = fixtures::tiny_region_parts(); mutate(&mut parts);
        assert_eq!(RegionDefinition::try_from(parts).unwrap_err(), expected);
    }
}
#[test] fn site_check_reasons() {
    let r = fixtures::tiny_region();
    assert_eq!(r.site_check(PlantKind::Peaker, CellCoord { x: 0, y: 1 }), Err(SiteError::NotLand));
    assert_eq!(r.site_check(PlantKind::Peaker, CellCoord { x: 40, y: 1 }), Err(SiteError::OutOfBounds));
    assert!(matches!(r.site_check(PlantKind::Nuclear, CellCoord { x: 2, y: 1 }), Err(SiteError::InsufficientCoolingWater { .. })));
    assert_eq!(r.site_check(PlantKind::Hydro, CellCoord { x: 9, y: 1 }), Err(SiteError::NoHydroSite));
    assert_eq!(r.site_check(PlantKind::Hydro, CellCoord { x: 10, y: 1 }), Ok(()));
}
#[test] fn invalid_start_fleet_site_is_rejected() {
    let mut p = fixtures::tiny_region_parts();
    p.start.fleet.push(StartPlantDef { kind: PlantKind::Hydro, cell: CellCoord { x: 2, y: 2 } });
    assert!(matches!(RegionDefinition::try_from(p), Err(RegionError::InvalidStartSite { reason: SiteError::NoHydroSite, .. })));
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain region`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the region module.** Validation runs in this order: grid → catalog → cities → start → shapes → bands. Report the first error found.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain region`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): validated region definition and test fixture"`

---

### Task 6: Plant lifecycle and ramping (`domain::generation`)

**Files:**
- Create: `crates/domain/src/generation/{mod.rs, plant.rs}`, `crates/domain/src/events.rs`
- Modify: `lib.rs`, `error.rs`
- Test: inline in `plant.rs`

**Interfaces:**
- Consumes: units, ids, `PlantSpec`, `PlantKind`, `SimMonth`.
- Produces:
  - **`enum PlantStatus`**: `UnderConstruction { months_left: u32 }`, `Offline`,
    `Starting { ticks_left: u32 }`, `Online { output: Megawatts, target: Megawatts }`,
    `Stopping { output: Megawatts }`.
  - **`CommitmentSchedule([bool; 24])`**, defined here (rules come in Task 7): `new([bool; 24])`,
    `none()`, `is_committed(hour: u8)`, `hours() -> [bool; 24]`, `clear_from(hour: u8)`.
  - **`Plant`**, with these methods:
    - `new_under_construction(id, node, cell, spec)` and
      `new_commissioned(id, node, cell, spec, month)`. A commissioned solar plant starts
      `Online { 0, 0 }`; every other kind starts `Offline`.
    - Getters: `id(), node(), cell(), kind(), spec(), status(), output()`, `is_online()`,
      `auto_follow()`, `commitment()`, `commitment_mut()`, `commissioned_on()`,
      `hydro_budget() -> Option<MegawattHours>`.
    - `set_auto_follow(on) -> Result<(), DomainError>`.
    - `start() -> Result<Vec<DomainEvent>, DomainError>` and `stop() -> Result<Vec<DomainEvent>, DomainError>`.
    - `set_target(mw) -> Result<(), DomainError>`.
    - `step(available: Megawatts) -> Vec<DomainEvent>`.
    - `headroom_up(available) -> Megawatts` and `headroom_down() -> Megawatts`, both ramp-limited
      and zero unless the plant is online and dispatchable.
    - `trip() -> Vec<DomainEvent>`, `force_to_minimum()`.
    - `reset_hydro_budget(MegawattHours)` and `hydro_available() -> Megawatts` (the remaining
      budget × 60 MW, capped at capacity).
    - `advance_construction(month: SimMonth) -> Vec<DomainEvent>`.
  - **`enum DomainEvent`** (`Debug, Clone, PartialEq`), with first variants
    `PlantStarting { plant }`, `PlantOnline { plant }`, `PlantStopped { plant }`,
    `PlantTripped { plant }`, `PlantCommissioned { plant, kind }` and
    `HydroBudgetExhausted { plant }`.
  - New `DomainError` variants: `PlantNotDispatchable { plant }`, `PlantNotOffline { plant }`,
    `PlantNotOnline { plant }`, `PlantNotRunning { plant }`,
    `BelowMinimumStableOutput { plant, min: Megawatts }`, `AboveCapacity { plant, max: Megawatts }`.
- **Rules:**
  - `start`: `Offline → Starting { startup.ticks() }`. A start-up of 0 goes straight to
    `Online { min_output, min_output }` and also emits `PlantOnline`.
  - `step`:
    - Starting plants count down; at 0 they become `Online { min_output, min_output }` and emit
      `PlantOnline`.
    - Online dispatchable plants move output toward `min(target, available)` by at most `ramp`.
    - Solar sets output = `available` with no ramp limit.
    - Stopping plants fall by `ramp`; at ≤ 0 they become `Offline` and emit `PlantStopped`.
    - Hydro deducts `output.energy_over_ticks(1)` from its budget, emitting `HydroBudgetExhausted`
      only on the transition to zero.
  - `set_target` is only valid `Online`, and requires `min_output ≤ mw ≤ capacity`.

- [ ] **Step 1: Write the failing tests**

```rust
// helper: fn plant(kind) -> Plant { Plant::new_commissioned(PlantId(1), NodeId(1), CellCoord{x:7,y:3}, tiny_region().spec(kind).clone(), april()) }
#[test] fn coal_takes_its_startup_ticks() {
    let mut p = plant(PlantKind::Coal); p.start().unwrap();
    for _ in 0..479 { p.step(Megawatts::new(600.0)); }
    assert!(matches!(p.status(), PlantStatus::Starting { ticks_left: 1 }));
    assert_eq!(p.step(Megawatts::new(600.0)), vec![DomainEvent::PlantOnline { plant: PlantId(1) }]);
    assert_eq!(p.output(), Megawatts::new(240.0));
}
#[test] fn peaker_ramps_40_mw_per_tick() {
    let mut p = online(PlantKind::Peaker); p.set_target(Megawatts::new(200.0)).unwrap();
    let outs: Vec<f64> = (0..4).map(|_| { p.step(Megawatts::new(200.0)); p.output().value() }).collect();
    assert_eq!(outs, vec![80.0, 120.0, 160.0, 200.0]);
}
#[test] fn nuclear_target_below_minimum_is_rejected() {
    let mut p = online(PlantKind::Nuclear);
    assert_eq!(p.set_target(Megawatts::new(700.0)), Err(DomainError::BelowMinimumStableOutput { plant: PlantId(1), min: Megawatts::new(770.0) }));
}
#[test] fn solar_cannot_be_started() { assert!(matches!(plant(PlantKind::Solar).start(), Err(DomainError::PlantNotDispatchable { .. }))); }
#[test] fn stopping_ramps_down_then_goes_offline() { /* online CCGT at 200 MW, stop(); 7 steps at 25 MW → 25 MW left; 8th step → Offline + PlantStopped */ }
#[test] fn hydro_never_exceeds_budget_and_reports_once() {
    let mut p = online(PlantKind::Hydro); p.reset_hydro_budget(MegawattHours::new(10.0)); p.set_target(Megawatts::new(800.0)).unwrap();
    let events: Vec<_> = (0..120).flat_map(|_| { let a = p.hydro_available(); p.step(a) }).collect();
    assert_eq!(events.iter().filter(|e| matches!(e, DomainEvent::HydroBudgetExhausted { .. })).count(), 1);
    assert_eq!(p.hydro_budget(), Some(MegawattHours::ZERO));
}
#[test] fn construction_commissions_after_build_months() { /* 2-month spec: advance twice → Offline, commissioned_on == second month, PlantCommissioned emitted once */ }
proptest! { #[test] fn online_output_respects_ramp_and_bounds(targets in prop::collection::vec(0.0f64..=500.0, 1..200)) {
    let mut p = online(PlantKind::Ccgt);
    for t in targets { let before = p.output(); let _ = p.set_target(Megawatts::new(t)); p.step(Megawatts::new(500.0));
        prop_assert!((p.output() - before).abs().value() <= 25.0 + 1e-9);
        prop_assert!(p.output().value() >= 200.0 - 1e-9 && p.output().value() <= 500.0 + 1e-9); }
}}
```

`online(kind)` is a test helper that starts the plant and steps it until `is_online()`.

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain generation`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `plant.rs` and `events.rs`** per the rules above.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain generation`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): plant lifecycle and ramping"`

---

### Task 7: Automatic commitment actions

**Files:**
- Create: `crates/domain/src/generation/commitment.rs` (move `CommitmentSchedule` here from Task 6); Modify: `generation/mod.rs`
- Test: inline

**Interfaces:**
- Produces **`enum CommitmentAction { Start(PlantId), Stop(PlantId) }`** and
  **`fn commitment_actions<'a>(plants: impl IntoIterator<Item = &'a Plant>, tick: u32) -> Vec<CommitmentAction>`**,
  ordered by `PlantId`.
- **Rules:**
  - **Start:** the plant is `Offline` and has a committed hour `h ≥ current hour` with
    `h × 60 − startup_ticks ≤ tick`. If the lead time has already passed, it starts immediately.
  - **Stop:** only at an hour boundary (`tick % 60 == 0`, `tick > 0`), when hour `h − 1` is committed,
    hour `h` is not, and the plant is `Online` or `Starting`.
  - Plants with no commitment are never touched.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn ccgt_starts_exactly_at_lead_time() { // committed hours 6..=17, startup 2 h
    let p = offline_with(PlantKind::Ccgt, 6..=17);
    assert!(commitment_actions([&p], 239).is_empty());
    assert_eq!(commitment_actions([&p], 240), vec![CommitmentAction::Start(p.id())]);
}
#[test] fn late_commitment_starts_immediately() { // nuclear committed 10..=23, startup 48 h
    let p = offline_with(PlantKind::Nuclear, 10..=23);
    assert_eq!(commitment_actions([&p], 0), vec![CommitmentAction::Start(p.id())]);
}
#[test] fn stops_at_first_uncommitted_hour_boundary() {
    let p = online_with(PlantKind::Ccgt, 6..=17);
    assert!(commitment_actions([&p], 1079).is_empty());
    assert_eq!(commitment_actions([&p], 1080), vec![CommitmentAction::Stop(p.id())]);
}
#[test] fn uncommitted_manual_plant_is_never_touched() {
    let p = online_with(PlantKind::Peaker, 0..0);
    assert!((0..1440).all(|t| commitment_actions([&p], t).is_empty()));
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain commitment`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `commitment_actions`.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain commitment`. Expected: PASS.
- [ ] **Step 5: Commit**: `git commit -am "feat(domain): automatic commitment actions"`

---

### Task 8: Demand, forecast and solar availability

**Files:**
- Create: `crates/domain/src/demand/{mod.rs, forecast.rs}`, `crates/domain/src/generation/solar.rs`; Modify: `lib.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **`DemandCenter`**: `new(def: CityDef, node: NodeId)`, `id()`, `node()`, `def()`.
  - **`fn shape_factor(shape: &DemandShape, month: SimMonth, tick: u32) -> f64`**: picks the season's
    array, interpolates linearly between hour `h` and `h + 1` (hour 23 wraps to hour 0) at
    `(tick % 60) / 60`, then multiplies by `monthly[month.index0()]`.
  - **`DemandNoise { error: f64 }`**: `Default`, plus
    `step(&mut self, shape: &DemandShape, rng: &mut SimRng) -> f64`. This is AR(1):
    `e ← φe + σ√(1 − φ²)·N(0, 1)`, with `φ = error_persistence` and `σ = forecast_error`.
  - **`fn forecast_demand<'a>(cities: impl IntoIterator<Item = &'a DemandCenter>, shape, month, tick) -> Megawatts`**
    (no noise).
  - **`ForecastPoint { hour: u8, mean: Megawatts, low: Megawatts, high: Megawatts }`** and
    **`fn hourly_forecast<'a>(cities, shape, month) -> [ForecastPoint; 24]`**: the mean is taken at
    minute 30 of each hour, and `low`/`high` = mean × (1 ∓ 2σ).
  - **`fn sun_factor(p: &SolarParams, month, tick) -> f64`**: `sin(π·(t − rise)/(set − rise))` in
    daylight, otherwise 0. Here `t` is in hours.
  - **`fn draw_clearness(p: &SolarParams, month, rng) -> f64`**:
    `clamp(normal(mean[m], sd), 0.2, 1.0)`.
  - **`fn solar_available(capacity: Megawatts, irradiance: Ratio, sun: f64, clearness: f64) -> Megawatts`**.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn shape_at_hour_start_is_table_value() {
    let r = tiny_region(); let apr = SimMonth::new(2027, 4).unwrap();
    assert!((shape_factor(r.demand(), apr, 0) - 0.70 * 0.90).abs() < 1e-12);
}
#[test] fn shape_interpolates_half_hour() { /* tick 30 → (0.70 + 0.66)/2 × 0.90 */ }
#[test] fn shape_wraps_hour_23_to_0() { /* tick 1410 → (0.73 + 0.70)/2 × 0.90 */ }
#[test] fn ar1_noise_is_stationary_with_sigma() {
    let r = tiny_region(); let mut n = DemandNoise::default(); let mut rng = SimRng::from_seed(9);
    let xs: Vec<f64> = (0..200_000).map(|_| n.step(r.demand(), &mut rng)).collect();
    let sd = (xs.iter().map(|x| x * x).sum::<f64>() / xs.len() as f64).sqrt();
    assert!((sd - 0.03).abs() < 0.003);
}
#[test] fn forecast_band_brackets_mean() { /* every point: low < mean < high */ }
#[test] fn sun_is_zero_at_night_and_one_at_solar_noon() { /* April daylight from fixture: tick at rise−1h → 0; midpoint → 1.0 ± 1e-9 */ }
proptest! { #[test] fn clearness_is_clamped(seed in any::<u64>()) {
    let c = draw_clearness(tiny_region().solar(), SimMonth::new(2027, 6).unwrap(), &mut SimRng::from_seed(seed));
    prop_assert!((0.2..=1.0).contains(&c));
}}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain demand solar`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the functions above.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain demand solar`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): demand profiles, forecast and solar availability"`

---

### Task 9: Network topology and connectivity

**Files:**
- Create: `crates/domain/src/grid/{mod.rs, network.rs}`; Modify: `lib.rs`, `error.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **`enum NodeKind { Substation, Plant(PlantId), City(CityId) }`** and **`Node { kind, cell }`**.
  - **`enum LineStatus { UnderConstruction { months_left: u32 }, InService }`**.
  - **`Line { id, from: NodeId, to: NodeId, status, length_cells: f64 }`**.
  - **`Network`** (private fields), with methods:
    - `new(root: NodeId, root_cell: CellCoord)` and `add_node(id, node)`.
    - `add_line(id, from, to, status) -> Result<(), DomainError>`.
    - `advance_construction() -> Vec<LineId>`, which returns the newly commissioned lines and
      recomputes connectivity.
    - `is_connected(node)`, `connected_nodes() -> &BTreeSet<NodeId>`, `node(id)`, `lines()`.
  - **`fn distance_cells(a: CellCoord, b: CellCoord) -> f64`** (Euclidean).
  - New `DomainError` variants: `UnknownNode { node }`, `LineToSelf { node }`,
    `DuplicateLine { from, to }`. A duplicate is the same pair in either direction, whatever its
    status.
- **Connectivity** is a BFS from the root over `InService` lines, recomputed on every change.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn root_is_connected_alone() { /* new network: connected == {root} */ }
#[test] fn line_under_construction_does_not_connect() { /* add node + line months_left 2 → not connected; advance twice → connected, returned LineId */ }
#[test] fn connectivity_is_transitive() { /* root–A–B in service → B connected */ }
#[test] fn duplicate_reverse_line_rejected() { /* add A→B then B→A → DuplicateLine */ }
#[test] fn self_and_unknown_lines_rejected() { /* LineToSelf, UnknownNode */ }
proptest! { #[test] fn connectivity_matches_union_find(edges in prop::collection::vec((0u32..12, 0u32..12), 0..30)) {
    // build network with nodes 0..12 (root 0), add each non-self, non-duplicate edge InService,
    // compare connected_nodes() with a naive union-find reference
}}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain network`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `network.rs`.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain network`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): network topology and connectivity"`

---

### Task 10: Merit order and AGC

**Files:**
- Create: `crates/domain/src/grid/dispatch.rs`; Modify: `grid/mod.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **`fn merit_order<'a>(plants: impl IntoIterator<Item = &'a Plant>) -> Vec<PlantId>`**: online
    dispatchable plants sorted by `(marginal_cost, id)`.
  - **`fn agc_retarget(plants: &mut [&mut Plant], available: &BTreeMap<PlantId, Megawatts>, shortfall: Megawatts) -> Megawatts`**,
    which returns the part of the shortfall it could not allocate (keeping its sign).
- **Algorithm:**
  - It considers only plants that are `auto_follow`, online and dispatchable.
  - If `shortfall > 0`, it walks those plants in ascending `(marginal_cost, id)` order. Each gets
    `alloc = min(remaining, headroom_up(available[id]))`, and its target is set to `output + alloc`.
  - If `shortfall < 0`, it walks them in descending order, using `headroom_down()`, and sets each
    target to `output − alloc`.
  - Plants that receive no allocation are held, with target = output.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn merit_order_sorts_by_cost_then_id_and_skips_solar_and_offline() { /* nuclear, coal, ccgt, peaker online + solar + offline ccgt → [nuclear, coal, ccgt, peaker] ids */ }
#[test] fn raises_cheapest_first_within_ramp() { /* CCGT (25/tick) + peaker (40/tick) both at min, shortfall 50 → ccgt target +25, peaker +25; returns 0 */ }
#[test] fn returns_unallocated_when_headroom_exhausted() { /* shortfall 1000, total headroom 65 → returns 935 */ }
proptest! { #[test] fn never_raises_expensive_while_cheaper_has_headroom(
    specs in prop::collection::vec((1.0f64..300.0, 0.0f64..1.0), 1..8), shortfall in -500.0f64..500.0) {
    // build auto-follow online plants with given marginal costs and output fraction;
    // after retarget compute alloc_i = |target_i − output_i| and headroom_i;
    // raising: for any i,j with cost_i < cost_j: alloc_j > 1e-9 ⇒ headroom_i − alloc_i ≤ 1e-9; lowering: mirrored;
    // and Σ alloc = min(|shortfall|, Σ headroom) ± 1e-6
}}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain dispatch`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `dispatch.rs`.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain dispatch`. Expected: PASS.
- [ ] **Step 5: Commit**: `git commit -am "feat(domain): merit order and ramp-limited AGC"`

---

### Task 11: Frequency and protection

**Files:**
- Create: `crates/domain/src/grid/{frequency.rs, protection.rs}`; Modify: `grid/mod.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **`fn next_deviation(dev: Hertz, imbalance: Megawatts, system_size: Megawatts, p: &FrequencyParams) -> Hertz`**:
    `dev + k_inertia × imbalance / max(system_size, 1 MW) − k_damping × dev`.
  - **`enum FrequencyBand { Normal, Low, Shedding, High }`** and **`fn band(dev, p) -> FrequencyBand`**:
    - `High` above `curtail_above`;
    - `Shedding` below `shed_below`;
    - `Low` below `−normal_band`;
    - otherwise `Normal`.
  - **`enum GridMode { Normal, Blackout }`** and **`ProtectionState { mode, shed: Ratio, shed_ticks: u32 }`**
    (`Default` is Normal with 0 shed).
  - **`enum ProtectionOutcome { None, Shed, Restored, Curtail, Blackout, BlackoutRestored }`**.
  - **`fn evaluate(state: &mut ProtectionState, dev: Hertz, p: &FrequencyParams, supply: Megawatts, demand: Megawatts) -> ProtectionOutcome`**.
- **Rules in Normal mode:**
  - When `dev < shed_below`: `shed = min(1, shed + step)` and `shed_ticks += 1`. If additionally
    `dev < blackout_below` or `shed_ticks ≥ blackout_after_shed_ticks`, switch to Blackout with
    `shed = 1` and return `Blackout`; otherwise return `Shed`.
  - When `dev ≥ −normal_band` and `shed > 0`: `shed −= step` (floored at 0), `shed_ticks = 0`, and
    return `Restored`.
  - When `dev > curtail_above`: return `Curtail`.
- **Rules in Blackout mode:** if `supply ≥ demand × (1 − shed + step)`, then `shed −= step`. When
  shed reaches 0, switch back to Normal and return `BlackoutRestored`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn deviation_converges_to_inertia_over_damping() {
    let p = tiny_region().frequency().clone(); let mut d = Hertz::ZERO;
    for _ in 0..500 { d = next_deviation(d, Megawatts::new(-20.0), Megawatts::new(1000.0), &p); }
    assert!((d.value() - (5.0 * -0.02 / 0.2)).abs() < 1e-6);
}
#[test] fn sheds_in_steps_below_threshold() { /* dev −0.6 twice → shed 0.10, Shed */ }
#[test] fn blacks_out_after_sustained_shedding() { /* 30 consecutive Shed evaluations → 30th returns Blackout, shed 1 */ }
#[test] fn blacks_out_immediately_below_blackout_threshold() { /* dev −1.6 → Blackout */ }
#[test] fn restoration_needs_supply() { /* Blackout, supply 0 → shed stays 1; supply ≥ demand → 20 calls restore to Normal with BlackoutRestored on the last */ }
#[test] fn curtails_above_threshold() { /* dev +0.6 → Curtail */ }
proptest! { #[test] fn shed_stays_in_unit_interval(devs in prop::collection::vec(-3.0f64..3.0, 1..300), supply in 0.0f64..2000.0) { /* shed ∈ [0,1] after each */ } }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain frequency protection`. Expected: FAIL to compile.
- [ ] **Step 3: Implement both modules.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain frequency protection`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): frequency model and protection with black-start restoration"`

---

### Task 12: The `PowerSystem` aggregate tick

**Files:**
- Create: `crates/domain/src/grid/{power_system.rs, summary.rs}`; Modify: `grid/mod.rs`, `events.rs`
- Test: inline in `power_system.rs`

**Interfaces:**
- Consumes: Tasks 6–11.
- Produces:
  - **`TickSummary`**:
    `{ tick: u32, demand, served, unserved, generation, curtailed, overgeneration: Megawatts, by_kind: BTreeMap<PlantKind, Megawatts>, by_plant: BTreeMap<PlantId, Megawatts>, frequency: Hertz, mode: GridMode, shed: Ratio }`.
    `frequency` is the absolute value (nominal + deviation).
  - **`PowerSystem`**:
    - `new(network: Network, plants: BTreeMap<PlantId, Plant>, cities: BTreeMap<CityId, DemandCenter>, model: NetworkModel)`.
    - `begin_day(&mut self, region: &RegionDefinition, month: SimMonth, rng: &mut SimRng)`. This
      resets each commissioned hydro plant's budget to
      `capacity × 24 h × hydro_potential(cell) × monthly_flow[m]` and draws the day's clearness.
    - `tick(&mut self, region: &RegionDefinition, month: SimMonth, tick: u32, rng: &mut SimRng) -> (TickSummary, Vec<DomainEvent>)`.
    - Accessors: `plants()`, `plant(id)`, `plant_mut(id)`, `insert_plant(Plant)`, `cities()`,
      `network()`, `network_mut()`, `deviation()`, `protection()`.
    - `connected_demand_forecast(region, month, tick) -> Megawatts`.
    - `expected_solar(region, month, tick) -> Megawatts` (uses the month's mean clearness).
    - `ramp_capability_up(within: SimDuration) -> Megawatts`. Connected online dispatchable plants
      contribute `min(ramp.over_ticks(w), capacity − output)`. Starting plants with
      `ticks_left ≤ w` contribute `min_output + ramp.over_ticks(w − ticks_left)`, capped at
      capacity.
    - `firm_capacity() -> Megawatts`: Σ capacity × capacity_credit over commissioned plants.
  - **`enum NetworkModel { CopperPlate }`**, defined here and re-exported by `session` in Task 15.
  - New `DomainEvent` variants: `TickCompleted(TickSummary)`, `FrequencyDeviated { band }`
    (emitted only when the band changes), `LoadShed { mw }`, `SolarCurtailed { mw }`, `Blackout`,
    `BlackoutRestored`.
- **Order inside `tick`** (spec steps 2–10; only connected plants and cities count):
  1. Demand = Σ `base_load × shape_factor × (1 + noise.step)`.
  2. Solar and hydro availability.
  3. `step` every plant.
  4. Apply `commitment_actions`.
  5. `agc_retarget(shortfall = demand × (1 − shed) − supply)`.
  6. Imbalance.
  7. `next_deviation` (held at 0 in Blackout mode).
  8. `evaluate`:
     - `Curtail` reduces solar output by `min(solar, supply − demand × (1 − shed))` and calls
       `force_to_minimum` on AGC plants.
     - `Blackout` trips every plant (emitting `PlantTripped` for each) and resets the deviation.
  9. `served = min(supply − curtailed, demand × (1 − shed))`, `unserved = demand − served`, and
     `overgeneration = supply − curtailed − served`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn ample_auto_follow_capacity_serves_all_demand() { /* tiny fleet all online + auto_follow, April, 1440 ticks → Σ unserved == 0 */ }
#[test] fn disconnected_plant_contributes_nothing() { /* add online peaker on a node with no line → absent from by_plant, generation unchanged */ }
#[test] fn no_supply_leads_to_blackout_and_trips_plants() { /* all plants offline, tick until Blackout emitted; every plant Offline */ }
#[test] fn hydro_budget_uses_cell_potential_and_month_flow() { /* hydro at (10,1), April: 800×24×0.8×1.2 = 18_432 MWh */ }
proptest! { #[test] fn energy_is_conserved_every_tick(seed in any::<u64>(), targets in prop::collection::vec(0.0f64..1.0, 3)) {
    // tiny fleet online at random fractions; 200 ticks; each summary:
    // |generation − (served + curtailed + overgeneration)| < 1e-6, unserved ≥ −1e-9, served ≤ demand + 1e-9
}}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain power_system`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `PowerSystem`** in the order above.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain`. Expected: PASS (all domain tests).
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): PowerSystem aggregate tick"`

---

### Task 13: Company, ledger, rate base and satisfaction

**Files:**
- Create: `crates/domain/src/company/{mod.rs, ledger.rs, rate_base.rs, satisfaction.rs}`; Modify: `lib.rs`, `events.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **`enum LedgerCategory { Revenue, Fuel, StartUp, FixedOm, Construction }`** and
    **`enum AssetRef { Plant(PlantId), Line(LineId) }`**.
  - **`LedgerEntry { month, category, amount: Money, subject: Option<AssetRef> }`**. Revenue is
    positive and costs are negative.
  - **`Company`**:
    - `new(starting_cash: Money, satisfaction: f64)`.
    - `record(&mut self, entry) -> DomainEvent`, which returns `LedgerEntryRecorded(entry)`.
    - `cash()`, `starting_cash()`, `ledger()`, `ledger_sum() -> Money`, `rate_base()`,
      `rate_base_mut()`, `satisfaction()`.
    - `push_reliability(served: MegawattHours, demand: MegawattHours)`, which keeps the last 3
      months, and `reliability() -> Ratio` (1 when there is no demand).
    - `update_satisfaction(&mut self, reliability: Ratio, tariff: Price, regulator: &RegulatorParams, params: &SatisfactionParams)`:
      `s += α × (target − s)`.
    - `close_solvency_month(&mut self) -> bool`: true once cash has been negative at 3 consecutive
      closes. The counter resets when cash is non-negative.
  - **`fn satisfaction_target(reliability: Ratio, tariff: Price, reference: Price, reliability_target: Ratio, p: &SatisfactionParams) -> f64`**
    = `100 × rel × price`, where:
    - `rel = clamp((r − floor) / (target − floor), 0, 1)`;
    - `price = clamp(1 − sensitivity × max(0, tariff/reference − 1), 0, 1)`.
  - **`RateBaseAsset { asset, original: Money, remaining: Money, monthly_depreciation: Money, added_on: SimMonth }`**.
  - **`RateBase`**:
    - `add(asset, cost, book_life_months, month)`.
    - `depreciate_month() -> Money`: the total for the month; an asset's final month takes its
      exact remainder.
    - `total() -> Money`, `added_in_year(year: i32) -> Money`.
    - `write_down_added_in_year(year, fraction: Ratio) -> Money`: proportional across those
      assets, rounded half-even, with the remainder applied to the last asset. It returns the
      amount disallowed.
  - New `DomainEvent::LedgerEntryRecorded(LedgerEntry)`.

- [ ] **Step 1: Write the failing tests**

```rust
proptest! { #[test] fn cash_change_equals_ledger_sum(amounts in prop::collection::vec(-1_000_000_000i64..1_000_000_000, 0..100)) {
    let mut c = Company::new(Money::from_dollars(5), 70.0);
    for a in amounts { c.record(entry(Money::from_cents(a))); }
    prop_assert_eq!(c.cash() - c.starting_cash(), c.ledger_sum());
}}
#[test] fn depreciation_puts_remainder_in_final_month() { /* 1_000 cents over 3 months → 333, 333, 334 then 0 */ }
proptest! { #[test] fn depreciation_over_life_equals_original(cents in 1i64..10_000_000_000, life in 1u32..600) { /* Σ depreciate_month() over life+5 months == cents */ } }
#[test] fn write_down_is_proportional_and_exact() { /* two assets 300 + 100 added 2027, fraction 0.5 → returns 200, remaining 150 + 50 */ }
#[test] fn bankrupt_after_three_negative_closes() { /* negative cash: close ×2 → false, 3rd → true; positive in between resets */ }
#[test] fn reliability_window_keeps_three_months() { /* push 4 months; reliability uses last 3 */ }
#[test] fn satisfaction_target_is_100_at_target_and_reference() { /* reliability 0.999, tariff = reference → 100.0 */ }
#[test] fn satisfaction_moves_by_smoothing() { /* from 50 toward target 100 with α 0.2 → 60.0 */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain company`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the company module.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain company`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): company ledger, rate base and satisfaction"`

---

### Task 14: Regulator, strikes and rate review

**Files:**
- Create: `crates/domain/src/regulation/{mod.rs, rate_review.rs}`; Modify: `lib.rs`, `events.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **`enum Licence { Active, Revoked }`**.
  - **`YearTally { demand: MegawattHours, served: MegawattHours, operating_cost: Money, peak_demand: Megawatts, firm_added: Megawatts }`**
    (`Default`).
  - **`Regulator`**:
    - `new(&RegulatorParams)`.
    - Getters: `tariff()`, `allowed_return()`, `strikes()`, `licence()`, `tally()`,
      `last_cost_per_mwh() -> Option<f64>`.
    - `record_month(demand, served, operating_cost: Money, peak: Megawatts)` and
      `record_firm_added(Megawatts)`.
    - `assess_month_reliability(&mut self, demand: MegawattHours, unserved: MegawattHours, p: &RegulatorParams) -> Vec<DomainEvent>`.
    - `apply_rate_review(&mut self, outcome: &RateReviewOutcome) -> Vec<DomainEvent>`, which
      emits `RateReviewCompleted` and resets the tally.
  - **`RateReviewInputs { tally: YearTally, depreciation: Money, rate_base: Money, firm_capacity: Megawatts, previous_tariff: Price, previous_cost_per_mwh: Option<f64> }`**.
  - **`RateReviewOutcome { tariff: Price, allowed_return: Ratio, revenue_requirement: Money, cost_per_mwh: Option<f64> }`**.
  - **`fn disallowed_fraction(firm: Megawatts, firm_added: Megawatts, peak: Megawatts, reserve_margin: Ratio) -> Ratio`**:
    `excess = firm − peak × (1 + margin)`. It returns 0 if `excess ≤ 0` or `firm_added ≤ 0`, and
    otherwise `min(1, excess / firm_added)`.
  - **`fn compute_rate_review(i: &RateReviewInputs, p: &RegulatorParams) -> RateReviewOutcome`**:
    - `reliability = served / demand`, or the target when demand is 0.
    - `cost_per_mwh = operating_cost / served`, or `None` when served is 0.
    - `return = base + return_per_reliability × (reliability − target)`, plus
      `return_per_cost_improvement × (prev − cur) / prev` when both values exist, clamped to
      `[min, max]`.
    - `RR = operating_cost + depreciation + round_ties_even(return × rate_base)`.
    - `tariff = RR / served`, or `previous_tariff` when served is 0.
  - New `DomainEvent` variants: `StrikeIssued { strikes: u8 }`, `StrikeExpired { strikes: u8 }`,
    `LicenseRevoked`, `RateReviewCompleted { tariff, allowed_return }`,
    `CapitalDisallowed { amount: Money }`.
- **Strike rules:**
  - When `unserved / demand > strike_threshold` (never when demand is 0), issue a strike and reset
    `clean_months` to 0.
  - Otherwise `clean_months += 1`. When it reaches `clean_months_to_expire` and `strikes > 0`,
    decrement `strikes`, emit `StrikeExpired`, and reset `clean_months` to 0.
  - When `strikes ≥ strikes_to_revoke`, set `Revoked` and emit `LicenseRevoked` (once).

- [ ] **Step 1: Write the failing tests**

```rust
proptest! {
    #[test] fn tariff_recovers_revenue_requirement(served in 1.0f64..1e9, cost in 0i64..1e14 as i64, rb in 0i64..1e14 as i64) {
        let o = compute_rate_review(&inputs(served, cost, rb), tiny_region().regulator());
        prop_assert!((o.tariff.value() * served - o.revenue_requirement.cents() as f64 / 100.0).abs() < 0.01);
    }
    #[test] fn allowed_return_stays_in_band(served in 0.0f64..1e9, demand in 0.0f64..1e9, prev in proptest::option::of(1.0f64..500.0)) { /* min ≤ return ≤ max */ }
    #[test] fn disallowance_bounds(firm in 0.0f64..1e5, added in 0.0f64..1e5, peak in 0.0f64..1e5) {
        let f = disallowed_fraction(Megawatts::new(firm), Megawatts::new(added), Megawatts::new(peak), Ratio::new(0.2).unwrap());
        if firm <= peak * 1.2 || added == 0.0 { prop_assert_eq!(f, Ratio::ZERO); }
        prop_assert!(f.value() <= 1.0);
    }
}
#[test] fn zero_served_energy_keeps_previous_tariff() { /* served 0, demand 0 → tariff == previous_tariff, no panic */ }
#[test] fn third_strike_revokes_licence() { /* three months over threshold → events contain StrikeIssued{1},{2},{3} and LicenseRevoked once */ }
#[test] fn strike_expires_after_three_clean_months() { /* 1 strike then 3 clean months → StrikeExpired{0} */ }
#[test] fn no_strike_without_demand() { /* demand 0 → no StrikeIssued */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain regulation`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the regulation module.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain regulation`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): regulator strikes and performance-based rate review"`

---

### Task 15: `GameSession`: creation, phases, commands and tick

**Files:**
- Create: `crates/domain/src/session/{mod.rs, settings.rs, commands.rs, views.rs, invariants.rs}`
- Modify: `lib.rs`, `error.rs`, `events.rs`
- Test: inline in `session/mod.rs`, `session/commands.rs`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - **`enum DayPhase { Planning, Operating, Review }`**, **`enum GameOverReason { Bankrupt, LicenseRevoked }`**,
    **`enum SessionStatus { Active, GameOver(GameOverReason) }`**.
  - **`GameSettings { fidelity: SimulationFidelity }`** and **`SimulationFidelity { network: NetworkModel }`**,
    both `Default` (copper plate).
  - **`GameSession`** (`Debug`):
    - `new(region: Arc<RegionDefinition>, seed: u64, settings: GameSettings) -> GameSession`.
      This creates a substation root node and one node per city, with an in-service line from the
      substation to each start-connected city. Each start fleet plant is commissioned on the start
      month with an in-service line to the substation. Phase is `Planning`, and the company starts
      with the start cash.
    - `build_plant(&mut self, kind, cell) -> Result<(PlantId, Vec<DomainEvent>), DomainError>`
      (Planning only). It runs `site_check`, creates the plant `UnderConstruction { build_months }`
      on a new, unconnected node, queues `capital_cost.split_even(build_months)` as construction
      instalments, and emits `PlantConstructionStarted { plant, kind, cell }`.
    - `build_line(&mut self, from: NodeId, to: NodeId) -> Result<(LineId, Vec<DomainEvent>), DomainError>`
      (Planning only).
      - `months = max(1, ceil(len / cells_per_month))` and
        `cost = cost_per_cell × ceil(len) as i64`, split into instalments.
      - It emits `LineConstructionStarted { line }`.
    - `commit_plant(&mut self, plant, hours: [bool; 24]) -> Result<Vec<DomainEvent>, DomainError>`
      (Planning only; the plant must be commissioned and dispatchable).
    - `set_dispatch_target(plant, mw)`, `set_auto_follow(plant, on)`, `start_plant(plant)` and
      `stop_plant(plant)`, valid in Planning or Operating. `stop_plant` also calls
      `commitment_mut().clear_from(current hour)`.
    - `begin_operating(&mut self) -> Result<Vec<DomainEvent>, DomainError>`: Planning → Operating,
      calls `power.begin_day`, emits `PhaseChanged { from, to }`.
    - `tick(&mut self) -> Result<Vec<DomainEvent>, DomainError>` (Operating only). It advances the
      clock, runs `power.tick` and appends `TickCompleted`. At day end it calls
      `self.close_month()`. In this task, `close_month` only moves the phase to `Review` and emits
      `PhaseChanged`; Task 16 fills it in.
    - `finish_review(&mut self) -> Result<Vec<DomainEvent>, DomainError>`: Review → Planning, and
      `clock.next_month()`.
    - Queries: `phase()`, `status()`, `clock()`, `region()`, `settings()`, `power()`, `company()`,
      `regulator()`, `hourly_forecast() -> [ForecastPoint; 24]`, `plant_views() -> Vec<PlantView>`,
      `city_views() -> Vec<CityView>`, `grid_snapshot() -> GridSnapshot`,
      `company_view() -> CompanyView`, and `state_hash() -> u64` (FNV-1a 64 over `format!("{self:?}")`).
    - `check_invariants(&self) -> Result<(), InvariantViolation>`, which checks that
      cash − start = ledger sum, that every online dispatchable plant is within its bounds, and
      that the last tick summary conserves energy.
  - **`PlantView { id, kind, cell, node, status, output, auto_follow, connected: bool, commitment: [bool; 24], hydro_budget: Option<MegawattHours> }`**.
  - **`CityView { id, name_key: String, cell, node, connected: bool }`**.
  - **`GridSnapshot { month, tick, phase, frequency: Hertz, mode: GridMode, shed: Ratio, last_tick: Option<TickSummary> }`**.
  - **`CompanyView { cash, reliability: Ratio, satisfaction: f64, tariff: Price, allowed_return: Ratio, strikes: u8, licence: Licence }`**.
  - **`enum InvariantViolation`**: `CashLedgerMismatch { cash_delta: Money, ledger_sum: Money }`,
    `PlantOutOfBounds { plant }`, `ConservationBroken { tick: u32 }`.
  - New `DomainError` variants: `UnknownPlant { plant }`, `WrongPhase { actual: DayPhase }`,
    `GameOver { reason: GameOverReason }`, `InvalidSite { kind, cell, reason: SiteError }`,
    `PlantNotCommissioned { plant }`.
  - New `DomainEvent` variants: `PhaseChanged { from: DayPhase, to: DayPhase }`,
    `PlantConstructionStarted { plant, kind, cell }`, `LineConstructionStarted { line }`.
- **Rules:** every command checks `status == Active` first and then the phase. Every command
  validates fully before mutating anything (`Err` means no change).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn new_session_matches_start_definition() {
    let s = GameSession::new(Arc::new(tiny_region()), 1, GameSettings::default());
    assert_eq!(s.phase(), DayPhase::Planning);
    assert_eq!(s.company().cash(), Money::from_dollars(2_000_000_000));
    assert_eq!(s.plant_views().iter().filter(|p| p.connected && p.status == PlantStatus::Offline).count(), 3);
    assert_eq!(s.city_views().iter().filter(|c| c.connected).count(), 2);
}
#[test] fn invalid_sites_are_rejected_with_reason() {
    let mut s = session();
    for (kind, cell, reason) in [
        (PlantKind::Peaker, CellCoord { x: 0, y: 1 }, SiteError::NotLand),
        (PlantKind::Peaker, CellCoord { x: 99, y: 1 }, SiteError::OutOfBounds),
        (PlantKind::Hydro, CellCoord { x: 9, y: 1 }, SiteError::NoHydroSite),
    ] { assert_eq!(s.build_plant(kind, cell).unwrap_err(), DomainError::InvalidSite { kind, cell, reason }); }
    assert!(matches!(s.build_plant(PlantKind::Nuclear, CellCoord { x: 2, y: 1 }), Err(DomainError::InvalidSite { reason: SiteError::InsufficientCoolingWater { .. }, .. })));
}
#[test] fn failed_command_leaves_state_unchanged() { /* hash before == hash after a rejected build_plant */ }
#[test] fn building_is_planning_only_and_ticking_is_operating_only() { /* begin_operating; build_plant → WrongPhase{Operating}; new session tick() → WrongPhase{Planning} */ }
#[test] fn full_day_moves_to_review() { /* begin_operating; 1440 ticks → phase Review; tick() → WrongPhase{Review}; finish_review → Planning, month May */ }
#[test] fn stopping_committed_plant_clears_rest_of_day() { /* commit coal 0..=23, begin, tick 600, stop_plant → commitment hours 10.. false, earlier unchanged */ }
#[test] fn same_seed_and_commands_give_same_hash() { /* two sessions seed 5: same commands + 1440 ticks → equal state_hash; seed 6 → different */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain session`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the session module.** The tick's step 11 (accounting) is not in this task: add the summary to a `MonthAccumulator` field that Task 16 defines; until then, a field holding a `Vec<TickSummary>` is enough.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): GameSession phases, commands and tick"`

---

### Task 16: Month close (accounting, construction, regulation, game over)

**Files:**
- Create: `crates/domain/src/session/{month_close.rs, report.rs}`; Modify: `session/mod.rs`, `events.rs`
- Test: inline in `month_close.rs`

**Interfaces:**
- Produces:
  - **`MonthAccumulator`**: per-plant energy and starts, plus demand, served, unserved, curtailed,
    overgeneration (MWh), peak demand and incidents. It has
    `add_tick(&mut self, &TickSummary, &[DomainEvent])` and `reset()`.
  - **`enum IncidentKind { LoadShed { mw: Megawatts }, Blackout, BlackoutRestored }`** and
    **`Incident { tick: u32, kind }`**.
  - **`MeritOrderEntry { plant, kind, marginal_cost: Price, energy: MegawattHours }`**.
  - **`MonthlyReport`**:
    `{ month, days: u32, revenue, fuel, start_up, fixed_om, construction, net: Money, energy_by_kind: BTreeMap<PlantKind, MegawattHours>, merit_order: Vec<MeritOrderEntry>, demand, served, unserved, curtailed: MegawattHours, peak_demand: Megawatts, reliability: Ratio, incidents: Vec<Incident>, rate_review: Option<RateReviewOutcome> }`.
    All energy figures are weighted by `days`, and `merit_order` is sorted by
    `(marginal_cost, plant)`.
  - New `DomainEvent` variants: `LineCommissioned { line }`, `CityConnected { city }`,
    `MonthClosed(Box<MonthlyReport>)`, `Bankrupt`.
- **The `close_month` sequence** (`days = month.days_in_month()`):
  1. Ledger entries:
     - revenue `+tariff.cost_of(served, days)`;
     - per plant, fuel `−marginal_cost.cost_of(energy, days)`;
     - start-ups `−startup_cost × starts × days`;
     - fixed O&M `−fixed_om_per_month` for each commissioned plant;
     - one popped construction instalment per asset under construction.
  2. Construction progress for plants and lines. Each newly commissioned asset is added to the
     rate base (capital cost, book life) and to `regulator.record_firm_added(capacity × credit)`.
     Emit `CityConnected` for each city node that becomes connected.
  3. `rate_base.depreciate_month()`, accumulated into the session's `year_depreciation`.
  4. `regulator.record_month(...)` and `assess_month_reliability(...)`. `Revoked` sets
     `GameOver(LicenseRevoked)`.
  5. `company.push_reliability(...)` and `update_satisfaction(...)`.
  6. In December only:
     - compute `disallowed_fraction`, then `write_down_added_in_year`, and emit
       `CapitalDisallowed` if the amount is above 0;
     - run `compute_rate_review` and then `apply_rate_review`, and reset `year_depreciation`.
  7. If `company.close_solvency_month()` returns true, set `GameOver(Bankrupt)` and emit `Bankrupt`.
  8. Emit `MonthClosed(report)`, set phase to `Review` and emit `PhaseChanged`. Reset the
     accumulator.

- [ ] **Step 1: Write the failing tests**

```rust
proptest! { #[test] fn revenue_is_weighted_and_rounded_once(seed in any::<u64>()) {
    // play one April day with auto-follow fleet; report.revenue == tariff.cost_of(unweighted served MWh, 30)
}}
#[test] fn ledger_matches_cash_after_a_month() { /* check_invariants() == Ok(()) after close */ }
#[test] fn peaker_commissions_after_twelve_months_with_exact_instalments() { /* build peaker + line; play 12 months; PlantCommissioned in month 12; Σ Construction entries for that asset == −$200,000,000 */ }
#[test] fn december_runs_rate_review() { /* start month set to December via fixture parts; after close: RateReviewCompleted emitted, report.rate_review is Some */ }
#[test] fn bankruptcy_ends_game_and_freezes_state() {
    // fixture with start cash −$1,000,000,000,000 → after 3 month closes: status GameOver(Bankrupt), Bankrupt emitted
    // then: let h = s.state_hash(); assert_eq!(s.begin_operating(), Err(DomainError::GameOver { reason: GameOverReason::Bankrupt })); assert_eq!(s.state_hash(), h);
}
#[test] fn sustained_shortfall_revokes_licence() { /* fixture with empty start fleet: 3 months → GameOver(LicenseRevoked) */ }
#[test] fn report_merit_order_sorted_by_cost() { /* entries' marginal_cost non-decreasing */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p domain month_close`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `month_close.rs` and `report.rs`**, following the sequence above.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p domain`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(domain): month close accounting, construction, regulation and game over"`

---

### Task 17: Application commands, pipeline, ports and `Game`

**Files:**
- Create: `crates/application/src/{command.rs, error.rs, ports.rs, game.rs}`, `crates/application/src/pipeline/{mod.rs, core.rs, tracing_layer.rs, invariant_layer.rs, command_log_layer.rs}`
- Modify: `crates/application/src/lib.rs`, `Cargo.toml`
- Test: inline, plus `crates/application/tests/pipeline.rs`

**Interfaces:**
- Consumes: the domain public API.
- Produces:
  - **`enum Command`** (`Debug, Clone, PartialEq`):
    `BuildPlant { kind, cell }`, `BuildLine { from: NodeId, to: NodeId }`,
    `CommitPlant { plant, hours: [bool; 24] }`, `BeginOperating`,
    `SetDispatchTarget { plant, mw: Megawatts }`, `SetAutoFollow { plant, on: bool }`,
    `StartPlant { plant }`, `StopPlant { plant }`, `AdvanceTicks { n: u32 }`, `FinishReview`.
  - **`enum CommandError`** (`thiserror`): `Domain(#[from] DomainError)`,
    `Region(#[from] RegionLoadError)`.
  - **`trait CommandHandler { fn handle(&mut self, session: &mut GameSession, command: &Command) -> Result<Vec<DomainEvent>, CommandError>; }`**.
  - **`CoreHandler`**: maps each variant to its session method. `AdvanceTicks` loops `tick()`
    and stops early as soon as the phase is no longer `Operating`.
  - **`TracingLayer<H>`**: an `info_span!("command", ?command)`, recording `ok`, the event count,
    and the error at `warn`.
  - **`InvariantCheckLayer<H>`**: after `Ok`, `session.check_invariants()` must pass, or it panics
    with the violation.
  - **`CommandLogLayer<H>`** (holding `Box<dyn CommandLogSink>`): appends
    `LogRecord::Command(cmd.clone())` after `Ok` only. A sink error is logged with `tracing::error!`
    and never fails the command.
  - **`fn build_pipeline(sink: Box<dyn CommandLogSink>) -> Box<dyn CommandHandler>`**:
    Tracing → InvariantCheck (only `#[cfg(debug_assertions)]`) → CommandLog → Core.
  - **Ports:**
    - `RegionSummary { id: RegionId, name_key: String }`;
    - `enum RegionLoadError { NotFound(RegionId), Io { region: RegionId, message: String }, Parse { region: RegionId, message: String }, Invalid { region: RegionId, error: RegionError } }`;
    - `trait RegionCatalog { fn list(&self) -> Result<Vec<RegionSummary>, RegionLoadError>; fn load(&self, id: &RegionId) -> Result<RegionDefinition, RegionLoadError>; }`;
    - `enum LogRecord { Header { region: RegionId, seed: u64 }, Command(Command) }`;
    - `CommandLogError { message: String }`;
    - `trait CommandLogSink { fn append(&mut self, record: &LogRecord) -> Result<(), CommandLogError>; }`;
    - `InMemoryCommandLog` (`Clone`, holding `Arc<Mutex<Vec<LogRecord>>>`) with `records() -> Vec<LogRecord>`.
  - **`Game`**:
    - `start_new(catalog: &dyn RegionCatalog, region: &RegionId, seed: u64, settings: GameSettings, sink: Box<dyn CommandLogSink>) -> Result<Game, CommandError>`,
      which appends `Header` first;
    - `execute(&mut self, command: Command) -> Result<Vec<DomainEvent>, CommandError>`;
    - `session(&self) -> &GameSession`.
  - **`fn replay(region: Arc<RegionDefinition>, seed: u64, settings: GameSettings, commands: &[Command]) -> Result<GameSession, CommandError>`**
    (uses `CoreHandler` only).
  - **`fn auto_select_region(regions: &[RegionSummary]) -> Option<&RegionId>`**: `Some` only
    when there is exactly one region.

- [ ] **Step 1: Write the failing tests** (in `tests/pipeline.rs`, with a `StaticCatalog` test double that returns `tiny_region()`)

```rust
#[test] fn advance_ticks_stops_at_month_close() {
    let mut g = game(); g.execute(Command::BeginOperating).unwrap();
    let events = g.execute(Command::AdvanceTicks { n: 5_000 }).unwrap();
    assert_eq!(g.session().phase(), DayPhase::Review);
    assert_eq!(events.iter().filter(|e| matches!(e, DomainEvent::PhaseChanged { to: DayPhase::Review, .. })).count(), 1);
}
#[test] fn log_records_header_and_successful_commands_only() {
    let log = InMemoryCommandLog::default(); let mut g = game_with_log(log.clone());
    g.execute(Command::AdvanceTicks { n: 1 }).unwrap_err(); // wrong phase
    g.execute(Command::BeginOperating).unwrap();
    assert_eq!(log.records(), vec![LogRecord::Header { region: RegionId::new("tiny"), seed: 1 }, LogRecord::Command(Command::BeginOperating)]);
}
#[test] fn replay_reproduces_live_session() { /* run a scripted day live; replay the logged commands; equal state_hash */ }
#[test] fn domain_errors_pass_through_unchanged() { /* Err(CommandError::Domain(WrongPhase{Planning})) for AdvanceTicks in Planning */ }
#[test] fn auto_select_only_with_exactly_one_region() { /* 0 → None, 1 → Some(id), 2 → None */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p application`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the application modules.** Annotate `CoreHandler::handle` with `#[tracing::instrument(skip(self, session))]`, as spec §9 requires.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p application`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(application): command pipeline, ports and Game service"`

---

### Task 18: Projections, part 1 (timeline, monthly reports, plant history, milestones)

**Files:**
- Create: `crates/application/src/projections/{mod.rs, timeline.rs, monthly_report.rs, plant_history.rs, milestones.rs}`
- Modify: `game.rs` (add `projections: Projections`, updated after every successful `execute`)
- Test: inline in each projection

**Interfaces:**
- Produces:
  - **`trait Projection { fn apply(&mut self, event: &DomainEvent, session: &GameSession); }`**.
  - **`Projections { timeline, monthly_reports, plant_history, milestones, ramp_warnings, rate_review, lessons }`**,
    with `apply_all(&mut self, events: &[DomainEvent], session: &GameSession)`. The last three
    fields arrive in Task 19.
  - **`Game::projections(&self) -> &Projections`**.
  - **`TimelinePoint { tick, demand, served, frequency: Hertz, by_kind: BTreeMap<PlantKind, Megawatts> }`**
    and **`TimelineProjection`**: the last 1,440 points from `TickCompleted`, exposed through
    `points() -> impl Iterator<Item = &TimelinePoint>`.
  - **`MonthlyReportProjection`**: `latest() -> Option<&MonthlyReport>` and
    `history() -> &[MonthlyReport]`, both from `MonthClosed`.
  - **`PlantHistory { starts: u32, trips: u32, energy: MegawattHours }`** and
    **`PlantHistoryProjection`**: `get(PlantId) -> Option<&PlantHistory>`, fed by `PlantStarting`,
    `PlantTripped` and the `MonthClosed` merit-order energy.
  - **`enum MilestoneKind { ServePeak { gigawatts: u32 }, FullSeasonWithoutStrike, ConnectCities { count: u32 } }`**,
    with the constant `MILESTONES = [ServePeak { gigawatts: 50 }, FullSeasonWithoutStrike, ConnectCities { count: 10 }]`.
  - **`MilestoneProjection`**: `reached() -> &[MilestoneKind]` and
    `take_newly_reached() -> Vec<MilestoneKind>`.
    - `ServePeak` comes from `report.peak_demand`.
    - `FullSeasonWithoutStrike` means 3 consecutive closed months in the same `Season` with no
      `StrikeIssued` between them.
    - `ConnectCities` counts connected cities via `session.city_views()` whenever `CityConnected`
      is emitted.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn timeline_keeps_last_1440_points() { /* apply 2000 TickCompleted → 1440 points, first tick == 560 */ }
#[test] fn monthly_report_is_stored_on_close() { /* MonthClosed → latest() == Some(report) */ }
#[test] fn plant_history_counts_starts_and_trips() { /* 2 PlantStarting + 1 PlantTripped → starts 2, trips 1 */ }
#[test] fn serve_peak_milestone_reached_once() { /* report peak 50_000 MW twice → reached contains it once; take_newly_reached returns it once */ }
#[test] fn full_season_requires_three_strike_free_months_of_one_season() { /* Mar, Apr, May closes without strikes → reached; with a strike in Apr → not */ }
#[test] fn game_updates_projections_after_execute() { /* BeginOperating + AdvanceTicks{10} → timeline has 10 points */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p application projections`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the four projections and `Projections`.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p application`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(application): timeline, report, history and milestone projections"`

---

### Task 19: Projections, part 2 (ramp warnings, rate review estimate, lessons)

**Files:**
- Create: `crates/application/src/projections/{ramp_warning.rs, rate_review.rs, lessons.rs}`
- Modify: `crates/domain/src/session/views.rs` (add the two queries below), `projections/mod.rs`
- Test: inline

**Interfaces:**
- Produces:
  - **Domain queries:**
    - `GameSession::forecast_net_load(tick: u32) -> Megawatts`, equal to
      `connected_demand_forecast − expected_solar`;
    - `GameSession::rate_review_estimate() -> RateReviewEstimate { projected_return: Ratio, reserve_margin: f64, capital_at_risk: Money }`.
      It runs `compute_rate_review` and `disallowed_fraction` on the year-to-date tally;
      `reserve_margin = firm / peak − 1`, or 0 when peak is 0.
  - **`RampWarning { at_tick: u32, horizon_minutes: u32, net_load_change: Megawatts, capability: Megawatts }`**
    and **`fn ramp_warning(at_tick, net_now: Megawatts, net_ahead: Megawatts, capability: Megawatts) -> Option<RampWarning>`**:
    `Some` when `net_ahead − net_now > capability`, with `horizon_minutes = 60`.
  - **`RampWarningProjection`**: `current() -> Option<&RampWarning>`. On each `TickCompleted` with
    `tick % 5 == 0`, it evaluates the warning using `forecast_net_load(t)`,
    `forecast_net_load(min(t + 60, 1439))` and `power().ramp_capability_up(60 min)`.
  - **`RateReviewProjection`**: `estimate() -> Option<&RateReviewEstimate>`, refreshed on
    `MonthClosed`, `PlantCommissioned` and `PhaseChanged`.
  - **`enum Lesson`**:
    - `ExpensivePeaking { energy: MegawattHours, cost: Money, alternative: Price }`
      (`alternative` = the region's CCGT marginal cost);
    - `UnservedEnergy { energy: MegawattHours, first_incident_tick: u32 }`;
    - `SolarCurtailed { energy: MegawattHours }`;
    - `StartUpCosts { total: Money, share: Ratio }`;
    - `RateReview { tariff: Price, allowed_return: Ratio }`;
    - `AllClear`.
  - **`fn lessons_for(report: &MonthlyReport, region: &RegionDefinition) -> Vec<Lesson>`**, applying
    rules in this priority order and returning at most 3:
    1. Peaker energy > 5% of served → `ExpensivePeaking`.
    2. Unserved > 0 → `UnservedEnergy`.
    3. Curtailed > 1% of served → `SolarCurtailed`.
    4. Start-up costs > 10% of operating cost → `StartUpCosts`.
    5. A rate review happened → `RateReview`.
    6. If none of the above apply → `[AllClear]`.
  - **`LessonsProjection`**: `latest() -> &[Lesson]`, updated on `MonthClosed`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn warns_when_net_load_rise_exceeds_capability() {
    assert_eq!(ramp_warning(1000, Megawatts::new(900.0), Megawatts::new(1500.0), Megawatts::new(490.0)).map(|w| w.net_load_change), Some(Megawatts::new(600.0)));
    assert!(ramp_warning(1000, Megawatts::new(900.0), Megawatts::new(1300.0), Megawatts::new(490.0)).is_none());
}
#[test] fn lessons_follow_priority_and_cap_at_three() { /* report triggering rules 1–5 → [ExpensivePeaking, UnservedEnergy, SolarCurtailed] */ }
#[test] fn clean_month_is_all_clear() { /* zero peaker, unserved, curtailment, low start-ups, no review → [AllClear] */ }
#[test] fn rate_review_estimate_flags_overbuild() { /* tiny session + 3 extra commissioned CCGTs → capital_at_risk > 0, reserve_margin > 0.2 */ }
#[test] fn ramp_projection_updates_every_fifth_tick() { /* TickCompleted ticks 1..=4 → current None-evaluated; tick 5 evaluated (use a duck-curve fixture: 1500 MW solar at sunset hour, assert Some) */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p application projections`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the domain queries and the three projections.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run --workspace`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(application): ramp warning, rate review estimate and lessons projections"`

---

### Task 20: Scenario snapshots, determinism and state-machine tests

**Files:**
- Create: `crates/application/tests/support/mod.rs`, `crates/application/tests/{scenarios.rs, determinism.rs, state_machine.rs}`
- Test: those files; snapshots go in `crates/application/tests/snapshots/`

**Interfaces:**
- Consumes: `Game`, `replay`, `InMemoryCommandLog`, domain `test-fixtures`.
- Produces (test support only):
  - `StaticCatalog(Vec<RegionDefinition>)`, which implements `RegionCatalog`;
  - `fn game_on(parts: RegionDefinitionParts, seed: u64) -> (Game, InMemoryCommandLog)`;
  - `fn play_month(game: &mut Game, plan: impl FnOnce(&mut Game))`, which runs `plan` in Planning,
    then `BeginOperating`, `AdvanceTicks { n: 1440 }` and `FinishReview`;
  - `fn auto_follow_all(game: &mut Game)`, which commits every dispatchable plant for hours 0–23
    and turns on AGC.
  - Fixture variants built from `tiny_region_parts()`:
    - `winter_peak()`: start month January;
    - `duck_curve()`: two solar plants (300 MW each) in the start fleet, start month June;
    - `overreach()`: city 3's base load raised to 2,500 MW, with the plan building a line to it in
      month 1;
    - `overbuild()`: start month January, with the plan building 4 CCGTs plus lines in month 1, then
      playing 36 months so that they commission and a December review runs.

- [ ] **Step 1: Write the scenario snapshot tests**

```rust
#[test] fn winter_peak_month() { let (mut g, _) = game_on(winter_peak(), 11); play_month(&mut g, auto_follow_all);
    insta::assert_debug_snapshot!(g.projections().monthly_reports.latest().unwrap()); }
#[test] fn duck_curve_month() { /* same pattern; additionally assert the report's curtailed or a RampWarning occurred during the day */ }
#[test] fn overreach_city_without_capacity() { /* after the line commissions, a month's report has unserved > 0 and a StrikeIssued event */ }
#[test] fn overbuild_rate_review_disallowance() { /* CapitalDisallowed emitted with amount > 0 in the first December after commissioning; snapshot that December's report */ }
```

- [ ] **Step 2: Write the determinism property**

```rust
proptest! { #![proptest_config(ProptestConfig::with_cases(32))]
#[test] fn live_and_replayed_sessions_hash_equal(seed in any::<u64>(), script in arb_planning_script()) {
    // arb_planning_script: 0..6 Commands chosen from CommitPlant/SetAutoFollow/SetDispatchTarget/StartPlant on fleet plant ids
    let (mut g, log) = game_on(tiny_region_parts(), seed);
    for c in script { let _ = g.execute(c); }
    let _ = g.execute(Command::BeginOperating); let _ = g.execute(Command::AdvanceTicks { n: 1440 });
    let commands: Vec<Command> = log.records().into_iter().filter_map(|r| match r { LogRecord::Command(c) => Some(c), _ => None }).collect();
    let replayed = replay(Arc::new(tiny_region()), seed, GameSettings::default(), &commands).unwrap();
    prop_assert_eq!(replayed.state_hash(), g.session().state_hash());
}}
```

- [ ] **Step 3: Write the state-machine test** (`proptest-state-machine`)

- The reference state is `{ phase: DayPhase, game_over: bool }`.
- Transitions are drawn from all `Command` variants. Plant and node IDs come from the current
  views (record them in the reference at construction). `AdvanceTicks` uses `n ∈ {1, 60, 1440}`.
- On `Ok`, the reference advances its phase the way the domain does. On `Err`, nothing changes.
- After every transition, `session.check_invariants() == Ok(())` and
  `session.phase() == reference.phase`.
- Config: `cases = 64`, up to 40 transitions each. The test runs in debug, so
  `InvariantCheckLayer` is active.

- [ ] **Step 4: Run them, review and accept the snapshots, then re-run**

Run: `cargo insta test -p application --review` (accept after reading each report for plausibility:
reliability in 0–1, revenue > 0), then `cargo nextest run -p application`.
Expected: PASS, with 4 snapshot files committed.

- [ ] **Step 5: Commit**: `git add -A && git commit -m "test(application): scenario snapshots, determinism and state-machine properties"`

---

### Task 21: Infrastructure: region DTOs, rasteriser, mapping and `RonRegionCatalog`

**Files:**
- Create: `crates/infrastructure/src/region/{mod.rs, dto.rs, raster.rs, mapping.rs, catalog.rs}`; Modify: `lib.rs`, `Cargo.toml`
- Test: inline in `raster.rs`; `crates/infrastructure/tests/region_catalog.rs` with `tests/support/minimal_region.ron`

**Interfaces:**
- Consumes: `RegionDefinitionParts`, `RegionCatalog`, `RegionLoadError`.
- Produces:
  - **`RegionFile`** and its nested DTOs (`#[derive(Deserialize)]`). Field names:
    - `id: String`, `name_key: String`;
    - `bounds: Bounds { west, east, south, north: f64 }`;
    - `grid: GridSize { width: u16, height: u16 }`;
    - `land: Vec<Vec<(f64, f64)>>` and `exclude: Vec<Vec<(f64, f64)>>` (lon/lat polygons);
    - `height`, `irradiance`, `cooling_water`, `hydro_potential: Field { base: f64, blobs: Vec<Blob { lon, lat, radius_deg, value: f64 }> }`;
    - `catalog: Vec<PlantSpecDto { kind: PlantKind DTO enum, capacity_mw, min_stable, ramp_mw_per_min, startup_hours: f64, startup_cost_usd: i64, marginal_cost_usd_per_mwh: f64, fixed_om_usd_per_month: i64, capital_cost_usd: i64, build_months: u32, capacity_credit: f64, book_life_months: u32, siting: SitingDto { Anywhere, CoolingWater(min), HydroSite(min) } }>`;
    - `cities: Vec<CityDto { id: u32, name_key, lon, lat, base_load_mw }>`;
    - `demand`, `solar`, `hydro`, `frequency`, `regulator`, `satisfaction`, `lines`: mirror the
      domain params, with money as integer USD (`*_usd: i64`) and frequency as `*_hz: f64`;
    - `start: StartDto { year: i32, month: u8, cash_usd: i64, substation: (f64, f64), connected_cities: Vec<u32>, fleet: Vec<(PlantKindDto, f64, f64)> }`.
  - **`fn rasterize(file: &RegionFile) -> TerrainGrid`**.
    - The cell centre is `lon = west + (x + 0.5) / width × (east − west)` and
      `lat = north − (y + 0.5) / height × (north − south)`.
    - `land` = inside any `land` polygon and outside every `exclude` polygon (even–odd ray casting).
    - Each field = `base + Σ value × exp(−d² / (2r²))`, with the longitude difference scaled by
      `cos(lat)`, clamped to `[0, 1]`.
  - **`fn to_cell(file: &RegionFile, lon: f64, lat: f64) -> CellCoord`**: the cell containing the
    point, clamped to the grid.
  - **`fn map_region(file: RegionFile) -> Result<RegionDefinition, RegionError>`**.
  - **`RonRegionCatalog::new(dir: impl Into<PathBuf>)`**, which implements `RegionCatalog`.
    - `list()` parses every `*.ron` file in the directory, sorted by file name.
    - `load(id)` reads `{dir}/{id}.ron`; a missing file gives `NotFound`.
    - RON errors become `Parse { message }`, including the line and column.

- [ ] **Step 1: Write the failing tests**

```rust
// raster.rs
#[test] fn square_polygon_marks_inner_cells_as_land() { /* 10×10 grid over 0..10°, square 2..8° → cells x,y 2..8 land, others water */ }
#[test] fn exclude_polygon_cuts_a_hole() { /* exclude 4..6° → those cells water */ }
#[test] fn blob_peaks_at_its_centre() { /* base 0.2, blob value 0.5 at cell centre → 0.7 ± 1e-9; clamps at 1.0 for value 2.0 */ }
// tests/region_catalog.rs (each test: TempDir, copy minimal_region.ron in)
#[test] fn loads_a_valid_region() { /* load(RegionId::new("minimal")) is Ok, terrain width matches */ }
#[test] fn lists_regions_sorted_by_file_name() { /* copy as b.ron and a.ron (ids edited) → ids [a, b] */ }
#[test] fn missing_region_is_not_found() { /* NotFound */ }
#[test] fn malformed_ron_reports_position() { /* truncate file → Parse { message contains "1:" or line number } */ }
#[test] fn city_in_water_is_invalid() { /* edit city lon into water → Invalid { error: CityNotOnLand(..) } */ }
proptest! { #[test] fn garbage_never_panics(s in "\\PC{0,400}") { /* write s as x.ron; load returns Err, no panic */ } }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p infrastructure`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the DTOs, rasteriser, mapping and catalog.** Write `minimal_region.ron` from the `tiny_region` values (a 12 × 8 grid over 0–12° lon and 0–8° lat, with water at `x = 0` expressed as a land polygon starting at 1°).
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p infrastructure`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(infrastructure): RON region DTOs, rasteriser and catalog"`

---

### Task 22: Author the two shipped regions

**Files:**
- Create: `assets/regions/eastern_interconnection.ron`, `assets/regions/western_interconnection.ron`
- Test: `crates/infrastructure/tests/shipped_regions.rs`

**Interfaces:**
- Consumes: the `RegionFile` format (Task 21) and `Game` (Task 17) for the smoke run.
- **Data decisions:**
  - **East:** `bounds` west −110, east −59, south 24.5, north 56; grid 100 × 60.
    - `land` covers the US east of the Rockies and the Canadian provinces Saskatchewan, Manitoba,
      Ontario, New Brunswick, Nova Scotia and PEI.
    - `exclude` holds an ERCOT polygon (Texas minus the Panhandle north of about 34.5°N and minus
      the El Paso area west of −104.5°) and a Quebec polygon.
    - Start territory: the Ohio Valley (Columbus, Cincinnati, Pittsburgh), with a starter fleet of
      coal, nuclear, CCGT and peakers.
  - **West:** `bounds` west −125.5, east −102, south 28.0, north 60; grid 60 × 80.
    - `land` covers the western US, British Columbia, Alberta, El Paso and Baja California north of
      28°N. Baja California Sur is excluded by those bounds.
    - Start territory: the Pacific Northwest (Seattle, Portland, Spokane), with a starter fleet of
      hydro (on Columbia River hydro blobs), CCGT and peakers.
  - **Cities:** 18–25 per region, as named metros with `name_key` set to `city-<slug>`. Base loads
    in MW should be roughly proportional to metro electricity demand, so that the East total is 3–5
    times the West total.
  - **Resource blobs:**
    - irradiance: highest in the US desert Southwest;
    - cooling water: the Great Lakes, the Mississippi/Ohio/Tennessee rivers and the coasts;
    - hydro potential: Columbia/Snake, BC, the Sierra Nevada, Niagara, Tennessee and Manitoba.
  - Start fleets supply firm capacity ≥ 1.10 × the connected peak.
  - Catalog values come from spec §3.2, with the fixed O&M values from Task 5. Regulator,
    frequency, satisfaction and lines params use the Task 5 fixture values. Start month is
    2027-01; start cash is $5,000,000,000 (East) and $3,000,000,000 (West).

- [ ] **Step 1: Write the failing tests**

```rust
fn catalog() -> RonRegionCatalog { RonRegionCatalog::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/regions")) }
#[test] fn ships_exactly_two_regions() { assert_eq!(ids(catalog().list().unwrap()), ["eastern_interconnection", "western_interconnection"]); }
#[test] fn east_is_three_to_five_times_west() { /* Σ base_load east / west ∈ [3.0, 5.0] */ }
#[test] fn landmarks_fall_on_the_right_side_of_boundaries() {
    // East: Toronto(−79.38,43.65) land; Amarillo(−101.83,35.22) land; Austin(−97.74,30.27) water; Montreal(−73.57,45.50) water.
    // West: Tijuana(−117.04,32.51) land; El Paso(−106.44,31.76) land; Seattle(−122.33,47.61) land.
}
#[test] fn start_fleet_covers_connected_peak_with_margin() { /* firm ≥ 1.10 × Σ connected base_load × max shape factor */ }
#[test] fn one_month_smoke_run_is_reliable() { /* each region: Game::start_new, then a local helper (infrastructure tests can't see application's tests/support) that commits every dispatchable plant for hours 0–23 and turns on AGC; play one month → reliability ≥ 0.99, no GameOver */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p infrastructure shipped`. Expected: FAIL (no files).
- [ ] **Step 3: Author both RON files** per the data decisions above. Use polygons of 40–80 vertices each. Comment each polygon and blob group with what it represents.
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run -p infrastructure shipped`. Expected: PASS. If the smoke run fails, adjust the start fleet, not the rules.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(data): Eastern and Western interconnection regions"`

---

### Task 23: `FileCommandLog`

**Files:**
- Create: `crates/infrastructure/src/command_log/{mod.rs, dto.rs, file_log.rs}`; Modify: `lib.rs`
- Test: `crates/infrastructure/tests/command_log.rs`

**Interfaces:**
- Produces:
  - **`FileCommandLog::create(path: &Path) -> Result<FileCommandLog, CommandLogError>`**, which uses
    `create_new` and fails if the file exists. It implements `CommandLogSink` by writing one RON
    record per line and flushing after each append.
  - **`fn read_command_log(path: &Path) -> Result<Vec<LogRecord>, CommandLogError>`**.
  - **`LogRecordDto` and `CommandDto`**: serde mirrors of `LogRecord` and `Command`, with
    `From`/`TryFrom` both ways.

- [ ] **Step 1: Write the failing tests** (each with its own `TempDir`)

```rust
proptest! { #[test] fn round_trips_any_command_sequence(cmds in prop::collection::vec(arb_command(), 0..50)) {
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("game.log");
    let mut log = FileCommandLog::create(&path).unwrap();
    let records: Vec<LogRecord> = std::iter::once(LogRecord::Header { region: RegionId::new("tiny"), seed: 3 }).chain(cmds.into_iter().map(LogRecord::Command)).collect();
    for r in &records { log.append(r).unwrap(); }
    prop_assert_eq!(read_command_log(&path).unwrap(), records);
}}
#[test] fn refuses_to_overwrite_existing_log() { /* create twice → second Err */ }
#[test] fn file_log_replays_to_identical_session() { /* Game with FileCommandLog plays a day; read log; application::replay → equal state_hash */ }
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo nextest run -p infrastructure command_log`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the DTOs and the file log.**
- [ ] **Step 4: Run them to verify they pass.** Run: `cargo nextest run --workspace`. Expected: PASS.
- [ ] **Step 5: Commit**: `git add -A && git commit -m "feat(infrastructure): file command log with replay round-trip"`

---

### Task 24: Mutation testing pass and scheduled job

**Files:**
- Create: `.cargo/mutants.toml`, `.github/workflows/mutants.yml`; Modify: `TESTING.md`; add tests wherever survivors are found

**Interfaces:**
- Produces:
  - **`.cargo/mutants.toml`**: `examine_globs = ["crates/domain/src/**/*.rs", "crates/application/src/**/*.rs"]`,
    `test_tool = "nextest"`, `timeout_multiplier = 3.0`. Each `exclude_re` entry carries a
    `#`-comment justifying it. Good candidates are `Debug` impls and tracing span fields.
  - **`mutants.yml`**: `schedule: cron "0 6 * * 1"` plus `workflow_dispatch`. It runs
    `cargo mutants --in-place=false -j 4` and uploads `mutants.out/` as an artifact.

- [ ] **Step 1: Run the baseline**

Run: `cargo mutants -p domain -p application -j 4`
Expected: a report in `mutants.out/`. Record the `missed` count.

- [ ] **Step 2: Kill the domain survivors.** For each `missed` mutant in `crates/domain`, add a test that fails against the mutant. Prefer extending the property tests with boundary values, e.g. ramp exactly equal to the limit, or a reserve margin exactly at its threshold. Only provably equivalent mutants get an `exclude_re` entry with a justification.

- [ ] **Step 3: Re-run until clean.** Run: `cargo mutants -p domain -j 4`. Expected: `0 missed`.

- [ ] **Step 4: Triage the application survivors.** Run: `cargo mutants -p application -j 4`. Kill the ones in `pipeline/` and `projections/`. Record the remaining count, with reasons, in `TESTING.md` under "Mutation testing status".

- [ ] **Step 5: Update `TESTING.md`** with the exact commands, the zero-survivor target for `domain`, how to read `mutants.out/`, and the scheduled job.

- [ ] **Step 6: Run the full verification.** Run: `cargo nextest run --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`. Expected: everything passes.

- [ ] **Step 7: Commit**: `git add -A && git commit -m "test: mutation testing pass, config and weekly job"`
