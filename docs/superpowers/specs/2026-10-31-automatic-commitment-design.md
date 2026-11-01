# Day-ahead weather and automatic commitment planning — design

Date: 2026-10-31. Status: draft for approval.

Two parts, built in this order, each in its own commit series:

- **Part A — day-ahead weather (§3).** Planning shows a forecast of the coming day's cloud cover and
  wind. The day's actual weather is that forecast plus a smaller surprise. This changes simulation
  rules.
- **Part B — automatic commitment (§4–§5).** At the start of each Planning phase the game trims
  expensive thermal plants to fit the forecast, with a small cushion. It is on by default, with a
  checkbox to turn it off. It changes no simulation rules.

B depends on A: the planner plans to the day-ahead weather.

## 1. Why

**Part B.** Commitment patterns persist from month to month (main spec §16.2), but demand and the
fleet change. Each Planning phase the player re-fits the grid by hand: switching peakers off when
they aren't needed, taking CCGTs off for the low-demand early part of the day, and sometimes taking
coal off entirely. The work is repetitive and mechanical, and it is the same reasoning every month.
The game should do that trimming itself, by default. With the grid already filled, the useful thing
to look at when Planning opens is the forecast, so Planning opens on the Forecast tab while the
planner is on.

**Part A.** A planner that switches thermal plants off depends on wind and sun. Today the day's
cloud cover and wind are drawn when the day starts, after Planning, and Planning only shows monthly
averages. So from Planning's point of view the whole day-to-day weather swing is a surprise. A bad
draw can leave anywhere from about 25% of forecast solar (Eastern, January) to about 72%
(Continental Europe, July). Real operators commit plants against a day-ahead weather forecast, so
only the forecast's error surprises them. The player chose to model that real mechanism rather than
have the planner hedge against an invisible draw.

## 2. Goals, non-goals, anti-goals

**Goals**

- **A:** Planning's forecast chart shows solar and wind output from the coming day's forecast
  cloud cover and wind. The planner and every Planning check also use that forecast. The actual
  day differs from
  it by a surprise about half the size of today's day-to-day spread. Long-run weather statistics,
  and so annual solar and wind energy, stay as calibrated.
- **B:** Always cover forecast demand **plus a small cushion** (§4.3), while burning as little
  expensive fuel as possible.
- **B:** Trim in merit order: peakers first, then CCGTs, then coal. Never switch off hydro, wind or
  solar, which cost almost nothing to run (the player's words). Nuclear and geothermal are never
  switched off either (§4.1).
- **B:** Treat batteries as part of the cushion against unexpected demand spikes, as far as is
  feasible (§4.3).
- **B:** Run automatically at the start of every Planning phase, on by default, with a checkbox to
  turn it off. Open Planning on the Forecast tab while the planner is on.

**Non-goals**

- An optimal unit-commitment solver. A deterministic greedy heuristic with the guarantees of §4.6
  is enough.
- Planning for the loss of the largest unit (N-1). It would keep much more thermal online and work
  against the fuel saving. Explicitly discussed and left out.
- Changing AGC flags, initial targets, battery enablement or HVDC schedules. The planner only
  writes commitment patterns.
- A summary of what the planner changed. The forecast chart is enough (the player's words).
- A per-save setting for the planner. The checkbox is a player preference (§5.2).
- Discrete weather events (storms, outages). They remain out of scope (main spec §3).
- A separate weather readout ("Tomorrow: mostly cloudy, light wind"). The forecast is visible only
  through the chart's solar and wind lines, so a player who hasn't built solar or wind won't see it.
  It doesn't affect their grid either. This is the player's choice.
- Changing the demand forecast or its ±2σ band.

**Anti-goals** (failures even if the code "works")

- **A:** a forecast that leaks the actual day. The actual weather must not exist until the day
  starts.
- **A:** shifting annual solar or wind energy. The regions' calibration (main spec §30, A16) must
  still hold.
- **B:** an auto-planned month that orders rotating outages a hands-off (all-day) player would not
  have seen. Saving fuel by dropping customers defeats the purpose.
- **B:** an auto-planned month that overgenerates where the player's own plan did not (§4.4).
- **B:** overwriting hand edits the player made in Planning and then saved manually, just because
  they loaded that save.
- **B:** a checkbox the player has to keep re-ticking. The preference sticks.

## 3. Part A — day-ahead weather (domain, rules change)

### 3.1 The split: forecast plus surprise

Each weather quantity becomes the sum of two independent parts with the same statistics as today
combined:

- a **forecast** part, drawn when Planning begins and visible all through Planning;
- a **surprise** part, drawn when the day begins (`BeginOperating`) and during it.

The surprise's standard deviation is **half** of today's, the player's choice. The forecast part
carries the rest of the variance, `sd_forecast = sd × √(1 − 0.5²) ≈ 0.866 × sd`, so the total
variance is unchanged. The ratio `0.5` is new region data (for example
`weather: (surprise_share: 0.5)`, the name is the builder's call), validated in `[0, 1]`, set to
0.5 in every shipped region and `test-fixtures`. A deterministic scenario can set it to 0, so the
forecast is the actual weather, or to 1, which reproduces today's behaviour: nothing known ahead.

**Solar clearness** (one regional value per day, main spec §30):

- Planning begins: `c_forecast = mean[month] + sd_forecast × z₁`.
- The day begins: `c = clamp(c_forecast + sd_surprise × z₂, MIN_CLEARNESS, MAX_CLEARNESS)`, with
  today's clamps.

**Wind factor** (the AR(1) process of main spec §25.3, `w_t = m + ρ(w_{t−1} − m) + σ z_t`). The
deviation from the monthly mean splits into two independent AR(1) processes with the **same**
persistence `ρ`. Their sum is again AR(1) with `ρ`, and the variances add:

- Planning begins: draw the whole forecast path `f_0..f_1439` (unclamped), with
  `f_0 = initial_sd_f × z` and `f_t = ρ f_{t−1} + σ_f z_t`, using
  `initial_sd_f = 0.866 × initial_sd` and `σ_f = 0.866 × innovation_sd`.
- The day begins: `w_0 = clamp(m + f_0 + initial_sd_s × z, 0, 1)`, with
  `initial_sd_s = 0.5 × initial_sd`.
- Each later minute, **keeping today's per-step clamp:**
  `w_t = clamp(m + f_t + ρ e_{t−1} + σ_s z_t, 0, 1)`, with `σ_s = 0.5 × innovation_sd`.
  The surprise is then carried as the **post-clamp residual**, `e_t = w_t − m − f_t`.

Why the residual form: today the game clamps the wind state at every step
(`generation/wind.rs:5`). Clamping only the sum `m + f + e` would be a different process near the
bounds. A simulation showed India's monsoon wind (`m = 0.95`) averaging 0.913 instead of today's
0.869, a 5% rise in annual wind energy, which breaks an anti-goal. Carrying the surprise as the
post-clamp residual reproduces today's process **exactly** at `surprise_share = 1` (`f ≡ 0`). Near
the bounds, the forecast and surprise are then not strictly independent. That's acceptable, and §6
measures it. *My call.*

*My call:* the split-AR(1) form gives the forecast a real shape across the whole day, like a real
day-ahead wind forecast, instead of only revealing the starting value. Today's forecast decays to
the monthly mean within a few hours.

### 3.2 Timing and randomness

- **When the forecast is drawn:** in `GameSession::new`, for the first day, and when `FinishReview`
  enters Planning. Nowhere else. Loading a save never redraws.
- **Share 1 draws nothing (implementation ruling).** A region with `surprise_share = 1`, which
  includes every pre-forecast region (§3.2, region schema), knows nothing ahead. Its forecast is
  the month's mean (`WeatherForecast::neutral`), and drawing it consumes no random numbers. Frozen
  saves from before this change then continue bit for bit, which the golden-save tests require.
  Share is fixed region data, so skipping the draws can't make the random stream depend on the
  fleet.
- **When the surprise is drawn:** at `BeginOperating`, where today's draws happen, and per minute as
  today. Keep the main spec's ordering rules (§25.3): one `SimRng` draw per `z`, drawn even when
  its coefficient is 0 and regardless of fleet, with the per-tick demand draw before wind.
- **Island regions:** clearness and wind are both regional today. `begin_islands` draws one wind
  factor (`islands.rs:503`) and copies it into every island at day start and on each tick
  (`islands.rs:531`, `:635`). The forecast state (`c_forecast`, the wind forecast path and the
  surprise `e`) is regional as well, and it must reach **every** copy that today receives
  `clearness` and `wind_factor`: the per-island systems, `scoped_island`, `island_session`, and the
  transfer controller's locals (`transfer_control.rs:390`). Otherwise the Planning checks, which run
  on those copies, read stale weather. `wind_for_node` (`islands.rs:237`) duplicates the forecast
  formula and must change with it. Better still, make it call the shared formula.
- **Storing the forecast path:** it needs either 1,440 values per wind process in state, or one
  `u64` sub-seed drawn from `SimRng` that seeds a `rand_chacha` stream regenerating the path purely
  on demand. *Builder's call.* Either way the forecast is part of portable state, so a save made in
  Planning restores the same forecast.
- **Versioning:** new random draws and a new draw order mean `SIM_RULES_VERSION` 31 → 32, a portable
  state schema bump, and every snapshot regenerated once with the diffs reviewed.
- **Region schema 5 → 6.** `surprise_share` is new required region data. Saves embed their region
  RON and re-parse it on load (`save/codec.rs:329`), and the solar and wind DTOs deny unknown
  fields (`region/dto.rs:150`). So it follows the project's precedent for new region data (main
  spec §25.6, `mapping.rs:599`): schema 6 requires the field, and schema ≤ 5 data reads it as
  **1.0**. *My call:* a game started before this change keeps today's weather behaviour, nothing is
  known ahead, and its upcast becomes trivial.
- **Upcasting older saves.** Old state stores only `wind_factor`, `wind_tick` and `wind_month`, but
  the new rules read the forecast every tick (battery plan, outage sizing, boundary dispatch, hydro
  re-planning, AGC's `wind_fall`). Every older save is upcast with the neutral values that share 1.0
  implies, in whatever phase it was saved: `f ≡ 0`, `e = w − m`, and `c_forecast` equal to the
  month's mean clearness. Loading the same save twice must give the same game.

### 3.3 What reads the forecast

- **Expected solar** (`expected_variable`, the forecast chart's solar line, the supply forecast,
  the battery preview, the shortfall and minimum-generation checks): today this uses the **mean of
  the bounded draw**, `mean_clearness(mean, sd)` (`region/mod.rs:715`). That is not a clamp of the
  mean, because main spec §30 (A16) forbids an optimistic forecast caused by clipping, and
  `solar.rs:168` asserts that the forecast equals the sampled mean. Keep that rule: the expected
  clearness becomes `mean_clearness(c_forecast, sd_surprise)`, computed once when the forecast is
  drawn (the quadrature is 4,096 steps) and stored with it. Archived regions that today use the raw
  authored mean keep doing so, with `c_forecast` in its place.
- **Wind forecast** (`wind_factor_forecast`): during Planning, `m + f_t`. During Operating, keep
  today's conditional forecast from the current actual value, with the forecast path in place of
  the monthly mean: `m + f_t + ρ^(t−now) e_now`.
- **Forecast bands:**
  - wind's band becomes `±2 × stationary sd of the surprise`, derived from region data. The existing
    `band: 0.25` field is then unused. *Builder's call* whether to remove it, which touches every
    region file, or keep it validated and ignored;
  - solar gets a band too: clearness `c_forecast ± 2 × sd_surprise`, clamped. The forecast chart
    draws it as a shaded band around the solar line, like wind's. The new legend label goes in the
    `.ftl` files and `keys.rs`.

Everything that today means "expected weather" now means "forecast weather". No reader may see the
surprise before it happens.

## 4. Part B — the planner (domain)

A pure query on `GameSession`, for example
`GameSession::automatic_commitment_plan(&self) -> BTreeMap<PlantId, CommitmentSchedule>`
(name is the builder's call). It reads the session and returns a pattern for every plant it is
allowed to change (§4.1). No RNG and no state change. It obeys the domain's rules:
`BTreeMap`/`BTreeSet` only, ties broken by plant ID, no Bevy, serde or I/O.

### 4.1 Which plants it may change

- **Trimmable:** connected, commissioned `Coal`, `Ccgt` and `Peaker` plants **on AGC**. The planner
  owns their patterns: it starts each one from all-day and may cut one off-gap per day (§4.5).
- **Left exactly as they are:** everything else, including
  - nuclear, hydro and geothermal: never switched off by the planner, and never switched back on
    by it either. A plant the player switched off by hand stays off. *My call.* The planner plans
    around the player's choice instead of overruling it, and the planner never adds minimum
    generation the player removed.
  - manual (non-AGC) thermal plants. The supply forecast credits a manual plant's initial target in
    every committed hour (`outage.rs:107`), but after an off-gap the plant really returns at
    minimum output with its target reset (`plant.rs:461`). Trimming one would overstate supply.
    The player who set a manual target is running that plant themselves. *My call.*
  - solar, wind and batteries, which are not dispatchable.

### 4.2 Scope: local grids, checked together

The coverage check (§4.3) is evaluated per synchronous island, using the same split the existing
checks use (`island_session(id)` in multi-area regions, the whole session otherwise). A surplus in
one island never covers another.

Islands are **not** independent when automatic HVDC links join them (SGCC ships with them): the
transfer schedule merit-orders every island's committed AGC plants together
(`transfer_control.rs:280`), and hydro previews subtract the transfers (`hydro_plan.rs:117`). So
**every candidate change is accepted only if every island still passes its check**, with transfers
and hydro previews recomputed from the candidate plan, exactly as `island_session` does during
Planning. *My call:* this is the only choice that stays correct when links move power, and it costs
nothing in regions without automatic links, because the other islands' checks don't change.

### 4.3 The cushion: what "covered" means

The existing Planning check (`capacity_shortfall_check`, main spec §15.5) compares the committed
fleet's deliverable supply with **mean** forecast demand at its assessed samples: hour ends,
local-hour bends, or every minute for fleets with storage or wind. The planner uses the same
samples and builds on the same supply forecast (`supply_forecast()`), with these differences:

1. **Demand: the top of the forecast band.** Required supply at each sample is forecast demand
   scaled by `1 + 2 × forecast_error`, which is the `high` edge of the band the player sees on the
   chart (`ForecastPoint::high`). Every shipped region uses `forecast_error = 0.03`, so this is
   about 6% above the line. The number comes from region data and is never written into code.
2. **Wind and solar: the low edge of the day-ahead forecast** (§3.3). Wind is credited at the low
   edge of its band, solar at clearness `c_forecast − 2 × sd_surprise`, clamped. The player
   originally chose "plan for a calm, dull day", then chose to model day-ahead weather so the
   planner hedges only against the forecast's error, not the whole day-to-day swing.
3. **Restarts are ramp-limited.** A plant that comes back from an off-gap is online at minimum
   output (`plant.rs:461`). In the hours after its gap, credit it with
   `min(availability, min_output + ramp × time since it came online)`, not its full availability.
   The shortfall check can ignore ramps because an all-day fleet never restarts. The planner adds
   restarts, so it must not ignore them. *My call.*
4. **Spare battery discharge counts.** Enabled, connected batteries already cover real deficits
   during their discharge window (06:00–24:00, main spec §29) before customers are shed. In a
   sample inside that window, a battery's *spare* power counts toward the cushion: the discharge
   capacity that the supply forecast's expected-day battery preview has not already scheduled.
   Demand forecast error is persistent (`error_persistence = 0.995`), so a high-demand day stays
   high for hours. Credit a battery only with the power it could sustain from its previewed stored
   energy for the rest of that day's window:
   `min(spare power, previewed stored energy × discharge efficiency ÷ hours left in the window)`.
   Outside the window, batteries contribute nothing.
5. **Charging is treated as the supply forecast treats it:** optional, taken from surplus. It is
   not a requirement that keeps thermal plants online. *My call*, so the planner and the shortfall
   check agree. If trimming overnight thermal leaves less to charge from, the preview stores less
   energy, so the evening battery credit shrinks automatically.

At each sample the planner computes:

- `required = demand × (1 + 2 × forecast_error)`;
- `supply(plan)`: gross supply under the plan, with points 2–4 applied;
- `floor = min(required, supply(all-day plan))`, where the all-day plan has every trimmable plant
  all-day and everything else as it is.

A plan **covers** the region when every island has `supply(plan) ≥ floor` at every sample.

In words: where the all-day fleet clears the cushion, the plan must clear it too. Where even the
all-day fleet falls short (thin margins, a growing grid), the plan must keep everything the
all-day fleet could deliver **in that sample**, but may still trim elsewhere in the day. *My
call:* a single tight hour shouldn't stop the planner from trimming overnight peakers. The existing
shortfall prompt still warns about the tight hour.

### 4.4 What "best" means

Plans are compared in this order:

1. **Coverage** (§4.3) is a hard constraint.
2. **Less overgeneration.** Minimum-generation excess is the sum, over the samples the existing
   `minimum_generation_forecast` uses, of `max(0, committed minimum − net load)`. Lower is better.
   Excess trips plants and can earn strikes (main spec §14.3–14.4), which fuel cost doesn't capture.
3. **Lower cost** on the forecast representative day (mean demand, forecast weather):
   - **Fuel:** at each assessed hour, dispatch the mean forecast net load across committed AGC
     plants by the `boundary_dispatch` rule: every participant starts at minimum stable output,
     then output is added in ascending `(marginal_cost, id)` order. Manual plants count their
     initial target and scheduled hydro its preview allocation, as the shortfall check does.
     Fuel is `Σ output × marginal_cost`.
   - **Start-ups:** `startup_cost` once for each off-gap in each pattern (one restart per played
     day). The month close weights the representative day by the month's days, so a per-day
     comparison is the right one.

   Fixed O&M doesn't depend on commitment and is ignored.

### 4.5 The search (builder's latitude, with these guarantees)

A suggested shape, which the builder may improve as long as §4.6 holds:

1. Start from the all-day plan (§4.3).
2. Visit trimmable plants in **descending** `(marginal_cost, id)` order: most expensive first, so
   peakers, then CCGTs, then coal.
3. For each plant, consider all-off and every single contiguous off-gap on the 24-hour circle,
   including one that wraps midnight, that `CommitmentSchedule::try_new` accepts (the gap holds the
   shutdown and the start-up). Keep the candidate that covers the region and is best by §4.4, if it
   beats keeping the plant all-day. Commit that choice, then move to the next plant.

One off-gap per plant per day matches how the player trims today: off overnight or through the
morning. Multiple gaps per day are out of scope.

### 4.6 Guarantees (each becomes a test)

- **Valid:** every returned pattern passes `CommitmentSchedule::try_new` for its plant's spec.
- **Covered:** the returned plan covers the region (§4.3).
- **Never worse than all-day:** by the §4.4 ordering, the plan is no worse than the all-day plan.
- **Leaves the rest alone:** no pattern outside §4.1's trimmable set is returned or changed.
- **Deterministic and pure:** the same session gives the same plan, and calling it changes
  nothing (state digest unchanged).

## 5. Part B — the game

### 5.1 When the planner runs

- **New game:** once, as the first Planning phase begins, trimming the inherited all-day schedule
  (main spec §16.3).
- **Each month:** when `FinishReview` is accepted and the session enters Planning. This is after the
  day-ahead weather is drawn (§3.2).
- **Ticking the checkbox on during Planning:** runs it immediately, so the player doesn't wait a
  month. *My call.*
- **Not on load.** Loading a save never runs the planner. Month-close autosaves are captured in
  Review, before Planning (`persistence.rs:1008`), so the planner runs normally when the player
  continues. A manual save made partway through Planning holds the player's edits, and loading it
  must keep them.
- **Not in Operating or Review**, ever.

The game applies the plan by sending one `CommitPlant { plant, hours }` per plant whose pattern
differs from its current one, through the normal `PlayerCommand` path. Those commands are recorded
like clicks, so saves, the command log and replays stay exact. Part B needs no rules version bump of
its own. Any open commitment-grid draft is discarded first, because it was drafted against the old
patterns.

A refusal from the domain is a bug, since every pattern was validated in §4.6. Log it and leave
that plant as it was. Do not show a toast.

### 5.2 The checkbox and the preference

- **Where:** at the top of the Commitments tab, above the grid. Suggested label: *"Plan
  commitments automatically each month"* (new key in `assets/locales/en-US/*.ftl` and
  `l10n/keys.rs`). Its tooltip says, in one sentence, that it switches off expensive plants when the
  forecast doesn't need them, keeping a cushion above the forecast.
- **Default:** on.
- **Stored:** as a new field in `ClientPreferences` (infrastructure), next to `speed` and `effects`.
  It applies to every game, not per save. *My call.* Older preference files lack the field and must
  read as **on**. The struct already defaults missing fields.
- **Presenter pattern:** the checkbox is a `ToggleModel` in the commitment presenter. Clicking it
  produces a `UiAction` that `dispatch` turns into a preference write, plus the immediate planner
  run when switched on (AGENTS.md: dispatch is the only place a click becomes a message or
  resource write).

### 5.3 Hand edits

The grid stays fully editable while the planner is on. A hand edit to a trimmable plant holds for
that month's day, and the next Planning phase replaces it. Edits to anything outside the trimmable
set (§4.1), including AGC switches, persist as they do today. That is the point of the checkbox: a
player who wants their own thermal patterns to persist switches it off.

### 5.4 Which tab Planning opens on

On every entry to Planning (new game, after `FinishReview`, and on loading a save that is in
Planning), set the Planning tab to **Forecast** when the preference is on and **Commitments** when
it is off. Within the phase, the player's own tab clicks are respected. The first-game tutorial
already switches to Forecast at its step 3, so the two agree.

The tab write is a game-initiated UI write. Give it its own side-effect-free `UiAction`. Don't reuse
`SetPlanningTab`: `action.rs:633` gives `SetPlanningTab(Commitments)` extra behaviour that a click
needs but an automatic write must not trigger.

## 6. Acceptance

**Part A**

- **Statistics preserved.** The existing calibration tests (`shipped_regions.rs:3841`) only check
  terrain irradiance, so they cannot catch a weather shift. Add new long-run tests over many drawn
  days that compare the actual daily draws with today's process, for every shipped region and
  month:
  - mean actual clearness within 0.005 of today's `mean_clearness(mean, sd)`;
  - mean actual wind factor within 0.005 of today's per-step-clamped process, including India's
    monsoon (`m = 0.95`) and IPS's July (`m = 0.26`);
  - stationary wind spread within 5% of today's.

  The tolerances are *my call*. The builder may tighten them, but not loosen them without asking.
- **Exact at share 1:** with `surprise_share = 1`, the actual clearness and wind sequences equal
  today's **bit for bit** for the same seed, once the extra forecast draws are accounted for. Pin
  this with an RNG-order test.
- **Expected solar is unbiased:** the extended `solar.rs:168` test holds for the forecast-conditioned
  expectation. Averaged over forecasts, it equals the sampled mean of the actual clearness.
- **No leak:** during Planning, no query's output depends on the surprise draws. Concretely, two
  sessions that differ only in RNG state after the forecast draw give identical Planning views.
- **Edge share 0:** with `surprise_share = 0`, the actual day equals the forecast, apart from
  clamping.
- **Determinism and saves:** RNG-order tests pinned as in main spec §25.3. A save made in Planning
  round-trips the forecast. Upcast older saves in each phase (Planning, Operating, Review) load to the
  same game twice and play on without panicking. A schema-5 region reads `surprise_share` as 1.0.
- **Versioning:** `SIM_RULES_VERSION` bumped, snapshots regenerated and reviewed, and the
  shipped-region year-one suites (beginner survival, `assert_no_outages_year_one`) still pass with
  the hands-off plan.
- **Chart:** the solar band and the forecast-shaped wind line are drawn. The new legend key is
  covered by the l10n coverage test.

**Part B**

- **Domain:** the §4.6 guarantees as property tests, plus unit tests on the fixture region:
  - a peaker comes off when the fleet covers the band without it, and stays when it doesn't;
  - a battery's spare discharge lets a peaker come off in the evening that would otherwise have to
    stay;
  - a gap too short for shutdown plus start-up is never chosen;
  - a start-up cost that outweighs the fuel saved keeps the plant all-day;
  - a restart's ramp limit keeps a plant on when it couldn't reach the morning ramp in time;
  - a nuclear plant the player switched off stays off, and the planner covers without it;
  - with one tight hour the all-day fleet can't cover, the planner still trims overnight;
  - a sunny forecast lets a CCGT come off at midday that a dull forecast keeps on;
  - on an SGCC-like linked fixture, trimming one island never leaves another uncovered.
- **Shipped regions:** add an auto-plan variant of `assert_no_outages_year_one`
  (`crates/infrastructure/tests/shipped_regions.rs:1104`). Same 12 noisy months, seeds 0–7, every
  shipped region, but the planner's commitments are applied at every Planning phase, on top of the
  existing hands-off setup (AGC on for every plant). Bar:
  - no rotating outage in any month;
  - the run survives year one;
  - fuel plus start-up spend is **no higher** than the hands-off year in every region and seed,
    and **strictly lower** summed across all of them.

  If a region fails the outage bar, the cushion or the ramp model is wrong. Fix the planner. Do not
  loosen the test. *My call* on "no higher, and lower in total": a region with nothing worth
  trimming shouldn't fail.
- **Game:** integration tests (modules under `tests/game/`) that
  - a new game with the preference on starts Planning on Forecast with trimmed patterns;
  - after `FinishReview`, patterns are re-planned and the tab is Forecast;
  - with the preference off, nothing is re-planned and the tab is Commitments;
  - loading a Planning-phase manual save keeps its patterns;
  - ticking the checkbox on in Planning re-plans immediately;
  - each new `UiAction` goes through the real `dispatch` (glue test).
- **Performance:** the planner runs once per month, but must not hitch the frame. Measure it on
  the largest shipped region, native and wasm. If it is slow, restrict candidates or cache
  per-plant supply contributions. Don't move it off the main thread unless measurement forces it.
- **Launch smoke:** `cargo run -p game -- --exit-after-frames 120` after the plugin changes.

## 7. Left to the builder

- **A:** how the forecast path is stored (§3.2), whether the unused wind `band` field is removed,
  and the region-data field's name.
- **B:** the planner query's name and where it lives within `domain::session`.
- **B:** how to evaluate candidates efficiently: incremental supply deltas versus recomputing the
  supply forecast.
- **B:** whether trimming should revisit earlier plants after later ones change (a second pass), as
  long as §4.6 holds.
