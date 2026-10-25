# Plan 7 regional economics: sourced rows, pacing and Europe–Maghreb profiles

**Research edition: 25 October 2026.** This note is the single place where Plan 7's real-world money
and construction-time figures live. Region files for Southern China, Brazil, India–Nepal–Bhutan,
IPS/UPS and Continental Europe–Maghreb cite its rows by ID; tuning numbers live only in region RON
(spec §28.7, §28.7.1). Every row records a number and a locator, not prose. Where no source was
found, the gap is listed under [Gaps](#gaps) instead of being estimated.

## Conventions

- **Row IDs** are `P7-<area>-<nnn>`. Areas: `US` (pacing reference only), `FX` (exchange rates),
  `SC`, `BR`, `IN` and `IPS` (region-wide baselines), ISO alpha-2 countries inside Europe–Maghreb
  (`ES`, `MA`, `DE`), and `EM` (Europe–Maghreb regional default).
- **Columns** follow the Plan 7 Task 1 brief. *Normalised value* is in the region currency
  (CNY, BRL, INR, RUB or EUR) and always names an FX row; `FX: none` means the source is already in
  the region currency. US rows are not normalised: they are only the pacing reference, and no US
  figure is converted into another region's money.
- **Sources** give publisher, title, URL, publication date (`pub.`), access date (`acc.`) and
  locator (`loc.`). All rows were accessed on 2026-10-25.
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
| Peaker | 500 ÷ 1,606 = **0.3113** (P7-US-013) | 2 ÷ 22 = **0.09091** (P7-US-014) | 10.2 ÷ 9.56 = **1.067** (P7-US-015) | 150 ÷ 42.07 = **3.565** (P7-US-028, -031) | 25.0 ÷ 55.13 = **0.4535** (P7-US-032) |
| Solar | 500 ÷ 1,502 = **0.3329** (P7-US-016) | 2 ÷ 12 = **0.1667** (P7-US-017) | 20.0 ÷ 20.23 = **0.9886** (P7-US-018) | n/a (game 0) | n/a |
| Hydro | 1,500 ÷ 7,073 = **0.2121** (P7-US-019) | 12 ÷ 36 = **0.3333** (P7-US-020) | 40.5 ÷ 33.54 = **1.208** (P7-US-021) | n/a (game 0) | gap |
| Battery | 500 ÷ 1,744 = **0.2867** (P7-US-022) | 2 ÷ 12 = **0.1667** (P7-US-023) | 6.0 ÷ 40.00 = **0.1500** (P7-US-024) | n/a (game 0) | n/a |

| Other metric | Ratio | US row |
|---|---:|---|
| AC line cost per km | 466,832 ÷ 2,796,170 = **0.1670** | P7-US-033 |
| Reference tariff per MWh | 100 ÷ 135.6 = **0.7375** | P7-US-034 |
| Starting tariff per MWh | 95 ÷ 135.6 = **0.7006**, which is 0.95 × the reference tariff, as shipped | P7-US-034 |
| AC line build speed | no ratio: no dated US real km-per-month source found (see [Gaps](#gaps)) | — |

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

### Southern China (`SC`, CNY)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-SC-001 | Southern China | China (national) | Wind | capital_per_kw | about 4,100 | CNY/kW | 2025 | 4,100 CNY/kW; FX: none | The Paper, reporting China Renewable Energy Engineering Institute (水电水利规划设计总院), *China Renewable Energy Engineering Cost Management Report 2025* — https://www.thepaper.cn/newsDetail_forward_33396923 — pub. 2026-06-17 — acc. 2026-10-25 — loc. onshore wind paragraph | Average total investment per kW, national onshore wind; down 2.4% year on year. National, not Southern China specific |
| P7-SC-002 | Southern China | China (national) | Solar | capital_per_kw | about 3,200 | CNY/kW | 2025 | 3,200 CNY/kW; FX: none | The Paper, reporting the same CREEI 2025 report (as P7-SC-001) — https://www.thepaper.cn/newsDetail_forward_33396923 — pub. 2026-06-17 — acc. 2026-10-25 — loc. centralised PV paragraph | Average total investment per kW, centralised onshore PV; down 7.2% year on year |
| P7-SC-003 | Southern China | Guangdong | Coal | capital_per_kw | 780,656 (10,000 CNY) for 2 × 1,000 MW | CNY | 2022 | derived 7.80656e9 ÷ 2.0e6 kW = 3,903 CNY/kW; FX: none | Guangdong Baolihua New Energy (SZSE 000690) — Announcement 2022-025 on approval of Lufeng Jiahu Bay units 3–4 — https://disc.static.szse.cn/disc/disk03/finalpage/2022-09-14/49356c75-5a7f-4405-bac8-c7680136c7bf.PDF — pub. 2022-09-14 — acc. 2026-10-25 — loc. paragraph 2 (total investment), citing Guangdong DRC approval 粤发改核准〔2022〕31号 | Ultra-supercritical 2 × 1,000 MW extension at an existing site |
| P7-SC-004 | Southern China | Guangdong | Coal | build_months_real | in service before end of 2024 | target date | — | *planned* window ≤ 27 months (2022-09 approval to 2024-12); FX: none | Baoxin Energy Announcement 2022-025 (as P7-SC-003) — https://disc.static.szse.cn/disc/disk03/finalpage/2022-09-14/49356c75-5a7f-4405-bac8-c7680136c7bf.PDF — pub. 2022-09-14 — acc. 2026-10-25 — loc. paragraph 2 (in-service target) | Approval-to-target window, not an observed construction duration |
| P7-SC-005 | Southern China | Guangdong (east and west, non-Pearl-River-Delta) | Tariff | retail_tariff_per_mwh | 67.986875 | fen/kWh (tax included) | 2025 (Aug) | derived 679.87 CNY/MWh; FX: none | Shantou Municipal Government, publishing the Guangdong grid company's agency-purchase tariff table — 广东省电网企业代理购电用户电价表（汕头市）, August 2025 — https://www.shantou.gov.cn/attachment/0/137/137749/2459290.pdf — pub. 2025-07-29 (PDF date) — acc. 2026-10-25 — loc. industrial and commercial, single-part, 1–10(20) kV, energy price column | Commercial and industrial users on agency purchase, flat period, tax included. Applies to Shantou, Chaozhou, Jieyang, Shanwei, Yangjiang, Zhanjiang, Maoming, Zhaoqing and three Jiangmen cities |

### IPS/UPS (`IPS`, RUB)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-IPS-001 | IPS/UPS | Russia (Moscow) | Tariff | retail_tariff_per_mwh | 7.87 from 2025-07-01 (6.99 before) | RUB/kWh | 2025 | derived 7,870 RUB/MWh; FX: none | RBC Realty, reporting the Moscow Department of Economic Policy and Development tariff order — https://realty.rbc.ru/news/673c9b499a79473b78eb639d — pub. 2024-11-19 — acc. 2026-10-25 — loc. tariff paragraph, homes with gas stoves, single-rate | Regulated household tariff, gas-stove homes, single rate; electric-stove homes 7.16. Press report of the order; the order's number is not given |

### Spain (`ES`, EUR)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-ES-001 | Europe–Maghreb | Spain | Wind | capital_per_kw | 70 million for 63 MW | EUR | 2021 | derived 70e6 ÷ 63,000 kW = 1,111 EUR/kW; FX: none | Iberdrola — Iberdrola puts the Herrera II wind complex into service — https://www.iberdrola.com/sala-comunicacion/noticias/detalle/iberdrola-pone-en-marcha-complejo-eolico-herrera-ii — pub. 2021-08-17 — acc. 2026-10-25 — loc. paragraphs 4–5 (capacity; investment) | Three onshore farms in Burgos, 14 × SG 4.5-145 turbines. The source words the amount as spent on promoting the complex; treated as whole-project capital |
| P7-ES-002 | Europe–Maghreb | Spain | Wind | capital_per_kw | 53 million for 50 MW | EUR | 2020 | derived 1,060 EUR/kW; FX: none | Asociación Empresarial Eólica (REVE) — Elecnor commissions Cofrentes wind farm — https://reve.aeeolica.org/2020/04/21/elecnor-pone-en-marcha-el-primer-parque-eolico-de-la-comunidad-valenciana-desde-2012/ — pub. 2020-04-21 — acc. 2026-10-25 — loc. investment paragraph | 13 × 3.83 MW GE turbines, Valencia; corroborates P7-ES-001 |
| P7-ES-003 | Europe–Maghreb | Spain | Wind | build_months_real | 8 | months | — | 8 months; FX: none | Iberdrola España — Iberdrola puts the Puylobo wind farm into operation — https://www.iberdrolaespana.com/sala-comunicacion/noticias/detalle/201130_np_01_parque_eolico_puylobo — pub. 2020-11-29 — acc. 2026-10-25 — loc. paragraph 1 (construction time) | 49 MW, 14 × SG 3.4-132, Zaragoza; built during the pandemic; the company calls it record time |
| P7-ES-004 | Europe–Maghreb | Spain | Solar | capital_per_kw | nearly 300 million for 500 MWp (391 MW grid connection) | EUR | 2019 | derived ≤ 300e6 ÷ 391,000 kWac = 767 EUR/kWac (600 EUR/kWp); FX: none | Iberdrola — Núñez de Balboa completed — https://www.iberdrola.com/press-room/news/detail/nunez-balboa-completed-iberdrola-finalizes-construction-largest-photovoltaic-plant-europe-within-year — pub. 2019-12-27 — acc. 2026-10-25 — loc. paragraph 2 (500 MW, investment); AC rating from Iberdrola project page https://www.iberdrola.com/about-us/power/solar-photovoltaic-energy/nunez-de-balboa-photovoltaic-plant (undated page), installed-capacity line | Badajoz. "Nearly" makes 767 an upper bound per kWac |
| P7-ES-005 | Europe–Maghreb | Spain | Solar | build_months_real | 12 | months | — | 12 months; FX: none | Iberdrola — https://www.iberdrola.com/press-room/news/detail/nunez-balboa-completed-iberdrola-finalizes-construction-largest-photovoltaic-plant-europe-within-year — Núñez de Balboa completed (as P7-ES-004) — pub. 2019-12-27 — acc. 2026-10-25 — loc. headline and paragraph 1 (construction in one year) | Construction completed December 2019; operation forecast for Q1 2020 |

### Morocco (`MA`, EUR via MAD)

| ID | Region | Market | Technology | Metric | Value as stated | Unit/currency | Price year | Normalised value (region currency) | Source | Scope notes |
|---|---|---|---|---|---|---|---|---|---|---|
| P7-MA-001 | Europe–Maghreb | Morocco | Wind | capital_per_kw | total project cost 130,000,000 for 100 MW | EUR | 2021 | derived 130e6 ÷ 100,000 kW = 1,300 EUR/kW; FX: none (stated in EUR) | EBRD — Koudia Al Baida Wind Farm, Project Summary Document 50957 — https://www.ebrd.com/work-with-us/projects/psd/50957.html — pub. 2021-12-23 — acc. 2026-10-25 — loc. Total Project Cost; Project Description | Repowering of the 50 MW Koudia Al Baida farm near Tangier to 100 MW with 5 MW turbines. Includes replacing the old units; reuses the site and grid connection |
| P7-MA-002 | Europe–Maghreb | Morocco | Wind | build_months_real | works launched 2022-07-18; construction complete and first MW injected by 2024-03-06 | dates | — | derived 597 days = 19.6 months; FX: none | Masen — press release on financial close and launch of works for Nassim Koudia Al Baida — https://www.masen.ma/sites/default/files/documents_presse/Communiqu%C3%A9%20de%20presse_KOUDIA_LANCEMENT_REPOWERING.pdf — pub. 2022-07-18 — acc. 2026-10-25 — loc. p. 1, paragraphs 2 and 4; and Le360, reporting a Masen communiqué — https://fr.le360.ma/economie/energies-renouvelables-les-parcs-eoliens-nassim-koudia-al-baida-et-nassim-jbel-lahdid-operationnels_J3YN6WL2KRBRBAXMOJE2FTIFDY/ — pub. 2024-03-06 — acc. 2026-10-25 — loc. Koudia paragraph | Launch to completion of construction; commercial operation was expected in the first half of 2024 |
| P7-MA-003 | Europe–Maghreb | Morocco | Wind | build_months_real | commercial operation planned for Q2 2024 | target date | — | derived *planned* 20.5–23.4 months from 2022-07-18; FX: none | Masen — https://www.masen.ma/sites/default/files/documents_presse/Communiqu%C3%A9%20de%20presse_KOUDIA_LANCEMENT_REPOWERING.pdf — Koudia release (as P7-MA-002) — pub. 2022-07-18 — acc. 2026-10-25 — loc. p. 1, paragraph 4 | Planned launch-to-operation window; corroborates P7-MA-002 |
| P7-MA-004 | Europe–Maghreb | Morocco | Wind | capital_per_kw | about 3.25 billion for 270 MW | MAD | 2024 | derived 3.25e9 ÷ 270,000 kW = 12,037 MAD/kW ÷ 10.7614 = 1,119 EUR/kW; FX: P7-FX-005, P7-FX-006 | Enerdata, reporting ONEE — Morocco starts operations of 270 MW Jbel Lahdid wind project — https://www.enerdata.net/publications/daily-energy-news/morocco-starts-operations-270-mw-jbel-lahdid-wind-project.html — pub. 2024-10-15 — acc. 2026-10-25 — loc. paragraph 1 | 54 × 5 MW, Essaouira; ONEE with Nareva and Enel Green Power. Counter-evidence to the Spain–Morocco capital gap (see contrast) |
| P7-MA-005 | Europe–Maghreb | Morocco | Solar | capital_per_kw | more than 750 million for 72 MW | MAD | 2017 | derived 750e6 ÷ 72,000 kW = 10,417 MAD/kW ÷ 10.9489 = 951 EUR/kW; FX: P7-FX-001, P7-FX-002 | Masen — press release on the launch of works at Noor Ouarzazate IV — https://www.masen.ma/sites/default/files/documents_presse/17.pdf — pub. 2017-04-01 — acc. 2026-10-25 — loc. p. 1, paragraph 3 | Estimated investment at launch; 72 MW maximum capacity, single-axis tracking PV; ACWA Power consortium. "More than" makes 951 a lower bound |
| P7-MA-006 | Europe–Maghreb | Morocco | Solar | build_months_real | works launched 2017-04-01; commissioning planned for Q1 2018 | dates | — | derived *planned* 9–12 months; FX: none | Masen — https://www.masen.ma/sites/default/files/documents_presse/17.pdf — Noor Ouarzazate IV release (as P7-MA-005) — pub. 2017-04-01 — acc. 2026-10-25 — loc. p. 1, paragraphs 1 and 3 | Planned; actual commissioning date not found |

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

## Europe–Maghreb economic-zone proposal

Zone IDs follow the proposal order. Values are the real figures to pace. The paced numbers below
are illustrative checks; Task 9 recomputes them with the method of record.

| Zone id | name_key | Covers | Plant overrides (rows) | AC overrides |
|---|---|---|---|---|
| 1 | `economic-zone-spain` | Spain's playable territory (the peninsula inside the footprint; the Balearics are already excluded by §28.3) | Wind: `capital_cost` from P7-ES-001 (P7-ES-002 corroborates), `build_months` from P7-ES-003. Solar: `capital_cost` from P7-ES-004, `build_months` from P7-ES-005 | none: no sourced Spanish line rows, so AC terms inherit the regional `lines` |
| 2 | `economic-zone-morocco` | Morocco's playable territory, as the region file assigns it under the grid-operation rule. Western Sahara cells are Task 9's ruling, recorded with an operator source | Wind: `capital_cost` from P7-MA-001, `build_months` from P7-MA-002 (P7-MA-003 corroborates). Solar: `capital_cost` from P7-MA-005, `build_months` from P7-MA-006 (*planned*) | none: no sourced Moroccan line rows |

Every other country stays in the regional default, zone 0 (next section). The Spain and Morocco
profiles override only the kinds with whole-project sources. Other kinds inherit the regional
catalog. Neither profile is a discount: the Moroccan rows are dearer and slower than the Spanish
ones, and that comes from the projects, not from a wage assumption.

Zone 0 is the regional default and needs a baseline catalog. **Proposed by authoring choice:**

- Capital from the German technology-level rows: P7-DE-001 to P7-DE-007 (midpoints).
- Fixed O&M from the pan-European ERAA rows (P7-EM-001 to P7-EM-003), and from P7-DE-008 to
  P7-DE-013 where ERAA has none.
- Tariffs from Eurostat (P7-EM-008, P7-EM-009).

Germany is the largest system in the synchronous area. No pan-regional whole-project source
covering every kind was reachable (see [Gaps](#gaps)). Zone-0 build durations, hydro capital and
fuel prices are unsourced.

## Spain–Morocco contrast

The required contrast (§28.7.1; Plan 7 Task 1, Step 5.2) is met by **onshore wind**:

| | Spain | Morocco | Difference |
|---|---|---|---|
| Real capital | 1,111 EUR/kW (P7-ES-001, 2021) | 1,300 EUR/kW (P7-MA-001, 2021) | +17.0% |
| Paced capital (× 0.6716, 3 s.f.) | 746 EUR/kW | 873 EUR/kW | **+17.0%** (≥ 10%) |
| Real duration | 8 months (P7-ES-003) | 19.6 months (P7-MA-002) | +11.6 months |
| Paced months (× 3/9, rounded) | round(2.67) = **3** | round(6.54) = **7** | **+4 game months** (≥ 1) |

- Wind has no pacing ceiling, so neither value is clamped. Both capital rows are 2021 figures.
- With the planned Moroccan window (P7-MA-003, 20.5–23.4 months), Morocco paces to 7–8 months.
  With Spain's corroborating Cofrentes capital (P7-ES-002, 1,060 EUR/kW), the gap widens to 22.6%.
- **Fragility (disclosed).** The capital gap rests on Koudia Al Baida, a repowering. Jbel Lahdid
  (P7-MA-004, 2024) is 1,119 EUR/kW, which paces to 751 EUR/kW: only +0.7% above Spain, so it
  alone would not meet 10%.
  - Averaging both Moroccan projects (1,209 EUR/kW) against both Spanish ones (1,086 EUR/kW) gives
    812 against 729 EUR/kW paced, a gap of +11.4%.
  - The duration contrast does not depend on Koudia's cost. Its only duration evidence is Koudia
    itself (P7-MA-002, P7-MA-003); no sourced Jbel Lahdid start date was found.
- **Solar does not qualify.** Capital differs (Spain ≤ 767, Morocco ≥ 951 EUR/kWac: 255 against
  317 EUR/kW paced, +24%). But both projects took, or planned, about 12 months, which paces to
  2 game months each. Solar is offered as a capital-only difference.

## IPS/UPS economic zones: decision

**Omit `economic_zones` (authoring choice).** The opt-in test needs sourced whole-project figures
for one same technology in both Russia and Kazakhstan or Uzbekistan.

- No Russian whole-project capital or duration row was found for any technology.
- The only Russian material found was press coverage of government CAPEX *caps* for capacity
  auctions. Caps are not project costs, so they were not used.
- Kazakhstan and Uzbekistan therefore cannot form a sourced contrast.
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
northern Algeria, Tunisia, and any micro-states and areas Task 9 rules into the footprint.

## Gaps

What was searched and not found or not usable, by region. "Not reached" means the source exists
but could not be read without a download, a login or a bot check, which this task did not do.

- **All regions**
  - Start-up costs. Only the US peaker has a source (P7-US-032).
  - AC line build speed. No dated US real km-per-month source, so `cells_per_month` has no pacing
    ratio. Region tasks need an authoring judgment, recorded as such.
  - IEA WEO 2025 technology-cost tables: behind a Cloudflare bot check, not bypassed.
  - IRENA *Renewable Power Generation Costs in 2025*: PDF over 10 MB, not reached without a
    download.
  - NEA/IEA *Projected Costs of Generating Electricity 2020*: HTTP 403.
  - IAEA PRIS reactor timelines: the site moved to a script-only app.
- **US.** AEO2026 Table 3 lead times include development. The S&L construction times are used
  instead, as documented.
- **Brazil**
  - Hydro (UHE) construction duration: not in the PDE 2035 cost caderno.
  - Per-voltage line cost: only the all-voltage average (P7-BR-028); the ANEEL price bank was not
    reached.
  - Start-up costs.
- **India**
  - Capital: onshore wind, CCGT and peaker. No Ministry normative wind cost was found.
  - Build durations for coal, nuclear, solar, wind, battery, CCGT and peaker. NTPC's 48/52-month
    Lara schedule appears only on a login-gated NTPC page.
  - Fuel or energy charges.
  - Per-km line cost: CEA gives only a per-MW-km figure.
  - The CEA *National Electricity Plan 2022-32* PDF (HTTP 404 at both known URLs) and the PFC
    *Report on Performance of Power Utilities* itself.
- **Southern China**
  - Nuclear capital: only media estimates of about 20 billion CNY per unit; no official figure,
    so none was used.
  - Capital for CCGT, peaker, hydro and battery. The CREEI report gives battery costs only as a
    forecast range for the 15th Five-Year Plan.
  - All observed build durations (P7-SC-004 is planned).
  - Fixed O&M, fuel, line cost and start-up.
  - A province-wide all-sector average tariff: the national average sales price was not found.
- **IPS/UPS**
  - Everything except the Moscow tariff: capital, durations, O&M, fuel and line cost for all kinds.
  - The *General Scheme to 2042* public-discussion text has no unit costs.
  - ATS renewable-auction results and Ministry of Energy aggregated price norms for lines were not
    reached.
- **Europe–Maghreb**
  - Zone-0 build durations for every kind, and zone-0 hydro capital.
  - Fuel prices; line costs (regional, Spanish and Moroccan).
  - Country whole-project rows for France (RTE *Futurs énergétiques 2050* cost annex not reached),
    Italy, Poland, Türkiye, Algeria and Tunisia.
  - The EU Reference Scenario 2020 technology assumptions are published only as an Excel download.
  - The actual commissioning date of Noor Ouarzazate IV.

## Verification

- Row IDs are unique and match `P7-[A-Z]{2,3}-\d{3}`.
- Every row has a URL, publication date, access date and locator. Undated web pages are marked
  "undated page" alongside a dated primary source.
- Every normalised value names an FX row or `FX: none`.
- The note contains no quotations; figures and locators only.
- Suggested reviewer re-fetch set (at least 10 rows, across regions, including every Spain–Morocco
  contrast row): P7-ES-001, P7-ES-003, P7-MA-001, P7-MA-002, P7-MA-004, P7-US-001, P7-US-033,
  P7-BR-001, P7-BR-029, P7-IN-006, P7-SC-003 and P7-FX-006.
