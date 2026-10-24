# Plan 6: Global Foundations Implementation Plan (Milestone 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every region per-city time zones on one system clock, a computed sun, region
calendars and demand climates, a local currency, region schema 4 and a world picker. Then move
both North American regions onto it for new games, and keep every archived save bit-identical.

**Architecture:**

- The domain gains pure region types (`GeoBounds`, `TimeZone`, `Climate`, `Calendar`,
  `SolarModel`, `Currency`).
- It answers every place-dependent question through `RegionDefinition` methods: `load_factor`,
  `sun`, `season` and `currency`. Simulation code never reads raw shapes or daylight tables again.
- Infrastructure probes the `region_schema` and maps schema 4 directly. Schemas 3 and earlier
  get frozen legacy values that reproduce today's arithmetic exactly.
- The game crate formats money, time and seasons from the region and draws the world picker in
  the Equal Earth projection.

**Tech Stack:** Rust 1.98, Bevy 0.20, bevy_egui 0.43 (egui 0.36), serde/ron, proptest, insta,
cargo-nextest.

**Spec:** [docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md](../specs/2026-10-18-megawatt-mogul-design.md),
Amendment A14 (§28): §28.4–§28.8, §28.10–§28.11 and the North American migration. The multi-area
work (§28.9) is Plan 8, and the new regions are Plan 7. Neither is in scope here.

## Global Constraints

- `domain` depends only on `rand_chacha` and `thiserror`. It has no I/O, serde or user-facing
  strings; translation keys are fine.
- Tuning lives in region RON and the `test-fixtures` fixture. The only exception is the frozen
  legacy defaults in `crates/infrastructure/src/region/legacy.rs` (§28.11).
- Determinism: use `BTreeMap`/`BTreeSet` only, add no new RNG draws, and keep demand noise at
  exactly one normal per tick (§28.4).
- Money stays `i64` minor units, rounded once per ledger entry. Supported currencies are USD, EUR,
  CNY, INR, RUB and BRL, all with exponent 2 (§28.7).
- Zone offsets are a multiple of 15 minutes, in `-720..=840`. A DST month adds exactly 60
  minutes, and the offset is constant for the whole month (§28.4).
- Sun: the representative date is the 15th of the month in a non-leap year;
  `δ = −23.44° × cos(2π(n + 10)/365)`; `sun = max(0, sin φ sin δ + cos φ cos δ cos H)`;
  `H = 15° × (UTC hours + λ/15 − 12)`. There is no equation of time and no refraction (§28.5).
- Schema-4 grids are at most 256 × 128 cells. Cells are equirectangular (§28.3).
- Region schemas 3 and earlier reproduce today's demand factors, sun factors, season labels,
  money text and lighting exactly (§28.11).
- Portable state schema becomes **3**, and the readers for schemas 1 and 2 are kept. Verify old
  bytes with old semantics before any upcast (§21, §25.6).
- `SIM_RULES_VERSION` goes from 15 to 16 once, in Task 10.
- One game test binary (`crates/game/tests/game.rs` modules), with a TempDir for every I/O test.
- HUD panels follow the presenter → draw → `UiAction` → dispatch pattern and the 4.5:1 contrast
  floor. New strings go in `assets/locales/en-US/*.ftl`, and new literal keys in
  `crates/game/src/l10n/keys.rs`.
- Never `--no-verify`. Run `cargo nextest run --workspace`, clippy with `-D warnings` and
  `cargo fmt --all --check` before every commit.
- Tasks 8–9 edit `crates/game/src/world/*` and `hud/region_atlas.rs`. Start them only after the
  user's uncommitted world-view work (as of 2026-10-24) is committed, and rebase onto it.

## Review Focus

1. **Archived saves.** A schema-3 save of each North American region resumes and produces the same
   state hash and reports as before. This needs bit-for-bit legacy `load_factor` and `sun`
   (Tasks 3 and 4, plus a golden test in Task 10).
2. **Midnight wrap.** Cities with negative or fractional offsets wrap at local midnight:
   BRT at system 01:00, IST/NPT/BTT against a UTC+0 system zone, and a DST month for both
   (Task 2).
3. **Polar and southern sun.** Polar night, midnight sun, the zenith and the southern hemisphere
   never produce NaN or a reversed arc in the sun or its lighting angles (Task 4).
4. **Missing currency glyphs.** If the shipped font lacks ₹ or ₽, money falls back to the ISO
   code, never a tofu box (Task 6).
5. **Uneven seasons.** A two-month season (India's winter) and a December–February season that
   wraps the year still let the strike-free-season milestone complete (Task 3).

---

### Task 1: Portable state schema 3 and the new Season tags

**Files:**
- Modify: `crates/domain/src/state.rs:20-70` (add the constants; replace `1..=2` and `schema: 2`)
- Modify: `crates/domain/src/time.rs:35-200` (add the `Season` variants and their tags)
- Modify: `crates/domain/src/session/mod.rs:460-510`, `crates/application/src/game.rs:75-150`,
  `crates/infrastructure/src/save/listing.rs:268` and `crates/infrastructure/src/save/codec.rs:273,391`
  (use the constants)
- Modify: `crates/game/src/l10n/localize.rs:380-386` and `assets/locales/en-US/ui.ftl`
  (`season-monsoon = Monsoon`, `season-post-monsoon = Post-monsoon`)
- Test: inline tests in `state.rs` and `time.rs`; `crates/infrastructure/tests/` save round-trips

**Interfaces:**
- Produces: `domain::state::CURRENT_STATE_SCHEMA: u32 = 3`,
  `domain::state::SUPPORTED_STATE_SCHEMAS: RangeInclusive<u32> = 1..=3` and
  `Season::{Monsoon, PostMonsoon}` (tags 4 and 5).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn season_tags_four_and_five_are_schema_three_only() {
    for (season, tag) in [(Season::Monsoon, 4u8), (Season::PostMonsoon, 5u8)] {
        let bytes = season.state_bytes_schema(3);
        assert_eq!(bytes, vec![tag]);
        assert_eq!(Season::from_state_bytes_schema(&bytes, 3), Ok(season));
        assert_eq!(Season::from_state_bytes_schema(&bytes, 2), Err(StateError::Invalid));
    }
}
#[test]
fn every_gate_accepts_exactly_the_supported_schemas() {
    assert_eq!(CURRENT_STATE_SCHEMA, 3);
    assert!(SUPPORTED_STATE_SCHEMAS.contains(&1) && SUPPORTED_STATE_SCHEMAS.contains(&3));
    assert!(!SUPPORTED_STATE_SCHEMAS.contains(&4));
}
```

Add an infrastructure test that writes a new save (schema 3), reads it back, then restores the
existing schema-2 golden fixtures unchanged. A schema-4 header must still fail as unsupported.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(season_tags_four_and_five) | test(every_gate_accepts)'`
Expected: they fail to compile, because the constants and variants don't exist yet.

- [ ] **Step 3: Implement**

- Add the constants and replace every literal `1..=2`, `schema: 2` and `snapshot_state_schema(2)`
  in the files above with `SUPPORTED_STATE_SCHEMAS` or `CURRENT_STATE_SCHEMA`.
- `Season::read` rejects tags 4–5 when `input.schema < 3`.
- `Season::localize` maps the new variants to `season-monsoon` and `season-post-monsoon`.
- No simulation outcome changes.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes, the schema-2 golden saves included.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: portable state schema 3 with monsoon season tags"
```

---

### Task 2: Region geography and time zones

**Files:**
- Create: `crates/domain/src/region/clock.rs`
- Modify: `crates/domain/src/region/mod.rs:78-200` (parts, fields, accessors, validation),
  `crates/domain/src/region/fixtures.rs` (`tiny_region` uses one zone at offset 0 and gets
  `GeoBounds`), `crates/infrastructure/src/region/mapping.rs` (build the legacy values so the code
  compiles; Task 7 adds schema 4)
- Test: inline in `clock.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct GeoBounds { pub west: f64, pub east: f64, pub south: f64, pub north: f64 }
  pub struct ZoneId(pub u16);
  pub struct TimeZone { pub id: ZoneId, pub name_key: Option<String>,
                        pub standard_offset_minutes: i32, pub dst_months: [bool; 12] }
  impl TimeZone { pub fn offset_minutes(&self, month: SimMonth) -> i32 }
  pub struct RegionClock { pub system_zone: ZoneId, pub zones: Vec<TimeZone> }
  // RegionDefinitionParts gains: geo: GeoBounds, clock: RegionClock,
  //   city_locales: BTreeMap<CityId, CityLocale>
  pub struct ClimateId(pub u16);   // defined here; Task 3 adds `Climate`
  pub struct CityLocale { pub zone: ZoneId, pub climate: ClimateId } // ClimateId(0) for every city until Task 3
  impl RegionDefinition {
      pub fn geo(&self) -> &GeoBounds;
      pub fn cell_lat_lon(&self, cell: CellCoord) -> (f64, f64);   // the cell centre, with row 0 at the north
      pub fn system_zone(&self) -> &TimeZone;
      pub fn city_zone(&self, city: CityId) -> &TimeZone;
      pub fn local_minute(&self, city: CityId, month: SimMonth, tick: u32) -> u32;
  }
  ```
- Per-city locale lives on the region, never in `CityDef`. `DemandCenter.def` is persisted, so
  changing `CityDef` would change portable state.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn offsets_follow_dst_months() {
    let cet = zone(60, [false,false,false,true,true,true,true,true,true,true,false,false]);
    assert_eq!(cet.offset_minutes(month(3)), 60);
    assert_eq!(cet.offset_minutes(month(4)), 120);
}
#[test] fn local_minute_wraps_both_ways() {
    // System UTC+0. City A: IST +330. City B: BRT −180.
    assert_eq!(r.local_minute(ist_city, month(1), 0), 330);
    assert_eq!(r.local_minute(ist_city, month(1), 1439), (1439 + 330) % 1440);
    assert_eq!(r.local_minute(brt_city, month(1), 60), 1440 - 120);
    // A DST system zone (CET, +60/+120) against a non-DST city (+60): equal in January,
    // one hour behind in July, wrapping at system 00:30.
    assert_eq!(r2.local_minute(cet_fixed_city, month(1), 30), 30);
    assert_eq!(r2.local_minute(cet_fixed_city, month(7), 30), 1440 - 30);
}
#[test] fn cell_centres_span_the_bounds() {
    // A 10 x 5 grid over west −10, east 10, south 0, north 5.
    assert_eq!(r.cell_lat_lon(CellCoord::new(0, 0)), (4.5, -9.0));
    assert_eq!(r.cell_lat_lon(CellCoord::new(9, 4)), (0.5, 9.0));
}
```

Validation tests:
- Each of these fails with `RegionError::InvalidShape { field }`: an offset that is not a
  multiple of 15 (`time_zones.offset`), an offset outside `-720..=840` (`time_zones.offset`), a
  duplicate zone id (`time_zones.id`), an unknown system zone (`system_zone`) and a city without a
  locale (`cities.zone`).
- A proptest: `local_minute < 1440`, and `(local_minute − tick) mod 1440` equals the offset
  difference.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(offsets_follow_dst) | test(local_minute_wraps) | test(cell_centres_span)'`
Expected: they fail to compile.

- [ ] **Step 3: Implement** `clock.rs` and wire up the region accessors.

`local_minute = (tick as i32 + city_offset − system_offset).rem_euclid(1440)`. The legacy and
fixture regions use one zone `ZoneId(0)`, `name_key: None`, offset 0 and no DST.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes, and no outcome changes.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: region geography and per-city time zones"
```

---

### Task 3: Demand climates, demand noise and the region calendar

**Files:**
- Create: `crates/domain/src/region/climate.rs`
- Modify:
  - `crates/domain/src/demand/mod.rs:40-90` (`shape_factor` becomes a `Climate` method);
  - `crates/domain/src/demand/forecast.rs:20-40`;
  - `crates/domain/src/grid/power_system.rs:506,1283,1350`;
  - `crates/domain/src/session/views.rs:366,425`;
  - `crates/domain/src/region/mod.rs` (replace `demand: DemandShape`);
  - `crates/domain/src/region/fixtures.rs`;
  - `crates/application/src/projections/milestones.rs:52-160`;
  - `crates/game/src/hud/top_bar.rs:98`;
  - `crates/game/src/hud/forecast_chart.rs:670-680`;
  - `crates/game/src/world/model.rs:281`;
  - the legacy mapping in `crates/infrastructure/src/region/mapping.rs`.
- Test: inline in `climate.rs` and `demand/mod.rs`; `milestones.rs` tests

**Interfaces:**
- Consumes: `RegionDefinition::local_minute` (Task 2).
- Produces:
  ```rust
  pub struct Climate { pub id: ClimateId, pub shapes: Vec<[f64; 24]>,
                       pub month_shapes: [usize; 12], pub monthly: [f64; 12] }
  impl Climate { pub fn factor(&self, month: SimMonth, local_minute: u32) -> f64 }
  pub struct DemandNoiseParams { pub forecast_error: Ratio, pub error_persistence: f64 }
  pub struct Calendar { pub seasons: [Season; 12] }
  impl Calendar {
      pub fn northern_meteorological() -> Self;
      pub fn run_length(&self, month: SimMonth) -> u8;
  }
  impl RegionDefinition {
      pub fn load_factor(&self, city: CityId, month: SimMonth, tick: u32) -> f64;
      pub fn demand_noise(&self) -> &DemandNoiseParams;
      pub fn season(&self, month: SimMonth) -> Season;
      pub fn calendar(&self) -> &Calendar;
  }
  pub fn forecast_demand<'a>(cities: impl IntoIterator<Item = &'a DemandCenter>,
                             region: &RegionDefinition, month: SimMonth, tick: u32) -> Megawatts;
  pub fn hourly_forecast<'a>(cities: impl IntoIterator<Item = &'a DemandCenter>,
                             region: &RegionDefinition, month: SimMonth) -> [ForecastPoint; 24];
  impl DemandNoise { pub fn step(&mut self, p: &DemandNoiseParams, rng: &mut SimRng) -> f64 }
  ```
- `Climate::factor` must keep the current expression and operation order exactly:
  `(h[hour] + (h[next] − h[hour]) × fraction) × monthly`, so legacy results are bit-identical.
- `run_length` is the length of the maximal cyclic run of equal labels that contains `month`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn legacy_climate_is_bit_identical_to_the_old_shape() {
    // Build the legacy climate from tiny_region's old winter/summer/shoulder arrays.
    for m in 1..=12 { for t in 0..1440 {
        assert_eq!(climate.factor(month(m), t).to_bits(), old_shape_factor(&old, month(m), t).to_bits());
    }}
}
#[test] fn a_southern_calendar_uses_summer_in_january() {
    assert_eq!(region_south().season(month(1)), Season::Summer);
}
#[test] fn cities_in_other_zones_peak_at_their_local_hour() {
    // Two cities, +0 and +180. A shape that is 1 at local 18:00 and 0.5 otherwise.
    assert_eq!(r.load_factor(east_city, month(1), 15 * 60), 1.0);
    assert_eq!(r.load_factor(west_city, month(1), 15 * 60), 0.5);
}
#[test] fn run_lengths_cover_short_and_wrapping_seasons() {
    let india = Calendar { seasons: [Winter, Winter, Summer, Summer, Summer, Monsoon, Monsoon,
        Monsoon, Monsoon, PostMonsoon, PostMonsoon, PostMonsoon] };
    assert_eq!(india.run_length(month(1)), 2);
    assert_eq!(Calendar::northern_meteorological().run_length(month(12)), 3);
}
```

- Keep `old_shape_factor` as a test-only copy of today's function.
- Milestone test: with the India calendar, strike-free January and February complete the
  milestone. `needed` reports 2 in January and 4 in July.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(legacy_climate_is_bit) | test(southern_calendar) | test(other_zones_peak) | test(run_lengths)'`
Expected: they fail to compile.

- [ ] **Step 3: Implement**

- Replace every `region.demand()` read with the new API, `month.season()` with
  `region.season(month)`, and `SEASON_MONTHS` with `calendar.run_length(month)`.
- The milestone projection receives the region calendar from the session.
- Validation fails with `InvalidShape`:
  - on 0 or more than 4 shapes (`climates.shapes`);
  - on a `month_shapes` index out of range (`climates.month_shapes`);
  - on non-finite or negative values (`climates.shapes`, `climates.monthly`);
  - on an unknown climate id in `city_locales` (`cities.climate`).
- Keep `SimMonth::season()` only as the meteorological default used by
  `Calendar::northern_meteorological`.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes. Snapshots and the rules guard are unchanged, because legacy is
bit-identical.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: demand climates, regional demand noise and region calendars"
```

---

### Task 4: The computed sun

**Files:**
- Modify:
  - `crates/domain/src/generation/solar.rs` (the model and formulas);
  - `crates/domain/src/region/params.rs:28-34` (`SolarParams`);
  - `crates/domain/src/grid/power_system.rs:396-410,765-790,1288-1305`;
  - `crates/domain/src/grid/power_system/wind.rs:45-68`;
  - every other `sun_factor` caller;
  - `crates/domain/src/region/mod.rs` (accessors, validation tests at 832, 841, 1299);
  - the fixtures.
- Test: inline in `solar.rs`; `grid/power_system/tests.rs` (941 and the others)

**Interfaces:**
- Consumes: `cell_lat_lon` and `system_zone` (Task 2).
- Produces:
  ```rust
  pub enum SolarModel { DaylightTable([(f64, f64); 12]), Astronomical }
  pub struct SolarParams { pub model: SolarModel, pub clearness_mean: [f64; 12], pub clearness_sd: f64 }
  pub struct SunAngles { pub elevation_deg: f64, pub azimuth_deg: f64 } // azimuth clockwise from north
  pub fn declination_deg(month: SimMonth) -> f64;
  pub fn sun_angles(lat: f64, lon: f64, utc_minutes: f64, month: SimMonth) -> SunAngles;
  impl RegionDefinition {
      pub fn sun(&self, cell: CellCoord, month: SimMonth, tick: u32) -> f64;
      pub fn sun_at_home(&self, month: SimMonth, tick: u32) -> Option<SunAngles>; // None for DaylightTable
      pub fn solar_potential(&self, cell: CellCoord) -> Ratio;
  }
  ```
- `sun` for `DaylightTable` is today's `sun_factor` at every cell, bit-identical. For
  `Astronomical`, it is `max(0, sin(elevation))`.
- `PowerSystem::available(region, plant, month, tick)` replaces the regional `sun` argument.
  `expected_solar` and `withheld_by_kind` use each plant's own cell.
- `solar_potential` is defined as follows:
  - **Astronomical:** irradiance × annual mean daily ∑sun at the cell's latitude (λ = 0, UTC
    offset 0) ÷ the same at latitude 0, clamped to [0, 1];
  - **DaylightTable:** irradiance, exactly as today.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn quito_equinox_noon_is_near_overhead() {           // −0.18, −78.47, March
    let a = sun_angles(-0.18, -78.47, utc_noon_at(-78.47), month(3));
    assert!(a.elevation_deg > 85.0);
}
#[test] fn murmansk_has_polar_night_and_midnight_sun() {     // 68.97 N, 33.08 E
    assert!((0..1440).all(|m| sun_at(68.97, 33.08, month(12), m) == 0.0));
    assert!(sun_at(68.97, 33.08, month(6), local_midnight_utc(33.08)) > 0.0);
}
#[test] fn sao_paulo_december_days_are_longer_than_june() {  // −23.55, −46.63
    assert!(daylight_minutes(-23.55, -46.63, month(12)) > daylight_minutes(-23.55, -46.63, month(6)) + 120);
}
#[test] fn eastern_sites_rise_earlier_on_the_system_clock() { // Novosibirsk vs Moscow on MSK
    assert!(first_sun_tick(novosibirsk_cell) < first_sun_tick(moscow_cell));
}
#[test] fn southern_noon_sun_is_in_the_north() {
    let a = sun_angles(-23.55, -46.63, utc_noon_at(-46.63), month(1));
    assert!(a.azimuth_deg < 20.0 || a.azimuth_deg > 340.0);
}
#[test] fn daylight_table_regions_are_unchanged() { /* tiny_region: sun(cell,…) == old sun_factor bits, every tick of every month */ }
```

Proptests:
- `sun` is in `[0, 1]` and finite for any latitude in `[-90, 90]`, longitude, month and tick.
- `solar_potential` is in `[0, 1]` and monotone in irradiance.
- `sun_angles` is never NaN, including at the poles and at the zenith (where `atan2(0, 0)` is
  defined).

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(quito) | test(murmansk) | test(sao_paulo) | test(eastern_sites) | test(southern_noon) | test(daylight_table_regions)'`
Expected: they fail to compile.

- [ ] **Step 3: Implement**

- Representative day of year `n = [0,31,59,90,120,151,181,212,243,273,304,334][m−1] + 15`.
- `UTC minutes = tick − system_offset(month)`.
- Azimuth is `atan2(sin H, cos H sin φ − tan δ cos φ)` converted to clockwise-from-north degrees
  in `[0, 360)`. At the zenith, guard `cos φ = 0` with `atan2`, never `tan`.
- Compute `sun` directly, without a per-cell cache. A 256 × 128 grid × 12 months × 1,440 minutes
  is far too large, and only plant cells are ever asked. Precompute only the per-row
  (per-latitude) annual integrals for `solar_potential` at region construction.
- The tiny fixture stays on `DaylightTable`. Add `fixtures::astronomical_region()` (gated by
  `test-fixtures`) for the new tests.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes, and the snapshots are unchanged.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: computed per-cell sun for astronomical regions"
```

---

### Task 5: Planning samples at quarter-hour demand bends

**Files:**
- Modify: `crates/domain/src/grid/power_system/outage.rs:28-40,290-310` (`HOUR_ENDS`) and the
  no-storage sampling in `crates/domain/src/session/views.rs` (supply forecast, §8.4)
- Test: `crates/domain/src/grid/power_system/tests.rs` or `outage` tests

**Interfaces:**
- Consumes: `RegionDefinition::local_minute` (Task 2).
- Produces:
  `RegionDefinition::hour_sample_minutes(&self, cities: impl IntoIterator<Item = CityId>, month) -> BTreeSet<u32>`.
  It returns minutes within an hour: always `{0, 59}`, plus `(60 − offset_delta mod 60) mod 60`
  for each city whose local hour starts mid-system-hour. Every no-storage hourly sampler uses it
  in place of `HOUR_ENDS`.

- [ ] **Step 1: Write the failing test**

```rust
#[test] fn ist_cities_add_the_half_hour_bend() {
    // System UTC+0, one IST city: its local hours start at system :30.
    assert_eq!(r.hour_sample_minutes([ist_city], month(1)), BTreeSet::from([0, 30, 59]));
    assert_eq!(r.hour_sample_minutes([utc_city], month(1)), BTreeSet::from([0, 59]));
}
#[test] fn a_peak_at_a_half_hour_bend_is_found() {
    // An IST city whose shape peaks at a local whole hour; a fleet sized just below that peak.
    // The Planning shortfall check reports the shortage at minute 30 of the system hour.
}
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(half_hour_bend)'`
Expected: they fail.

- [ ] **Step 3: Implement** by threading the sample set through the outage and views samplers.
  Single-zone regions return `{0, 59}`, so legacy behaviour is unchanged.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: sample planning demand at every local-hour bend"
```

---

### Task 6: Local currency and money formatting

**Files:**
- Create: `crates/domain/src/units/currency.rs` (or extend `units.rs`, following the local style)
- Modify:
  - `crates/domain/src/region/mod.rs` (`currency` part and accessor);
  - `crates/game/src/l10n/format.rs:30-145` (`money`, `price`);
  - every `money(`/`price(` caller in `crates/game/src` (about 39);
  - `assets/locales/en-US/ui.ftl:303,431` and new currency keys;
  - `crates/game/src/l10n/keys.rs`;
  - the font setup, which builds a `MoneyStyle` resource once per region.
- Test: `format.rs` tests; a `tests/game/` glyph test

**Interfaces:**
- Produces:
  ```rust
  pub enum Currency { Usd, Eur, Cny, Inr, Rub, Brl }          // domain; no strings
  impl Currency { pub fn iso_code(self) -> &'static str }     // data identity, not display text
  impl RegionDefinition { pub fn currency(&self) -> Currency }
  // game::l10n::format
  pub struct MoneyStyle { pub prefix: String, pub suffix: String }
  impl MoneyStyle { pub fn resolve(currency: Currency, l10n: &Localizer, has_glyph: impl Fn(char) -> bool) -> Self }
  pub fn money(amount: Money, style: &MoneyStyle) -> String;
  pub fn price(price: Price, style: &MoneyStyle) -> String;    // "€45.00/MWh"
  ```
- Fluent keys hold the symbol and its placement: `currency-usd = $`, `currency-eur = €`,
  `currency-cny = ¥`, `currency-inr = ₹`, `currency-rub = ₽`, `currency-brl = R$`. Every one is
  a prefix in en-US.
- If `has_glyph` fails for any char of the symbol, the style becomes
  `prefix = "<ISO> "`, as in `"INR 1.20M"`. Unit labels such as `supply-chart-unit-cost` take
  a `{ $symbol }` argument.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn usd_money_text_is_unchanged() { /* the existing table at format.rs:298-305 passes with MoneyStyle for USD */ }
#[test] fn every_currency_formats_compactly() {
    assert_eq!(money(cents(1_234_000.0), &style(Currency::Eur)), "€1.23M");
    assert_eq!(money(cents(-2_000_000_000.0), &style(Currency::Brl)), "−R$2.00B");
}
#[test] fn a_missing_glyph_falls_back_to_the_iso_code() {
    let s = MoneyStyle::resolve(Currency::Inr, &l10n, |c| c != '₹');
    assert_eq!(money(cents(1_200_000.0), &s), "INR 1.20M");
}
```

Also add a game-binary test that paints each shipped currency's symbol with the real egui
fonts. It asserts either that the glyph renders, meaning it isn't the replacement glyph, or that
`MoneyStyle` chose the ISO fallback.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(usd_money_text) | test(every_currency_formats) | test(missing_glyph)'`
Expected: they fail to compile.

- [ ] **Step 3: Implement** the change at every caller.
  - Presenters read `MoneyStyle` from a resource built when a session loads, and pass it on.
  - Save-library cards resolve a style from each save's region currency.
  - Legacy regions are USD, so their text is byte-identical.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes, including l10n coverage.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: region currency and currency-aware money formatting"
```

---

### Task 7: Region schema 4: DTOs, mapping and frozen legacy values

**Files:**
- Modify:
  - `crates/infrastructure/src/region/dto.rs` (add `RegionFileV4` and its sub-DTOs; keep
    `RegionFile` as the v1–3 DTO; add `ParsedRegionFile::{Legacy, V4}`);
  - `crates/infrastructure/src/region/catalog.rs` (probe and dispatch in `parse_region_file`
    before strict DTO deserialisation; update listing and `load_region_bytes`);
  - `crates/infrastructure/src/region/mapping.rs:24-60` (keep the legacy mapper and add the
    v4 mapper);
  - `crates/infrastructure/src/region/legacy.rs` (the legacy values from Tasks 2–6, gathered in
    one `legacy_locale` function);
  - `crates/infrastructure/src/region/mod.rs:17` (`CURRENT_REGION_SCHEMA = 4`);
  - `crates/infrastructure/src/region/outline.rs` (consume either parsed variant and read the
    optional `atlas` without deserialising the whole file twice);
  - `crates/domain/src/region/mod.rs` (`start_line_names` part and
    `start_line_name(city) -> Option<&str>`).
- Test: `crates/infrastructure/tests/region_catalog.rs` and `save_snapshots.rs`, with v4 fixture
  files under each test's TempDir

**Interfaces:**
- Consumes: every domain type from Tasks 2–6.
- Produces:
  - `RegionFileV4`, which has the v3 fields minus `demand` and `solar.daylight`, with the money
    fields renamed. It adds `system_zone: u16`, `time_zones: Vec<TimeZoneDto>`, a per-city
    `zone: u16` and `climate: u16`, `climates: Vec<ClimateDto>`, `demand_noise`,
    `calendar: Vec<SeasonDto>`, `currency: CurrencyDto`, `start.line_names: Vec<(u32, String)>`
    (optional) and `atlas: Option<AtlasDto { letter: String, colour: String }>`.
  - The renamed money fields are `startup_cost`, `marginal_cost_per_mwh`, `fixed_om_per_month`,
    `capital_cost`, `starting_tariff_per_mwh`, `reference_tariff_per_mwh`, `cost_per_cell` and
    `start.cash`.
  - `parse_region_file(source_bytes, region)` returns `ParsedRegionFile`, checks the requested
    region id for either variant, and remains the shared parser for catalog listing, loading and
    outlines. The public `load_region_bytes` interface stays unchanged for archived-save callers.
  - Keep the existing `map_region(file: RegionFile)` for legacy DTO callers, and add
    `map_region_v4(file: RegionFileV4)`. `load_region_bytes` selects the mapper from the parsed
    variant; listing reads its id/name without mapping or rasterising.
- Dispatch at the catalog parsing boundary:
  - In `parse_region_file`, probe `region_schema` with a permissive struct **before** attempting
    either strict DTO deserialisation. A v4 file must never be decoded as the legacy `RegionFile`.
  - Schemas `None | 2 | 3` go to the existing path plus the legacy values.
  - Schema `4` goes to `RegionFileV4` with `deny_unknown_fields`, so a `_usd` field is an
    unknown-field error.
  - Anything else returns `UnsupportedRegionSchema`.
- Schema-4 checks: the grid is at most 256 × 128 (`InvalidShape { field: "grid" }`), and every
  required field is present (`MissingField { field }`).

- [ ] **Step 1: Write the failing tests**

- `a_v4_file_maps_every_new_field`: a small v4 file with two zones, two climates, a southern
  calendar, EUR and a line name. Assert the domain accessors.
- `a_v4_file_rejects_usd_suffixed_fields`, `..._an_unknown_currency`, `..._a_missing_calendar`
  and `..._a_257_wide_grid`.
- `a_v4_file_lists_loads_and_has_an_outline`: write the fixture to a TempDir, then exercise
  `RonRegionCatalog::list`, `RonRegionCatalog::load` and `region_outline`. Assert the listing has
  a valid summary, the mapped v4 fields are present and the outline matches the same source bytes.
  This must test the public catalog path, not just a direct call to the v4 mapper.
- `archived_v3_bytes_map_to_identical_behaviour`: both shipped NA files at their committed v3
  bytes (copy them into `crates/infrastructure/tests/fixtures/` before Task 10 rewrites them).
  Each must map to one zone, the daylight table, a northern calendar and USD, with
  `load_factor`/`sun` bit-identical to the pre-plan values captured in the test.
- `archived_v3_save_restores_with_a_v4_catalog`: with the current catalog pointing at a v4
  fixture for the same region id, decode and restore a supported portable save containing archived
  v3 region bytes through the normal save-loading path. Assert it retains the exact archived bytes
  and legacy domain values, and reproduces the saved canonical bytes under the source state schema.
- `atlas_metadata_reaches_the_outline_only`.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(a_v4_file) | test(archived_v3_bytes) | test(archived_v3_save) | test(atlas_metadata)'`
Expected: they fail.

- [ ] **Step 3: Implement** the DTOs, shared catalog parser and mapping.
  - Route catalog listing, loading and outlines through `ParsedRegionFile`; archived regions
    reach the same parser through `load_region_bytes`. Keep id checks and fingerprints tied to
    the exact input bytes.
  - Share the unchanged sub-DTOs (`Field`, `Blob`, `FrequencyDto`, `WindDto`, `HydroDto`,
    `SatisfactionDto`, `BatteryDto`, `Bounds`, `GridSize`).
  - Legacy values live only in `legacy.rs` and never apply to v4.

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace`
Expected: everything passes, and the shipped v3 files still load.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: region schema 4 with time zones, climates, calendar and currency"
```

---

### Task 8: In-game presentation of time, seasons, the sun and submarine cables

**Files:**
- Modify:
  - `crates/game/src/hud/top_bar.rs` (system time and zone; season from the region);
  - the city card presenter (local time and zone);
  - the commitment grid's axis label (system zone);
  - `crates/game/src/world/lighting.rs:19-70` (`sun_at_home`, with the legacy arc kept for
    `DaylightTable`);
  - the sunshine overlay (uses `solar_potential`);
  - the line card and line render (submarine segments, the start-line name);
  - `crates/domain/src/region/terrain.rs` (water helper);
  - `assets/locales/en-US/ui.ftl` and `keys.rs`.
- Test: presenter unit tests; `crates/game/tests/game/` glue and paint tests

**Interfaces:**
- Consumes: `system_zone`, `city_zone`, `local_minute`, `season`, `sun_at_home`,
  `solar_potential` and `start_line_name` (Tasks 2–7).
- Produces:
  `TerrainGrid::water_cells_on_segment(&self, a: CellCoord, b: CellCoord) -> Vec<CellCoord>`.
  This is a supercover traversal in deterministic integer order, returning only non-land cells.
  The line card shows "Submarine cable · N cells" when the result is non-empty.

- [ ] **Step 1: Write the failing tests**

- `top_bar_shows_system_time_with_zone`: "14:20 MSK" for a region whose system zone has
  `name_key` `zone-msk`, and "14:20" for a legacy zone with `name_key: None`.
- `city_card_shows_local_time`.
- `season_label_follows_the_region_calendar`.
- `sun_points_north_at_noon_in_the_south` (lighting `toward_sun.z < 0`, because north is −Z
  and the vector points toward the sun) and `polar_night_has_no_sunlight`.
- `legacy_lighting_is_unchanged`.
- A proptest: `water_cells_on_segment` contains both endpoints' cells when they are water, is
  symmetric under swapping `a` and `b`, and is empty across all-land grids.
- `the_gibraltar_style_starter_line_is_named_and_drawn_as_a_cable`, using a small v4 fixture with
  a strait.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run -p game -E 'test(system_time_with_zone) | test(local_time) | test(season_label) | test(north_at_noon) | test(polar_night) | test(legacy_lighting) | test(gibraltar_style)'`
Expected: they fail.

- [ ] **Step 3: Implement** through the presenter → draw → `UiAction` → dispatch flow.
  - Submarine segments render darker and dashed over water cells, with the existing pulses kept.
  - They reuse render entities and assets across frames (§8.3).
  - The line card adds "Transfer limits are not simulated (copper plate)".

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace` and then `cargo run -p game -- --exit-after-frames 120`
Expected: every test passes, and the launch smoke exits 0.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: local time, region seasons, computed sunlight and submarine cables in the HUD"
```

---

### Task 9: The world picker

**Files:**
- Modify: `crates/game/src/hud/region_atlas.rs` (projection, backdrop, palette, labels),
  `crates/game/src/hud/menu.rs` (cards), `crates/infrastructure/src/region/outline.rs`
  (`RegionOutline` gains `atlas`, `frequency_hz`, `currency`, `zone_span`, `hemisphere`),
  `assets/locales/en-US/ui.ftl` and `keys.rs`
- Test: inline proptests in `region_atlas.rs`; `tests/game/` picker tests

**Interfaces:**
- Produces:
  - `fn equal_earth(lon: f64, lat: f64) -> (f64, f64)` and its numeric inverse
    `fn equal_earth_inverse(x, y) -> Option<(f64, f64)>` (Newton iteration, at most 10 steps).
    These replace `project`/`unproject`, with the map cropped to 60°S–75°N.
  - The palette comes from `atlas.colour` when present (A–H per §28.10), and otherwise from the
    existing `COLOURS` fallback.

- [ ] **Step 1: Write the failing tests**

- `equal_earth_round_trips` (a proptest across the cropped extent, tolerance 1e-6°), plus
  `equal_earth_matches_published_values` at (0, 0), (180, 0) and (0, 90), using the projection's
  published constants A1 = 1.340264, A2 = −0.081106, A3 = 0.000893, A4 = 0.003796.
- `atlas_letters_and_colours_come_from_region_data`.
- `cards_show_frequency_currency_zones_and_hemisphere`.
- `every_region_is_startable_from_its_card_at_minimum_window_size`.
- `atlas_labels_meet_contrast` (painted glyph check at 4.5:1).

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo nextest run --workspace -E 'test(equal_earth) | test(atlas_letters) | test(cards_show) | test(startable_from_its_card) | test(atlas_labels)'`
Expected: they fail.

- [ ] **Step 3: Implement**
  - Draw the backdrop from a coarse built-in world coastline. Embed it as a static asset in the
    game crate; it is not region data.
  - Keep hover, click, keyboard start, seed and disabled behaviour (§2.6).

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo nextest run --workspace` and then `cargo run -p game -- --exit-after-frames 120`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: world region picker in Equal Earth"
```

---

### Task 10: Migrate both North American regions to schema 4 (rules 16)

**Files:**
- Modify:
  - `assets/regions/eastern_interconnection.ron` and `assets/regions/western_interconnection.ron`;
  - `crates/domain/src/lib.rs:23` (`SIM_RULES_VERSION = 16`);
  - the rules guard snapshot and scenario snapshots (`cargo insta review`);
  - the spec §28 status line;
  - `assets/locales/en-US/ui.ftl` (zone and city keys).
- Test: the existing beginner, early-expansion, rules-guard and scenario tests;
  `crates/infrastructure/tests/` golden saves

**Interfaces:**
- Consumes: everything above.
- Authored values, which are this task's tuning, pinned in RON:
  - **East zones:**
    - ET (−300; DST March–October) is the system zone;
    - CT (−360; DST March–October);
    - Saskatchewan CST (−360, no DST);
    - AT (−240; DST March–October).
  - **East cities:** ET for New York, Washington, Philadelphia, Toronto, Miami, Charlotte,
    Atlanta, Tampa, Boston, Detroit, Pittsburgh, Cincinnati, Columbus and Cleveland. CT for
    Chicago, Nashville, New Orleans, St Louis, Minneapolis, Kansas City, Oklahoma City, Winnipeg
    and Amarillo. Saskatchewan CST for Regina, and AT for Halifax.
  - **West zones:**
    - PT (−480; DST March–October) is the system zone and covers Baja California;
    - MT (−420; DST March–October) covers Alberta, El Paso and Boise;
    - Arizona MST (−420, no DST).
  - **Calendar:** northern meteorological. **Currency:** USD.
  - **Climates:** the current shapes become one climate per region. Optionally split a second
    climate where the data task justifies it.
  - Re-author irradiance without the latitude effect, so that `solar_potential` keeps each
    region's current relative site ranking.

- [ ] **Step 1: Capture the archive goldens before editing**

Add `archived_na_saves_resume_identically`. For a schema-3 save of each region, mid-Operating
with a charged battery, the save restores byte-identically, continues 60 ticks, and matches the
state hash and reports recorded in the test under the old region bytes. Run it and confirm it
passes against the unmodified files.

- [ ] **Step 2: Rewrite both RON files to schema 4 with the values above**, then run the tests.

Run: `cargo nextest run --workspace`
Expected: the beginner, §18.3 and rules-guard tests may fail. The archive goldens must pass.

- [ ] **Step 3: Retune region data only** (starter fleets, cash, irradiance and climate scale)
  until these pass on both regions, seeds 0–7:
  - the no-command and hands-off year-one runs (§26.7);
  - the early-expansion acceptance (§18.3).

  Never change rules to pass.

- [ ] **Step 4: Bump `SIM_RULES_VERSION` to 16 and regenerate the rules guard and snapshots**,
  reviewing every diff with `cargo insta review`.

Run: `cargo nextest run --workspace`, clippy, `cargo fmt --all --check`, and
`cargo run -p game -- --exit-after-frames 120`
Expected: everything is green.

- [ ] **Step 5: Isolated playtest.** Run a full representative day in each North American region
  in a scratch save directory. Check zone-labelled time, season, lighting arc, currency text and
  picker selection. Don't open the window while the user may be playtesting.

- [ ] **Step 6: Commit and update the spec status**

```bash
git commit -am "feat: move North American regions to schema 4 (rules 16)"
```
