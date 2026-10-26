# Plan 7: Five Single-Area Regions Implementation Plan (Milestone 2)

**Status:** Implemented 2026-10-26 under rules 18; region schema stays 4 and portable state schema
stays 3. Seven regions ship. The isolated native launch smoke passed (controller run 2026-10-26,
exit 0). The IPS/UPS and Brazil playtests (Task 10, Step 4) are pending: the user plays them by hand.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship five new playable single-area regions: Southern China, Brazil, India–Nepal–Bhutan,
IPS/UPS and Continental Europe–Maghreb. Each uses local, sourced money and construction pacing.
Europe–Maghreb opts into country construction profiles, and the pre-built Gibraltar link joins
Seville to Tangier. Every region passes the beginner and early-expansion acceptance.

**Architecture:** This plan is region data plus the tools and tests that keep it honest. No
simulation rule changes.

- A sourced research note (Task 1) holds every real-world number.
- Region RON holds only game tuning, and cites research rows by ID.
- Offline authoring tools turn public-domain geography and the research map into RON polygons and
  economic-zone cell maps. Tests regenerate their output and compare it.
- Shared acceptance tests iterate one list of shipped regions.
- Each region task adds its file, its locale keys, its region-specific checks and its ID in that
  list.

**Tech Stack:** Rust 1.98, Bevy 0.20, bevy_egui 0.43 (egui 0.36), serde/ron, proptest, insta,
cargo-nextest. Python 3 (standard library only) for the offline geography tools.

**Spec:** [docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md](../specs/2026-10-18-megawatt-mogul-design.md),
Amendment A14 (§28): §28.2–§28.3, §28.4–§28.8 as they apply to the new data, §28.10 for the cards,
§28.11's versioning, and §28.12's Regions, Gibraltar, Construction-economics and Saves acceptance.

**Handoffs consumed:**
- [Plan 6a Task 6](2026-10-25-plan-6a-economic-zones.md#task-6-region-authoring-handoff-to-plan-7)
  is binding scope here (Tasks 1, 2 and 9).
- Plan 6's unclaimed check is also in scope: frame budgets at the 256 × 128 maximum grid (Task 4).

**Out of scope:** SGCC, HVDC and multi-area islands (§28.9) are Plan 8. Plan 7 ships seven
regions, not eight.

## Global Constraints

- **Base commit:** start from `codex/m2-global-foundations` at or after b9108e0, with rules 17,
  region schema 4 and portable state schema 3. This plan bumps `SIM_RULES_VERSION` from 17 to 18
  exactly once, in Task 10 (§28.11). Portable state stays 3 and region schema stays 4.
- **Region IDs and colours** (§28.3, §28.10):
  - `southern_china` `#008F9D`
  - `brazil_sin` `#1E8F59`
  - `india_nepal_bhutan` `#B29400`
  - `ips_ups` `#7652B2`
  - `continental_europe_maghreb` `#226EC7`
- **Frequency and currency:** 50 Hz everywhere except `brazil_sin`, which is 60 Hz. Currencies are
  CNY, BRL, INR, RUB and EUR, the region-wide currency for every city, including Hong Kong, Macao,
  Nepal, Bhutan, Kazakhstan and the Maghreb.
- **Where numbers live:**
  - All tuning numbers are in region RON. All real-world figures are in the Task 1 research note,
    under row IDs matching `P7-[A-Z]{2,3}-\d{3}`.
  - Every catalog entry, tariff pair, line block, starting cash and economic-zone profile in a new
    region file has a comment directly above it citing at least one row. A test enforces this
    (Task 3).
  - No US-market figure is converted into another region's money. US rows serve only as the
    pacing reference (Task 1).
- **Footprints** (§28.3):
  - The grid-operation rule settles membership: an area belongs to the region whose grid operates
    it.
  - Crimea, including Sevastopol, is excluded from both Europe–Maghreb and IPS/UPS, using the one
    canonical polygon set from Task 2. Mainland Ukraine is in Europe–Maghreb (user decision,
    2026-10-25; spec §28.3).
  - For each area the notes don't address, the region file records its operator source.
- **Grids:**
  - At most 256 × 128 cells.
  - Equirectangular cells within 25% of square in km at the region's middle latitude.
  - IPS/UPS uses about 1.0° × 0.5° cells.
- **Cities:**
  - 18 to 40 cities per new region, keeping real relative sizes.
  - Total base load within ±25% of `40,850 MW × capacity ÷ 980`, with capacities EM 1350,
    India 520, Southern China 500, IPS/UPS 345 and Brazil 250 (research notes).
  - The capacities are test data only and never enter region RON.
- **Starting territories** (§28.3), each with my expansion city for §18.3:

  | Region | Home | Starter cities | Expansion city |
  |---|---|---|---|
  | Southern China | Guangzhou | Foshan, Dongguan | Shenzhen |
  | Brazil | São Paulo | Campinas, Santos | Sorocaba |
  | India–Nepal–Bhutan | Delhi | Gurugram, Noida | Faridabad |
  | IPS/UPS | Moscow | Tula, Tver | Kaluga |
  | Europe–Maghreb | Seville | Málaga, Tangier | Córdoba |

- **Pacing ceilings:** build months stay within the existing pacing test's ceilings, including
  every economic-zone override: Peaker 3, Solar 3, CCGT 6, Coal 12, Hydro 18 and Nuclear 24.
- **Region-wide parameters:**
  - Protection and performance values in `frequency` match the shipped North American defaults.
    Only `nominal_hz` differs.
  - Currency-neutral `regulator` and `satisfaction` ratios match the North American values. Only
    `starting_tariff_per_mwh` and `reference_tariff_per_mwh` are local.
- **Repository rules:**
  - `domain` is unchanged except where a task names it. No new crate dependencies; the Python
    tools use the standard library only.
  - There is one game test binary. Ignored timing tests live in it, so no new binary is linked.
    Every I/O test gets its own TempDir.
  - New player-facing text goes in `assets/locales/en-US/*.ftl`; new literal keys also go in
    `crates/game/src/l10n/keys.rs`.
- **Before every commit:** `cargo nextest run --workspace`, clippy with `-D warnings` and
  `cargo fmt --all --check`. Never `--no-verify`. Don't push or merge without the user's
  integration choice.
- **Ask the user first** before:
  - any download, stating name, URL and size;
  - any step that opens a game window (launch smoke, native playtests), since they may be
    playtesting.

## Review Focus

1. **Excluded and overlapping territory.** A city or buildable cell in Crimea, Kaliningrad,
   Yunnan, Bangladesh or Paraguay would contradict the footprint. So would a cell
   playable in both Europe–Maghreb and IPS/UPS. Tasks 2, 3, 8 and 9 test landmarks and overlap.
2. **Cross-zone starter assets.** The Gibraltar starter line runs from a Spain cell to a Morocco
   cell. Its original rate base must equal §28.7.1's route valuation, not `N × cost_per_cell`.
   It must survive save/resume, and the economic-zone cells must stay reproducible from their
   authoring polygons whenever land changes (Tasks 2 and 9).
3. **Fractional and many-zone clocks in a real territory.** With Kathmandu (+5:45) and Thimphu
   (+6:00) connected to an IST system, Planning must sample `{0, 30, 45, 59}`; a Delhi-only start
   samples `{0, 59}`. IPS/UPS spans seven offsets. Moscow's evening peak must not be the system
   peak once Siberian cities connect (Tasks 7 and 8).
4. **Polar light and the equator crossing.**
   - Murmansk's December solar output is zero all day, and its June output at local midnight is
     positive.
   - Brazil's grid crosses the equator (Roraima), so its hemisphere card reads Both.
   - Lighting never produces NaN at any shipped home city (Tasks 6 and 8).
5. **Large local money.** RUB and INR values run 80–100 times USD magnitudes. Quotes, the ledger
   and checked sums must not overflow. Text at or above 1,000B (for example ₹1,234B) must fit the
   HUD at the minimum window, with k/M/B suffixes only (§28.7). Covered in Tasks 3 and 10.

---

### Task 1: Sourced country and regional economics research

This task produces documents only. It runs before any region data and gates Tasks 5–9.

**Files:**
- Create: `docs/research/2026-10-25-plan-7-regional-economics.md`
- Modify: `docs/research/2026-10-18-world-power-grids-top-8.md`, adding one line in "Supporting
  data and provenance" that links the new note.

**Interfaces:**
- Produces:
  - The row table every region file cites.
  - A pacing table.
  - Profile proposals for Europe–Maghreb.
  - The IPS/UPS opt-in decision.
  - A gap list.
- **Row table.** Columns, in order:
  - `ID`, such as `P7-ES-004`;
  - `Region`;
  - `Market` (country or subnational area);
  - `Technology` (`PlantKind`, `AcLine`, `Tariff` or `Fx`);
  - `Metric` (`capital_per_kw`, `build_months_real`, `fixed_om_per_kw_year`, `fuel_or_marginal`,
    `startup`, `line_cost_per_km`, `line_build_km_per_month`, `retail_tariff_per_mwh` or
    `fx_rate`);
  - `Value as stated`, `Unit/currency`, `Price year`;
  - `Normalised value (region currency)`, with the FX row ID it used;
  - `Source`: publisher, title, URL, publication date, access date, table or page locator;
  - `Scope notes`.
- **Area codes:**
  - `US` is the pacing reference only.
  - `SC`, `BR`, `IN` and `IPS` are region-wide baselines.
  - Countries use ISO alpha-2 inside Europe–Maghreb: `ES`, `PT`, `FR`, `DE`, `IT`, `PL`, `TR`,
    `MA`, `DZ`, `TN` and others as sourced. Kazakhstan (`KZ`) and Uzbekistan (`UZ`) may appear
    if IPS/UPS opts in.
  - `EM` is Europe–Maghreb's regional default.

- [x] **Step 1: Gather the US pacing references**

  Use one dated, published US source family per metric, such as EIA *Capital Cost and Performance
  Characteristics* or NREL ATB. Cover:
  - capital per kW and a typical real construction duration for all eight `PlantKind`s;
  - AC transmission cost per km;
  - fixed O&M;
  - fuel/marginal cost;
  - average US retail tariff.

- [x] **Step 2: Compute the pacing table**

  For each money metric and kind:
  - `ratio = shipped game value ÷ US real value`, averaging Eastern and Western where both ship the
    kind.
  - Game values come from `assets/regions/{eastern,western}_interconnection.ron`.
  - Per-kW figures use the catalog `capacity_mw`, and per-km figures use the cell size in km at
    each region's middle latitude.

  For durations: `month_ratio = shipped build_months ÷ US real months`.

  **Pacing method (my call).** Each region task computes:
  - `game value = local real value × ratio`, rounded to 3 significant figures;
  - `game months = max(1, round(local real months × month_ratio))`, clamped to the pacing
    ceilings.

  Write this formula verbatim in the note. Starting cash, starter fleets and city loads are tuned
  freely, not derived.

- [x] **Step 3: Gather the local baselines**

  For each of SC, BR, IN and IPS, cover every `PlantKind` the region's catalog will carry: capital
  per kW, real build duration, fixed O&M, fuel/marginal cost, AC line cost, and a representative
  regulated or average end-user tariff at local price levels.

  Source preference:
  1. local-currency national sources (regulators, ministries, national planners, operators);
  2. country-specific international measurements (IRENA, IEA, World Bank), recorded with a dated
     FX row.

  A US or global-average figure is never a local baseline.

- [x] **Step 4: Gather Europe–Maghreb**

  Cover:
  - an EM regional-default baseline;
  - complete Spain and Morocco profiles for at least one same-kind technology: capital **and** real
    duration (for example solar PV, onshore wind or CCGT);
  - every other country market where whole-project sources exist.

  Rules:
  - Record EUR normalisation with a dated FX row for MAD, DZD, TND, TRY, PLN and others.
  - No wage-to-capital multiplier and no blanket Maghreb discount (§28.7.1).
  - Unsourced countries use the regional default (zone 0). List them under "Regional default by
    authoring choice".

- [x] **Step 5: Write the profile proposal and the decisions**

  1. **Profile proposal.** For each proposed Europe–Maghreb zone, give its id, name key
     (`economic-zone-<country>`), the countries it covers, and the plant kinds and AC terms it
     overrides, with row IDs.
  2. **The required contrast.** Show that Spain and Morocco differ by at least 10% in capital
     **and** at least 1 game month in duration for one same kind, after pacing.
  3. **IPS/UPS decision.** Default: omit `economic_zones` and state why. Opt in only if both
     Russia and Kazakhstan or Uzbekistan have sourced whole-project figures for one same
     technology.
  4. **Gaps.** List what was searched and not found.

- [x] **Step 6: Stop if the contrast can't be sourced**

  If no technology supports the required Spain–Morocco contrast in both cost and duration, stop.
  Report the evidence to the user and ask for a decision. Don't weaken the spec's requirement by a
  ruling.

- [x] **Step 7: Verify**

  - Every row has a non-empty URL, publication date, access date and locator.
  - Row IDs are unique.
  - Every normalised value names its FX row.
  - The note contains no quotation longer than 15 words.
  - A reviewer re-fetches at least 10 rows chosen across regions, including every Spain–Morocco
    contrast row, and confirms each stated value.

- [x] **Step 8: Commit**

```bash
git add docs/research/2026-10-25-plan-7-regional-economics.md docs/research/2026-10-18-world-power-grids-top-8.md
git commit -m "docs: sourced regional economics for Plan 7"
```

---

### Task 2: Offline geography and economic-zone authoring tools

**Files:**
- Create:
  - `tools/region_authoring/natural_earth.py` (polygon extraction and simplification);
  - `tools/region_authoring/research_svg_extent.py` (a research-map provenance check);
  - `tools/region_authoring/test_region_authoring.py` (unittest);
  - `tools/region_authoring/README.md` (dataset release, SHA-256 values, commands).
- Create:
  - `crates/infrastructure/authoring/ukraine_ips_operating_extent.ron`;
  - `crates/infrastructure/examples/author_economic_zone_cells.rs`.
- Modify: `crates/infrastructure/src/region/raster.rs` (zone rasteriser) and
  `crates/infrastructure/src/region/mod.rs` (its export).
- Test: `raster.rs` inline tests and `crates/infrastructure/tests/region_catalog.rs`.

**Interfaces:**
- Produces:
  ```text
  python3 -I tools/region_authoring/natural_earth.py \
      --admin0 <ne_50m_admin_0_countries.geojson> --admin1 <ne_50m_admin_1_states_provinces.geojson> \
      --include ISO:ESP,ISO:MAR,... --include-admin1 "RUS:Moskva,..." --exclude-admin1 "UKR:Crimea,..." \
      --bounds W,E,S,N --tolerance-deg T
  ```
  This prints RON `land: [...]` and `exclude: [...]` polygon lists, as `(lon, lat)` with 3 decimals,
  plus a provenance comment naming the dataset release, file SHA-256 and selection.
  Simplification is Douglas–Peucker at `T`, half a cell by default. It clips to the bounds and
  drops rings that fall under 2 cells' area.
- Produces: `research_svg_extent.py <svg> --check-extent <ron>` checks the canonical exclusion
  against the research map (`World_Power_Grids_Top_8.svg`, SHA-256 recorded in the README).
  *Amended 2026-10-25: the map has no violet Ukraine patch, so it cannot supply the polygon.*
  It does the following:
  1. Calibrate pixels to Equal Earth from the graticule `line2d` ticks.
  2. Inverse-project with Newton iteration, using A1 = 1.340264, A2 = −0.081106, A3 = 0.000893 and
     A4 = 0.003796.
  3. Assert that the violet Crimea point marker lies inside the extent.
  4. Assert that no `#7652b2` patch covers mainland Ukraine.

  The extent itself is the Natural Earth 10m admin-1 "Crimea" and "Sevastopol" polygons, from
  `natural_earth.py`. Its 2-cell area floor applies to land rings only; exclude rings are never
  dropped.
- The output file `crates/infrastructure/authoring/ukraine_ips_operating_extent.ron` is
  `(polygons: [[(lon, lat), ...], ...])`.
- Produces:
  ```rust
  // infrastructure::region
  pub struct ZonePolygons { pub id: u16, pub polygons: Vec<Vec<(f64, f64)>> }
  pub fn economic_zone_cells(file: &RegionFileV4, zones: &[ZonePolygons])
      -> Result<Vec<u16>, RegionLoadError>;
  ```
  The result is row-major from the northwest cell:
  - a non-playable cell gets `0`;
  - a playable cell whose centre lies in exactly one zone's polygons gets that zone's id;
  - a playable cell in no zone gets `0`.

  A playable cell claimed by two zones is `InvalidShape { field: "economic_zones.cells" }`.
  Point-in-polygon must use the same even-odd test as the land rasteriser.
- The zone authoring input lives in
  `crates/infrastructure/authoring/<region>_economic_zones.ron` as
  `(zones: [(id: u16, polygons: [[(lon, lat), ...]])])`. The `author_economic_zone_cells` example
  reads `<region.ron> <zones.ron>` and prints the `cells: [...]` block, with 20 values per line and
  a row comment every row.

- [x] **Step 1: Write the failing tests**

  - **Python** (`test_region_authoring.py`):
    - `test_douglas_peucker_keeps_endpoints_and_drops_collinear`;
    - `test_clip_keeps_rings_inside_bounds`;
    - `test_equal_earth_inverse_round_trips` (to 1e-6° across 60°S–75°N);
    - `test_equal_earth_matches_published_values` at (0, 0) and (180, 0), matching Plan 6
      Task 9's constants;
    - `test_svg_calibration_recovers_graticule_ticks` (on a synthetic two-tick SVG).
  - **Rust** (`raster.rs`):
    - `zone_cells_follow_cell_centres`: two square zones on a 10 × 10 fixture;
    - `zone_cells_are_zero_off_land_and_outside_every_zone`;
    - `a_cell_claimed_by_two_zones_is_rejected`.
  - **Rust** (`region_catalog.rs`): `generated_zone_cells_load_through_the_catalog`. Generate cells
    for `crates/infrastructure/tests/support/economic_region_v4.ron`'s grid with two zone
    polygons, write them into a TempDir copy, and load through `RonRegionCatalog`.

- [x] **Step 2: Run them and confirm they fail**

  Run: `python3 -I -m unittest tools/region_authoring/test_region_authoring.py` and
  `cargo nextest run -p infrastructure -E 'test(zone_cells) | test(claimed_by_two_zones) | test(generated_zone_cells)'`

  Expected: the modules and functions don't exist yet.

- [x] **Step 3: Implement the tools and the rasteriser**

  Python reads GeoJSON with `json` only.

- [x] **Step 4: Ask the user before downloading**

  The files are Natural Earth 1:50m `ne_50m_admin_0_countries.geojson` and
  `ne_50m_admin_1_states_provinces.geojson`, from the `nvkelso/natural-earth-vector` release tag
  recorded in the README. The data is public domain. State the URLs and sizes when asking.

  Store the files under `target/natural-earth/`, which is git-ignored. Never commit them.

- [x] **Step 5: Generate the canonical exclusion**

  1. Generate `ukraine_ips_operating_extent.ron` from the Natural Earth admin-1 Crimea and
     Sevastopol polygons.
  2. Run the `research_svg_extent.py --check-extent` provenance check on
     `~/Downloads/World_Power_Grids_Top_8.svg`.
  3. Commit the extent with a header comment: the dataset release and SHA-256, the SVG SHA-256,
     the commands, and "no sovereignty position".
  4. Don't commit the 7 MB SVG; its hash is the provenance.
  5. Check the output:
     - inside it: Sevastopol city on land, Simferopol (34.10, 44.95) and Kerch (36.47, 45.36);
     - outside it: Henichesk (34.82, 46.17), Kherson (32.62, 46.64), Donetsk (37.80, 48.00),
       Kyiv (30.52, 50.45), Odesa (30.73, 46.48) and Kharkiv (36.23, 49.99).

- [x] **Step 6: Run the tests and confirm they pass**

  Run: both commands from Step 2, then `cargo nextest run --workspace`.

  Expected: everything passes.

- [x] **Step 7: Commit**

```bash
git add tools/region_authoring crates/infrastructure/authoring crates/infrastructure/examples/author_economic_zone_cells.rs crates/infrastructure/src/region
git commit -m "feat: offline region geography and economic-zone authoring tools"
```

---

### Task 3: Shared shipped-region acceptance and presentation plumbing

**Files:**
- Modify:
  - `crates/infrastructure/tests/shipped_regions.rs`:
    - add `SHIPPED`, `NEW_REGIONS`, `RESEARCH_CAPACITY_GW` and `EXPANSION_CITY` tables;
    - generalise the region-agnostic tests;
    - add the authoring-rule and provenance tests;
  - `crates/game/src/hud/menu.rs:395-405` (description keys derived from `name_key`);
  - `assets/locales/en-US/ui.ftl:368-369` (move to `regions.ftl` as
    `region-eastern-interconnection-description` and `region-western-interconnection-description`);
  - `crates/game/tests/game/l10n_coverage.rs:911-935` (zone, economic-zone, line-name and
    description keys);
  - `crates/game/tests/game/save_resume.rs:41-42` (iterate a shared list);
  - `crates/game/tests/game/support/mod.rs` (`SHIPPED_REGIONS`).
- Test: the same files.

**Interfaces:**
- Produces:
  - `const SHIPPED: &[&str]` in `shipped_regions.rs`, sorted like `RonRegionCatalog::list`.
    It starts as `[EAST, WEST]`, and each region task inserts its ID.
  - `NEW_REGIONS: &[&str]` is every ID added by Plan 7. The provenance, scale and squareness tests
    iterate it.
  - `EXPANSION_CITY: &[(&str, &str)]` maps a region ID to its §18.3 city key. It replaces the
    `if id == EAST` at `shipped_regions.rs:686`.
  - `RESEARCH_CAPACITY_GW: &[(&str, f64)]` holds
    `[("eastern_interconnection", 980.0), ("western_interconnection", 340.0), ...]`.
  - `HOME_CITY: &[(&str, &str)]` maps a region ID to its home-city key. It replaces the inline pairs
    at `shipped_regions.rs:241`.
  - `game::tests::support::SHIPPED_REGIONS` holds the same IDs for the game binary.
  - The menu description key is `format!("{}-description", region.name_key)`. Missing keys show no
    description, as now.
- **Year-one test layout.** Generate one `#[test]` per region and mode with a small
  `macro_rules!`, so nextest runs regions in parallel. Name them
  `year_one_hands_off_<id>` and `year_one_no_commands_<id>`. The 0–7 seed loop stays inside each
  test.
- **Region-agnostic tests** iterate `SHIPPED`:
  - year-one (both modes);
  - first expansion;
  - start without shortfall;
  - one-month smoke;
  - no hands-off outages;
  - tycoon pacing, which also checks every economic-zone override's build months;
  - frequency/performance defaults (protection fields equal, `nominal_hz` per region);
  - home-city anchoring (home key from a `HOME_CITY` table);
  - `cities_are_named_metros`, using `18..=40`.

  North America-specific tests stay as they are: `east_is_three_to_five_times_west`, the West
  hydro tests, battery four-pm and solar migration.

- [x] **Step 1: Write the failing tests**

  - `ships_exactly_the_listed_regions` replaces `ships_exactly_two_regions`:
    `assert_eq!(ids, SHIPPED)`.
  - `new_region_cells_are_nearly_square`: at the middle latitude,
    `max(w_km, h_km) / min(w_km, h_km) <= 1.25`, and the grid is at most 256 × 128.
  - `new_region_load_scales_with_capacity`: `total / (40_850.0 * cap / 980.0)` is in `0.75..=1.25`.
  - `new_region_entries_cite_research_rows`:
    - Scan each new region's RON text.
    - Every `kind: <PlantKind>` line inside `catalog: [`, every `name_key: "economic-zone-` line,
      the `lines:` line, `starting_tariff_per_mwh` and `cash:` each need a comment, directly above
      it, containing an ID matching `\[P7-[A-Z]{2,3}-\d{3}\]`.
    - Every cited ID must appear as a table row (`| P7-… |`) in the research note.
  - `shipped_regions_do_not_overlap`: no cell centre of one region is playable land in another.
    Check every pair, using the existing `is_land` helper.
  - `excluded_extent_is_unplayable_everywhere`: no shipped region has a playable cell whose centre
    lies in `ukraine_ips_operating_extent.ron`.
  - `every_shipped_region_and_city_name_key_exists` also covers:
    - each zone's `name_key`;
    - each economic zone's `name_key`;
    - each `start.line_names` key;
    - `<region name_key>-description`.
  - `home_lighting_is_finite_for_every_shipped_region`: for every shipped region, month and tick,
    `sun_at_home` is `None` (a daylight table) or has finite elevation and azimuth.
  - `money_at_a_thousand_billion_fits` (`format.rs` tests): pin `money` for ₹1,234,000,000,000 and
    ₽5,000,000,000,000 to the current renderer's output, keeping the B suffix. Fit-test the widest
    top-bar cash at the minimum window in the game binary.

- [x] **Step 2: Run them and confirm they fail**

  Run: `cargo nextest run --workspace -E 'test(ships_exactly_the_listed) | test(nearly_square) | test(scales_with_capacity) | test(cite_research) | test(do_not_overlap) | test(excluded_extent) | test(name_key_exists) | test(thousand_billion)'`

  Expected:
  - The new tests fail to compile or fail the description-key check.
  - With `NEW_REGIONS` empty, the per-region tests pass vacuously, and the `every_..._key_exists`
    test fails only for the renamed description keys.

- [x] **Step 3: Implement the tables, the macro, the key derivation and the moved strings**

  Keep North American text byte-identical; only the keys move.

- [x] **Step 4: Run the tests and confirm they pass**

  Run: `cargo nextest run --workspace`

  Expected: everything passes with `SHIPPED = [EAST, WEST]`.

- [x] **Step 5: Commit**

```bash
git commit -am "test: shared shipped-region acceptance tables and provenance checks"
```

---

### Task 4: Frame budgets at the 256 × 128 maximum grid

**Files:**
- Modify: `crates/game/tests/game/world.rs` (ignored timing tests and structural tests) and
  `crates/domain/src/region/fixtures.rs` (`max_grid_region()`, gated by `test-fixtures`: 256 × 128,
  varied relief, 40 cities, astronomical sun).
- Test: the same files.

**Interfaces:**
- Produces: `fixtures::max_grid_region() -> RegionDefinition`.
- **Budgets (my call).** The game draws at 60 fps, so a 16.7 ms frame. At 256 × 128, release
  build, on the development Mac:
  - `TerrainGeometry::build` ≤ 250 ms once per region load;
  - `pick_world` ≤ 1 ms per call;
  - the overlay refresh ≤ 4 ms;
  - one region-picker paint of all shipped outlines at the minimum window ≤ 4 ms.

- [x] **Step 1: Write the failing tests**

  - `terrain_mesh_is_built_once_per_region`: count `TerrainGeometry` builds across 30 updates with
    the camera moving. Expect 1.
  - `max_grid_mesh_stays_within_vertex_bound`: vertices ≤ `4 × cells + 4 × wall faces`, using the
    current owned-wall rule, and every index is in range.
  - `#[ignore] max_grid_frame_budgets`: time each item above over 100 iterations and assert the
    medians against the budgets.

- [x] **Step 2: Run them and confirm they fail**

  Run: `cargo nextest run -p game -E 'test(built_once_per_region) | test(max_grid_mesh)'`

  Expected: the fixture doesn't exist yet.

- [x] **Step 3: Implement the fixture**

  If a budget fails, fix the hot path without changing the picture. For example, reuse the mesh
  buffer or cache the overlay colour per cell. Don't relax a budget without recording a ruling.

- [x] **Step 4: Run the tests and confirm they pass**

  Run: `cargo nextest run --workspace`, then
  `cargo nextest run -p game --release --run-ignored ignored-only -E 'test(max_grid_frame_budgets)'`.

  Expected: everything passes. Record the measured medians in this task's report.

- [x] **Step 5: Commit**

```bash
git commit -am "test: frame budgets for the 256 x 128 maximum grid"
```

---

### Region authoring procedure (Tasks 5–9)

Every region task follows these steps. Its own section lists its values and its extra tests.

1. **Geography.** Write `tools/region_authoring/selections/<id>.args` holding the extractor
   arguments. Run `natural_earth.py` and paste the `land`/`exclude` output into
   `assets/regions/<id>.ron`.
   - Nudge shores so coastal metros and required islands (Malta, Crete, Hainan, Sicily) have at
     least one land cell. EI's file shows the precedent.
   - The header comment records the region's grid-operation footprint, the research-note section,
     the dataset release and each disputed-area ruling with its operator source.
2. **Grid and clock.**
   - `bounds` and `grid` satisfy the Task 3 squareness test.
   - `time_zones`, `system_zone`, `calendar`, `currency`, `atlas` and `name_key` (`region-<id-kebab>`)
     are as the task lists.
   - DST months mark a month when most of its days observe DST. EU zones use April–October.
3. **Cities.**
   - 18–40 named metros (`city-<kebab>`) with real coordinates.
   - Relative base loads come from sourced metro population or demand shares, cited in the comment.
   - The total sits in the Task 3 scale band.
   - Each city names a zone and a climate.
   - The home and starter cities are exactly those in the Global Constraints table.
4. **Climates and resources.**
   - Climates are hourly shapes per city group.
   - Monthly `wind`, `solar.clearness_mean`, `hydro.monthly_flow`, `irradiance` (Gaussian form),
     `wind_potential`, `hydro_potential` and `cooling_water` are authored from sources the comments
     cite: national meteorology, operators and resource atlases.
   - Irradiance is site quality without latitude (§28.5).
5. **Money.**
   - The catalog, `lines`, tariffs and cash come from Task 1's rows through its pacing formula.
   - Each entry's comment cites its rows and shows its arithmetic.
   - `frequency`, `regulator` and `satisfaction` follow the Global Constraints.
   - The battery windows (`discharge_start_hour`, `charge_end_hour`) are set on the system clock
     against the starter territory's aggregate net-load shape (§28.4).
6. **Start.**
   - Year 2027, month 1.
   - A compact starter fleet sited at real plants, named in comments and following the region's
     real mix.
   - It must cover the connected peak with the margin `start_fleet_covers_connected_peak_with_margin`
     uses.
7. **Locale.** Add the region name, `<name_key>-description` with the §28.3 lessons, cities, zone
   short names and any economic-zone or line-name keys to `regions.ftl` and `ui.ftl`. Insert the ID
   into `SHIPPED`, `NEW_REGIONS`, the expansion/home tables and `SHIPPED_REGIONS`.
8. **Tune.** Retune region data only (cash, starter fleet, city loads within the band, then
   recorded per-field pacing judgments) until every shared and region test passes on seeds 0–7.
   Never change rules or relax a shared check.
9. **Verify.** Run `cargo nextest run --workspace`, clippy and fmt. The region's
   `save_resume_mid_day_…` iteration must pass.

---

### Task 5: Southern China (`southern_china`)

**Files:**
- Create: `assets/regions/southern_china.ron` and `tools/region_authoring/selections/southern_china.args`
- Modify: `assets/locales/en-US/regions.ftl` and `ui.ftl`; the tables from Task 3
- Test: `crates/infrastructure/tests/shipped_regions.rs` (region module `southern_china`)

**Interfaces:**
- Consumes: Task 1 rows `P7-SC-*`, Task 2's extractor and Task 3's tables.
- Values:
  - **Footprint:** Guangdong, Guangxi, Guizhou, Hainan, Hong Kong and Macao (admin-0 HKG and MAC;
    admin-1 for the provinces). Yunnan is excluded.
  - **Bounds:** about 104–117.5°E and 18–29.5°N.
  - **Zone:** one, `zone-cst` = "CST" (+480, no DST), which is the system zone.
  - **Calendar:** northern meteorological. **Currency:** CNY. 50 Hz.
  - **Lessons** (description): Guizhou and Guangxi hydro with coastal nuclear.

- [x] **Step 1: Write the failing region tests**

  - `southern_china_landmarks`:
    - Guangzhou (113.26, 23.13), Hong Kong (114.17, 22.32), Macao (113.54, 22.20),
      Guiyang (106.63, 26.65), Nanning (108.37, 22.82) and Haikou (110.35, 20.02) are land;
    - Kunming (102.83, 24.88), Hanoi (105.85, 21.03) and Changsha (112.94, 28.23) are not.
  - `hainan_line_crosses_water_as_a_submarine_cable`:
    - From a new game, `line_quote` from Zhanjiang's node to Haikou's is accepted.
    - Its straight path has a non-empty `water_cells_on_segment`.
- [x] **Step 2: Run them and confirm they fail** (`southern_china` isn't listed yet).
- [x] **Step 3: Author the region** following the region authoring procedure.
- [x] **Step 4: Run** `cargo nextest run --workspace`. Expected: everything passes.
- [x] **Step 5: Commit**: `feat: ship the Southern China region`

---

### Task 6: Brazil (`brazil_sin`)

**Files:** as in Task 5, for `brazil_sin`.

**Interfaces:**
- Values:
  - **Footprint:** Brazil's mainland (admin-0 BRA), as the main-grid geographic proxy (research
    notes). Paraguay, Argentina, Uruguay and Venezuela are excluded. Roraima is in.
  - **Bounds:** about 74–34°W and 34°S–5.5°N. The grid crosses the equator.
  - **Zones:**
    - `zone-brt` "BRT" (−180), the system zone;
    - `zone-amt` "AMT" (−240), for Manaus and Porto Velho;
    - `zone-act` "ACT" (−300), for Rio Branco.
    - None has DST.
  - **Calendar:** `[Summer, Summer, Autumn, Autumn, Autumn, Winter, Winter, Winter, Spring, Spring,
    Spring, Summer]`. **Currency:** BRL. **60 Hz.**
  - **Hydro:** `monthly_flow` is high from December to April.
  - **Wind:** monthly values peak from August to October (northeastern wind).
  - **Itaipu:** its 60 Hz units are a `hydro_potential` blob at Foz do Iguaçu.
  - **Lessons:** a hydro system with wet and dry seasons, plus northeastern wind.

- [x] **Step 1: Write the failing region tests**

  - `brazil_landmarks`:
    - São Paulo, Boa Vista (−60.67, 2.82), Manaus, Rio Branco and Fortaleza are land;
    - Asunción (−57.58, −25.26), Montevideo and Caracas are not.
  - `brazil_january_is_summer`:
    - `season(month(1)) == Summer`;
    - São Paulo's January demand uses its summer shape;
    - its December daylight is longer than June's.
  - `brazil_wet_season_and_northeast_wind`:
    - the mean Dec–Apr flow is above the mean Jun–Oct flow;
    - the maximum monthly wind falls in Aug–Oct.
  - `brazil_card_reads_both_hemispheres` (an outline test): `Hemisphere::Both`.
  - `brazil_home_lighting_points_north_at_noon`: `sun_at_home` azimuth is within 20° of north at
    São Paulo's solar noon in January.
- [x] **Step 2: Run them and confirm they fail.**
- [x] **Step 3: Author the region** following the region authoring procedure.
- [x] **Step 4: Run** `cargo nextest run --workspace`. Expected: everything passes.
- [x] **Step 5: Commit**: `feat: ship the Brazil region`

---

### Task 7: India–Nepal–Bhutan (`india_nepal_bhutan`)

**Files:** as in Task 5, for `india_nepal_bhutan`.

**Interfaces:**
- Values:
  - **Footprint:** mainland India (admin-0 IND, including the Indian-operated north), Nepal and
    Bhutan.
  - **Excluded:** Bangladesh (an exclude polygon), Sri Lanka, Myanmar, and the Andaman, Nicobar and
    Lakshadweep islands (isolated systems).
  - **Bounds:** about 68–97.5°E and 7.5–37°N.
  - **Zones:**
    - `zone-ist` "IST" (+330), the system zone;
    - `zone-npt` "NPT" (+345);
    - `zone-btt` "BTT" (+360).
    - None has DST.
  - **Calendar:** `[Winter, Winter, Summer, Summer, Summer, Monsoon, Monsoon, Monsoon, Monsoon,
    PostMonsoon, PostMonsoon, PostMonsoon]` (IMD). **Currency:** INR. 50 Hz.
  - **Monsoon resources:** wind and hydro are highest in June–September, and clearness is lowest
    then.
  - **Climates:** a demand peak before the monsoon (May), and an evening ramp as solar falls.
  - **Lessons:** the monsoon and the evening solar ramp.

- [x] **Step 1: Write the failing region tests**

  - `india_landmarks`:
    - Delhi, Leh (77.58, 34.15), Kathmandu (85.32, 27.72), Thimphu (89.64, 27.47) and Chennai are
      land;
    - Dhaka (90.41, 23.81), Colombo (79.86, 6.93) and Port Blair (92.73, 11.62) are not.
  - `india_labels_follow_the_imd_calendar`:
    - the HUD season label for July is Monsoon;
    - `calendar().run_length(month(1)) == 2`.
  - `india_monsoon_resources`:
    - the June–September means of wind and flow exceed the year mean;
    - the June–September clearness is below it.
  - `nepal_and_bhutan_add_quarter_hour_samples`:
    - `hour_sample_minutes` over {Delhi, Kathmandu, Thimphu} equals `{0, 30, 45, 59}`;
    - over {Delhi} alone it equals `{0, 59}`.
- [x] **Step 2: Run them and confirm they fail.**
- [x] **Step 3: Author the region** following the region authoring procedure.
- [x] **Step 4: Run** `cargo nextest run --workspace`. Expected: everything passes.
- [x] **Step 5: Commit**: `feat: ship the India–Nepal–Bhutan region`

---

### Task 8: IPS/UPS (`ips_ups`)

**Files:** as in Task 5, for `ips_ups`. The file's `exclude` includes every polygon from
`crates/infrastructure/authoring/ukraine_ips_operating_extent.ron` verbatim.

**Interfaces:**
- Values:
  - **Footprint:** Russia's first synchronous zone, as an administrative approximation by admin-1
    (research notes, F), plus Belarus, Kazakhstan, Uzbekistan, mainland Azerbaijan, Kyrgyzstan,
    main Tajikistan and Georgia.
  - **Excluded:** the Far East zone (Amur, Khabarovsk, Primorye, the Jewish AO and southern
    Yakutia), isolated Russian systems (Norilsk/Taimyr, northern Yakutia, Chukotka, Magadan,
    Kamchatka, Sakhalin), Kaliningrad, Armenia, Nakhchivan, the Pamir (Gorno-Badakhshan),
    Turkmenistan, Iran, Afghanistan, Mongolia, and the Task 2 extent.
  - **Areas the notes don't address:** Abkhazia and South Ossetia follow the grid-operation rule,
    recorded with operator sources. Expected default: both inside IPS/UPS.
  - **Bounds:** about 23–122°E and 37–72°N, at about 1.0° × 0.5° cells.
  - **Zones** (my call: display names are UTC offsets apart from MSK):
    - `zone-msk` "MSK" (+180, including Belarus), the system zone;
    - `zone-utc-plus-4` "UTC+4" (+240: Samara, Azerbaijan, Georgia);
    - `zone-utc-plus-5` "UTC+5" (+300: Yekaterinburg, Kazakhstan, Uzbekistan, Tajikistan);
    - `zone-utc-plus-6` "UTC+6" (+360: Omsk, Kyrgyzstan);
    - `zone-utc-plus-7` "UTC+7" (+420: Novosibirsk, Krasnoyarsk);
    - `zone-utc-plus-8` "UTC+8" (+480: Irkutsk, Buryatia);
    - `zone-utc-plus-9` "UTC+9" (+540: Zabaykalsky).
    - None has DST.
  - **Calendar:** northern meteorological. **Currency:** RUB. 50 Hz.
  - **Required cities:** Murmansk (polar light), St Petersburg, Minsk, Novosibirsk, Irkutsk, Chita,
    Almaty, Tashkent and Baku.
  - **Economic zones:** follow Task 1's IPS/UPS decision. If omitted, the header comment states
    "economic_zones omitted by authoring choice" and gives the reason.
  - **Lessons:** six time zones, Siberian hydro and polar light.

- [x] **Step 1: Write the failing region tests**

  - `ips_landmarks`:
    - Moscow, Minsk, Almaty, Tashkent, Tbilisi, Baku, Novosibirsk, Irkutsk, Chita and Murmansk
      (33.08, 68.97) are land;
    - Kaliningrad (20.51, 54.71), Vladivostok, Khabarovsk, Yerevan, Tehran, Ulaanbaatar and
      Sevastopol are not.
  - `ips_exclude_contains_the_canonical_extent`: each canonical polygon appears unchanged in
    `exclude`.
  - `murmansk_polar_night_and_midnight_sun`:
    - a Solar plant at Murmansk's cell has zero available output at every December tick;
    - it has positive output at the June tick of Murmansk's local midnight.
  - `novosibirsk_sunrise_precedes_moscow_on_msk`.
  - `siberian_cities_flatten_the_system_peak`: connect every city in a test session. The
    aggregate forecast's peak-to-mean ratio is below the Moscow city's own ratio.
- [x] **Step 2: Run them and confirm they fail.**
- [x] **Step 3: Author the region** following the region authoring procedure.
- [x] **Step 4: Run** `cargo nextest run --workspace`. Expected: everything passes, including
  `shipped_regions_do_not_overlap` and `excluded_extent_is_unplayable_everywhere`.
- [x] **Step 5: Commit**: `feat: ship the IPS/UPS region`

---

### Task 9: Continental Europe–Maghreb (`continental_europe_maghreb`)

**Files:**
- Create: `assets/regions/continental_europe_maghreb.ron`, its selections file and
  `crates/infrastructure/authoring/continental_europe_maghreb_economic_zones.ron`.
- Modify: the locale files and Task 3's tables.
- Test: `shipped_regions.rs` (region module) and `crates/infrastructure/tests/economic_construction.rs`
  (shipped-profile scenarios).

**Interfaces:**
- Consumes: Task 1's EM rows and profile proposal, `economic_zone_cells` (Task 2) and the canonical
  extent (Task 2).
- Values:
  - **Footprint:** the Continental Europe synchronous area, including:
    - DK1 (Jutland and Funen only);
    - Türkiye, connected Ukraine and Moldova, the Baltic states;
    - Sicily, Malta and Crete;
    - Morocco, northern interconnected Algeria and Tunisia.
  - **Excluded:** the Nordic area, DK2 (Zealand, Lolland-Falster and Bornholm), Great Britain,
    Ireland, Sardinia, Corsica, the Balearics, isolated southern Algeria (a schematic boundary at
    about 30°N, recorded) and the Task 2 extent, verbatim.
  - **Areas the notes don't address:** each one's operator source is recorded. Expected defaults:
    - Transnistria and Kosovo are inside;
    - Western Sahara is inside only where a source shows ONEE's interconnected grid operating, with
      the rest excluded.
  - **Bounds:** chosen for the squareness test within 256 × 128, at about 0.4° × 0.3°.
  - **Zones:**
    - `zone-wet` "WET" (0, DST April–October; Portugal);
    - `zone-cet` "CET" (+60, DST April–October), the system zone;
    - `zone-cet-fixed` "CET" (+60, no DST; Morocco, Algeria and Tunisia);
    - `zone-eet` "EET" (+120, DST April–October; Greece, Bulgaria, Romania, Ukraine, Moldova and
      the Baltic states);
    - `zone-trt` "TRT" (+180, no DST).
  - **Calendar:** northern meteorological. **Currency:** EUR. 50 Hz.
  - **Gibraltar:**
    - Tangier's starter line from Seville is named in `start.line_names: [(<Tangier id>,
      "line-spain-morocco")]`.
    - Add `line-spain-morocco = Spain–Morocco interconnector` if the key doesn't exist yet.
  - **Economic zones:**
    - Write `economic_zones.zones` from Task 1's proposal, with IDs from 1 in the proposal's order
      and keys `economic-zone-<country>`.
    - `cells` is exactly `author_economic_zone_cells` output for the authoring file, whose
      polygons come from `natural_earth.py` country selections.
    - Unsourced countries stay at 0, listed in the header.
  - **Lessons:** a three-hour time-zone spread and Maghreb solar.

- [x] **Step 1: Write the failing region tests**

  - `europe_maghreb_landmarks`:
    - Seville, Tangier (−5.81, 35.76), Lisbon, Aarhus (10.20, 56.16), Palermo, Valletta
      (14.51, 35.90), Heraklion (25.14, 35.34), Istanbul, Kyiv, Donetsk, Chișinău, Tunis and Algiers
      are land;
    - Copenhagen (12.57, 55.68), London, Dublin, Oslo, Cagliari (9.11, 39.22),
      Palma (2.65, 39.57), Ajaccio, Tamanrasset (5.52, 22.79), Sevastopol and Simferopol are not.
  - `europe_maghreb_exclude_contains_the_canonical_extent`.
  - `messina_malta_and_crete_are_reachable_by_line` (§28.8): from a new game, `line_quote` is
    accepted between nodes placed at Reggio Calabria and Messina, Syracuse and Valletta, and the
    Peloponnese and Heraklion. Each path has a non-empty `water_cells_on_segment`.
  - `europe_maghreb_zone_cells_are_reproducible`: `economic_zone_cells(file, authoring)` equals
    the file's `economic_zones.cells`.
  - `spain_and_morocco_differ_in_cost_and_duration`:
    - For Task 1's contrast kind, the resolved specs at Seville's and Tangier's cells differ in
      `capital_cost` by at least 10% and in `build_months` by at least 1.
    - Both stay within the pacing ceilings.
  - `gibraltar_starts_served_named_and_valued_by_route`:
    - A new game serves Seville, Málaga and Tangier.
    - Tangier's starter line has name key `line-spain-morocco`, a non-empty water crossing and an
      original rate base equal to `RegionDefinition::line_terms` for that geometry.
    - The rate base differs from `N × lines.cost_per_cell` whenever Spain's and Morocco's AC terms
      differ.
  - **Shipped-profile scenarios** (`economic_construction.rs`): for each authored zone, build each
    overridden plant kind at a playable cell in that zone (my call: the cell nearest the zone's
    largest city). Check that the quote, instalment sum and commission month equal its resolved
    terms.
  - `europe_maghreb_mid_day_save_resumes_identically`: provided by `SHIPPED_REGIONS` in
    `save_resume.rs`, with a cross-zone order in flight.
- [x] **Step 2: Run them and confirm they fail.**
- [x] **Step 3: Author the region** following the region authoring procedure, then generate the
  zone cells.
- [x] **Step 4: Run** `cargo nextest run --workspace`. Expected: everything passes. Spain–Morocco
  differences come only from cited rows.
- [x] **Step 5: Tick Plan 6a's Task 6 boxes**, each with a link to the test or file that discharges
  it.
- [x] **Step 6: Commit**: `feat: ship the Continental Europe–Maghreb region with country construction profiles`

---

### Task 10: Rules 18, picker, documentation and release verification

**Files:**
- Modify:
  - `crates/domain/src/lib.rs:23` (`SIM_RULES_VERSION = 18`);
  - the rules guard and scenario snapshots;
  - the spec §28 status line;
  - Plan 6a's status line (Task 6 complete);
  - `ARCHITECTURE.md` and `TESTING.md` (shipped-region tables, the authoring tools, the provenance
    rule).
- Test: the existing picker tests (`crates/game/tests/game/region_picker.rs`) and the l10n
  coverage.

- [x] **Step 1: Check the picker with all seven regions at the minimum window**

  *Amended 2026-10-25:* the picker is now the interactive globe (§28.10, commit b9ac0ce). Its
  existing startability, card-layout, picking and contrast tests in
  `crates/game/tests/game/region_picker.rs` must pass over all seven listings at the minimum
  window. Card start and card-select rotation must work for every region.

- [x] **Step 2: Bump the rules version and regenerate**

  Bump `SIM_RULES_VERSION` to 18 and regenerate the rules guard and snapshots. Review every diff:
  the only expected change is the version line.

  Run: `cargo nextest run --workspace`, clippy, `cargo fmt --all --check` and
  `cargo nextest run -p game --release --run-ignored ignored-only -E 'test(max_grid_frame_budgets)'`
  (with the real IPS/UPS and Europe–Maghreb outlines in the picker).

  Expected: everything is green.

- [x] **Step 3: Launch smoke (ask the user first)**

  Run: `cargo run -p game -- --exit-after-frames 120`, with isolated `--log-dir` and save
  directories.

  Expected: exit 0.

  Controller run 2026-10-26: `cargo run -p game -- --exit-after-frames 120` with isolated scratch
  log/save directories exited 0. No panic or system-parameter conflict; only the known winit
  "Skipped event Destroyed" warning.

- [ ] **Step 4: Isolated native playtests (ask the user first)**

  Play IPS/UPS and Brazil (§28.12), one full representative day each, seed 7, with isolated saves
  and logs. Record:
  - the zone-labelled clocks;
  - lighting at night, morning, midday and dusk, including Brazil's northern noon sun;
  - local money text;
  - the review's served share and incidents.

  Never use the user's saves.

  Pending: the user plays IPS/UPS and Brazil by hand.

- [x] **Step 5: Update the status lines, then commit**

  Update the spec status (Plan 7 implemented under rules 18; Plan 8 pending), Plan 6a's status
  and the docs.

```bash
git commit -am "feat: ship five single-area regions (rules 18)"
```
