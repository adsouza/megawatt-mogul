# Plan 7 regional economics: sourced rows, pacing and Europe–Maghreb profiles

**Research edition: 25 October 2026.** This note is the single place where Plan 7's real-world money
and construction-time figures live. Region files for Southern China, Brazil, India–Nepal–Bhutan,
IPS/UPS and Continental Europe–Maghreb cite its rows by ID; tuning numbers live only in region RON
(spec §28.7, §28.7.1). Every row records a number and a locator, not prose. Where no source was
found, the gap is listed under [Gaps](#gaps) instead of being estimated.

## Conventions

- **Row IDs** are `P7-<area>-<nnn>`. Areas: `US` (pacing reference only), `FX` (exchange rates),
  `SC`, `BR`, `IN` and `IPS` (region-wide baselines), ISO alpha-2 countries inside Europe–Maghreb
  (`ES`, `MA`, `DE`, and `DZ` and `TN` from the 2026-10-26 follow-up), and `EM` (Europe–Maghreb
  regional default).
- **Columns** follow the Plan 7 Task 1 brief. *Normalised value* is in the region currency
  (CNY, BRL, INR, RUB or EUR) and always names an FX row; `FX: none` means the source is already in
  the region currency. US rows are not normalised: they are only the pacing reference, and no US
  figure is converted into another region's money.
- **Sources** give publisher, title, URL, publication date (`pub.`), access date (`acc.`) and
  locator (`loc.`). All rows were accessed on 2026-10-25, except the 2026-10-26 follow-up rows (`P7-DZ-*`, `P7-TN-*`
  and P7-FX-024 to P7-FX-030), which give their own access date. Rows from downloaded files also record the file's SHA-256.
- **Per-kW basis.** Capital and fixed O&M are per kW of net (AC) capacity. Battery capital is per
  kW of a 4-hour system, matching the shipped 200 MW / 800 MWh battery.
- **`fuel_or_marginal`** means fuel plus variable non-fuel O&M per MWh generated. Each row says
  which parts its source includes.
- **`build_months_real`** means construction (or the source's nearest equivalent), excluding
  development and permitting, unless the row says otherwise. Planned durations are marked
  *planned*.
- **Derived values** (division, midpoints, medians, cross rates) are marked *derived* and show their
  arithmetic. They are not presented as published figures.
- **Cell size in km** (for per-km pacing) is the geometric mean of a cell's east–west and
  north–south lengths at the region's middle latitude, using 111.195 km per degree (spherical Earth,
  R = 6,371 km): `w_km = Δlon/width × 111.195 × cos(mid_lat)`, `h_km = Δlat/height × 111.195`,
  `cell_km = √(w_km × h_km)`. Each region task uses the same convention for its own grid.

## Pacing method

This section is the pacing method of record (Plan 7 Task 1, Step 2). The formulas are verbatim:

- `ratio = shipped game value ÷ US real value`, averaging Eastern and Western where both ship the
  kind.
- Per-kW figures use the catalog `capacity_mw`, and per-km figures use the cell size in km at each
  region's middle latitude.
- For durations: `month_ratio = shipped build_months ÷ US real months`.
- Each region task computes:
  - `game value = local real value × ratio`, rounded to 3 significant figures;
  - `game months = max(1, round(local real months × month_ratio))`, clamped to the pacing ceilings.
- Starting cash, starter fleets and city loads are tuned freely, not derived.

Application notes (authoring conventions for the region tasks, not new rules):

- Round the per-kW (or per-km, per-MWh) game value to 3 significant figures, then multiply by the
  region catalog's `capacity_mw` × 1,000 (or by the region's `cell_km`) to get the RON field in
  whole major units.
- `round` is round-half-away-from-zero (Rust `f64::round`). The pacing ceilings are Peaker 3,
  Solar 3, CCGT 6, Coal 12, Hydro 18 and Nuclear 24 game months; Wind and Battery have none.
- Use each ratio at full precision. The table shows 4 significant figures.
- The shipped Eastern and Western catalogs are identical, so the averaged value equals either one.
  Only the line ratio differs between the two, through cell size.
- Price years differ between metrics (capital 2023 USD, tariffs and fuel 2025 USD). The ratio is
  per metric, so no inflation adjustment is applied. Local rows should use their latest sourced year.

### Shipped North American inputs

From `assets/regions/{eastern,western}_interconnection.ron` (catalog, `lines`, `regulator`,
`bounds`, `grid`):

| Kind | capacity_mw | capital_cost (USD) | game $/kW | build_months | fixed_om_per_month | game $/kW-yr | marginal $/MWh | startup_cost | game $/MW-start |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Wind | 300 | 300,000,000 | 1,000 | 3 | 500,000 | 20.0 | 0 | 0 | 0 |
| Nuclear | 1,100 | 2,400,000,000 | 2,181.8 | 18 | 11,000,000 | 120.0 | 10 | 1,000,000 | 909.1 |
| Coal | 600 | 700,000,000 | 1,166.7 | 8 | 2,000,000 | 40.0 | 30 | 150,000 | 250.0 |
| Ccgt | 500 | 300,000,000 | 600 | 4 | 600,000 | 14.4 | 45 | 40,000 | 80.0 |
| Peaker | 200 | 100,000,000 | 500 | 2 | 170,000 | 10.2 | 150 | 5,000 | 25.0 |
| Solar | 300 | 150,000,000 | 500 | 2 | 500,000 | 20.0 | 0 | 0 | 0 |
| Hydro | 800 | 1,200,000,000 | 1,500 | 12 | 2,700,000 | 40.5 | 0 | 1,000 | 1.25 |
| Battery | 200 | 100,000,000 | 500 | 2 | 100,000 | 6.0 | 0 | 0 | 0 |

- Lines: `cost_per_cell` 20,000,000 USD and `cells_per_month` 4 in both regions.
- Tariffs: `starting_tariff_per_mwh` 95 and `reference_tariff_per_mwh` 100 in both regions.
- Eastern: bounds −110 to −59°E, 24.5–56°N, grid 100 × 60, middle latitude 40.25°. Cell
  43.28 km × 58.38 km, `cell_km` 50.27 km, so 397,880 USD/km.
- Western: bounds −125.5 to −102°E, 28–60°N, grid 60 × 80, middle latitude 44.0°. Cell
  31.33 km × 44.48 km, `cell_km` 37.33 km, so 535,784 USD/km.
- The game line cost averages 466,832 USD/km.

### Pacing table

| Kind | Capital ratio (US row) | Month ratio (US row) | Fixed O&M ratio (US row) | Marginal ratio (US rows) | Start-up ratio |
|---|---:|---:|---:|---:|---:|
| Wind | 1,000 ÷ 1,489 = **0.6716** (P7-US-001) | 3 ÷ 9 = **0.3333** (P7-US-002) | 20.0 ÷ 33.06 = **0.6050** (P7-US-003) | n/a (game 0) | n/a |
| Nuclear | 2,181.8 ÷ 7,861 = **0.2775** (P7-US-004) | 18 ÷ 52 = **0.3462** (P7-US-005) | 120.0 ÷ 156.20 = **0.7682** (P7-US-006) | 10 ÷ 9.16 = **1.092** (P7-US-025, -029) | gap |
| Coal | 1,166.7 ÷ 4,103 = **0.2843** (P7-US-007) | 8 ÷ 36 = **0.2222** (P7-US-008) | 40.0 ÷ 61.60 = **0.6494** (P7-US-009) | 30 ÷ 27.30 = **1.099** (P7-US-026, -030) | gap |
| Ccgt | 600 ÷ 921 = **0.6515** (P7-US-010) | 4 ÷ 22 = **0.1818** (P7-US-011) | 14.4 ÷ 15.51 = **0.9284** (P7-US-012) | 45 ÷ 27.30 = **1.648** (P7-US-027, -031) | gap |
| Peaker | 500 ÷ 1,606 = **0.3113** (P7-US-013) | 2 ÷ 22 = **0.09091** (P7-US-014) | 10.2 ÷ 9.56 = **1.067** (P7-US-015) | 150 ÷ 42.07 = **3.565** (P7-US-028, -031) | 25.0 ÷ 55.13 = **0.4535** (P7-US-032), superseded by ruling 2 (no local start-up source exists) |
| Solar | 500 ÷ 1,502 = **0.3329** (P7-US-016) | 2 ÷ 12 = **0.1667** (P7-US-017) | 20.0 ÷ 20.23 = **0.9886** (P7-US-018) | n/a (game 0) | n/a |
| Hydro | 1,500 ÷ 7,073 = **0.2121** (P7-US-019) | 12 ÷ 36 = **0.3333** (P7-US-020) | 40.5 ÷ 33.54 = **1.208** (P7-US-021) | n/a (game 0) | gap |
| Battery | 500 ÷ 1,744 = **0.2867** (P7-US-022) | 2 ÷ 12 = **0.1667** (P7-US-023) | 6.0 ÷ 40.00 = **0.1500** (P7-US-024) | n/a (game 0) | n/a |

| Other metric | Ratio | US row |
|---|---:|---|
| AC line cost per km | 466,832 ÷ 2,796,170 = **0.1670** | P7-US-033 |
| Reference tariff per MWh | 100 ÷ 135.6 = **0.7375** | P7-US-034 |
| Starting tariff per MWh | 95 ÷ 135.6 = **0.7006**, which is 0.95 × the reference tariff, as shipped | P7-US-034 |
| AC line build speed | no ratio; the line-speed ruling below applies | — |

### Controller rulings (round 2, 25 October 2026)

These rulings close three method gaps. They are authoring rules, not sourced figures, and need no
rows.

- **Durations.** A baseline build duration with no local source inherits the shipped North
  American game months. Its RON comment is labelled "authoring judgment: no sourced local
  duration". Spain and Morocco zone durations stay sourced (P7-ES-003, P7-ES-005, P7-MA-002,
  P7-MA-006).
- **Start-up costs.** Use `local game startup = NA game startup × (local game marginal ÷ NA game
  marginal)`, per kind. The NA values are 1,000,000 (Nuclear), 150,000 (Coal), 40,000 (Ccgt) and
  5,000 (Peaker), against marginals of 10, 30, 45 and 150. Hydro's NA marginal is 0, so the
  ratio has no Hydro term of its own; ruling 2 below uses the Ccgt marginal ratio for Hydro's
  1,000 start-up, and the region task records it as an authoring judgment.
- **Line build speed.** Keep East's km per month, converted to each region's cells. East builds
  4 cells × 50.27 km = 201.07 km per game month, so
  `cells_per_month = max(1, round(201.07 ÷ region cell_km))`. The rounding is the application
  convention, as for months.
- **Fuel conversion.** Several local fuel rows are prices per GJ or per m³, not per MWh.
  - Convert them with the S&L heat rates (P7-US-026 to P7-US-028; higher-heating-value basis):
    `fuel per MWh = price per MMBtu × heat rate in MMBtu/MWh`, where 1 MMBtu = 1.055056 GJ.
  - Rows priced on a net (lower) calorific basis are marked LHV. The region task records how it
    treats the basis as a pacing judgment.

## Region-authoring rulings (Plan 7)

These 28 rulings bound the region tasks (Tasks 5–9). Region RON files and
`crates/infrastructure/tests/shipped_regions.rs` cite them as "ruling N". They are authoring
rules, not sourced figures, and need no rows. Where a ruling says "authoring judgment", the region
file's comment carries that label.

**Money method.** The pacing formula is in [Pacing method](#pacing-method); apply it exactly.

1. Unsourced baseline build duration: inherit the shipped NA game months for that kind. The comment
   says "authoring judgment: no sourced local duration". Sourced local durations go through
   `month_ratio`, clamped to the pacing ceilings.
2. Start-up cost = NA game start-up for the kind × (local game marginal ÷ NA game marginal) for the
   same kind. Hydro, whose NA marginal is 0, uses the Ccgt marginal ratio. Record it as an authoring
   judgment. A P7-US-032-style ratio applies only where a local start-up source exists.
3. Unsourced plant capital = the region's nearest sourced kind's local capital per kW × the NA
   shipped per-kW ratio between the two kinds. The pairs are Ccgt↔Coal, Peaker↔Ccgt,
   Battery↔Solar and Nuclear↔Coal. Label it "authoring judgment: derived from <row> by NA kind
   relativity", and cite that row.
4. Unsourced fixed O&M = local capital × the NA shipped (fixed O&M ÷ capital) ratio for that kind,
   labelled the same way.
5. Line `cells_per_month` keeps the Eastern Interconnection's km per month: 4 cells × East's
   middle-latitude cell width in km, converted to this region's middle-latitude cell width and
   rounded to the nearest whole cell, minimum 1. Record it as an authoring judgment.
6. Every catalog entry, `lines:`, `starting_tariff_per_mwh` and `cash:` line, and every
   economic-zone profile, gets a comment directly above it citing at least one `[P7-…]` row and
   showing the arithmetic. The test `new_region_entries_cite_research_rows` (Plan 7 Task 3)
   enforces this.

**Geography.**

7. Use `tools/region_authoring/natural_earth.py` with the approved files already in
   `target/natural-earth/` (Natural Earth v5.1.2 `ne_50m_admin_0_countries.geojson` and
   `ne_10m_admin_1_states_provinces.geojson`). Download nothing else. Run Python with `-I`.
8. Use `--dissolve` for any multi-feature selection.
9. Check that the extractor's output exclude ring count equals the selection's exclusions.
   Simplification can drop a collapsed exclude ring and silently re-admit land.
10. A pair involving a new region must have zero overlapping playable cell centres
    (`shipped_regions_do_not_overlap`). Where regions share a border, make sure no cell centre is
    playable in both.
11. The canonical Crimea extent (`crates/infrastructure/authoring/ukraine_ips_operating_extent.ron`)
    must appear verbatim in `exclude` for Europe–Maghreb and IPS/UPS only (user decision: Crimea and
    Sevastopol only; mainland Ukraine is in Europe–Maghreb).

**Process.**

12. Stage explicit paths only. Never `git commit -a`/`-am` or `git add -A`/`.`. Leave files you did
    not touch alone, since another session may work in the same worktree.
13. Commit with hooks enabled. Never use `--no-verify`. Do not push. Do not open a game window.
14. If a task test cannot pass without changing a rule, or without relaxing a shared check, stop and
    report BLOCKED. Never change rules.
15. The region's tests go in `crates/infrastructure/tests/shipped_regions.rs` as a module named for
    the region. Insert the region ID into every Task 3 table: `SHIPPED`, `NEW_REGIONS`,
    `RESEARCH_CAPACITY_GW`, `HOME_CITY`, `EXPANSION_CITY`, `NOMINAL_HZ`, the `year_one_cases!` macro
    list and the game binary's `SHIPPED_REGIONS`. The table-coverage tests report any miss.
16. Measure optimised timings (when needed) with `CARGO_PROFILE_DEV_OPT_LEVEL=3
    CARGO_PROFILE_DEV_DEBUG=0`. Never `--release`.

**Names** (user requirement, 2026-10-25).

17. Every player-facing region, city, zone, economic-zone and line name in
    `assets/locales/en-US/*.ftl` is its English name, which shows on both the HUD and the 3D map
    labels. Use the title of the English Wikipedia article: Guangzhou, Moscow, Kyiv, Tbilisi,
    São Paulo, Málaga. Latin diacritics are fine where that English title uses them. Never use
    non-Latin scripts or local-language forms (no 广州, Москва or 東莞). Keep the `city-<kebab>` keys
    ASCII.
18. Glyph coverage: `every_shipped_name_paints_with_the_shipped_fonts`, in the game binary next to
    the currency glyph test, paints every shipped region, city, zone and economic-zone name with the
    real egui fonts. It asserts that no character renders as the replacement glyph. Task 5 adds it;
    later regions extend its coverage automatically.

**Added after Task 5.**

19. Unsourced fuel or marginal cost = NA game marginal for the kind × (local reference tariff ÷ NA
    reference tariff), a price-level proxy. Record it as an authoring judgment citing the tariff
    row. A sourced local fuel row always wins.
20. Unsourced AC line cost per km = local coal capital per kW × the NA shipped (line cost per km ÷
    coal capital per kW), by ruling 3's relativity, labelled the same way and citing the coal row.
21. Ruling 5's "cell width" means this note's `cell_km` definition, as used in its pacing table, for
    consistency.
22. Admin-1 features (for example Hong Kong and Macau in the 1:10m admin-1 file) may replace admin-0
    where the 1:50m outlines leave gaps. Record which features were used.
23. Facts not from memory. City populations or demand shares, plant names and coordinates, and
    climate and resource sources are read from a source now (reading pages with WebSearch or
    WebFetch is allowed; small page fetches are not "downloads"). Cite each in the RON comment with
    a locator (publisher, page or table, date). If no source can be found, say so in the comment and
    report it.

**Added after the Task 5 review (binding for Tasks 6–9).**

24. Where a local unit's `capacity_mw` differs from NA's for the same kind, scale per-unit
    quantities per MW: start-up cost (after ruling 2's marginal ratio) and `ramp_mw_per_min`.
    Example: 150,000 × 150 ÷ 30 × 1,000 ÷ 600 for a 1,000 MW coal unit. Say so in that entry's
    comment.
25. Derivations use full-precision intermediates, such as the unrounded local tariff 501.378. Round
    only the final RON value, to 3 significant figures, round half away from zero.
26. Every catalog entry's own comment repeats its "authoring judgment …" label and the ruling
    number. A preamble alone is not enough.
27. A city name that is ambiguous across shipped regions takes the English Wikipedia qualifier in
    its display text, for example "Yulin, Guangxi". Keys stay unique.

**Added after Task 8.**

28. Privacy: every web request (WebFetch, curl or API calls) uses a generic user agent. Never put
    the user's email, name or any personal identifier in a header, URL or payload, even if an API's
    etiquette asks for contact details.

## Tariff bases

Every new region scales its local tariff by the US all-sector ratio (P7-US-034, 135.6 USD/MWh,
all sectors, 2025): starting tariff × 95 ÷ 135.6 and reference tariff × 100 ÷ 135.6. The local
rows do not share that basis, and none was re-based. The region files label this "authoring
judgment: basis differs from the US all-sector reference; not re-based", and no number changed.
Ruling 19's fuel proxy scales by the same reference tariff, so where a region applies it (Southern
China, India, IPS/UPS and the Europe–Maghreb nuclear fuel) it inherits the same basis.

| Region | Row | Basis |
|---|---|---|
| IPS/UPS | P7-IPS-001 | Moscow household tariff |
| Southern China | P7-SC-005 | Commercial and industrial, 1–10 kV, tax included |
| Europe–Maghreb | P7-EM-008, P7-EM-009 | Unweighted household and non-household mean, excluding taxes |
| India–Nepal–Bhutan | P7-IN-011 | All consumers |
| Brazil | P7-BR-029 | All consumer classes |

## Row table

### US pacing reference

US capital, construction time, fixed O&M and variable O&M all come from one source family: the
Sargent & Lundy report prepared for EIA. It is the cost basis of EIA's AEO2026 Electricity Market
Module Table 3, published April 2026. AEO2026 Table 3 itself no longer lists unabated coal and
gives lead times that include development, so the report's *Plant Construction Time* is used
instead. Its short name below is **S&L 2023** (EIA, *Capital Cost and Performance Characteristic
Estimates for Utility Scale Electric Power Generating Technologies*, SL-018001 Final Rev A,
https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf).

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-US-001 | Pacing reference | US | Wind | capital_per_kw | 1,489 | USD/kW | 2023 | n/a (pacing reference); FX: none | Sargent & Lundy for EIA — S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 (Rev A; EIA release Jan 2024) — acc. 2026-10-25 — loc. Table 1-2, Case 13, p. II | Onshore wind, 200 MW of 2.8 MW turbines, Great Plains; overnight, excludes interest during construction |
| P7-US-002 | Pacing reference | US | Wind | build_months_real | 9 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 13-1, Typical Project Timelines, p. 82 | Plant Construction Time; excludes 12 months of development, permitting and engineering |
| P7-US-003 | Pacing reference | US | Wind | fixed_om_per_kw_year | 33.06 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 13, p. II | Fixed O&M |
| P7-US-004 | Pacing reference | US | Nuclear | capital_per_kw | 7,861 | USD/kW | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 9, p. II | 2 × AP1000, 2,156 MW net, brownfield |
| P7-US-005 | Pacing reference | US | Nuclear | build_months_real | 52 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 9-1, Typical Project Timelines, p. 63 | Plant Construction Time; excludes 32 months of development |
| P7-US-006 | Pacing reference | US | Nuclear | fixed_om_per_kw_year | 156.20 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 9, p. II | Fixed O&M |
| P7-US-007 | Pacing reference | US | Coal | capital_per_kw | 4,103 | USD/kW | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 1, p. II | Ultra-supercritical, no carbon capture, 650 MW net, greenfield |
| P7-US-008 | Pacing reference | US | Coal | build_months_real | 36 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-1 (continued), Typical Project Timelines, p. 6 | Plant Construction Time; excludes 24 months of development |
| P7-US-009 | Pacing reference | US | Coal | fixed_om_per_kw_year | 61.60 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 1, p. II | Fixed O&M |
| P7-US-010 | Pacing reference | US | Ccgt | capital_per_kw | 921 | USD/kW | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 6, p. II | 1 × 1 × 1 single-shaft H-class, 627 MW net; chosen as the closest unit size to the 500 MW catalog unit. Case 5 (2 × 2 × 1, 1,227 MW) is 868 USD/kW and 24 months |
| P7-US-011 | Pacing reference | US | Ccgt | build_months_real | 22 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 6-1 (continued), Typical Project Timelines, p. 42 | Plant Construction Time; excludes 18 months of development |
| P7-US-012 | Pacing reference | US | Ccgt | fixed_om_per_kw_year | 15.51 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 6, p. II | Fixed O&M |
| P7-US-013 | Pacing reference | US | Peaker | capital_per_kw | 1,606 | USD/kW | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 3, p. II | 4 × 54 MW aeroderivative simple cycle, 211 MW net; chosen as the closest size to the 200 MW catalog unit. Case 4 (H-class frame, 419 MW) is 836 USD/kW |
| P7-US-014 | Pacing reference | US | Peaker | build_months_real | 22 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 3-1, Typical Project Timelines, p. 21 | Plant Construction Time; excludes 18 months of development |
| P7-US-015 | Pacing reference | US | Peaker | fixed_om_per_kw_year | 9.56 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 3, p. II | Fixed O&M |
| P7-US-016 | Pacing reference | US | Solar | capital_per_kw | 1,502 | USD/kWac | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 16, p. III | 150 MWac, single-axis tracking |
| P7-US-017 | Pacing reference | US | Solar | build_months_real | 12 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 16-1, Typical Project Timelines, p. 98 | Plant Construction Time; excludes 24 months of development |
| P7-US-018 | Pacing reference | US | Solar | fixed_om_per_kw_year | 20.23 | USD/kWac-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 16, p. III | Fixed O&M |
| P7-US-019 | Pacing reference | US | Hydro | capital_per_kw | 7,073 | USD/kW | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 12, p. II | 100 MW new stream-reach development |
| P7-US-020 | Pacing reference | US | Hydro | build_months_real | 36 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 12-1 (continued), Typical Project Timelines, p. 79 | Plant Construction Time; excludes 36 months of development |
| P7-US-021 | Pacing reference | US | Hydro | fixed_om_per_kw_year | 33.54 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 12, p. II | Fixed O&M |
| P7-US-022 | Pacing reference | US | Battery | capital_per_kw | 1,744 | USD/kW | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 19, p. III | Lithium-ion, 150 MW / 600 MWh (4 h); 436 USD/kWh |
| P7-US-023 | Pacing reference | US | Battery | build_months_real | 12 | months | — | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 19-1, Typical Project Timelines, p. 115 | Plant Construction Time |
| P7-US-024 | Pacing reference | US | Battery | fixed_om_per_kw_year | 40.00 | USD/kW-yr | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 19, p. III | Fixed O&M |
| P7-US-025 | Pacing reference | US | Nuclear | fuel_or_marginal | 2.52 | USD/MWh | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 9, p. II | Variable non-fuel O&M only |
| P7-US-026 | Pacing reference | US | Coal | fuel_or_marginal | 6.40 | USD/MWh | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 1, p. II | Variable non-fuel O&M only; heat rate 8,638 Btu/kWh (same table) |
| P7-US-027 | Pacing reference | US | Ccgt | fuel_or_marginal | 3.33 | USD/MWh | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 6, p. II | Variable non-fuel O&M only; heat rate 6,226 Btu/kWh (same table) |
| P7-US-028 | Pacing reference | US | Peaker | fuel_or_marginal | 5.70 | USD/MWh | 2023 | n/a; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 3, p. II | Variable non-fuel O&M only; heat rate 9,447 Btu/kWh (same table) |
| P7-US-029 | Pacing reference | US | Nuclear | fuel_or_marginal | 6.64 | mills/kWh = USD/MWh | 2025 | n/a; derived fuel + VOM = 6.64 + 2.52 (P7-US-025) = 9.16 USD/MWh; FX: none | EIA — *Electric Power Annual*, Table 8.4, Average power plant operating expenses for major U.S. investor-owned electric utilities — https://www.eia.gov/electricity/annual/html/epa_08_04.html — pub. 2026-10-21 — acc. 2026-10-25 — loc. Table 8.4, Nuclear, Fuel, 2025 | Fuel expense per kWh, major investor-owned utilities |
| P7-US-030 | Pacing reference | US | Coal | fuel_or_marginal | 2.42 | USD/MMBtu | 2025 | n/a; derived 8.638 MMBtu/MWh × 2.42 = 20.90 fuel + 6.40 VOM (P7-US-026) = 27.30 USD/MWh; FX: none | EIA — *Electric Power Annual*, Table 7.4, Weighted average cost of fossil fuels for the electric power industry — https://www.eia.gov/electricity/annual/html/epa_07_04.html — pub. 2026-10-21 — acc. 2026-10-25 — loc. Table 7.4, Coal (all ranks), 2025 | Delivered fuel cost; heat rate from P7-US-026 |
| P7-US-031 | Pacing reference | US | Ccgt | fuel_or_marginal | 3.85 | USD/MMBtu | 2025 | n/a; derived Ccgt 6.226 × 3.85 = 23.97 + 3.33 (P7-US-027) = 27.30 USD/MWh; Peaker 9.447 × 3.85 = 36.37 + 5.70 (P7-US-028) = 42.07 USD/MWh; FX: none | EIA — *Electric Power Annual*, Table 7.4 — https://www.eia.gov/electricity/annual/html/epa_07_04.html — pub. 2026-10-21 — acc. 2026-10-25 — loc. Table 7.4, Natural gas, 2025 | Natural gas cost, used for both Ccgt and Peaker with their own heat rates |
| P7-US-032 | Pacing reference | US | Peaker | startup | 23,100 | USD per start | 2023 | n/a; derived 23,100 ÷ 419 MW = 55.13 USD/MW-start; FX: none | S&L 2023 — https://www.eia.gov/analysis/studies/powerplants/capitalcost/pdf/capital_cost_AEO2025.pdf — pub. 2023-12-06 — acc. 2026-10-25 — loc. Table 1-2, Case 4 variable O&M column, p. II | Start-based maintenance cost of a 419 MW H-class frame CT (Case 4), the only start cost in the source; not the Case 3 peaker used for capital |
| P7-US-033 | Pacing reference | US | AcLine | line_cost_per_km | 4.1M to 5.1M by state; median 4.5M | USD/mile | 2024 | n/a; derived median 4.5M ÷ 1.609344 = 2,796,170 USD/km; FX: none | MISO — *Transmission Cost Estimation Guide for MTEP24* — https://cdn.misoenergy.org/20240501%20PSC%20Item%2004%20MISO%20Transmission%20Cost%20Estimation%20Guide%20for%20MTEP24632680.pdf — pub. 2024-05-01 — acc. 2026-10-25 — loc. Table 4.1-1, new single-circuit AC line $/mile, 500 kV column, p. 38 | Median of the 15 state values; includes 30% contingency and 7.5% AFUDC; 2023 data escalated 5% to 2024 per the guide's purpose statement. 500 kV is used because local backbones are 400–500 kV |
| P7-US-034 | Pacing reference | US | Tariff | retail_tariff_per_mwh | 13.56 | cents/kWh | 2025 | n/a; derived 135.6 USD/MWh; FX: none | EIA — *Electric Power Annual*, Table 2.4, Average price of electricity to ultimate customers by end-use sector — https://www.eia.gov/electricity/annual/html/epa_02_04.html — pub. 2026-10-21 — acc. 2026-10-25 — loc. Table 2.4, All sectors, 2025 | All-sector average (residential 17.23, commercial 13.37, industrial 8.51 cents/kWh in the same table) |

### Exchange rates

Source for every FX row: World Bank, World Development Indicators, *Official exchange rate (LCU per
US$, period average)*, indicator `PA.NUS.FCRF`, API
`https://api.worldbank.org/v2/country/<ISO3>/indicator/PA.NUS.FCRF?format=json`, dataset last
updated 2026-10-08. Cross rates into EUR are derived as `LCU per USD ÷ EUR per USD` for the same
year.

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-FX-001 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.885206 | EUR per USD | 2017 | base rate for 2017 EUR cross rates; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2017&format=json — pub. 2026-10-08 (last updated) — acc. 2026-10-25 — loc. EMU, 2017 | Annual average |
| P7-FX-002 | Europe–Maghreb | Morocco | Fx | fx_rate | 9.692000 | MAD per USD | 2017 | derived 10.9489 MAD per EUR; FX: P7-FX-001 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/MAR/indicator/PA.NUS.FCRF?date=2017&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. MAR, 2017 | Annual average |
| P7-FX-003 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.845494 | EUR per USD | 2021 | base rate for 2021; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2021&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. EMU, 2021 | Annual average |
| P7-FX-004 | Europe–Maghreb | Morocco | Fx | fx_rate | 8.988484 | MAD per USD | 2021 | derived 10.6310 MAD per EUR; FX: P7-FX-003 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/MAR/indicator/PA.NUS.FCRF?date=2021&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. MAR, 2021 | Annual average |
| P7-FX-005 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.923890 | EUR per USD | 2024 | base rate for 2024; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. EMU, 2024 | Annual average |
| P7-FX-006 | Europe–Maghreb | Morocco | Fx | fx_rate | 9.942359 | MAD per USD | 2024 | derived 10.7614 MAD per EUR; FX: P7-FX-005 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/MAR/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. MAR, 2024 | Annual average |
| P7-FX-007 | Europe–Maghreb | Algeria | Fx | fx_rate | 134.0532 | DZD per USD | 2024 | derived 145.0966 DZD per EUR; FX: P7-FX-005 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/DZA/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. DZA, 2024 | Annual average; no Algerian cost row uses it yet |
| P7-FX-008 | Europe–Maghreb | Tunisia | Fx | fx_rate | 3.10745 | TND per USD | 2024 | derived 3.36344 TND per EUR; FX: P7-FX-005 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/TUN/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. TUN, 2024 | Annual average; no Tunisian cost row uses it yet |
| P7-FX-009 | Europe–Maghreb | Türkiye | Fx | fx_rate | 32.80586 | TRY per USD | 2024 | derived 35.5084 TRY per EUR; FX: P7-FX-005 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/TUR/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. TUR, 2024 | Annual average; high inflation, so match the cost row's own year |
| P7-FX-010 | Europe–Maghreb | Poland | Fx | fx_rate | 3.980783 | PLN per USD | 2024 | derived 4.30872 PLN per EUR; FX: P7-FX-005 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/POL/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. POL, 2024 | Annual average; no Polish cost row uses it yet |
| P7-FX-011 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.884969 | EUR per USD | 2025 | base rate for 2025; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. EMU, 2025 | Annual average |
| P7-FX-012 | Europe–Maghreb | Morocco | Fx | fx_rate | 9.350277 | MAD per USD | 2025 | derived 10.5657 MAD per EUR; FX: P7-FX-011 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/MAR/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. MAR, 2025 | Annual average |
| P7-FX-013 | Europe–Maghreb | Algeria | Fx | fx_rate | 131.6059 | DZD per USD | 2025 | derived 148.7125 DZD per EUR; FX: P7-FX-011 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/DZA/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. DZA, 2025 | Annual average |
| P7-FX-014 | Europe–Maghreb | Tunisia | Fx | fx_rate | 2.997367 | TND per USD | 2025 | derived 3.38697 TND per EUR; FX: P7-FX-011 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/TUN/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. TUN, 2025 | Annual average |
| P7-FX-015 | Europe–Maghreb | Türkiye | Fx | fx_rate | 39.45481 | TRY per USD | 2025 | derived 44.5833 TRY per EUR; FX: P7-FX-011 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/TUR/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. TUR, 2025 | Annual average |
| P7-FX-016 | Europe–Maghreb | Poland | Fx | fx_rate | 3.760542 | PLN per USD | 2025 | derived 4.24935 PLN per EUR; FX: P7-FX-011 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/POL/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. POL, 2025 | Annual average |
| P7-FX-017 | IPS/UPS | Russia | Fx | fx_rate | 92.55240 | RUB per USD | 2024 | base rate for 2024 RUB cross rates; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/RUS/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. RUS, 2024 | Annual average; for KZ/UZ rows if IPS/UPS ever opts in |
| P7-FX-018 | IPS/UPS | Kazakhstan | Fx | fx_rate | 468.9625 | KZT per USD | 2024 | derived 5.06701 KZT per RUB; FX: P7-FX-017 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/KAZ/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. KAZ, 2024 | Annual average; not used by a cost row |
| P7-FX-019 | IPS/UPS | Uzbekistan | Fx | fx_rate | 12,652.29 | UZS per USD | 2024 | derived 136.704 UZS per RUB; FX: P7-FX-017 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/UZB/indicator/PA.NUS.FCRF?date=2024&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. UZB, 2024 | Annual average; not used by a cost row |
| P7-FX-020 | Southern China | China | Fx | fx_rate | 7.189847 | CNY per USD | 2025 | base rate for 2025 USD→CNY (IRENA rows); FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/CHN/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 (last updated) — acc. 2026-10-25 — loc. CHN, 2025 | Annual average; used only for country-specific international (IRENA) rows |
| P7-FX-021 | India–Nepal–Bhutan | India | Fx | fx_rate | 87.15845 | INR per USD | 2025 | base rate for 2025 USD→INR; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/IND/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. IND, 2025 | Annual average; IRENA rows only |
| P7-FX-022 | Brazil | Brazil | Fx | fx_rate | 5.587286 | BRL per USD | 2025 | base rate for 2025 USD→BRL; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/BRA/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. BRA, 2025 | Annual average; IRENA rows only |
| P7-FX-023 | IPS/UPS | Russia | Fx | fx_rate | 83.6525 | RUB per USD | 2025 | base rate for 2025 USD→RUB; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/RUS/indicator/PA.NUS.FCRF?date=2025&format=json — pub. 2026-10-08 — acc. 2026-10-25 — loc. RUS, 2025 | Annual average; IRENA rows only |
| P7-FX-024 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.778338 | EUR per USD | 2012 | base rate for 2012; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2012&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. EMU, 2012 | Annual average; follow-up 2026-10-26 (P7-TN-016) |
| P7-FX-025 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.752728 | EUR per USD | 2014 | base rate for 2014; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2014&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. EMU, 2014 | Annual average; follow-up 2026-10-26 |
| P7-FX-026 | Europe–Maghreb | Japan | Fx | fx_rate | 105.9448 | JPY per USD | 2014 | derived 105.9448 ÷ 0.752728 = 140.7477 JPY per EUR; FX: P7-FX-025 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/JPN/indicator/PA.NUS.FCRF?date=2014&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. JPN, 2014 | Annual average; for the JICA Radès row (P7-TN-012) |
| P7-FX-027 | Europe–Maghreb | Algeria | Fx | fx_rate | 80.57902 | DZD per USD | 2014 | derived 80.57902 ÷ 0.752728 = 107.0493 DZD per EUR; FX: P7-FX-025 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/DZA/indicator/PA.NUS.FCRF?date=2014&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. DZA, 2014 | Annual average; for Kabertène (P7-DZ-009) |
| P7-FX-028 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.949624 | EUR per USD | 2022 | base rate for 2022; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2022&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. EMU, 2022 | Annual average; for the IFC Kairouan row (P7-TN-009) |
| P7-FX-029 | Europe–Maghreb | Euro area (EMU) | Fx | fx_rate | 0.924840 | EUR per USD | 2023 | base rate for 2023; FX: self | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/EMU/indicator/PA.NUS.FCRF?date=2023&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. EMU, 2023 | Annual average; follow-up 2026-10-26 |
| P7-FX-030 | Europe–Maghreb | Algeria | Fx | fx_rate | 135.8429 | DZD per USD | 2023 | derived 135.8429 ÷ 0.924840 = 146.8827 DZD per EUR; FX: P7-FX-029 | World Bank — WDI PA.NUS.FCRF — https://api.worldbank.org/v2/country/DZA/indicator/PA.NUS.FCRF?date=2023&format=json — pub. 2026-10-08 — acc. 2026-10-26 — loc. DZA, 2023 | Annual average; for the 2023 solar awards (P7-DZ-001, -002) and P7-DZ-014 |

### Brazil (`BR`, BRL)

Main source: EPE and MME, *PDE 2035 — Caderno de Parâmetros de Custos: Geração e Transmissão*, rev.
10 Nov 2025 (short name **EPE PDE 2035 Custos**),
https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf.
Its money base is December 2024. Its CAPEX excludes interest during construction, and it states an
FX assumption of R$ 6.10/USD (p. 8), which is not used here because the rows are already in BRL.
*Tempo médio de desembolso* is the average disbursement period: the planner's construction-cash
schedule, which the rows use as `build_months_real`.

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-BR-001 | Brazil | Brazil (SIN) | Wind | capital_per_kw | levels 5,000 / 5,500 / 6,200 / 7,000 (range 4,500–8,000) | BRL/kW | 2024 (Dec) | derived median of levels 5,850 BRL/kW; FX: none | EPE/MME — EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Eólica Onshore | Onshore; four cost levels, no single reference value |
| P7-BR-002 | Brazil | Brazil (SIN) | Wind | build_months_real | 24 | months | — | 24 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Eólica Onshore, Tempo médio de desembolso | Average disbursement period |
| P7-BR-003 | Brazil | Brazil (SIN) | Wind | fixed_om_per_kw_year | 110 | BRL/kW-yr | 2024 (Dec) | 110 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Eólica Onshore, O&M | Same at all four levels; taxes and charges (150–160 BRL/kW-yr) shown separately and excluded |
| P7-BR-004 | Brazil | Brazil (SIN) | Solar | capital_per_kw | levels 3,000 / 3,500 / 4,500 / 5,500 (range 3,000–6,000) | BRL/kW | 2024 (Dec) | derived median of levels 4,000 BRL/kW; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Solar Fotovoltaica | Utility PV; four cost levels |
| P7-BR-005 | Brazil | Brazil (SIN) | Solar | build_months_real | 12 | months | — | 12 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Solar Fotovoltaica, Tempo médio de desembolso | Average disbursement period |
| P7-BR-006 | Brazil | Brazil (SIN) | Solar | fixed_om_per_kw_year | 60 | BRL/kW-yr | 2024 (Dec) | 60 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Solar Fotovoltaica, O&M | All levels |
| P7-BR-007 | Brazil | Brazil (SIN) | Battery | capital_per_kw | levels 5,000 / 5,500 / 6,000 (range 5,000–9,000) | BRL/kW | 2024 (Dec) | 5,500 BRL/kW (middle level); FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 10, Armazenamento – Baterias, footnote 5 | Lithium-ion BESS, about 4 hours (footnote 5) |
| P7-BR-008 | Brazil | Brazil (SIN) | Battery | build_months_real | 12 | months | — | 12 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 10, Armazenamento – Baterias, Tempo médio de desembolso | Average disbursement period |
| P7-BR-009 | Brazil | Brazil (SIN) | Battery | fixed_om_per_kw_year | levels 130 / 140 / 160 | BRL/kW-yr | 2024 (Dec) | 140 BRL/kW-yr (middle level); FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 10, Armazenamento – Baterias, O&M | Paired with the capital levels |
| P7-BR-010 | Brazil | Brazil (SIN) | Coal | capital_per_kw | 15,000 (range 9,000–17,000) | BRL/kW | 2024 (Dec) | 15,000 BRL/kW; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Carvão Nacional, CAPEX Referência | Domestic coal |
| P7-BR-011 | Brazil | Brazil (SIN) | Coal | build_months_real | 48 | months | — | 48 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Carvão Nacional, Tempo médio de desembolso | Average disbursement period |
| P7-BR-012 | Brazil | Brazil (SIN) | Coal | fixed_om_per_kw_year | 300 | BRL/kW-yr | 2024 (Dec) | 300 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 11, Carvão Nacional, O&M | |
| P7-BR-013 | Brazil | Brazil (SIN) | Coal | fuel_or_marginal | 180 | BRL/MWh | 2024 (Dec) | 180 BRL/MWh; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 17, CVU table, Carvão Nacional | CVU = fuel cost + variable O&M, per the page's definition |
| P7-BR-014 | Brazil | Brazil (SIN) | Nuclear | capital_per_kw | 35,000 (range 30,000–50,000) | BRL/kW | 2024 (Dec) | 35,000 BRL/kW; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Nuclear, CAPEX Referência | |
| P7-BR-015 | Brazil | Brazil (SIN) | Nuclear | build_months_real | 60 | months | — | 60 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Nuclear, Tempo médio de desembolso | Average disbursement period |
| P7-BR-016 | Brazil | Brazil (SIN) | Nuclear | fixed_om_per_kw_year | 760 | BRL/kW-yr | 2024 (Dec) | 760 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Nuclear, O&M | |
| P7-BR-017 | Brazil | Brazil (SIN) | Nuclear | fuel_or_marginal | 60 | BRL/MWh | 2024 (Dec) | 60 BRL/MWh; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 17, CVU table, Nuclear | CVU |
| P7-BR-018 | Brazil | Brazil (SIN) | Ccgt | capital_per_kw | 7,600 (range 3,500–9,000) | BRL/kW | 2024 (Dec) | 7,600 BRL/kW; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Gás Natural – 100% Inflexível (flat) | Mapping of the flat baseload gas plant to Ccgt is an authoring judgment; the source names no cycle type. Alternative: 70% inflexível (seasonal), 5,500 BRL/kW, O&M 850, 36 months, CVU 490 |
| P7-BR-019 | Brazil | Brazil (SIN) | Ccgt | build_months_real | 36 | months | — | 36 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Gás Natural – 100% Inflexível, Tempo médio de desembolso | Average disbursement period |
| P7-BR-020 | Brazil | Brazil (SIN) | Ccgt | fixed_om_per_kw_year | 180 | BRL/kW-yr | 2024 (Dec) | 180 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Gás Natural – 100% Inflexível, O&M | |
| P7-BR-021 | Brazil | Brazil (SIN) | Ccgt | fuel_or_marginal | 430 | BRL/MWh | 2024 (Dec) | 430 BRL/MWh; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 17, CVU table, Gás Natural – 100% Inflexível | CVU at a gas price of 7.0 USD/MMBtu at the plant; includes fuel taxes listed on the page |
| P7-BR-022 | Brazil | Brazil (SIN) | Peaker | capital_per_kw | 4,500 (range 3,000–5,000) | BRL/kW | 2024 (Dec) | 4,500 BRL/kW; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Gás Natural – 100% Flexível | Mapping of the fully flexible gas plant to Peaker is an authoring judgment |
| P7-BR-023 | Brazil | Brazil (SIN) | Peaker | build_months_real | 24 | months | — | 24 months; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Gás Natural – 100% Flexível, Tempo médio de desembolso | Average disbursement period |
| P7-BR-024 | Brazil | Brazil (SIN) | Peaker | fixed_om_per_kw_year | 190 | BRL/kW-yr | 2024 (Dec) | 190 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 12, Gás Natural – 100% Flexível, O&M | |
| P7-BR-025 | Brazil | Brazil (SIN) | Peaker | fuel_or_marginal | 1,000 | BRL/MWh | 2024 (Dec) | 1,000 BRL/MWh; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 17, CVU table, Gás Natural – 100% Flexível | CVU at 13.0 USD/MMBtu at the plant |
| P7-BR-026 | Brazil | Brazil (SIN) | Hydro | capital_per_kw | 12,919 | BRL/kW | 2024 (Dec) | 12,919 BRL/kW; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 14, UHE Bem Querer (650 MW) | Bem Querer is the median of the 11 individually costed new UHEs (range 11,700–16,901 BRL/kW) |
| P7-BR-027 | Brazil | Brazil (SIN) | Hydro | fixed_om_per_kw_year | 50 | BRL/kW-yr | 2024 (Dec) | 50 BRL/kW-yr; FX: none | EPE PDE 2035 Custos — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20Par%C3%A2metros%20de%20Custos_rev10.11.25.pdf — pub. 2025-11-10 — acc. 2026-10-25 — loc. p. 14, UHE Bem Querer, O&M | |
| P7-BR-028 | Brazil | Brazil (SIN) | AcLine | line_cost_per_km | 78.0 billion for 28,781 km | BRL | 2024 (Dec) | derived 78.0e9 ÷ 28,781 = 2,710,121 BRL/km; FX: none | EPE/MME — *PDE 2035 — Caderno de Transmissão de Energia Elétrica* — https://www.epe.gov.br/sites-pt/publicacoes-dados-abertos/publicacoes/PublicacoesArquivos/publicacao-894/PDE%202035_Caderno%20de%20Transmiss%C3%A3o%20de%20Energia.pdf — pub. 2026-06-12 (PDF date) — acc. 2026-10-25 — loc. p. 35 (line investment, reference scenario, 68.0 with + 10.0 without concession) and p. 36 (2026–2035 line evolution, 28,781 km) | Average over all voltages (230–800 kV), including 2,936 km of ±800 kV HVDC poles. Lengths are counted per circuit and per pole. Costs are from the ANEEL price bank |
| P7-BR-029 | Brazil | Brazil (SIN) | Tariff | retail_tariff_per_mwh | 748.41 | BRL/MWh | 2025 (Dec) | 748.41 BRL/MWh; FX: none | MME — *Informativo Tarifário do Setor Elétrico 2025* — https://www.gov.br/mme/pt-br/assuntos/secretarias/secretaria-nacional-energia-eletrica/informativo-tarifario-do-setor-eletrico/informativo-tarifario-do-setor-eletrico-ano-2025.pdf/@@download/file — pub. 2026-03-24 (PDF date) — acc. 2026-10-25 — loc. Tabela 7, Total por Região, Brasil column, p. 36 | Average billed tariff, all consumer classes, December 2025, ANEEL data (residential 746.42) |
| P7-BR-030 | Brazil | Brazil | Hydro | capital_per_kw | 1,463 | USD/kW | 2025 | derived 1,463 × 5.587286 = 8,174 BRL/kW; FX: P7-FX-022 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 6.5, large hydropower weighted average 2018–2025, Brazil label, p. 107 | Cross-check only: market weighted average of commissioned projects, far below EPE planning value P7-BR-026; EPE stays the baseline |

### India–Nepal–Bhutan (`IN`, INR)

1 crore = 10,000,000 and 1 lakh = 100,000.

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-IN-001 | India–Nepal–Bhutan | India | Coal | capital_per_kw | 15,530 crore for 2 × 800 MW | INR | 2024 | derived 1.553e11 ÷ 1.6e6 kW = 97,063 INR/kW; FX: none | PIB, Ministry of Power — Lara Super Thermal Power Project release — https://www.pib.gov.in/PressReleaseIframePage.aspx?PRID=2008344&reg=3&lang=2 — pub. 2024-02-23 — acc. 2026-10-25 — loc. paragraph 2 (Stage-II investment) | NTPC Lara Stage-II, ultra-supercritical, brownfield on Stage-I land; Stage-I cost about 15,800 crore |
| P7-IN-002 | India–Nepal–Bhutan | India | Coal | capital_per_kw | 8.34 | crore INR/MW | 2021-22 basis | derived 83,400 INR/kW; FX: none | CEA — *Report on Optimal Generation Capacity Mix for 2029-30, Version 2.0* (copy hosted by JMK Research; CEA link not reachable) — https://jmkresearch.com/wp-content/uploads/2023/05/Report_on_Optimal_Generation_Mix_in_year_2030_1683189805.pdf — pub. 2023-04 — acc. 2026-10-25 — loc. §2 (coal capex), PDF p. 15 | Planner's new-coal capex assumption |
| P7-IN-003 | India–Nepal–Bhutan | India | Coal | fixed_om_per_kw_year | 23.20 | lakh INR/MW-yr | FY 2024-25 | derived 2,320 INR/kW-yr; FX: none | CERC — *Terms and Conditions of Tariff Regulations, 2024* — https://cercind.gov.in/regulations/notification-2024.pdf — pub. 2024-03-15 — acc. 2026-10-25 — loc. Reg. 36(1)(1), table, 800 MW series and above, FY 2024-25, p. 70 | Normative O&M; water, security, ash and spares allowed separately |
| P7-IN-004 | India–Nepal–Bhutan | India | Ccgt | fixed_om_per_kw_year | 18.18 | lakh INR/MW-yr | FY 2024-25 | derived 1,818 INR/kW-yr; FX: none | CERC — https://cercind.gov.in/regulations/notification-2024.pdf — Tariff Regulations 2024 (as P7-IN-003) — pub. 2024-03-15 — acc. 2026-10-25 — loc. Reg. 36(1)(3), gas turbine/combined-cycle stations (other than small), FY 2024-25, p. 71 | Normative O&M |
| P7-IN-005 | India–Nepal–Bhutan | India | Nuclear | capital_per_kw | 1,05,000 crore for 10 × 700 MW | INR | 2017 (constant price) | derived 1.05e12 ÷ 7.0e6 kW = 150,000 INR/kW; FX: none | PIB, Department of Atomic Energy — Construction of New PHWR — https://www.pib.gov.in/newsite/PrintRelease.aspx?relid=168650&reg=48&lang=2 — pub. 2017-07-19 — acc. 2026-10-25 — loc. paragraph 2 | Fleet-mode 700 MW PHWRs; excludes escalation and interest during construction |
| P7-IN-006 | India–Nepal–Bhutan | India | Hydro | capital_per_kw | 8,146.21 crore for 700 MW | INR | 2025 | derived 8.14621e10 ÷ 7.0e5 kW = 116,374 INR/kW; FX: none | PIB, CCEA — Tato-II Hydro Electric Project approval — https://www.pib.gov.in/PressReleasePage.aspx?PRID=2155471&reg=48&lang=2 — pub. 2025-08-12 — acc. 2026-10-25 — loc. headline and paragraph 1 | NEEPCO joint venture, 4 × 175 MW, Arunachal Pradesh; excludes 458.79 crore of central support for roads and lines |
| P7-IN-007 | India–Nepal–Bhutan | India | Hydro | build_months_real | 72 | months | — | 72 months; FX: none | PIB, CCEA — https://www.pib.gov.in/PressReleasePage.aspx?PRID=2155471&reg=48&lang=2 — Tato-II (as P7-IN-006) — pub. 2025-08-12 — acc. 2026-10-25 — loc. paragraph 1 (estimated completion period) | *Planned* completion period at investment approval |
| P7-IN-008 | India–Nepal–Bhutan | India | Solar | capital_per_kw | 4 | crore INR/MW | 2023 | derived 40,000 INR/kW; FX: none | PIB, MNRE — Government initiatives to incentivise investment by private sector in renewable energy — https://www.pib.gov.in/Pressreleaseshare.aspx?PRID=1896063&reg=48&lang=2 — pub. 2023-02-03 — acc. 2026-10-25 — loc. final paragraph (normative cost) | Ministry's normative cost for solar installations |
| P7-IN-009 | India–Nepal–Bhutan | India | Battery | capital_per_kw | 9,400 crore outlay for 4,000 MWh | INR | 2023 | derived 9.4e10 ÷ 4.0e6 kWh = 23,500 INR/kWh, × 4 h = 94,000 INR/kW; FX: none | Prime Minister's Office — Cabinet approves VGF for BESS — https://www.pmindia.gov.in/en/news_updates/cabinet-approves-the-scheme-titled-viability-gap-funding-for-development-of-battery-energy-storage-systems-bess/ — pub. 2023-09-06 — acc. 2026-10-25 — loc. paragraphs 1–2 | Outlay = capital cost: the 3,760 crore budget support is 40% of 9,400. The 4-hour conversion is derived |
| P7-IN-010 | India–Nepal–Bhutan | India | AcLine | line_cost_per_km | 10,163 | INR per MW-km | 2023 | not convertible to INR/km without a line rating; FX: none | CEA — https://jmkresearch.com/wp-content/uploads/2023/05/Report_on_Optimal_Generation_Mix_in_year_2030_1683189805.pdf — Optimal Generation Capacity Mix 2029-30 v2.0 (as P7-IN-002) — pub. 2023-04 — acc. 2026-10-25 — loc. §7.10, below Table 5, PDF p. 28 | Inter-regional candidate lines (twice this value to or from NER); per MW of transfer capacity, so not a per-km line cost |
| P7-IN-011 | India–Nepal–Bhutan | India | Tariff | retail_tariff_per_mwh | 7.03 | INR/kWh | FY 2024-25 | derived 7,030 INR/MWh; FX: none | Dataful, compiling Ministry of Power and PFC data — ACS–ARR gap (cash adjusted) of state distribution utilities — https://dataful.in/datasets/23048/ — pub. 2026-02-25 (last updated) — acc. 2026-10-25 — loc. All India grand total, 2024-25, ARR column | Average revenue realised, all consumers of state distribution companies (ACS 7.10). Third-party compilation; the PFC report itself was not reached |
| P7-IN-012 | India–Nepal–Bhutan | India | Coal | fixed_om_per_kw_year | 19.54 | lakh INR/MW-yr | 2021-22 | derived 1,954 INR/kW-yr; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) | Planner assumption; CERC norm P7-IN-003 is the regulated alternative |
| P7-IN-013 | India–Nepal–Bhutan | India | Coal | build_months_real | 4 | years | — | 48 months; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156), Construction Time column | Planner construction-time assumption |
| P7-IN-014 | India–Nepal–Bhutan | India | Hydro | capital_per_kw | 6 to 20 | crore INR/MW | 2021-22 | derived 60,000–200,000 INR/kW; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) | Project-specific range from developers and CEA cost estimates; P7-IN-006 (Tato-II, 116,374) lies inside it |
| P7-IN-015 | India–Nepal–Bhutan | India | Hydro | fixed_om_per_kw_year | 2.5% of capex | share | 2021-22 | derived 2.5% × 116,374 (P7-IN-006) = 2,909 INR/kW-yr; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) | Percentage applied to the Tato-II capital row is a derived pairing |
| P7-IN-016 | India–Nepal–Bhutan | India | Hydro | build_months_real | 5 to 8 | years | — | 60–96 months; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156), Construction Time column | Range; P7-IN-007 (72 months) lies inside it |
| P7-IN-017 | India–Nepal–Bhutan | India | Solar | capital_per_kw | 4.5 (2021-22) falling to 4.1 (2029-30) | crore INR/MW | 2021-22 | derived 45,000 INR/kW (2021-22); FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156), footnote on solar trajectory | Planner assumption; corroborates P7-IN-008 |
| P7-IN-018 | India–Nepal–Bhutan | India | Solar | fixed_om_per_kw_year | 1% of capex | share | 2021-22 | derived 1% × 45,000 = 450 INR/kW-yr; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) |  |
| P7-IN-019 | India–Nepal–Bhutan | India | Solar | build_months_real | 0.5 | years | — | 6 months; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156), Construction Time column | Planner assumption |
| P7-IN-020 | India–Nepal–Bhutan | India | Wind | capital_per_kw | 6 | crore INR/MW | 2021-22 | derived 60,000 INR/kW; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) | Onshore wind planner assumption |
| P7-IN-021 | India–Nepal–Bhutan | India | Wind | fixed_om_per_kw_year | 1% of capex | share | 2021-22 | derived 600 INR/kW-yr; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) |  |
| P7-IN-022 | India–Nepal–Bhutan | India | Wind | build_months_real | 1.5 | years | — | 18 months; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156), Construction Time column | Planner assumption |
| P7-IN-023 | India–Nepal–Bhutan | India | Battery | fixed_om_per_kw_year | 1% of capex | share | 2021-22 | derived 1% × 94,000 (P7-IN-009) = 940 INR/kW-yr; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156) | Percentage applied to P7-IN-009 capital is a derived pairing |
| P7-IN-024 | India–Nepal–Bhutan | India | Battery | build_months_real | 0.5 | years | — | 6 months; FX: none | CEA — *National Electricity Plan (Volume I) Generation 2022-32*, gazette edition (copy hosted by Climate Policy Radar; both CEA URLs return 404) — https://cpr-prod-cclw-document-admin-store.s3.eu-west-1.amazonaws.com/NEP_2022_32_FINAL_GAZETTE_English.pdf (downloaded; SHA-256 01f0dd82a2aed75e685d9668b5fcfea1471fc5f4bb570b848577695d9ea93471) — pub. 2023-05 — acc. 2026-10-25 — loc. Chapter 5, Financial Parameters table (cost level 2021-22), p. 5.29–5.30 (PDF p. 156), Construction Time column | Planner assumption |
| P7-IN-025 | India–Nepal–Bhutan | India | Wind | capital_per_kw | 1,172 | USD/kW | 2025 | derived 1,172 × 87.15845 = 102,150 INR/kW; FX: P7-FX-021 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Table 2.1, India, 2025 weighted average, p. 46 | Market weighted average of commissioned onshore projects; well above the NEP planning value P7-IN-020 |
| P7-IN-026 | India–Nepal–Bhutan | India | Solar | capital_per_kw | 545 | USD/kW | 2025 | derived 545 × 87.15845 = 47,501 INR/kW; FX: P7-FX-021 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Table 3.1, India, 2025 weighted average, p. 62 | Utility-scale PV; corroborates P7-IN-008 and P7-IN-017 |
| P7-IN-027 | India–Nepal–Bhutan | India | Hydro | capital_per_kw | 2,003 | USD/kW | 2018–2025 (2025 USD) | derived 2,003 × 87.15845 = 174,578 INR/kW; FX: P7-FX-021 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 6.5, large hydropower weighted average 2018–2025, India label, p. 107 | Cross-check; above P7-IN-006 |

### Southern China (`SC`, CNY)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-SC-001 | Southern China | China (national) | Wind | capital_per_kw | about 4,100 | CNY/kW | 2025 | 4,100 CNY/kW; FX: none | The Paper, reporting China Renewable Energy Engineering Institute (水电水利规划设计总院), *China Renewable Energy Engineering Cost Management Report 2025* — https://www.thepaper.cn/newsDetail_forward_33396923 — pub. 2026-06-17 — acc. 2026-10-25 — loc. onshore wind paragraph | Average total investment per kW, national onshore wind; down 2.4% year on year. National, not Southern China specific |
| P7-SC-002 | Southern China | China (national) | Solar | capital_per_kw | about 3,200 | CNY/kW | 2025 | 3,200 CNY/kW; FX: none | The Paper, reporting the same CREEI 2025 report (as P7-SC-001) — https://www.thepaper.cn/newsDetail_forward_33396923 — pub. 2026-06-17 — acc. 2026-10-25 — loc. centralised PV paragraph | Average total investment per kW, centralised onshore PV; down 7.2% year on year |
| P7-SC-003 | Southern China | Guangdong | Coal | capital_per_kw | 780,656 (10,000 CNY) for 2 × 1,000 MW | CNY | 2022 | derived 7.80656e9 ÷ 2.0e6 kW = 3,903 CNY/kW; FX: none | Guangdong Baolihua New Energy (SZSE 000690) — Announcement 2022-025 on approval of Lufeng Jiahu Bay units 3–4 — https://disc.static.szse.cn/disc/disk03/finalpage/2022-09-14/49356c75-5a7f-4405-bac8-c7680136c7bf.PDF — pub. 2022-09-14 — acc. 2026-10-25 — loc. paragraph 2 (total investment), citing Guangdong DRC approval 粤发改核准〔2022〕31号 | Ultra-supercritical 2 × 1,000 MW extension at an existing site |
| P7-SC-004 | Southern China | Guangdong | Coal | build_months_real | in service before end of 2024 | target date | — | *planned* window ≤ 27 months (2022-09 approval to 2024-12); FX: none | Baoxin Energy Announcement 2022-025 (as P7-SC-003) — https://disc.static.szse.cn/disc/disk03/finalpage/2022-09-14/49356c75-5a7f-4405-bac8-c7680136c7bf.PDF — pub. 2022-09-14 — acc. 2026-10-25 — loc. paragraph 2 (in-service target) | Approval-to-target window, not an observed construction duration |
| P7-SC-005 | Southern China | Guangdong (east and west, non-Pearl-River-Delta) | Tariff | retail_tariff_per_mwh | 67.986875 | fen/kWh (tax included) | 2025 (Aug) | derived 679.87 CNY/MWh; FX: none | Shantou Municipal Government, publishing the Guangdong grid company's agency-purchase tariff table — 广东省电网企业代理购电用户电价表（汕头市）, August 2025 — https://www.shantou.gov.cn/attachment/0/137/137749/2459290.pdf — pub. 2025-07-29 (PDF date) — acc. 2026-10-25 — loc. industrial and commercial, single-part, 1–10(20) kV, energy price column | Commercial and industrial users on agency purchase, flat period, tax included. Applies to Shantou, Chaozhou, Jieyang, Shanwei, Yangjiang, Zhanjiang, Maoming, Zhaoqing and three Jiangmen cities |
| P7-SC-006 | Southern China | China (national) | Hydro | capital_per_kw | 2,302 | USD/kW | 2018–2025 (2025 USD) | derived 2,302 × 7.189847 = 16,551 CNY/kW; FX: P7-FX-020 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 6.5, large hydropower weighted average 2018–2025, China label, p. 107 | Country-specific international measurement; no local-currency hydro source found |
| P7-SC-007 | Southern China | China (national) | Wind | capital_per_kw | 789 | USD/kW | 2025 | derived 789 × 7.189847 = 5,673 CNY/kW; FX: P7-FX-020 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Table 2.1, China, 2025 weighted average, p. 46 | Cross-check; 38% above CREEI P7-SC-001. CREEI (local currency, national planner) stays the baseline |
| P7-SC-008 | Southern China | China (national) | Solar | capital_per_kw | 533 | USD/kW | 2025 | derived 533 × 7.189847 = 3,832 CNY/kW; FX: P7-FX-020 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Table 3.1, China, 2025 weighted average, p. 62 | Cross-check of P7-SC-002 (+20%) |

### IPS/UPS (`IPS`, RUB)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-IPS-001 | IPS/UPS | Russia (Moscow) | Tariff | retail_tariff_per_mwh | 7.87 from 2025-07-01 (6.99 before) | RUB/kWh | 2025 | derived 7,870 RUB/MWh; FX: none | RBC Realty, reporting the Moscow Department of Economic Policy and Development tariff order — https://realty.rbc.ru/news/673c9b499a79473b78eb639d — pub. 2024-11-19 — acc. 2026-10-25 — loc. tariff paragraph, homes with gas stoves, single-rate | Regulated household tariff, gas-stove homes, single rate; electric-stove homes 7.16. Press report of the order; the order's number is not given |
| P7-IPS-002 | IPS/UPS | Russia | Nuclear | capital_per_kw | 13,785 billion for 29.299 million kW | RUB (incl. VAT, forecast prices of each year) | 2025–2042 | derived 13,785e9 ÷ 29.299e6 kW = 470,494 RUB/kW; FX: none | Government of the Russian Federation — Order No. 4153-r of 30 Dec 2024 approving the *General Scheme for the Location of Electric Power Facilities to 2042* — http://static.government.ru/media/files/Rwf9Akjf5FwAnustDEL2m7PEvZ26i7k3.pdf (downloaded; SHA-256 d50d7cd30c93f4f34a836eea0a6a7b832ecd388bcbc3f1676814cb5849d2f07c) — pub. 2024-12-30 — acc. 2026-10-25 — loc. Annex 31, Total, nuclear (PDF p. 153); §IV, nuclear additions (p. 14) | Planner aggregate: investment ÷ planned additions, not one project; includes VAT and future-year nominal prices; includes decentralised-zone units |
| P7-IPS-003 | IPS/UPS | Russia | Hydro | capital_per_kw | 2,517 billion for 7.756 million kW | RUB (incl. VAT, forecast prices) | 2025–2042 | derived 324,523 RUB/kW; FX: none | Government of the Russian Federation — Order No. 4153-r of 30 Dec 2024 approving the *General Scheme for the Location of Electric Power Facilities to 2042* — http://static.government.ru/media/files/Rwf9Akjf5FwAnustDEL2m7PEvZ26i7k3.pdf (downloaded; SHA-256 d50d7cd30c93f4f34a836eea0a6a7b832ecd388bcbc3f1676814cb5849d2f07c) — pub. 2024-12-30 — acc. 2026-10-25 — loc. Annex 31, Total, hydro and pumped storage (PDF p. 153); §IV, hydro additions (p. 14) | Planner aggregate; includes pumped storage |
| P7-IPS-004 | IPS/UPS | Russia (Zabaykalsky Krai) | Coal | capital_per_kw | about 172 billion for 2 × 230 MW | RUB | 2025 | derived 172e9 ÷ 460,000 kW = 373,913 RUB/kW; FX: none | Investfuture, reporting Inter RAO CEO S. Dregval at SPIEF — Inter RAO building new Kharanorskaya GRES units by 2029 — https://investfuture.ru/articles/inter-rao-vedet-stroitelstvo-novykh-energoblokov-kharanorskoy-gres-k-2029-godu-36855650 — pub. 2026-06-09 — acc. 2026-10-25 — loc. investment and schedule paragraphs | Whole project, steam units in the Siberian south-east (Far-East-adjacent remote site); press report of company statement. The General Scheme thermal aggregate (4,782 bn RUB ÷ 35.378 million kW = 135,169 RUB/kW, PDF p. 153 and p. 15) mixes gas, coal and reconstruction and is not used |
| P7-IPS-005 | IPS/UPS | Russia (Zabaykalsky Krai) | Coal | build_months_real | construction launched 2025-10; commissioning planned 2029 | dates | — | derived *planned* 39–50 months; FX: none | Investfuture (as P7-IPS-004) — https://investfuture.ru/articles/inter-rao-vedet-stroitelstvo-novykh-energoblokov-kharanorskoy-gres-k-2029-godu-36855650 — pub. 2026-06-09 — acc. 2026-10-25 — loc. schedule paragraph | Planned. `ips_ups.ron` uses it: the midpoint of the range, 44.5 months, paced through the coal month ratio to 10 game months (authoring judgment on the midpoint; ruling 1 sources the duration) |
| P7-IPS-006 | IPS/UPS | Russia (Krasnodar Krai) | Ccgt | capital_per_kw | 275 | million RUB/MW | 2025 | 275,000 RUB/kW; FX: none | Kommersant — southern TPP projects and capex prepared for government approval — https://www.kommersant.ru/doc/8159614 — pub. 2025-10-27 — acc. 2026-10-25 — loc. Udarnaya TPP paragraph | Udarnaya TPP expansion, three CCGT units, 555 MW, from government-commission materials as reported; Tavricheskaya (Crimea) excluded by footprint |
| P7-IPS-007 | IPS/UPS | Russia | Wind | capital_per_kw | 1,818 | USD/kW | 2025 | derived 1,818 × 83.6525 = 152,080 RUB/kW; FX: P7-FX-023 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Russian Federation, 2025 label, p. 47 | Country market weighted average of commissioned onshore wind (+54% on 2024) |
| P7-IPS-008 | IPS/UPS | Russia (Stavropol Krai) | Wind | capital_per_kw | 64.86 | thousand RUB/kW | 2019 bid (COD 2024) | 64,860 RUB/kW; FX: none | Kommersant, reporting ATS-published bids in the 2019 renewables CSA selection — https://www.kommersant.ru/doc/3998706 — pub. 2019-06-11 — acc. 2026-10-25 — loc. wind paragraph (cap 109.23) | Bid capex of a 71.25 MW project; dated; cap was 109.23 thousand RUB/kW |
| P7-IPS-009 | IPS/UPS | Russia | Solar | capital_per_kw | 49.78 | thousand RUB/kW | 2019 bid (COD 2022) | 49,780 RUB/kW; FX: none | Kommersant (as P7-IPS-008) — https://www.kommersant.ru/doc/3998706 — pub. 2019-06-11 — acc. 2026-10-25 — loc. solar paragraph | Fortum 5.6 MW bid; other bids 53.8 and 54.9; dated and small |
| P7-IPS-010 | IPS/UPS | Eurasia (IRENA region) | Solar | capital_per_kw | 998 | USD/kW | 2025 | derived 998 × 83.6525 = 83,485 RUB/kW; FX: P7-FX-023 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Table 3.1, Eurasia, 2025 weighted average, p. 62 | Regional aggregate (IRENA Eurasia), not Russia alone; more current than P7-IPS-009 |
| P7-IPS-011 | IPS/UPS | Russia (Moscow) | Ccgt | fuel_or_marginal | 7,763 | RUB per 1,000 m³ (excl. VAT) | 2025 (from 1 July) | derived 7,763 ÷ 33.080 GJ = 234.7 RUB/GJ; FX: none | FAS Russia — Order No. 1009/24 of 13 Dec 2024 on wholesale gas prices (Gazprom), text at pravo.ppt.ru — https://pravo.ppt.ru/prikaz/fas/n-1009-24-310500 — pub. 2024-12-13 (registered 2025-02-27) — acc. 2026-10-25 — loc. annex, Moscow row; calorific basis 7,900 kcal/m³ (33,080 kJ/m³) | Regulated wholesale gas price at transmission exit, consumers other than households; fuel only. Also used for Peaker |
| P7-IPS-012 | IPS/UPS | Russia | AcLine | line_cost_per_km | Л1-07 8,944 + Л3-07 5,483 + Л5-09 1,225 × 3 | thousand RUB/km (excl. VAT) | 2018 (prices at 01.01.2018) | derived 18,102 thousand = 18,102,000 RUB/km; FX: none | Minenergo — Order No. 10 of 17 Jan 2019, aggregated price norms for typical grid construction solutions, text at legalacts.ru — https://legalacts.ru/doc/prikaz-minenergo-rossii-ot-17012019-n-10-ob-utverzhdenii/ — pub. 2019-01-17 — acc. 2026-10-25 — loc. §2 (price base); tables Л1, Л3, Л5 | 500 kV single circuit, lattice towers: works + towers + three conductors per phase (Table С2 recommends 3 for 500 kV). Sum of components is derived; excludes land, design and regional factors. Superseded by Order No. 131 of 26 Feb 2024 (not reached) |
| P7-IPS-013 | IPS/UPS | Kazakhstan | Wind | capital_per_kw | 1,353 | USD/kW | 2025 | derived 1,353 × 83.6525 = 113,182 RUB/kW; FX: P7-FX-023 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Kazakhstan, 2025 label, p. 47 | Country market average; recorded for the IPS/UPS opt-in test (not a whole-project figure) |
| P7-IPS-014 | IPS/UPS | Uzbekistan | Wind | capital_per_kw | 1,382 | USD/kW | 2025 | derived 1,382 × 83.6525 = 115,608 RUB/kW; FX: P7-FX-023 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Uzbekistan, 2025 label, p. 47 | Country market average; recorded for the opt-in test |

### Spain (`ES`, EUR)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-ES-001 | Europe–Maghreb | Spain | Wind | capital_per_kw | 70 million for 63 MW | EUR | 2021 | derived 70e6 ÷ 63,000 kW = 1,111 EUR/kW; FX: none | Iberdrola — Iberdrola puts the Herrera II wind complex into service — https://www.iberdrola.com/sala-comunicacion/noticias/detalle/iberdrola-pone-en-marcha-complejo-eolico-herrera-ii — pub. 2021-08-17 — acc. 2026-10-25 — loc. paragraphs 4–5 (capacity; investment) | Three onshore farms in Burgos, 14 × SG 4.5-145 turbines. The source words the amount as spent on promoting the complex; treated as whole-project capital |
| P7-ES-002 | Europe–Maghreb | Spain | Wind | capital_per_kw | 53 million for 50 MW | EUR | 2020 | derived 1,060 EUR/kW; FX: none | Asociación Empresarial Eólica (REVE) — Elecnor commissions Cofrentes wind farm — https://reve.aeeolica.org/2020/04/21/elecnor-pone-en-marcha-el-primer-parque-eolico-de-la-comunidad-valenciana-desde-2012/ — pub. 2020-04-21 — acc. 2026-10-25 — loc. investment paragraph | 13 × 3.83 MW GE turbines, Valencia; corroborates P7-ES-001 |
| P7-ES-003 | Europe–Maghreb | Spain | Wind | build_months_real | 8 | months | — | 8 months; FX: none | Iberdrola España — Iberdrola puts the Puylobo wind farm into operation — https://www.iberdrolaespana.com/sala-comunicacion/noticias/detalle/201130_np_01_parque_eolico_puylobo — pub. 2020-11-29 — acc. 2026-10-25 — loc. paragraph 1 (construction time) | 49 MW, 14 × SG 3.4-132, Zaragoza; built during the pandemic; the company calls it record time |
| P7-ES-004 | Europe–Maghreb | Spain | Solar | capital_per_kw | nearly 300 million for 500 MWp (391 MW grid connection) | EUR | 2019 | derived ≤ 300e6 ÷ 391,000 kWac = 767 EUR/kWac (600 EUR/kWp); FX: none | Iberdrola — Núñez de Balboa completed — https://www.iberdrola.com/press-room/news/detail/nunez-balboa-completed-iberdrola-finalizes-construction-largest-photovoltaic-plant-europe-within-year — pub. 2019-12-27 — acc. 2026-10-25 — loc. paragraph 2 (500 MW, investment); AC rating from Iberdrola project page https://www.iberdrola.com/about-us/power/solar-photovoltaic-energy/nunez-de-balboa-photovoltaic-plant (undated page), installed-capacity line | Badajoz. "Nearly" makes 767 an upper bound per kWac |
| P7-ES-005 | Europe–Maghreb | Spain | Solar | build_months_real | 12 | months | — | 12 months; FX: none | Iberdrola — https://www.iberdrola.com/press-room/news/detail/nunez-balboa-completed-iberdrola-finalizes-construction-largest-photovoltaic-plant-europe-within-year — Núñez de Balboa completed (as P7-ES-004) — pub. 2019-12-27 — acc. 2026-10-25 — loc. headline and paragraph 1 (construction in one year) | Construction completed December 2019; operation forecast for Q1 2020 |
| P7-ES-006 | Europe–Maghreb | Spain | Wind | capital_per_kw | 1,422 | USD/kW | 2025 | derived 1,422 × 0.884969 = 1,258 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Spain, 2025 label, p. 47 | Market weighted average, not a single project; Spain's wind capital of record in the 2024–25 pairing with P7-MA-004 (user decision 2026-10-26; see the contrast) |
| P7-ES-007 | Europe–Maghreb | Spain | Solar | capital_per_kw | 690 | USD/kW | 2025 | derived 610.6 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 3.2, Spain, 2025 label, p. 63 | Market weighted average |

### Morocco (`MA`, EUR via MAD)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-MA-001 | Europe–Maghreb | Morocco | Wind | capital_per_kw | total project cost 130,000,000 for 100 MW | EUR | 2021 | derived 130e6 ÷ 100,000 kW = 1,300 EUR/kW; FX: none (stated in EUR) | EBRD — Koudia Al Baida Wind Farm, Project Summary Document 50957 — https://www.ebrd.com/work-with-us/projects/psd/50957.html — pub. 2021-12-23 — acc. 2026-10-25 — loc. Total Project Cost; Project Description | Repowering of the 50 MW Koudia Al Baida farm near Tangier to 100 MW with 5 MW turbines. Includes replacing the old units; reuses the site and grid connection |
| P7-MA-002 | Europe–Maghreb | Morocco | Wind | build_months_real | works launched 2022-07-18; construction complete and first MW injected by 2024-03-06 | dates | — | derived 597 days = 19.6 months; FX: none | Masen — press release on financial close and launch of works for Nassim Koudia Al Baida — https://www.masen.ma/sites/default/files/documents_presse/Communiqu%C3%A9%20de%20presse_KOUDIA_LANCEMENT_REPOWERING.pdf — pub. 2022-07-18 — acc. 2026-10-25 — loc. p. 1, paragraphs 2 and 4; and Le360, reporting a Masen communiqué — https://fr.le360.ma/economie/energies-renouvelables-les-parcs-eoliens-nassim-koudia-al-baida-et-nassim-jbel-lahdid-operationnels_J3YN6WL2KRBRBAXMOJE2FTIFDY/ — pub. 2024-03-06 — acc. 2026-10-25 — loc. Koudia paragraph | Launch to completion of construction; commercial operation was expected in the first half of 2024 |
| P7-MA-003 | Europe–Maghreb | Morocco | Wind | build_months_real | commercial operation planned for Q2 2024 | target date | — | derived *planned* 20.5–23.4 months from 2022-07-18; FX: none | Masen — https://www.masen.ma/sites/default/files/documents_presse/Communiqu%C3%A9%20de%20presse_KOUDIA_LANCEMENT_REPOWERING.pdf — Koudia release (as P7-MA-002) — pub. 2022-07-18 — acc. 2026-10-25 — loc. p. 1, paragraph 4 | Planned launch-to-operation window; corroborates P7-MA-002 |
| P7-MA-004 | Europe–Maghreb | Morocco | Wind | capital_per_kw | about 3.25 billion for 270 MW | MAD | 2024 | derived 3.25e9 ÷ 270,000 kW = 12,037 MAD/kW ÷ 10.7614 = 1,119 EUR/kW; FX: P7-FX-005, P7-FX-006 | Enerdata, reporting ONEE — Morocco starts operations of 270 MW Jbel Lahdid wind project — https://www.enerdata.net/publications/daily-energy-news/morocco-starts-operations-270-mw-jbel-lahdid-wind-project.html — pub. 2024-10-15 — acc. 2026-10-25 — loc. paragraph 1 | 54 × 5 MW, Essaouira; ONEE with Nareva and Enel Green Power. An announced project total; Morocco's wind capital of record in the 2024–25 pairing with P7-ES-006 (user decision 2026-10-26; see the contrast) |
| P7-MA-005 | Europe–Maghreb | Morocco | Solar | capital_per_kw | more than 750 million for 72 MW | MAD | 2017 | derived 750e6 ÷ 72,000 kW = 10,417 MAD/kW ÷ 10.9489 = 951 EUR/kW; FX: P7-FX-001, P7-FX-002 | Masen — press release on the launch of works at Noor Ouarzazate IV — https://www.masen.ma/sites/default/files/documents_presse/17.pdf — pub. 2017-04-01 — acc. 2026-10-25 — loc. p. 1, paragraph 3 | Estimated investment at launch; 72 MW maximum capacity, single-axis tracking PV; ACWA Power consortium. "More than" makes 951 a lower bound |
| P7-MA-006 | Europe–Maghreb | Morocco | Solar | build_months_real | works launched 2017-04-01; commissioning planned for Q1 2018 | dates | — | derived *planned* 9–12 months; FX: none | Masen — https://www.masen.ma/sites/default/files/documents_presse/17.pdf — Noor Ouarzazate IV release (as P7-MA-005) — pub. 2017-04-01 — acc. 2026-10-25 — loc. p. 1, paragraphs 1 and 3 | Planned; actual commissioning date not found |
| P7-MA-007 | Europe–Maghreb | Morocco | Wind | capital_per_kw | actual investment 215.85 million for 140 MW | EUR | 2011 (commissioned) | derived 1,542 EUR/kW; FX: none (stated in EUR) | KfW Development Bank — *Ex post evaluation – Morocco: Wind Farm Tangier II* (evaluation report 2015) — https://www.kfw-entwicklungsbank.de/PDF/Evaluierung/Ergebnisse-und-Publikationen/PDF-Dokumente-L-P_EN/Marokko_Windpark_E_2016.pdf (downloaded; SHA-256 1da025e6b444762c8e5c3651ba2a52f415d8157a06c807bbfcf015e24be408d9) — pub. 2016-05 (PDF) — acc. 2026-10-25 — loc. p. 1 header table (actual); p. 4 efficiency section | ONEE Tangier II, 165 × 850 kW; 29% over the planned 167 million; old turbine class and price year, so not used for the paced contrast |
| P7-MA-008 | Europe–Maghreb | Morocco | Wind | build_months_real | put into operation 25 months later than planned | months of delay | — | +25 months versus plan; FX: none | KfW Development Bank — *Ex post evaluation – Morocco: Wind Farm Tangier II* (evaluation report 2015) — https://www.kfw-entwicklungsbank.de/PDF/Evaluierung/Ergebnisse-und-Publikationen/PDF-Dokumente-L-P_EN/Marokko_Windpark_E_2016.pdf (downloaded; SHA-256 1da025e6b444762c8e5c3651ba2a52f415d8157a06c807bbfcf015e24be408d9) — pub. 2016-05 (PDF) — acc. 2026-10-25 — loc. p. 4, efficiency section | Independent evidence of long Moroccan wind schedules (land rights, turbine repositioning, cabling); a delay, not a full duration |
| P7-MA-009 | Europe–Maghreb | Morocco | Wind | capital_per_kw | total cost 704 million for 150 + 100 + 200 MW | EUR | 2013 (appraisal) | derived 704e6 ÷ 450,000 kW = 1,564 EUR/kW; FX: none | EIB — ONEE - Projet Éolien, project 20120174 — https://www.eib.org/en/projects/all/20120174 — pub. 2013-11-06 (release date) — acc. 2026-10-25 — loc. Total cost; Description | Planned programme cost for Midelt, Tangier and Essaouira farms; capacities later changed; independent of EBRD |

### Algeria (`DZ`, EUR via DZD) — follow-up 2026-10-26

All `DZ` rows were accessed on 2026-10-26. Algerian figures come mostly from the Algerian trade press reporting
Sonelgaz, ministry or APS statements. No development-bank appraisal of an Algerian generation or
line project was found (see [Gaps](#gaps)). Solar lot capacities are peak (DC) ratings. The 2023
award article states the programme as 2,000 MWc in its scope description, though it gives each lot
in MW. The Touggourt lot is 150 MW in that article and 150 MWc in its 2025 re-award (P7-DZ-006).

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-DZ-001 | Europe–Maghreb | Algeria | Solar | capital_per_kw | provisional award 15.76 billion for 200 MW (Tendla lot) | DZD | 2023 | derived 15.76e9 ÷ 200,000 kW = 78,800 DZD/kW ÷ 146.8827 = 536.5 EUR/kW, treated as per kWp; FX: P7-FX-029, P7-FX-030 | Algérie Eco, reporting the provisional award notice of Sonelgaz Energies renouvelables — article on the provisional award of the 2,000 MW project — https://www.algerie-eco.com/2023/11/03/projet-2000-mw-attribution-provisoire-des-marches/ — pub. 2023-11-03 — acc. 2026-10-26 — loc. Tendla (El M'Ghaier) lot paragraph | CSCEC; EPC scope (studies, engineering, civil works, supply, transport, installation, testing and commissioning, including the works connecting the plant to the transmission grid), so it excludes the owner's costs and is a lower bound; tax basis not stated; 14-month delivery period. DC basis: the article states the programme as 2,000 MWc (programme paragraph), and the Touggourt lot is 150 MW here and 150 MWc in its 2025 re-award (P7-DZ-006). The notice's lot capacities sum to 2,060 MW against a stated 2,000 MW programme, so no programme average is derived |
| P7-DZ-002 | Europe–Maghreb | Algeria | Solar | capital_per_kw | provisional award 18.13 billion for 200 MW (Laghrous lot) | DZD | 2023 | derived 18.13e9 ÷ 200,000 kW = 90,650 DZD/kW ÷ 146.8827 = 617.2 EUR/kW, treated as per kWp; FX: P7-FX-029, P7-FX-030 | Algérie Eco — https://www.algerie-eco.com/2023/11/03/projet-2000-mw-attribution-provisoire-des-marches/ — 2,000 MW award article (as P7-DZ-001) — pub. 2023-11-03 — acc. 2026-10-26 — loc. Laghrous (Biskra) lot paragraph | El Ghrous plant; PowerChina NTL–Sinohydro; EPC scope as P7-DZ-001, including the grid-connection works, so a lower bound; 16-month delivery period; DC basis as P7-DZ-001 |
| P7-DZ-003 | Europe–Maghreb | Algeria | Solar | build_months_real | delivery periods 14 months (Tendla) and 16 months (Laghrous); programme range 7–16 months | months | — | *planned* 14 and 16 months; FX: none | Algérie Eco — https://www.algerie-eco.com/2023/11/03/projet-2000-mw-attribution-provisoire-des-marches/ — 2,000 MW award article (as P7-DZ-001) — pub. 2023-11-03 — acc. 2026-10-26 — loc. lot paragraphs; programme paragraph | Contracted delivery periods; the actual durations (P7-DZ-004, -005) ran 8–12 months longer |
| P7-DZ-004 | Europe–Maghreb | Algeria | Solar | build_months_real | first stone laid 2024-03-25; plant commissioned by the minister 2026-05-18 | dates | — | derived 784 days ÷ 30.4375 = 25.8 months (upper bound: the date of grid energisation was not found); FX: none | Algérie Eco — article on the first stone of the 200 MW El M'Ghaier PV plant — https://www.algerie-eco.com/2024/03/25/el-meghaier-pose-de-la-1ere-pierre-dune-centrale-solaire-photovoltaique-de-200-mw/ — pub. 2024-03-25 — acc. 2026-10-26 — loc. paragraphs 1–2 (first stone; 14-month plan); and Horizons — article on the commissioning of the 200 MW El M'Ghaier PV plant — https://www.horizons.dz/2026/05/adjal-met-en-service-une-centrale-photovoltaique-de-200-mw-a-el-meghaier/ — pub. 2026-05-18 — acc. 2026-10-26 — loc. paragraphs 1–2 | Tendla, El M'Ghaier wilaya, in the footprint. The ceremony is dated by the article's day (Monday). Radio Algérie's report (https://news.radioalgerie.dz/fr/node/86449) rejected the request: not reached |
| P7-DZ-005 | Europe–Maghreb | Algeria | Solar | build_months_real | 24 months of works; plant received in April 2026; inaugurated 2026-05-18 | months | — | 24 months; FX: none | Algérie Eco — article on the inauguration of the 200 MW El Ghrous PV plant — https://algerie-eco.com/2026/05/19/biskra-inauguration-de-la-centrale-solaire-photovoltaique-de-200-mw-del-ghrous/ — pub. 2026-05-19 — acc. 2026-10-26 — loc. paragraphs 1–3 | El Ghrous (Laghrous lot of P7-DZ-002), Biskra wilaya, in the footprint; works duration as stated by the article, against a 16-month delivery period |
| P7-DZ-006 | Europe–Maghreb | Algeria | Solar | capital_per_kw | 87 million excluding VAT for 150 MWc (Touggourt lot) | EUR | 2025 | derived 87e6 ÷ 150,000 kWp = 580 EUR/kWp; FX: none (stated in EUR) | Team France Export (Business France), citing DZTenders — sector note on Sonelgaz EnR's award of the last three lots of its 2,000 MW solar programme — https://www.teamfrance-export.fr/infos-sectorielles/37223/37223-sonelgaz-energies-renouvelables-attribue-trois-dernier-lots-de-son-programme-de-2-000-mw-solaire-lance-en-2023 — pub. 2025-04-28 — acc. 2026-10-26 — loc. Touggourt paragraph | CSCEC; award decisions dated 2025-04-21, replacing the cancelled 2023 award (14.47 billion DZD in the P7-DZ-001 source); EPC price, not whole project; 33 million EUR of it repatriable |
| P7-DZ-007 | Europe–Maghreb | Algeria | Solar | capital_per_kw | nearly 153 million excluding VAT for 250 MWc (Temacine lot) | EUR | 2025 | derived < 153e6 ÷ 250,000 kWp = 612 EUR/kWp (upper bound); FX: none | Team France Export — https://www.teamfrance-export.fr/infos-sectorielles/37223/37223-sonelgaz-energies-renouvelables-attribue-trois-dernier-lots-de-son-programme-de-2-000-mw-solaire-lance-en-2023 — Sonelgaz EnR awards (as P7-DZ-006) — pub. 2025-04-28 — acc. 2026-10-26 — loc. Temacine paragraph | Algerian contractor (SARL Hamdi de Transport); EPC price |
| P7-DZ-008 | Europe–Maghreb | Algeria | Solar | capital_per_kw | nearly 84 million excluding VAT for 120 MWc (Kenadsa lot) | EUR | 2025 | derived < 84e6 ÷ 120,000 kWp = 700 EUR/kWp (upper bound); FX: none | Team France Export — https://www.teamfrance-export.fr/infos-sectorielles/37223/37223-sonelgaz-energies-renouvelables-attribue-trois-dernier-lots-de-son-programme-de-2-000-mw-solaire-lance-en-2023 — Sonelgaz EnR awards (as P7-DZ-006) — pub. 2025-04-28 — acc. 2026-10-26 — loc. Kenadsa paragraph | PowerChina International and Sinohydro; Béchar wilaya; EPC price; 41 million EUR repatriable |
| P7-DZ-009 | Europe–Maghreb | Algeria | Wind | capital_per_kw | overall cost 2.8 billion for 12 × 0.85 MW | DZD | 2014 (commissioned) | derived 2.8e9 ÷ 10,200 kW = 274,510 DZD/kW ÷ 107.0493 = 2,564 EUR/kW; FX: P7-FX-025, P7-FX-027 | Maghreb Émergent — article on the inauguration of Algeria's first 10 MW wind farm, Adrar — https://maghrebemergent.news/fr/l-algerie-inaugure-sa-premiere-ferme-eolienne-de-10-megawatts-a-adrar/ — pub. 2014-07-03 — acc. 2026-10-26 — loc. paragraphs 1–2; Algérie360 — article on the commissioning of Algeria's first wind farm — https://www.algerie360.com/energies-renouvelables-mise-en-service-de-la-1ere-ferme-eolienne-en-algerie/ — pub. 2014-07-03 — acc. 2026-10-26 — loc. cost paragraph (same figure) | Kabertène pilot, 72 km north of Adrar: **outside the footprint** (isolated southern Algeria). Cegelec with Sonelgaz subsidiaries; 850 kW turbine class; price year spans 2011–2014 |
| P7-DZ-010 | Europe–Maghreb | Algeria | Wind | build_months_real | works began October 2011; nearly 32 months of works; commissioned 2014-07-03 | months | — | 32 months as stated (33.1 months from 2011-10-01 to 2014-07-03); FX: none | Maghreb Émergent — https://maghrebemergent.news/fr/l-algerie-inaugure-sa-premiere-ferme-eolienne-de-10-megawatts-a-adrar/ — Kabertène inauguration (as P7-DZ-009) — pub. 2014-07-03 — acc. 2026-10-26 — loc. paragraph 2 | Outside the footprint; a 10 MW pilot, not a utility-scale schedule |
| P7-DZ-011 | Europe–Maghreb | Algeria | Ccgt | capital_per_kw | 184.09 billion for 1,406.017 MW | DZD | 2026 (launch of works) | derived 184.09e9 ÷ 1,406,017 kW = 130,930 DZD/kW ÷ 148.7125 = 880.4 EUR/kW; FX: P7-FX-011, P7-FX-013 (2025, the latest WDI year) | TSA Algérie, citing APS and the Ministry of Energy and Renewable Energies — article on the launch of works on a 1.4 billion USD power plant — https://www.tsa-algerie.com/lalgerie-lance-le-chantier-dune-centrale-electrique-pour-14-milliard/ — pub. 2026-04-27 — acc. 2026-10-26 — loc. paragraphs 1–3; Algérie Eco, same day and figures — https://algerie-eco.com/?p=239422 — acc. 2026-10-26 | Tébessa (El Aouinet) combined cycle; Sonelgaz-Production de l'électricité with CNEEC. Estimate at the launch of works, not an outturn cost |
| P7-DZ-012 | Europe–Maghreb | Algeria | Ccgt | build_months_real | 48-month execution period from the launch of works on 2026-04-27 | months | — | *planned* 48 months; FX: none | TSA Algérie — https://www.tsa-algerie.com/lalgerie-lance-le-chantier-dune-centrale-electrique-pour-14-milliard/ — Tébessa launch (as P7-DZ-011) — pub. 2026-04-27 — acc. 2026-10-26 — loc. paragraph 3 | Planned |
| P7-DZ-013 | Europe–Maghreb | Algeria | Ccgt | build_months_real | contract signed in 2014; suspended June 2024; two units planned for 2026 and final reception for 2027 | dates | — | derived *planned* ≥ 12 years from contract to final reception; not a construction duration; FX: none | Algérie Eco — article on Sonelgaz's roadmap to complete the Aïn Oussara plant — https://algerie-eco.com/?p=230883 — pub. 2025-11-19 — acc. 2026-10-26 — loc. paragraphs 1–4 | Djelfa, 1,200 MW; original contractor Duro Felguera (about 544 million EUR, 2014), now in dispute; a 93 billion DZD envelope is stated without a price year, so no capital is derived. Evidence of long Algerian CCGT schedules, like P7-MA-008 |
| P7-DZ-014 | Europe–Maghreb | Algeria | AcLine | line_cost_per_km | 1 billion USD (135.46 billion DZD) for a 700 km double 400 kV line with three 400/220 kV and four 220/60 kV substations | DZD | 2023 | derived 135.46e9 ÷ 700 km = 193.5 million DZD/km ÷ 146.8827 = 1.317 million EUR/km including substations; FX: P7-FX-029, P7-FX-030 | Algérie Eco, citing APS — article on the interconnection of the In Salah–Adrar–Timimoun pole — https://algerie-eco.com/?p=169990 — pub. 2023-05-02 — acc. 2026-10-26 — loc. paragraphs 1–3 | Hassi R'Mel–Timimoun; mostly south of the footprint; substations not separable. **Not usable** as an AC line cost |

### Tunisia (`TN`, EUR) — follow-up 2026-10-26

All `TN` rows were accessed on 2026-10-26. No Tunisian figure is in TND; the sources state EUR, USD or JPY.
afdb.org returned HTTP 403 to every request (project pages and press releases), so AfDB material
is cited from a syndicated copy or recorded as not reached.

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-TN-001 | Europe–Maghreb | Tunisia | Solar | capital_per_kw | total project cost 92,700,000 for 100 MWac | EUR | 2025 (PSD) | derived 92.7e6 ÷ 100,000 kWac = 927 EUR/kWac; FX: none | EBRD — Sidi Bouzid R2, Project Summary Document 56184 — https://www.ebrd.com/work-with-us/projects/psd/56184.html — pub. 2025-08-06 (PSD disclosed; updated 2025-09-23) — acc. 2026-10-26 — loc. Total Project Cost; Project Description | Scatec (50%) and Aeolus (50%) project company; includes a 12 km 225 kV connection line; EBRD approval 2025-10-22. The same plant as P7-TN-002 (inferred from site, sponsors and lenders) |
| P7-TN-002 | Europe–Maghreb | Tunisia | Solar | capital_per_kw | estimated capex 96 million for 120 MW | EUR | 2026 | derived 800 EUR per kW of the stated 120 MW, whose basis is not stated (960 EUR/kWac on P7-TN-001's 100 MWac); FX: none | Scatec — Scatec reaches financial close and starts construction of 120 MW solar power plant in Tunisia — https://www.scatec.com/en/scatec-reaches-financial-close-and-starts-construction-of-120-mw-solar-power-plant-in-tunisia/ — pub. 2026-06-17 — acc. 2026-10-26 — loc. paragraphs 1–2 | Sidi Bouzid II; EBRD and EIB senior lenders; corroborates P7-TN-001 |
| P7-TN-003 | Europe–Maghreb | Tunisia | Solar | build_months_real | construction started at financial close (2026-06-17); commercial operation expected in the second half of 2027 | dates | — | derived *planned* 379–562 days = 12.5–18.5 months; FX: none | Scatec — https://www.scatec.com/en/scatec-reaches-financial-close-and-starts-construction-of-120-mw-solar-power-plant-in-tunisia/ — Sidi Bouzid II release (as P7-TN-002) — pub. 2026-06-17 — acc. 2026-10-26 — loc. paragraphs 1 and 3 | Planned |
| P7-TN-004 | Europe–Maghreb | Tunisia | Solar | capital_per_kw | total project cost 47,678,366.94 for 50 MW | EUR | 2022 (PSD) | derived 953.6 EUR/kW; FX: none | EBRD — Scatec Tozeur PV Power, Project Summary Document 51763 — https://www.ebrd.com/work-with-us/projects/psd/51763.html — pub. 2022-03-24 — acc. 2026-10-26 — loc. Total Project Cost; Project Description | Round-1 concession; AC or DC basis not stated. PSD 51776 (Scatec Sidi Bouzid Mezzouna PV Power, https://www.ebrd.com/work-with-us/projects/psd/51776.html) states the identical cost and capacity. Superseded by the sponsor's 2024 estimate (P7-TN-005) |
| P7-TN-005 | Europe–Maghreb | Tunisia | Solar | capital_per_kw | total project cost estimated at 79 million for Sidi Bouzid (60 MW) and Tozeur (60 MW) | EUR | 2024 | derived 79e6 ÷ 120,000 kW = 658 EUR/kW (790 EUR/kW on the 2 × 50 MW of P7-TN-004 and MIGA); FX: none | Scatec — Scatec and Aeolus, part of Toyota Tsusho Group, join forces for Tunisia solar projects — https://www.scatec.com/en/scatec-and-aeolus-part-of-toyota-tsusho-group-join-forces-for-tunisia-solar-projects/ — pub. 2024-08-05 — acc. 2026-10-26 — loc. financial-close paragraph | Two plants. The lenders' 50 MW against the sponsor's 60 MW suggests 50 MWac / 60 MWp, but no source says so |
| P7-TN-006 | Europe–Maghreb | Tunisia | Solar | build_months_real | financial close announced 2024-08-05, with construction to start after it; commercial operation effective 2026-01-01 | dates | — | derived 514 days = 16.9 months (upper bound); FX: none | Scatec — https://www.scatec.com/en/scatec-and-aeolus-part-of-toyota-tsusho-group-join-forces-for-tunisia-solar-projects/ — 2024-08-05 release (as P7-TN-005); and Scatec — Scatec starts commercial operation of the 60 MW Sidi Bouzid solar power plant in Tunisia — https://www.scatec.com/en/scatec-starts-commercial-operation-of-the-60-mw-sidi-bouzid-solar-power-plant-in-tunisia/ — pub. 2026-03-10 — acc. 2026-10-26 — loc. paragraph 1 | Sidi Bouzid; operation date set retroactively |
| P7-TN-007 | Europe–Maghreb | Tunisia | Solar | build_months_real | financial close announced 2024-08-05; commercial operation effective 2026-03-04 | dates | — | derived 576 days = 18.9 months (upper bound); FX: none | Scatec, as P7-TN-006 for the start; and Scatec — Scatec starts commercial operation of the 60 MW Tozeur solar power plant in Tunisia (press release on pv magazine) — https://www.pv-magazine.com/press-releases/scatec-starts-commercial-operation-of-the-60-mw-tozeur-solar-power-plant-in-tunisia/ — pub. 2026-04-16 — acc. 2026-10-26 — loc. paragraph 1 | Tozeur, built in parallel with Sidi Bouzid |
| P7-TN-008 | Europe–Maghreb | Tunisia | Solar | build_months_real | PV construction expected to begin June 2024 and last up to 15 months | months | — | *planned* ≤ 15 months; FX: none | MIGA — Tozeur and Sidi Bouzid Solar Plants (project summary) — https://miga.org/project/tozeur-and-sidi-bouzid-solar-plants-0 — pub. 2024-06-03 (disclosed) — acc. 2026-10-26 — loc. project description | Planned; the actual durations (P7-TN-006, -007) were 17–19 months |
| P7-TN-009 | Europe–Maghreb | Tunisia | Solar | capital_per_kw | project cost approximately 100 million for 100 MW | USD | 2022 | derived 1,000 USD/kW × 0.949624 = 950 EUR/kW; FX: P7-FX-028 | IFC — Kairouan Solar Plant, Summary of Investment Information 43011 — https://disclosures.ifc.org/project-detail/SII/43011/kairouan-solar-plant — pub. 2022-10-26 — acc. 2026-10-26 — loc. Project Description; project cost | AMEA Power; built as 120 MWp; the 100 MW basis is not stated |
| P7-TN-010 | Europe–Maghreb | Tunisia | Solar | capital_per_kw | deal worth 86 million for 120 MWp | USD | 2024 | derived 716.7 USD/kWp × 0.923890 = 662 EUR/kWp; FX: P7-FX-005 | Invest Africa, republishing AMEA Power's release (dateline Dubai, 2024-05-08) — AMEA Power breaks ground on 120 MWp solar PV project in Tunisia — https://www.investafrica.com/insights-and-news/amea-power-breaks-ground-on-120-mwp-solar-pv-project-in-tunisia — pub. 2024-05-20 — acc. 2026-10-26 — loc. paragraphs 1–2 | Described as the value of the build-own-operate deal financed by IFC and AfDB; it may be the financing package rather than total cost. Conflicts with P7-TN-009 |
| P7-TN-011 | Europe–Maghreb | Tunisia | Solar | build_months_real | groundbreaking announced 2024-05-08; commissioned 2025-12-16 | dates | — | derived 587 days = 19.3 months; FX: none | AMEA Power release (as P7-TN-010) — loc. paragraph 1; and African Development Bank press release, syndicated by allAfrica — AfDB welcomes the commissioning of the 120 MWp Kairouan Solar Photovoltaic Project — https://allafrica.com/stories/202512220042.html — pub. 2025-12-19 — acc. 2026-10-26 — loc. paragraph 1 | Kairouan (Metbassta). The afdb.org original (https://www.afdb.org/en/news-and-events/press-releases/tunisia-african-development-bank-welcomes-commissioning-120-mwp-kairouan-solar-photovoltaic-project-major-milestone-89873) returned HTTP 403: not reached |
| P7-TN-012 | Europe–Maghreb | Tunisia | Ccgt | capital_per_kw | estimated project cost 45,828 million (loan 38,075 million) for a 430–500 MW plant | JPY | 2014 (appraisal) | derived 45,828e6 ÷ 140.7477 = 325.6 million EUR ÷ 450,000 kW = 723.6 EUR/kW (651–757 EUR/kW over 430–500 MW); FX: P7-FX-025, P7-FX-026 | JICA — Ex-Ante Evaluation (for Japanese ODA Loan): Rades Combined Cycle Power Plant Construction Project — https://www2.jica.go.jp/en/evaluation/pdf/2014_TS-P39_1_f.pdf (downloaded; SHA-256 1e28f28854c20f1741ef66697ee7f32f25c892919e3a02751eb2e811f21ebb1b) — pub. 2014 (loan agreement 2014-07-17; PDF modified 2014-12-17) — acc. 2026-10-26 — loc. §3(4) Estimated Project Cost, p. 3 | Radès C, STEG; 450 MW as contracted (P7-TN-013). An appraisal estimate; its components are not itemised |
| P7-TN-013 | Europe–Maghreb | Tunisia | Ccgt | build_months_real | EPC contract announced 2017-06-21; combined-cycle full operation planned for April 2020 | dates | — | derived *planned* 1,015–1,044 days = 33.3–34.3 months; FX: none | Mitsubishi Hitachi Power Systems — news release of 2017-06-21 on the Rades C order — https://power.mhi.com/news/20170621.html — pub. 2017-06-21 — acc. 2026-10-26 — loc. paragraphs 1–2 | 450 MW; gas-turbine first operation planned for May 2019. JICA's appraisal schedule was 58 months from July 2014, including procurement (P7-TN-012 file, §3(5), p. 3) |
| P7-TN-014 | Europe–Maghreb | Tunisia | Ccgt | build_months_real | contract 2017-06-21; start of operations announced 2022-09-06 | dates | — | derived 1,903 days = 62.5 months (contract to operation; upper bound); FX: none | Mitsubishi Power — news of 2022-09-06 on the start of operations at Rades C — https://power.mhi.com/regions/emea/news/20220906.html — pub. 2022-09-06 — acc. 2026-10-26 — loc. paragraph 1 | About 29 months later than the planned April 2020; the period includes COVID-19 |
| P7-TN-015 | Europe–Maghreb | Tunisia | Ccgt | capital_per_kw | total cost approximately 388 million for around 400 MW | EUR | 2010 (appraisal) | derived 970 EUR/kW; FX: none | EIB — STEG - Sousse Power Plant, project 20090293 — https://www.eib.org/en/projects/all/20090293 — pub. 2010-04-29 (release date) — acc. 2026-10-26 — loc. Total cost; Description | Sousse C single-shaft CCGT; indicative appraisal figure |
| P7-TN-016 | Europe–Maghreb | Tunisia | Ccgt | capital_per_kw | total project cost 456.71 million | USD | 2012 (approval) | derived 456.71e6 ÷ 424,000 kW = 1,077 USD/kW × 0.778338 = 838 EUR/kW; FX: P7-FX-024 | OPEC Fund — Sousse-D Combined Cycle Power Plant Project — https://opecfund.org/where-we-work/list/sousse-d-combined-cycle-power-plant-project — undated page (approval 2012-09-12) — acc. 2026-10-26 — loc. Total project cost; with the 424 MW capacity from Leonardo — Finmeccanica: Ansaldo Energia awarded EUR 390 million contracts in Tunisia and Russia — https://www.leonardo.com/en/press-release-detail/-/detail/finmeccanica-ansaldo-energia-awarded-eur-390-million-contracts-in-tunisia-and-russia — pub. 2013-02-26 — acc. 2026-10-26 — loc. paragraph 1 | Co-financed by IsDB, the Saudi Fund and STEG; status completed |
| P7-TN-017 | Europe–Maghreb | Tunisia | Peaker | capital_per_kw | contracts worth 240 million (turnkey EPC plus long-term service agreement) for 625 MW | EUR | 2018 | derived ≤ 384 EUR/kW for EPC plus service; FX: none | Ansaldo Energia — press release, Genoa (hosted by CDP) — https://www.cdp.it/resources/cms/documents/PressRelease_AE_Mornaguia_Eng.pdf (downloaded; SHA-256 65bc57b132166e4cb2006a4b5716386c3cdbca343adab02edf7a0458ad2e03c5) — pub. 2018-04-27 — acc. 2026-10-26 — loc. headline and paragraph 1 | Mornaguia open cycle, 2 × AE94.3A, STEG. A contract value, **not whole-project**: it excludes the owner's costs and includes maintenance services |
| P7-TN-018 | Europe–Maghreb | Tunisia | Peaker | build_months_real | 22 months from Notice to Proceed (planned); completed ahead of schedule, full 600 MW reported 2020-05-29 | months | — | *planned* 22 months; derived ≤ 763 days = 25.1 months from contract signature (2018-04-27) to the report; FX: none | Ansaldo Energia release (as P7-TN-017) — loc. paragraph 2; and Italian Ministry of Foreign Affairs, economic-diplomacy news — https://www.esteri.it/en/?p=49545 — pub. 2020-05-29 — acc. 2026-10-26 — loc. paragraph 1 | Paces to the inherited 2 game months either way |
| P7-TN-019 | Europe–Maghreb | Tunisia | AcLine | line_cost_per_km | total project cost 43,000,000 for an 85 km 400 kV double-circuit line plus about 25 km of entry/exit lines | EUR | 2025 (PSD) | derived 43e6 ÷ 85 km = 505,882 EUR/km of main line (upper bound); 43e6 ÷ 110 km = 390,909 EUR/km of all line; FX: none | EBRD — ELMED Grid Reinforcement (Grombalia 2 – Kondar), Project Summary Document 56889 — https://www.ebrd.com/work-with-us/projects/psd/56889.html — pub. 2025-09-01 (updated 2026-01-13) — acc. 2026-10-26 — loc. Total Project Cost; Project Description | Planned; the total equals the EBRD loan, so other funding may exist. Double circuit, 2 × 570 mm² bundles; entry/exit links are one 400 kV (5 km) and two 225 kV (10 km each) |

### Germany (`DE`, EUR) — technology-level costs used for the regional-default proposal

Source for all DE rows: Fraunhofer ISE, *Stromgestehungskosten Erneuerbare Energien* (Levelized Cost
of Electricity, July 2024 edition; PDF revision of January 2025),
https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf.
The values are specific plant costs in Germany for 2024, excluding VAT. These are
technology-level market ranges, not single whole projects.

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-DE-001 | Europe–Maghreb | Germany | Wind | capital_per_kw | 1,300–1,900 | EUR/kW | 2024 | derived midpoint 1,600 EUR/kW; FX: none | Fraunhofer ISE — LCOE 2024 (https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf) — pub. 2024-07 (rev. 2025-01) — acc. 2026-10-25 — loc. Tabelle 1, Wind Onshore, p. 12 | Low and high 2024 investment |
| P7-DE-002 | Europe–Maghreb | Germany | Coal | capital_per_kw | 1,700–2,300 | EUR/kW | 2024 | derived midpoint 2,000 EUR/kW; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 1, Steinkohle, p. 12 | Hard coal |
| P7-DE-003 | Europe–Maghreb | Germany | Ccgt | capital_per_kw | 900–1,300 | EUR/kW | 2024 | derived midpoint 1,100 EUR/kW; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 1, GuD, p. 12 | Combined cycle |
| P7-DE-004 | Europe–Maghreb | Germany | Peaker | capital_per_kw | 450–700 | EUR/kW | 2024 | derived midpoint 575 EUR/kW; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 1, Gasturbine, p. 12 | Open-cycle gas turbine |
| P7-DE-005 | Europe–Maghreb | Germany | Nuclear | capital_per_kw | 6,000–16,000 | EUR/kW | 2024 | derived midpoint 11,000 EUR/kW; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 1, Kernkraft, p. 12 | New nuclear; very wide range |
| P7-DE-006 | Europe–Maghreb | Germany | Solar | capital_per_kw | 700–900 | EUR/kWp | 2024 | derived midpoint 800 EUR/kWp; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 1, PV Freifläche (> 1 MWp), p. 12 | Ground-mounted PV, DC rating |
| P7-DE-007 | Europe–Maghreb | Germany | Battery | capital_per_kw | 400–600 | EUR/kWh (usable) | 2024 | derived midpoint 500 EUR/kWh × 4 h = 2,000 EUR/kW; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 1, battery for ground PV (> 1 MWp, 3:2), p. 12 | PV-coupled battery, installed; the 4-hour conversion is derived |
| P7-DE-008 | Europe–Maghreb | Germany | Wind | fixed_om_per_kw_year | 32 | EUR/kW-yr | 2024 | 32 EUR/kW-yr; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 2, OPEX fix, Wind Onshore, p. 13 | |
| P7-DE-009 | Europe–Maghreb | Germany | Coal | fixed_om_per_kw_year | 37 | EUR/kW-yr | 2024 | 37 EUR/kW-yr; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 2, OPEX fix, Steinkohle, p. 13 | |
| P7-DE-010 | Europe–Maghreb | Germany | Ccgt | fixed_om_per_kw_year | 20 | EUR/kW-yr | 2024 | 20 EUR/kW-yr; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 2, OPEX fix, GuD, p. 13 | |
| P7-DE-011 | Europe–Maghreb | Germany | Peaker | fixed_om_per_kw_year | 23 | EUR/kW-yr | 2024 | 23 EUR/kW-yr; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 2, OPEX fix, GT, p. 13 | |
| P7-DE-012 | Europe–Maghreb | Germany | Nuclear | fixed_om_per_kw_year | 100 | EUR/kW-yr | 2024 | 100 EUR/kW-yr; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 2, OPEX fix, Kernkraftwerk, p. 13 | |
| P7-DE-013 | Europe–Maghreb | Germany | Solar | fixed_om_per_kw_year | 13.3 | EUR/kWp-yr | 2024 | 13.3 EUR/kWp-yr; FX: none | Fraunhofer ISE — https://www.ise.fraunhofer.de/content/dam/ise/de/documents/publications/studies/DE2024_ISE_Studie_Stromgestehungskosten_Erneuerbare_Energien.pdf — LCOE 2024 — pub. 2024-07 — acc. 2026-10-25 — loc. Tabelle 2 (PV part), OPEX fix, PV Freiflächenanlagen, p. 13 | DC rating |

### Europe–Maghreb regional (`EM`, EUR)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-EM-001 | Europe–Maghreb | ENTSO-E area | Ccgt | fixed_om_per_kw_year | 34 | EUR/kW-yr | 2024 edition | 34 EUR/kW-yr; FX: none | ENTSO-E, adopted by ACER Decision 07/2025 — *ERAA 2024 Annex 1: Input Data and Assumptions* — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — pub. 2025-07-15 (PDF modified) — acc. 2026-10-25 — loc. Table 12, CCGT, p. 35 | Average of national cost-of-new-entry (CONE) studies; pan-European |
| P7-EM-002 | Europe–Maghreb | ENTSO-E area | Peaker | fixed_om_per_kw_year | 21 | EUR/kW-yr | 2024 edition | 21 EUR/kW-yr; FX: none | ENTSO-E/ACER — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — ERAA 2024 Annex 1 (as P7-EM-001) — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 12, OCGT, p. 35 | Average of CONE studies |
| P7-EM-003 | Europe–Maghreb | ENTSO-E area | Coal | fixed_om_per_kw_year | 26–39 | EUR/kW-yr | 2024 edition | derived midpoint 32.5 EUR/kW-yr; FX: none | ENTSO-E/ACER — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — ERAA 2024 Annex 1 — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 12, Hard coal, p. 35 | Value taken from EU Reference Scenario 2020 |
| P7-EM-004 | Europe–Maghreb | ENTSO-E area | Nuclear | fuel_or_marginal | 8 | EUR/MWh | 2024 edition | 8 EUR/MWh (VOM only); FX: none | ENTSO-E/ACER — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — ERAA 2024 Annex 1 — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 15, VOM, Nuclear, 2025 column, p. 38 | Variable non-fuel O&M only; fuel not included |
| P7-EM-005 | Europe–Maghreb | ENTSO-E area | Ccgt | fuel_or_marginal | 2.3–2.7 | EUR/MWh | 2024 edition | 2.3–2.7 EUR/MWh (VOM only); FX: none | ENTSO-E/ACER — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — ERAA 2024 Annex 1 — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 15, CCGT, 2025, p. 38 | VOM only |
| P7-EM-006 | Europe–Maghreb | ENTSO-E area | Peaker | fuel_or_marginal | 2.4 | EUR/MWh | 2024 edition | 2.4 EUR/MWh (VOM only); FX: none | ENTSO-E/ACER — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — ERAA 2024 Annex 1 — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 15, OCGT, 2025, p. 38 | VOM only |
| P7-EM-007 | Europe–Maghreb | ENTSO-E area | Coal | fuel_or_marginal | 2.8–4.1 | EUR/MWh | 2024 edition | 2.8–4.1 EUR/MWh (VOM only); FX: none | ENTSO-E/ACER — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — ERAA 2024 Annex 1 — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 15, Hard coal, 2025, p. 38 | VOM only |
| P7-EM-008 | Europe–Maghreb | EU27 | Tariff | retail_tariff_per_mwh | 0.2075 | EUR/kWh | 2025 (H1) | derived 207.5 EUR/MWh; FX: none | Eurostat — Electricity prices for household consumers, bi-annual data (nrg_pc_204) — https://ec.europa.eu/eurostat/databrowser/view/nrg_pc_204/default/table — pub. 2026-10-19 (dataset update) — acc. 2026-10-25 — loc. EU27_2020, band DC (2,500–4,999 kWh), excluding taxes and levies, 2025-S1 | Household; 0.2879 with all taxes |
| P7-EM-009 | Europe–Maghreb | EU27 | Tariff | retail_tariff_per_mwh | 0.1387 | EUR/kWh | 2025 (H1) | derived 138.7 EUR/MWh; FX: none | Eurostat — Electricity prices for non-household consumers, bi-annual data (nrg_pc_205) — https://ec.europa.eu/eurostat/databrowser/view/nrg_pc_205/default/table — pub. 2026-10-19 (dataset update) — acc. 2026-10-25 — loc. EU27_2020, band ID (2,000–19,999 MWh), excluding taxes and levies, 2025-S1 | Non-household; 0.1640 excluding VAT only |
| P7-EM-010 | Europe–Maghreb | EU | Hydro | capital_per_kw | 2,100 | EUR2015/kW | 2015 (2020 column) | 2,100 EUR/kW; FX: none | European Commission (E3-Modelling) — *EU Reference Scenario 2020: REF2020 Technology Assumptions* (xlsx) — https://circabc.europa.eu/rest/download/336c6844-6ee8-4861-91e9-7e86107c35ad (downloaded; SHA-256 cb4e63d737b26e391a6b1637617a26fd65d792528f8c353aa206371a5946a9e2) — pub. 2021-07 — acc. 2026-10-25 — loc. sheet Power&Heat, row Lakes, overnight investment 2020 | Reservoir hydro, greenfield, excluding financing; run of river 1,715 |
| P7-EM-011 | Europe–Maghreb | EU | Hydro | fixed_om_per_kw_year | 25.5 | EUR2015/kW-yr | 2015 (2020 column) | 25.5 EUR/kW-yr; FX: none | European Commission (E3-Modelling) — *EU Reference Scenario 2020: REF2020 Technology Assumptions* (xlsx) — https://circabc.europa.eu/rest/download/336c6844-6ee8-4861-91e9-7e86107c35ad (downloaded; SHA-256 cb4e63d737b26e391a6b1637617a26fd65d792528f8c353aa206371a5946a9e2) — pub. 2021-07 — acc. 2026-10-25 — loc. sheet Power&Heat, row Lakes, fixed O&M 2020 |  |
| P7-EM-012 | Europe–Maghreb | EU (ACER sample) | AcLine | line_cost_per_km | median 0.397 (average 0.465) | million EUR/km | 2023 (publication) | 397,000 EUR/km (median); FX: none | ACER — *Unit investment cost indicators for energy infrastructure categories* (04.07.2023) — https://www.acer.europa.eu/sites/default/files/documents/Publications_annex/ACER_UIC_indicators_table.pdf (downloaded; SHA-256 d59526c167e394de6b6ff4ac86fc15f6f69280c89f85f00d6bcc0000f24c4c01) — pub. 2023-07-04 — acc. 2026-10-25 — loc. Overhead line, 380–400 kV 1 circuit row | 18 assets; Q1 0.298, Q3 0.606; price base not stated in the table |
| P7-EM-013 | Europe–Maghreb | ENTSO-E area | Ccgt | fuel_or_marginal | 7.60 | EUR2023 per net GJ | 2025 | 7.60 EUR/GJ (LHV); FX: none | ENTSO-E/ACER — ERAA 2024 Annex 1 — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 7, Natural Gas, 2025, p. 28 | Natural gas price (Bloomberg for 2025); fuel only, net calorific basis; also used for Peaker |
| P7-EM-014 | Europe–Maghreb | ENTSO-E area | Coal | fuel_or_marginal | 2.97 | EUR2023 per net GJ | 2025 | 2.97 EUR/GJ (LHV); FX: none | ENTSO-E/ACER — ERAA 2024 Annex 1 — https://www.acer.europa.eu/sites/default/files/documents/Individual%20Decisions_annex/ACER-Decision-07-2025-ERAA-2024-Annex-Ib-Assumptions_0.pdf — pub. 2025-07-15 — acc. 2026-10-25 — loc. Table 7, Hard coal, 2025, p. 28 | Fuel only. The table's nuclear fuel price is taken from EIA (a US figure) and is not recorded |
| P7-EM-015 | Europe–Maghreb | Europe (IRENA region) | Hydro | capital_per_kw | 2,324 | USD/kW | 2018–2025 (2025 USD) | derived 2,324 × 0.884969 = 2,057 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 6.5, Europe label, p. 107 | Corroborates P7-EM-010 |

### Other Europe–Maghreb country markets (`FR`, `IT`, `PL`, `TR`, EUR)

IRENA country weighted averages of commissioned projects (2025 USD). They are market averages, not whole projects, so they are recorded for authoring context and are not proposed as zones. IRENA gives no Algerian or Tunisian value; the 2026-10-26 follow-up recorded whole-project rows for both (P7-DZ-*, P7-TN-*).

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-FR-001 | Europe–Maghreb | France | Wind | capital_per_kw | 1,932 | USD/kW | 2025 | derived 1,932 × 0.884969 = 1,710 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, France, 2025 label, p. 47 | Country market weighted average, not a whole project; regional default stays zone 0 |
| P7-FR-002 | Europe–Maghreb | France | Solar | capital_per_kw | 860 | USD/kW | 2025 | derived 860 × 0.884969 = 761 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 3.2, France, 2025 label, p. 63 | Utility-scale PV market weighted average, not a whole project |
| P7-IT-001 | Europe–Maghreb | Italy | Wind | capital_per_kw | 1,483 | USD/kW | 2025 | derived 1,483 × 0.884969 = 1,312 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Italy, 2025 label, p. 47 | Country market weighted average, not a whole project; regional default stays zone 0 |
| P7-IT-002 | Europe–Maghreb | Italy | Solar | capital_per_kw | 1,017 | USD/kW | 2025 | derived 1,017 × 0.884969 = 900 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 3.2, Italy, 2025 label, p. 63 | Utility-scale PV market weighted average, not a whole project |
| P7-PL-001 | Europe–Maghreb | Poland | Wind | capital_per_kw | 1,855 | USD/kW | 2025 | derived 1,855 × 0.884969 = 1,642 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Poland, 2025 label, p. 47 | Country market weighted average, not a whole project; regional default stays zone 0 |
| P7-PL-002 | Europe–Maghreb | Poland | Solar | capital_per_kw | 1,145 | USD/kW | 2025 | derived 1,145 × 0.884969 = 1,013 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 3.2, Poland, 2025 label, p. 63 | Utility-scale PV market weighted average, not a whole project |
| P7-TR-001 | Europe–Maghreb | Türkiye | Wind | capital_per_kw | 1,345 | USD/kW | 2025 | derived 1,345 × 0.884969 = 1,190 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 2.4, Türkiye, 2025 label, p. 47 | Country market weighted average, not a whole project; regional default stays zone 0 |
| P7-TR-002 | Europe–Maghreb | Türkiye | Solar | capital_per_kw | 1,095 | USD/kW | 2025 | derived 1,095 × 0.884969 = 969 EUR/kW; FX: P7-FX-011 | IRENA — *Renewable Power Generation Costs in 2025* — https://www.irena.org/-/media/Files/IRENA/Agency/Publication/2026/Jul/IRENA_TEC_RPGC_in_2025_2026.pdf (downloaded; SHA-256 38c8c70b848187c89d64b8817f0ebe831a9587c00257c1cc93195f22833a0eab) — pub. 2026-07 — acc. 2026-10-25 — loc. Figure 3.2, Türkiye, 2025 label, p. 63 | Utility-scale PV market weighted average, not a whole project |

## Europe–Maghreb economic-zone proposal

Zone IDs follow the proposal order. Values are the real figures to pace. The paced numbers below
are illustrative checks; Task 9 recomputes them with the method of record.

| Zone id | name_key | Covers | Plant overrides (rows) | AC overrides |
|---|---|---|---|---|
| 1 | `economic-zone-spain` | Spain's playable territory (the peninsula inside the footprint; the Balearics are already excluded by §28.3) | Wind: `capital_cost` from P7-ES-006 by user decision on 2026-10-26 (originally P7-ES-001, with P7-ES-002 corroborating; see the contrast), `build_months` from P7-ES-003. Solar: `capital_cost` from P7-ES-004, `build_months` from P7-ES-005 | none: no sourced Spanish line rows, so AC terms inherit the regional `lines` |
| 2 | `economic-zone-morocco` | Morocco's playable territory, as the region file assigns it under the grid-operation rule. Western Sahara cells are Task 9's ruling, recorded with an operator source | Wind: `capital_cost` from P7-MA-004 by user decision on 2026-10-26 (originally P7-MA-001; see the contrast), `build_months` from P7-MA-002 (P7-MA-003 corroborates). Solar: `capital_cost` from P7-MA-005, `build_months` from P7-MA-006 (*planned*) | none: no sourced Moroccan line rows |

Every other country stays in the regional default, zone 0 (next section), except northern Algeria
and Tunisia, which became zones 3 and 4 on 2026-10-26 (see the
[Algeria and Tunisia zone proposal](#algeria-and-tunisia-zone-proposal-follow-up-2026-10-26)). The
Spain and Morocco profiles override only the kinds with project sources, except Spain's wind
capital, a market average used by user decision. Other kinds inherit the regional catalog. Neither
profile is a blanket discount: Moroccan wind is cheaper but slower than Spanish wind in the
2024–25 pairing, and Moroccan solar is dearer, and that comes from the sources, not from a wage
assumption.

Zone 0 is the regional default and needs a baseline catalog. **Proposed by authoring choice:**

- Capital from the German technology-level rows: P7-DE-001 to P7-DE-007 (midpoints).
- Fixed O&M from the pan-European ERAA rows (P7-EM-001 to P7-EM-003), and from P7-DE-008 to
  P7-DE-013 where ERAA has none.
- Tariffs from Eurostat (P7-EM-008, P7-EM-009).
- Hydro capital and fixed O&M from the EU Reference Scenario (P7-EM-010, P7-EM-011);
  P7-EM-015 corroborates.
- Fuel from ERAA Table 7 (gas P7-EM-013, hard coal P7-EM-014), with VOM from P7-EM-004 to
  P7-EM-007.
- AC line cost from the ACER median (P7-EM-012).

Germany is the largest system in the synchronous area. No pan-regional whole-project source
covering every kind was reachable (see [Gaps](#gaps)). Zone-0 build durations follow the duration
ruling: they inherit North American game months as an authoring judgment. Nuclear fuel has no
non-US source.

## Spain–Morocco contrast

The required contrast (§28.7.1; Plan 7 Task 1, Step 5.2) is met by **onshore wind**. **The pairing
of record is the 2024–25 one, by user decision on 2026-10-26:** IRENA's 2025 Spain market average
(P7-ES-006) against the 2024 Jbel Lahdid project (P7-MA-004). The 2021 project pair below is kept
as recorded alternative evidence.

| | Spain | Morocco | Difference |
|---|---|---|---|
| Real capital | 1,258.4 EUR/kW (P7-ES-006, 2025 market average) | 1,118.5 EUR/kW (P7-MA-004, 2024) | −11.1% |
| Paced capital (× 0.6716, 3 s.f.) | 845 EUR/kW | 751 EUR/kW | **−11.1%**; \|845 − 751\| ÷ 751 = 12.5% (≥ 10%) |
| Real duration | 8 months (P7-ES-003) | 19.6 months (P7-MA-002) | +11.6 months |
| Paced months (× 3/9, rounded) | round(2.67) = **3** | round(6.54) = **7** | **+4 game months** (≥ 1) |

- Morocco's wind is cheaper but slower. Wind has no pacing ceiling, so neither value is clamped.
- The Spanish figure is a country market weighted average, not a single whole project. It is used
  by user decision. The Jbel Lahdid figure is an announced project total ("about" 3.25 billion MAD).
- Paced with full-precision intermediates (ruling 25): 1,422 × 0.884969 = 1,258.43 EUR/kW ×
  0.671592 = 845.15, so 845; 3.25e9 ÷ 270,000 = 12,037.0 MAD/kW ÷ 10.76141 = 1,118.54 EUR/kW ×
  0.671592 = 751.20, so 751 (the rounded 1,119 would give 751.51, so 752).
- The test checks the capital difference in either direction, \|a − b\| ÷ min(a, b) ≥ 0.10, so
  it does not depend on which country is dearer.
- The duration contrast is unchanged. Its only Moroccan duration evidence is Koudia Al Baida
  (P7-MA-002, P7-MA-003); no sourced Jbel Lahdid start date was found. With the planned Moroccan
  window (P7-MA-003, 20.5–23.4 months), Morocco paces to 7–8 months. P7-MA-008 (Tangier II, 25
  months late) independently supports long Moroccan schedules.
- **Recorded alternative evidence: the 2021 project pair.** Herrera II (P7-ES-001, 1,111 EUR/kW)
  against the Koudia Al Baida repowering (P7-MA-001, 1,300 EUR/kW) paces to 746 against 873
  EUR/kW: Morocco +17.0% dearer. This was the pair of record until 2026-10-26.
  - With Spain's corroborating Cofrentes capital (P7-ES-002, 1,060 EUR/kW), that gap widens to
    22.6%.
  - Averaging the two Moroccan projects (P7-MA-001 and P7-MA-004, 1,209 EUR/kW) against the two
    Spanish ones (P7-ES-001 and P7-ES-002, 1,086 EUR/kW) gives 812 against 729 EUR/kW paced,
    +11.4%.
  - KfW's Tangier II evaluation (1,542 EUR/kW actual, P7-MA-007) and EIB's 2013 programme
    appraisal (1,564 EUR/kW, P7-MA-009) also point to dearer Moroccan wind, with 2011 and 2013
    price years.
- **The sign of the capital gap differs between the pairs.** The 2021 pair has Morocco dearer;
  the 2024–25 pair has Morocco cheaper. Both differ by more than 10%, and the duration difference
  (P7-MA-002 and P7-MA-008 against P7-ES-003) holds in both.
- **No second qualifying technology.** IRENA has no Moroccan solar or wind country value. Solar
  durations stay about 12 months on both sides. No Spanish or Moroccan CCGT build source was
  found.
- **Solar does not qualify.** Capital differs (Spain ≤ 767, Morocco ≥ 951 EUR/kWac: 255 against
  317 EUR/kW paced, +24%). But both projects took, or planned, about 12 months, which paces to
  2 game months each. Solar is offered as a capital-only difference.

## IPS/UPS economic zones: decision

**Omit `economic_zones` (authoring choice).** The opt-in test needs sourced whole-project figures
for one same technology in both Russia and Kazakhstan or Uzbekistan.

- Round 2 found Russian whole-project figures for coal (P7-IPS-004) and CCGT (P7-IPS-006),
  but none for Kazakhstan or Uzbekistan in those technologies.
- For wind, all three countries have only IRENA country market averages (P7-IPS-007,
  P7-IPS-013, P7-IPS-014). These are not whole-project figures. The only Russian wind project
  value is a dated 2019 bid (P7-IPS-008).
- The test is therefore still unmet, and the decision is unchanged.
- Task 8's header comment should read "economic_zones omitted by authoring choice", giving this
  reason. FX rows P7-FX-017 to P7-FX-019 exist if a later edition opts in.

## Regional default by authoring choice

These Europe–Maghreb countries have no country profile and use zone 0. Most have no whole-project
source here. Germany has technology-level rows (P7-DE-*), but they are proposed as the zone-0
baseline, not as a separate zone:

Portugal, France, Belgium, the Netherlands, Luxembourg, Germany, Switzerland, Austria, Italy
(including Sicily), Malta, Slovenia, Croatia, Bosnia and Herzegovina, Serbia, Montenegro, North
Macedonia, Albania, Greece (including Crete), Bulgaria, Romania, Hungary, Slovakia, Czechia,
Poland, Lithuania, Latvia, Estonia, Denmark (DK1), Ukraine (connected part), Moldova, Türkiye,
and any micro-states and areas Task 9 rules into the footprint.

Northern Algeria and Tunisia left this list on 2026-10-26, when the
[Algeria and Tunisia zone proposal](#algeria-and-tunisia-zone-proposal-follow-up-2026-10-26) was
accepted: they are zones 3 and 4.

## Algeria and Tunisia zone proposal (follow-up 2026-10-26)

This section was added after the user asked for targeted research on 2026-10-26. It follows the Spain and
Morocco pattern:
- a kind is overridden only with a sourced whole-project capital figure, a sourced duration, or both;
- missing members inherit zone 0;
- AC terms stay regional unless a line row qualifies.

The paced values use the method of record. They are illustrative, and the region task recomputes them.
Zone ids 3 and 4 follow on from Spain (1) and Morocco (2). The proposal overrides only kinds with project
evidence. Every override is cheaper but slower than zone 0, kind by kind, and each value comes from a
sourced project, not from a wage assumption. It is therefore not a blanket Maghreb discount.

**Final proposed values** (game EUR per kW and game months; the arithmetic is below):

- **Algeria (zone 3).**
  - Solar: **263 EUR/kW**, with zone 0's DC-to-AC conversion (authoring judgment: DC to AC
    conversion by the Núñez de Balboa ratio, P7-ES-004), and **3 months**.
  - Ccgt: **574 EUR/kW**, a planned launch-of-works budget envelope, and **6 months** (planned).
- **Tunisia (zone 4).**
  - Solar: **309 EUR/kW** and **3 months**.
  - Ccgt: **471 EUR/kW** and **6 months**. The capital is a 2014 appraisal on a 450 MW basis (430 MW
    gives 493). The duration includes about 29 months of delay.

| Zone id | name_key | Covers | Plant overrides (rows) | AC overrides |
|---|---|---|---|---|
| 3 | `economic-zone-algeria` | Northern Algeria's playable territory | **Solar:** `capital_cost` from P7-DZ-002 (P7-DZ-001 and P7-DZ-006 to -008 corroborate), `build_months` from P7-DZ-005 (P7-DZ-004 corroborates). **Ccgt:** `capital_cost` from P7-DZ-011 and `build_months` from P7-DZ-012, both *planned* (P7-DZ-013 supports the long schedule) | none: P7-DZ-014 is not usable |
| 4 | `economic-zone-tunisia` | Tunisia's playable territory | **Solar:** `capital_cost` from P7-TN-001 (P7-TN-002 corroborates), `build_months` from P7-TN-011 (P7-TN-006 and -007 corroborate). **Ccgt:** `capital_cost` from P7-TN-012 (P7-TN-015 and -016 bracket it), `build_months` from P7-TN-014 (P7-TN-013's planned window gives the same game value) | none: P7-TN-019 is offered but not proposed (below) |

### Illustrative pacing

| Zone | Kind | Real value | Paced | Zone 0 (shipped) |
|---|---|---|---|---|
| Algeria | Solar capital | 617.2 EUR/kWp (P7-DZ-002) | DC to AC by zone 0's authoring judgment (× 500 ÷ 391, P7-ES-004): 789.2 EUR/kWac × 0.33289 = 262.7, so **263** EUR/kW × 300,000 kW = 78,900,000. Sensitivity only, per kWp without conversion: 617.2 × 0.33289 = 205.4, so 205 | 341 EUR/kW (102,300,000) |
| Algeria | Solar months | 24 months (P7-DZ-005) | 24 × 2 ÷ 12 = 4, clamped to the ceiling, so **3** (P7-DZ-004: 25.8 × 2 ÷ 12 = 4.29, also 3) | 2 |
| Algeria | Ccgt capital | 880.4 EUR/kW, a *planned* launch-of-works budget envelope (P7-DZ-011) | × (600 ÷ 921 = 0.65147) = 573.6, so 574 EUR/kW × 500,000 kW = 287,000,000 | 717 EUR/kW (358,500,000) |
| Algeria | Ccgt months | 48 months *planned* (P7-DZ-012) | 48 × 4 ÷ 22 = 8.73, so 9, clamped to the ceiling: **6** | 4 |
| Tunisia | Solar capital | 927 EUR/kWac (P7-TN-001) | × 0.33289 = 308.6, so 309 EUR/kW × 300,000 kW = 92,700,000 | 341 EUR/kW |
| Tunisia | Solar months | 19.3 months (P7-TN-011) | 19.29 × 2 ÷ 12 = 3.21, so **3** (P7-TN-006: 2.81 → 3; P7-TN-007: 3.15 → 3) | 2 |
| Tunisia | Ccgt capital | 723.6 EUR/kW (P7-TN-012, 2014 appraisal, 450 MW basis) | × 0.65147 = 471.4, so 471 EUR/kW × 500,000 kW = 235,500,000 (on 430 MW: 757.2 × 0.65147 = 493.3, so 493) | 717 EUR/kW |
| Tunisia | Ccgt months | 62.5 months (P7-TN-014) | 62.52 × 4 ÷ 22 = 11.37, so 11, clamped: **6** (planned P7-TN-013: 33.35 × 4 ÷ 22 = 6.06, so 6) | 4 |

### Disclosures

- **Algerian solar capital is an EPC award, not a whole-project outturn.** The figures are provisional
  award prices. Their scope includes the grid-connection works, but they exclude the owner's costs,
  and their tax basis is not stated. They are therefore a lower bound on the whole project.
  - The capacities are peak (DC): the award article states the programme as 2,000 MWc, and the
    Touggourt lot is 150 MW in 2023 and 150 MWc in 2025 (P7-DZ-006).
  - The note's convention is capital per kW of AC capacity, so 263 applies zone 0's DC-to-AC
    conversion (P7-ES-004) and carries the same authoring-judgment label. 205 (per kWp) is a
    sensitivity only.
  - Tendla (P7-DZ-001) would pace to 228 per kWac on the same conversion.
- **The Algerian CCGT values are planned.** They are a launch-of-works estimate and a planned 48 months,
  like Morocco's planned solar row (P7-MA-006). The paced months reach the CCGT ceiling of 6 under any
  reading. P7-DZ-013 shows a 2014 contract that is still unfinished, so a long schedule is not an
  artefact of planning optimism.
- **The Tunisian CCGT capital is a 2014 appraisal estimate.** Zone 0's figure is 2024 (P7-DE-003), and
  no inflation adjustment is applied (pacing method).
  - The three Tunisian CCGT rows span 724–970 EUR/kW real (Radès 2014, Sousse D 2012, Sousse C 2010),
    which pace to 471–632. All are below zone 0's 717, so the sign is robust; the size is not.
  - Radès is proposed because it pairs with sourced durations for the same plant.
- **The Tunisian CCGT duration includes about 29 months of delay over plan.** Contract to operation was
  62.5 months, through COVID-19. The planned 33–34 months also reaches the ceiling (6.06 → 6), so the
  game value does not depend on the delay.
- **Not proposed: wind.**
  - Algeria has only the Kabertène pilot (P7-DZ-009, -010). It is 10 MW, uses 850 kW turbines,
    dates from 2011–2014 and lies outside the footprint. It would pace to 1,720 EUR/kW and 11 months.
    That is the same reason P7-MA-007 was not used for Morocco. The controller may still choose it.
  - Tunisia's only wind farms are STEG's Sidi Daoud and Bizerte (Metline, Kchabta; commissioned by
    2012). Press figures for Bizerte conflict, mix phases and currencies, and come from no operator
    or lender document reached here. Wind stays on zone 0 in both countries.
- **Not proposed: Peaker.** Mornaguia (P7-TN-017) is a contract value that includes maintenance
  services, so it is not whole-project. Its duration (P7-TN-018) paces to the inherited 2 months
  anyway. Algeria has no peaker row.
- **Not proposed: AC lines.**
  - Tunisia's Grombalia 2–Kondar line (P7-TN-019) is a planned double-circuit line. Its cost may be
    only the EBRD-financed share. Across all line km it gives 391 thousand EUR/km, within 2% of zone
    0's 397 thousand (P7-EM-012), so an override would change nothing material.
  - Algeria's PIAT figure (P7-DZ-014) includes substations and lies mostly outside the footprint.
  - No line build durations were found for either country.
- **Battery, coal, nuclear and hydro:** no Algerian or Tunisian rows. They inherit zone 0.
- **Cells.** The region task assigns Algerian and Tunisian cells to zones 3 and 4 by the same rule as
  Morocco.

## Gaps

What was searched and not found or not usable, after round 2. "Not reached" means the source exists
but could not be read without a login, a bot check or a download beyond the round-2 budget.

- **Closed by ruling, not by rows:** baseline durations (ruling 1), start-up costs (ruling 2, with
  ruling 24's per-MW scaling) and line build speed (rulings 5 and 21). See
  [Controller rulings](#controller-rulings-round-2-25-october-2026) and
  [Region-authoring rulings](#region-authoring-rulings-plan-7). A gap closed by a derivation
  below says "closed by ruling N (derived, labelled in the region file)"; the derived value is
  an authoring judgment, not a sourced figure.
- **All regions**
  - IEA WEO 2025 technology-cost tables: behind a Cloudflare bot check, not bypassed.
  - NEA/IEA *Projected Costs of Generating Electricity 2020*: HTTP 403.
  - IAEA PRIS reactor timelines: a script-only app.
- **Brazil**
  - Hydro (UHE) construction duration: closed by ruling 1 (derived, labelled in the region file);
    it inherits North American months.
  - Per-voltage line cost: only the all-voltage average (P7-BR-028); the ANEEL price bank was not
    reached within the download budget. The region uses P7-BR-028, because a sourced row wins over
    ruling 20's coal relativity.
- **India**
  - Capital: CCGT and peaker. The CEA plan models no new gas build; no CERC benchmark was found.
    Closed by ruling 3 (derived, labelled in the region file).
  - Fuel or energy charges: closed by ruling 19 (derived, labelled in the region file).
  - Per-km line cost: CEA gives only a per-MW-km figure (P7-IN-010). Closed by ruling 20
    (derived, labelled in the region file).
  - Nuclear, CCGT and peaker durations: closed by ruling 1 (derived, labelled in the region
    file).
- **Southern China**
  - Capital: nuclear (only media estimates of about 20 billion CNY per unit; not used), CCGT,
    peaker and battery. Closed by ruling 3 (derived, labelled in the region file).
    - A Shenzhen Gas announcement gives 22.81 billion CNY for two combined-cycle sets, but no
      capacity, so it is not used.
  - Fixed O&M: closed by ruling 4 (derived, labelled in the region file).
  - Fuel prices (no Guangdong gate price for generation found): closed by ruling 19 (derived,
    labelled in the region file).
  - Line cost: closed by ruling 20 (derived, labelled in the region file).
  - Observed durations: closed by ruling 1 (derived, labelled in the region file); P7-SC-004 is
    planned.
- **IPS/UPS**
  - Fixed O&M for every kind: closed by ruling 4 (derived, labelled in the region file).
  - Coal price: closed by ruling 19 (derived, labelled in the region file).
  - Hydro and nuclear project-level costs (only the General Scheme aggregates, P7-IPS-002 and
    P7-IPS-003).
  - The current Minenergo line norm (Order No. 131 of 2024); the superseded 2019 norm is used.
  - The current ATS renewable-selection bids, which since 2021 use a price-based bid and not capex.
- **Europe–Maghreb**
  - Nuclear fuel: ERAA's value is an EIA (US) figure, so it is excluded. Closed by ruling 19
    (derived, labelled in the region file).
  - Spanish and Moroccan line costs (zones inherit the regional line).
  - Whole-project rows for France, Italy, Poland and Türkiye. IRENA market averages cover them
    (P7-FR-*, P7-IT-*, P7-PL-*, P7-TR-*).
  - The actual commissioning date of Noor Ouarzazate IV.
- **Algeria and Tunisia (follow-up 2026-10-26)**
  - Not reached:
    - afdb.org returned HTTP 403 to every request: project pages P-TN-FF0-004 (Kairouan) and
      press releases. AfDB appraisal reports for Tunisian lines or plants were therefore not read.
    - Radio Algérie (news.radioalgerie.dz) rejected the request.
    - Africa Energy Portal returned HTTP 403.
    - None was bypassed.
  - Algeria:
    - No recent development-bank appraisal of an Algerian generation or line project was found.
      A combined AfDB, EIB, World Bank and JICA search found only deals from before 2000, such
      as World Bank POWER III (1988) and an EIB loan of 1997.
    - No Sonelgaz, SKTM, SPE or CREG document with a project cost was reached. The Algerian rows
      rest on the trade press reporting Sonelgaz, ministry or APS statements.
    - Missing for solar: an AC rating and an outturn cost.
    - Missing for CCGT: an outturn cost, and a completed plant's start-to-commissioning duration.
      Ras Djinet (Cap Djinet) has only a Daewoo EPC share (830 million USD, KED Global 2023-03-03)
      without the separately supplied Siemens turbines, and a 2012 order with provisional acceptance
      in March 2023, so no row was recorded.
    - No wind project inside the footprint.
    - No open-cycle plant, and no northern AC line with its own cost.
    - Hassi R'Mel's hybrid plant and the 15 GW plan's later tenders were not found with project
      costs.
  - Tunisia:
    - No wind project newer than Bizerte (2012), whose press figures conflict.
    - No AC line with an outturn cost, and no line build duration.
    - No Tunisian battery row.
    - Kairouan's AC rating.
    - Actual start dates for Sidi Bouzid and Tozeur: financial close is used, so their durations
      are upper bounds.
    - The STEG and energy ministry sites were not searched beyond press coverage.

## Verification

- Row IDs are unique and match `P7-[A-Z]{2,3}-\d{3}`.
- Every row has a URL, publication date, access date and locator. Undated web pages are marked
  "undated page" alongside a dated primary source.
- Every normalised value names an FX row or `FX: none`.
- The note contains no quotations; figures and locators only.
- Suggested reviewer re-fetch set (at least 10 rows, across regions, including every Spain–Morocco
  contrast row): P7-ES-001, P7-ES-003, P7-MA-001, P7-MA-002, P7-MA-004, P7-US-001, P7-US-033,
  P7-BR-001, P7-BR-029, P7-IN-006, P7-SC-003 and P7-FX-006. Round 2 adds P7-IPS-002, P7-IPS-006,
  P7-IN-020, P7-MA-007 and P7-EM-012. The 2026-10-26 follow-up adds P7-DZ-002, P7-DZ-005,
  P7-DZ-011, P7-TN-001, P7-TN-011, P7-TN-012 and P7-TN-014.
- Downloaded files (round 2, and the follow-up's two PDFs in `target/research/dz-tn/`) are kept
  outside git in `target/research/`. Each row names its URL and
  SHA-256, so a reviewer can re-download it and check the bytes.
