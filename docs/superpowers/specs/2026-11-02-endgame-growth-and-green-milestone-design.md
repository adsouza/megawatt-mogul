# Endgame: demand growth and the green milestone — design

Date: 2026-11-02. Status: draft for approval.

Two parts, built in this order, each in its own commit series:

- **Part A — demand growth (§3).** Once every city in the region is connected, all cities' demand
  grows month by month at the region's real-world forecast rate. This changes simulation rules.
- **Part B — the green milestone (§4).** A bonus milestone appears once the three regional
  milestones are reached: run a full year with no coal, gas or peaker generation and no strikes.
  It is read-side only and changes no simulation rules.

A follow-up, **demolishing plants**, is described in §5 but is not part of this work.

## 1. Why

The player tested UPS/IPS and Brazil to the end of their maps. Once every city was connected,
nothing in the game asked for more: they built some extra supply, "but it started to feel a bit
pointless". Today every city's demand is a fixed authored figure (`CityDef.base_load`) for the
whole game, so after the last connection the reserve-margin review, the outlook and the build menu
have nothing new to respond to.

Real utilities face two pressures at that stage: load keeps growing (data centres, electrification,
cooling, industry), and they are asked to decarbonise while it does. The two parts model exactly
that, and they reinforce each other: a green target alone is met once and done, but under growth
the player must keep adding clean capacity to stay green.

The player chose not to start growth on day one. Early on the network is small and whole cities
come online at once, so growth would add nothing there. Growth starts as the reward for completing
the map.

## 2. Goals, non-goals, anti-goals

**Goals**

- **A:** After the region's last city is connected, every city's demand grows each month at one
  per-region annual rate drawn from real published forecasts (the player approved real-world rates).
- **A:** Everything that forecasts demand sees the growth: the rate review's forward peak, Planning
  forecasts, the monthly outlook, connection warnings, the automatic commitment planner, city
  heights and the city card.
- **A:** The player is told when growth begins and at what rate.
- **B:** A fourth, bonus milestone, hidden until the three regional milestones are all reached, and
  earned by 12 consecutive closed months, each strike-free and with zero energy from `Coal`,
  `Ccgt` and `Peaker` plants (the player's words: fossil plants "idle for a full year"). Fossil
  plants may still be owned.
- **B:** Progress reads like the existing strike-free counter ("7 of 12 clean, strike-free months").

**Non-goals**

- CO2 accounting in tonnes, emission factors, carbon prices or policy. The milestone needs only
  "did any fossil plant generate this month".
- Growth before the map is complete, per-city growth rates, or growth that varies by year or
  scenario (high/low cases).
- Changing the three regional milestone targets. They stay frozen on authored loads
  (`regional_milestones`, `milestones.rs:44-71`); growth only makes the peak target arrive sooner.
- Demolishing, retiring or selling plants (§5, follow-up).
- Growing existing saves. See §3.7.

**Anti-goals** (failures even if it technically works)

- Punishing the player for building ahead of growth: if the regulator's reserve assessment ignored
  growth, prudent building would be held back as excess capacity.
- Growth fast enough to make the game a treadmill. Real rates are 2–5% a year; at 32× a game year
  is about two minutes of play.
- Breaking determinism or replay: golden pre-change saves must still continue bit for bit.
- Invented numbers presented as real. Every rate carries its source, as other region data does.

## 3. Part A — demand growth

### 3.1 Region data

- New region schema **7**. Field: `demand_growth_per_year` (a fraction, e.g. `0.019`), required at
  schema ≥ 7 and rejected below 7, following the `weather` pattern
  (`region_schema6_requires_weather_and_schema5_defaults_to_one`, `region_catalog.rs:1671`). The
  frozen default for schemas ≤ 6 and legacy files is **0** (no growth). Widen the three hard-coded
  `4..=6` checks (`region/catalog.rs:154,163`, `region/mapping.rs:602`).
- All shipped regions move to schema 7 with the rates in §6, each with a source comment in the
  region file's existing citation style. The test fixture (`test-fixtures`) gets its own rate;
  plant and demand tuning numbers stay out of code constants (AGENTS.md).
- The domain gets the value through `RegionDefinitionParts` / a `RegionDefinition` getter, like
  `weather()`.

### 3.2 When growth runs

- The region is **complete** when every city in `region.cities()` is connected (active). Cities can
  never be disconnected and there is no demolish command, so completion is permanent.
- Growth steps at **every month close at which the region is complete**, checked after
  `advance_construction` has commissioned that month's lines. So the close that connects the last
  city already applies the first step.
- Each step multiplies a persisted session factor `demand_growth` (starts at exactly `1.0`) by the
  monthly factor `1 + demand_growth_per_year / 12`. *(My call.)* This is a nominal monthly rate:
  2% a year compounds to 2.018%, well inside the forecasts' uncertainty, and it avoids `powf`,
  whose last bits differ across platforms (`ARCHITECTURE.md:341-345`). Repeated IEEE
  multiplication is deterministic everywhere.
- A rate of 0 must take an exact "off" path: the factor stays `1.0`, and the demand formulas are
  bit-identical to today's (multiply only when the factor is not 1, or prove `x * 1.0` is a no-op
  everywhere it is applied). No new RNG draws; the demand-noise stream keeps one draw per tick.
- The step is applied after `build_report`, so the month just played reports the demand that was
  actually served. Where exactly (late in `close_month`, or in `finish_review`) is the builder's
  choice. The constraint: Review's outlook and the review-time auto-commit precompute
  (`application/src/auto_commit.rs:83-115`, which clones the session, runs `finish_review()` and
  compares `state_hash`) must agree on next month's demand.

### 3.3 Where the factor applies

- **Never write it into `CityDef.base_load`.** `validate_snapshot` (`session/mod.rs:1058-1068`)
  requires each `DemandCenter`'s `CityDef` to equal the region's. Growth is a separate factor
  applied wherever authored demand becomes live or forecast demand: `forecast_demand` /
  `connected_demand_forecast` and `hourly_forecast` (`demand/mod.rs:59-69`, `demand/forecast.rs`),
  and their callers (live tick, boundary dispatch, outage supply forecast, hydro plan, transfer
  control, battery planning, connection, outlook, `forecast_net_load`).
- Every `PowerSystem::new` (or wherever the factor lives) must carry it: scoped islands
  (`islands.rs:466`), `connection_power` (`connection.rs:178`), session start. A projected system
  built without the factor silently under-forecasts.
- Code that reads `base_load` directly needs a decision each: `city_detail` share
  (`views.rs:406-446`, including its bit-exact "identical climate" branch at `:441`) — shares are
  unchanged by a uniform factor, so it may stay; the city card's "Base load" stat
  (`city_card.rs:60`) shows grown base load (`base_load × factor`); `regional_milestones` keeps
  authored loads deliberately.

### 3.4 Forecasts look ahead with growth

Any projection for a future month `k` months ahead uses `factor × monthly^k` (repeated
multiplication), counting only steps at closes when the region is (projected to be) complete. This
covers:

- **The annual rate review's forward peak** (`projected_connected_peak`, `power_system.rs:1431`),
  and so the reserve-margin assessment and the rate-review estimate. This is the anti-goal guard:
  capacity built for next year's grown peak is prudent, not excess.
- The monthly ("crystal ball") and operational outlooks, the capacity-shortfall warning, and the
  connection capacity confirmation. When a pending line will complete the region, growth in these
  projections starts at the close that commissions it.
- Planning's day forecast and the automatic commitment planner already read the current month's
  demand, so they need only the current factor.

### 3.5 What the player sees

- **A toast when growth begins** *(my call)*: at the close that completes the region, e.g. "Every
  city is connected. Demand will now grow about 3.3% a year." It comes from a new domain event
  (e.g. `DemandGrowthStarted { annual_rate }`), since toasts only come from domain events
  (`hud/toasts.rs:111-137`). Text in `assets/locales/en-US/*.ftl`, key in `l10n/keys.rs`.
- **City heights** grow with demand. Today the Planning height uses `base_load`, normalised by the
  largest authored base load (`world/model.rs:355-380`), so a uniform factor would cancel out. Use
  `base_load × factor` in Planning and keep the reference fixed at the authored maximum, so cities
  visibly grow taller (main spec §2.4 / line 753 already allows heights above scale 3).
- **City card**: grown base load, plus the region's growth rate once growth has started *(my call)*.

### 3.6 Rules and schema

- Bump `SIM_RULES_VERSION` and regenerate the rules-version guard and scenario snapshots, as every
  rules change does (main spec §26.6, §28.11).
- Bump `CURRENT_STATE_SCHEMA` to 11 for the persisted factor. Gate every new field
  `schema >= 11`: restore re-encodes at the source schema and compares bytes before upgrading
  (`application/src/game.rs:166-170`). Update `SUPPORTED_STATE_SCHEMAS` and the hard-coded schema
  asserts (`state.rs:397`, `application/tests/save_resume.rs:66`,
  `infrastructure/tests/save_codec.rs:774`). Older-schema round-trip tests must mirror any restore
  upgrade (see memory "restore-path migrations break old-schema tests").

### 3.7 Existing saves

A save keeps the region bytes it started with (`infrastructure/src/save/codec.rs:329`), which
decode at schema ≤ 6 to a growth rate of 0. So **games started before this change never grow**;
only new games do. This matches the project's rule that pre-change saves continue bit for bit
(golden tests: `infrastructure/tests/geothermal.rs`, `save_snapshots.rs`,
`economic_construction.rs`). The default was stated to the player during design; growing old
saves would need a region-data migration and is out of scope.

## 4. Part B — the green milestone

### 4.1 Rule

- New `MilestoneKind::FossilFreeYear` *(name is the builder's)*. Earned at the close of the 12th
  consecutive closed month in which `energy_by_kind` (`report.rs:75`) shows **zero** energy for
  `Coal`, `Ccgt` and `Peaker`. Plants of those kinds may be owned and connected; their entries
  appear with `0.0` (`power_system.rs:796-799`), so test the energy value, not the key. Use an
  exact zero or a tolerance of 1e-9 MWh, not a looser one: a few MWh of start-up or shutdown tail
  is real fossil generation and breaks the run.
- **The month must also be strike-free** (the player's decision). Without this, a player could
  switch every fossil plant off, accept rotating outages up to the 0.5% unserved threshold and up
  to two strikes, and still earn "green". The projection already tracks strikes for
  `FullSeasonWithoutStrike` (`strike_pending`, set by `StrikeIssued`); reuse that. A frequency
  notice is not a strike and does not break the run.
- Batteries, nuclear, hydro, wind, solar and geothermal are all clean.
- Any month with fossil energy or a strike resets the run to 0. Run length counts closed months regardless of
  season or calendar year.
- **Hidden until revealed.** It is revealed at the first month close after which all three
  regional milestones are reached (the targets in `goals`, whichever order).
- **Months count retroactively** (the player's decision). The run is tracked from the start of
  the game, hidden or not. At the reveal the panel shows the run as it stands, and if it is
  already 12 or more the milestone is earned at the reveal month. Otherwise it is earned at the
  close where the run reaches 12.
- Like other milestones it is earned once and the game does not end.

### 4.2 Implementation shape

- Keep `goals: [MilestoneKind; 3]` and add the bonus as separate state in
  `MilestoneProjection` (revealed flag or month, current run), because `targets()`, the HUD and
  `save_snapshots.rs:715` assume three. `same_objective` must cover the new kind, or `check()`
  pushes it again every month.
- New kind and progress tags (`MilestoneProgress::FossilFreeMonths { months, needed }`) are gated
  `schema >= 12` (see below). `state_record!` allows one `sinceN` group per type (`state.rs:296-370`) and
  `MilestoneProjection` already uses `since10`, so extend the macro or hand-write that encoder.
- Restore of schema ≤ 11 saves: seed the run from the closed months the save already holds
  (`MonthlyReportProjection` keeps every `MonthlyReport`, whose `energy_by_kind` gives fossil
  energy), counting back from the latest month. Reports don't record strikes directly; the builder
  finds where strike months can be read exactly (regulator state, or the report's unserved share
  and frequency fields against the regulator's rule). If they can't be recovered exactly, start the
  run at 0 at the restored month rather than guess. If the three milestones are already reached,
  reveal at the restored month (and award if the seeded run is ≥ 12), without a notification
  (mirror `upgrade_targets`, `milestones.rs:189-201`). Old saves can therefore earn it.
- It changes no simulation outcome, so it needs no `SIM_RULES_VERSION` bump of its own. It does
  get **its own state schema, 12** *(my call)*: every push to `main` deploys the web build, so
  Part A may ship alone and players will create schema-11 saves without the bonus fields. Part B
  gates its fields `schema >= 12`, upgrades schema ≤ 11 saves on restore as above, and updates
  the schema asserts and old-schema round-trip tests again.

### 4.3 What the player sees

- The Milestones panel shows three rows until the reveal, then a fourth row with progress
  "N of 12 clean, strike-free months" and, once earned, the month reached like the others.
- **A toast when it appears, and a toast whenever any milestone is earned** *(my call)*.
  Milestones have no notification path today (no `MilestoneReached` event; `newly_reached` is
  never drained in the game), and the reveal is the whole point of a hidden goal. Once the game
  drains `newly_reached`, the three regular milestones toast too, rather than filtering them out
  on purpose. These are read-side facts, so drain them from the projection in the game (not a new
  domain event), and never re-notify on restore or replay (main spec line 2033).
- New `.ftl` strings for the name, progress and toasts; typed-value keys follow
  `Localize for MilestoneKind` (`l10n/localize.rs:496-541`) and the coverage tables
  (`tests/game/l10n_coverage.rs:401-425`).

## 5. Follow-up, not in this work: demolishing plants

The player would like to demolish fossil plants once they are green, with no milestone attached.
There is no retire, demolish or sell command today (`application/src/command.rs:13-89`). Idle
plants still pay fixed O&M (`month_close.rs:212-221`) and count as firm capacity in the reserve
review, so a real retirement mechanism has economic and regulatory consequences (remaining book
value, rate base, decommissioning cost) that deserve their own design.

## 6. Growth rates

Rates are per year, rounded to one decimal. Sources were read on 2026-11-02; the downloaded copies
are not committed.

| Region | Rate | Measure, period | Source | Driver |
|---|---|---|---|---|
| `eastern_interconnection` | 1.9% | Peak, 2026–2035 | NERC, *2025 Long-Term Reliability Assessment* (Jan 2026), Total Internal Demand by assessment area; sum of the Eastern areas (MISO, PJM, SPP, SERC ×4, NPCC New England, New York, Ontario, Maritimes, Manitoba, SaskPower), 629.2 → 746.3 GW | Data centres |
| `western_interconnection` | 2.2% | Peak, 2026–2035 | Same, sum of the 8 WECC areas, 187.1 → 228.2 GW | Data centres |
| `brazil_sin` | 3.3% | Peak, 2025–2035 | EPE/MME, *PDE 2035 – Demanda de Eletricidade* (Sept 2025), p. 29, "Demanda máxima integrada, SIN", reference case, 109.2 → 151.4 GWh/h | Services growth, GDP |
| `continental_europe_maghreb` | 2.4% | Energy, 2026–2030 | IEA, *Electricity 2026*: EU 2.3%/yr (p. 14), Türkiye 2.6% (p. 189), Morocco 2.8% (p. 209), weighted by demand. *(My call over ENTSO-E ERAA 2025's ~3%, which ACER says "appears to overestimate" demand.)* | Electrification |
| `india_nepal_bhutan` | 5.3% | Peak, FY2026-27 → 2035-36 | CEA, *Long-Term National Resource Adequacy Plan 2026-27 to 2035-36* (Mar 2026), Annexure-A, 288.6 → 459.4 GW | EVs, green hydrogen, data centres |
| `ips_ups` | 2.6% | Mixed, ~2024–2032 | Demand-weighted blend: SO UPS draft *SiPR 2026–2031* (Russia 2.1% peak), Kazakhstan Ministry of Energy forecast balances (5.3% peak), Uzbekistan ~6% energy (World Bank). Low confidence | New industrial load |
| `sgcc` | 5.2% | Energy, 2025–2030 | NDRC/NEA *New-Type Power System 15th Five-Year Plan* (3 Aug 2026, via Xinhua): ~600 TWh/yr added on 10.37 PWh; IEA gives 4.9% | Computing, EV charging |
| `southern_china` | 5.2% | Energy, 2025–2030 | Same national figure; CSG publishes no forward forecast (its area grew 5.4% in 2025) | Same |

The builder writes the citation comments into each region file and should re-check any figure
marked low confidence before shipping.

## 7. Testing

- **Determinism / legacy:** a rate of 0 leaves every golden pre-change continuation bit for bit
  (`cargo nextest run -p infrastructure -E 'test(prechange) | test(capture)'` early).
- **Region data:** schema 7 requires the field; schema ≤ 6 decodes to 0; a schema ≤ 6 file that
  contains the field is rejected. A missing-field test deletes the key and asserts the specific error kind.
- **Domain:** no growth while any city is unconnected; first step at the completing close; the
  factor after `n` steps equals the repeated product; property test that live and forecast demand
  scale by exactly the factor; the month report shows pre-step demand.
- **Projections:** forward review peak, outlook and connection warning include future steps, and
  start them at a pending completing line; reserve assessment does not hold back capacity built for
  the grown peak.
- **Milestone:** hidden before all three; reveal month; a run already ≥ 12 at the reveal is
  earned at the reveal month; a run of 5 at the reveal is earned 7 closes later; a fossil MWh
  resets the run; a
  fossil-free month with a strike resets it; a frequency notice does not; zero-energy
  fossil entries don't; earned at 12; schema ≤ 11 restore seeds the run from report history and reveals without
  notification; save and
  resume mid-run.
- **Game:** the growth toast, the reveal toast and each milestone's earned toast appear once and
  never on resume or replay; Milestones panel has
  four rows only after reveal and fits at minimum window and maximum zoom; city heights grow in
  Planning; l10n coverage. Launch smoke after HUD changes.

## 8. Left to the builder

- Exactly where the growth step runs (§3.2 constraint applies) and where the factor lives
  (`GameSession` or `PowerSystem`).
- Names of the region field, event, milestone kind and progress variant.
- Toast wording, the city card layout for the growth rate.
- Whether the bonus milestone is earned with exact zero or 1e-9 MWh tolerance.
