# Megawatt Mogul: Design Spec (Milestone 1)

- **Date:** 2026-10-18
- **Status:** Draft for review
- **Amendments:** A1, a hydro peak-shaving schedule (§12, 2026-10-18); A2, proration of the
  partial-year rate review (§13, 2026-10-18)
- **Engine:** Rust 1.98, Bevy 0.20, bevy_egui 0.43, bevy-inspector-egui 0.38

## 1. Vision and goals

Megawatt Mogul is a single-player 3D simulation. You run a power company that connects electricity
suppliers with consumers and grows it, while keeping the grid balanced, customers happy and costs
down. Supply has to match demand at every moment. Cheap baseload plants ramp slowly, while
fast-ramping plants are expensive, so the player learns to cover predictable demand with cheap,
slow plants committed ahead of time and to patch the margins with expensive fast ones. This is the
**merit order**, as real grid operators use it.

Three goals carry equal weight. A trade-off between them is a design problem to solve, not a choice
to make quietly.

1. **Fun and polish.** It feels like a real game, not a tech demo.
2. **Architectural elegance.** Clean Architecture and Domain-Driven Design, applied pragmatically.
3. **Teach how real grids work.** Realistic mechanics teach through play, not text dumps.

## 2. Player experience

### 2.1 Core loop: dispatcher + builder

Real-time balancing is the heartbeat. Between operating days the player builds plants and
transmission lines and connects new consumers.

### 2.2 The day cycle

Each played day is the **representative day of one calendar month** (§2.3) and has three phases,
modelled as the domain state machine `DayPhase`:

1. **Planning** (paused). The player sees the coming month's hourly demand forecast with an uncertainty
   band, the solar forecast and each hydro plant's water budget. They **commit** plants hour by
   hour, queue construction and build lines to connect cities. The current regulated tariff and a
   projection of the next rate review are shown, but the player does not set them (§2.5).
2. **Operating** (runs). The 24 game hours run as 1,440 ticks (1 tick = 1 game minute) at a
   selectable speed: pause, 1×, 2×, 4× or 8×. At 1× a day takes about 6 real minutes. The player
   adjusts plant targets, starts and stops plants, and toggles automatic load-following (AGC) on
   individual plants. Predictive warnings flag trouble ahead.
3. **Review** (paused). P&L, the merit order that actually ran, a timeline of reliability incidents,
   and 2–3 plain-language lessons generated from that day's events. Money and energy totals are
   shown for the whole month (§2.3).

### 2.3 Calendar compression

**One played day is the representative day of a calendar month.** A game year is 12 played days,
and the date reads "March 2027". Real grid planning models (e.g. NREL's ReEDS) use the same
technique: they simulate a few representative days and weight each by the number of real days it
stands for.

- **The physics is literal.** The 24 hours played are one real day, at 1,440 ticks.
- **The accounting is weighted.** Energy, costs and revenue for the played day are multiplied by
  the number of days in that month (Gregorian, including leap years) to give the month's totals.
  Daily restarts scale too. A separate month-boundary start, needed when returning from a month
  offline, is charged once; it is not multiplied by the number of days (§6.1).
- **Commitment is a repeating daily pattern.** Each representative day starts from the state
  implied by that month's pattern, including any start-up already in progress across midnight.
  Physical plant state never carries from March's representative day into April's. Persistent
  construction, company and regulator state still advances by calendar month.
- **Seasons change visibly every few played days.** Months drive the demand shape, solar
  irradiance and hydro water budgets, so seasonal swings become central to play.
- **Build times are counted in months** (= played days, §3.2), so they read naturally and keep
  their real-world proportions. Building nuclear is a long-term bet.
- One game year takes about 72 minutes of operating at 1×.

### 2.4 Service territory and growth

The player does not start out operating an entire interconnection. Each region defines a
**starting service territory**: a few connected cities, a starter fleet and a starting substation.
The company grows by **building transmission lines to connect new demand centres**. Only connected
demand counts, both as revenue and as load that must be served. Unconnected cities exist on the map
but are not simulated. Connecting a city too early, before there is capacity to serve it, is a
classic overreach that the game should punish fairly.

This keeps early games manageable (tens of plants, not hundreds) and puts the "connect suppliers
with consumers" fantasy at the centre of the game.

### 2.5 Scoring and failure: endless tycoon

The game has no ending. It tracks three scores:

- **Cash.** Integer cents, exact.
- **Reliability.** The share of connected demand actually served, over a rolling 3-month window.
- **Satisfaction.** 0–100. Each month has a target of `100 × reliability_factor × price_factor`, and
  satisfaction moves toward it as an exponential moving average (α = 0.2). The shape of each factor's
  curve is region data.

There are two ways to fail:

- **Bankruptcy:** cash below zero at the close of 3 consecutive months.
- **Licence revocation:** the regulator issues a **strike** for each month in which unserved energy
  exceeds a threshold share of that month's demand (default 0.5%, region data). One strike expires
  after each 3 consecutive clean months.
  Three active strikes revoke the licence.

**Milestones** give direction without ending the game (e.g. "Serve a 50 GW peak", "A full season
without a strike").

**The tariff is regulated, not set by the player.** The company is the sole supplier in its
territory, so it works like a real regulated utility under **cost-of-service, performance-based
ratemaking (PBR)**:

- **Annual rate review.** When December closes, the regulator resets the tariff ($/MWh) for the
  coming year:
  `tariff = revenue_requirement / energy served over the past year`, where
  `revenue_requirement = prudent operating costs (fuel, start-up, O&M) + depreciation + allowed_return × eligible_rate_base × (months in the review period ÷ 12)`.
  Year 1 uses a starting tariff from region data. A review period shorter than a year, such as a
  first year that didn't start in January, earns a prorated return (Amendment A2, §13).
- **Rate base** is the undepreciated capital cost of plants and lines in service. Depreciation is
  straight-line over each type's book life (region data). Construction capital is paid in cash in
  equal monthly instalments over the build. It enters the rate base when the asset is commissioned.
- **Performance-based return.** The allowed return starts from a base (e.g. 9%). It is adjusted up
  or down by the year's **reliability** (against a target, e.g. 99.9% served) and **cost
  performance** (operating cost per MWh served against the previous year), within a band (e.g.
  7–11%).
- **Forward-looking reserve assessment.** The review peak is the higher of the past year's actual
  peak and the projected coincident peak over the next 12 calendar months of all cities connected
  at review time, including connections commissioned at that month's close. The projection uses
  the sum of their hourly mean load profiles, with seasonal/monthly factors and no random noise;
  it is not the sum of individual cities' separate peaks. Unbuilt connections do not count.
  Firm capacity is the sum of `capacity × capacity_credit` (§3.2), and the allowed **planning
  reserve margin** (e.g. 20%) is applied to this review peak.
- **"Plant held for future use" before disallowance.** Excess capacity is assessed against newly
  added capital and capital already held from the previous review. The excess portion earns no
  return in the tariff set by that review, but is re-tested at the next annual review. A held
  portion that becomes useful is released and its consecutive-excess count resets. Only the same
  portion still excess at **two consecutive reviews** is permanently written off. Retain asset IDs,
  held capacity/capital portions and their review history; a held asset remains a candidate after
  its commissioning year. Re-test existing held portions first, then allocate remaining excess
  proportionally across new additions, with deterministic ID tie-breaking. Plants remain available
  to dispatch even when their capital is held or written off.
- **Held-capital accounting.** Holding suspends the allowed return on the held portion, not its
  straight-line book depreciation. Depreciation continues to enter the revenue requirement; a
  permanent write-off removes only the remaining held book value and future recovery of that value.
  Holding, release and write-off are non-cash rate-base adjustments. The eligible rate base is the
  remaining book value excluding held and written-off portions. A plant's **associated line capital**
  is every line commissioned in the same review year with an endpoint at that plant's node; it is
  allocated the plant's assessed excess fraction, while held line portions keep their own history.
  Lines with no plant endpoint (substation-to-city, city-to-city) are never held.
- **Satisfaction** depends on reliability and on the tariff relative to the region's reference
  tariff. Overbuilding therefore hurts twice: through disallowance and through a higher tariff.

The player profits by running *better than the formula expects*: cheap dispatch, high
reliability, and building only what the reserve margin justifies.

### 2.6 Regions

The player chooses a region when starting a new game. Milestone 1 ships two:

- **Eastern Interconnection (North America):** the US east of the Rockies plus central and eastern
  Canada, **excluding Quebec** (its own interconnection) and **ERCOT**. The Texas Panhandle belongs
  here because it is synchronised with the Eastern grid.
- **Western Interconnection (North America):** the western US, British Columbia, Alberta and
  **Baja California** (the northern Mexican state that is synchronised with the Western grid).
  El Paso belongs here. Baja California Sur is not included; in reality it is an isolated system.

Each region is a **single balancing area** (copper plate, §5.4). Both run at 60 Hz, and money is
in US$. The two regions play differently:

- **East:** about 4× the load, dense cities, a legacy coal and nuclear fleet, and Great Lakes
  cooling water.
- **West:** long distances, mountains, southwest solar, Pacific Northwest and BC hydro, and a steep
  evening duck curve.

**The region picker is skipped automatically when exactly one region exists.** With two regions it
is shown.

## 3. Milestone 1 scope

### 3.1 In scope

- Main menu, then the region picker (auto-skipped when only one region exists), then a new game
  with a seed.
- Both regions as data files (`assets/regions/*.ron`).
- A 3D diorama map built from the region's terrain cell grid, with phase-specific overlays (§8.3).
- Six plant types (§3.2), plant construction, and transmission-line construction for connectivity.
- Demand centres with daily, seasonal and noise profiles, plus forecasts.
- The full day cycle (one representative day per month), frequency model, protection, economics, regulator, scoring and failure.
- An egui HUD for every phase, and dev tools behind a `dev` feature.
- Command logging with versioned replay inputs and an archived region file as the basis for future
  replays and saves (§7).
- Localisation scaffolding with en-US only.

### 3.2 Plant types (initial tuning values, stored as region data and not in code)

| Type | Unit size | Min stable | Ramp | Start-up | Start-up cost | Marginal cost | Capital cost | Build (months) | Lesson |
|---|---|---|---|---|---|---|---|---|---|
| Nuclear | 1,100 MW | 70% | 5 MW/min | 48 h | $1.0M | $10/MWh | $6.0B | 84 | Commitment is a promise |
| Coal | 600 MW | 40% | 6 MW/min | 8 h | $150k | $30/MWh | $2.0B | 48 | Cheap baseload, start-up costs |
| Combined-cycle gas (CCGT) | 500 MW | 40% | 25 MW/min | 2 h | $40k | $45/MWh | $0.6B | 30 | The flexible middle |
| Gas peaker | 200 MW | 20% | 40 MW/min | 10 min | $5k | $150/MWh | $0.2B | 12 | Pay dearly for agility |
| Solar | 300 MW | 0% | n/a (weather) | n/a | n/a | $0/MWh | $0.4B | 12 | The duck curve |
| Hydro | 800 MW | 0% | 100 MW/min | 5 min | $1k | $0/MWh | $3.0B | 72 | Energy-limited dispatch |

- **Hydro** has a **daily water budget** (MWh per day, varying by month and site). Output stops when
  the budget runs out, and unused water does not carry over to the next month in milestone 1. Under
  AGC, hydro follows a **peak-shaving schedule** that spends its water on the day's highest net
  load (Amendment A1, §12). Hydro can
  only be built on cells with a hydro resource.
- **Solar** output is irradiance × sun angle × cloud noise. Protection can curtail it but nobody can
  dispatch it up.
- **Regulatory accounting values** (region data): capacity credit, which is the share of capacity
  the regulator counts as firm (thermal 0.95, hydro 0.8, solar 0.3); and book life in months
  (nuclear 480, coal 480, CCGT 360, peaker 360, solar 300, hydro 600).
- **Siting constraints** come from cell resources. Nuclear and coal need cooling water, solar output
  scales with irradiance, and hydro needs a hydro site.

### 3.3 Out of scope for Milestone 1

- Transmission congestion and power-flow physics.
- Wind, storage and other plant types.
- Weather events.
- Competitors and markets.
- Loans.
- Save/load (the command log is recorded but not yet loadable).
- Audio.
- A tutorial campaign.
- More regions.
- Minimum up/down times.
- Hydro reservoir carry-over.
- Per-city satisfaction.
- Equipment ageing and maintenance.
- Player-filed rate cases (the regulator's annual review is automatic).

## 4. Architecture

### 4.1 Cargo workspace

```
Cargo.toml                 # workspace: members, shared [workspace.dependencies], lints
crates/
  domain/                  # pure Rust: model, rules, events. NO Bevy, NO serde, NO I/O
  application/             # commands, handlers, pipeline layers, queries, projections, ports
  infrastructure/          # adapters: RON region loader (DTOs + mapping), command-log writer
  game/                    # Bevy binary: plugins for sim, world view, overlays, camera, HUD, dev tools
assets/
  regions/                 # eastern_interconnection.ron, western_interconnection.ron
  locales/en-US/           # *.ftl (Fluent)
docs/superpowers/specs/
```

### 4.2 Dependency rule (enforced by the compiler)

```
game ──▶ application ──▶ domain
  └────▶ infrastructure ─┘ (infrastructure implements application ports)
```

- `domain` depends on nothing but small, pure crates (`rand_chacha`, `thiserror`). Its `Cargo.toml`
  does not list Bevy, so an import is impossible rather than merely discouraged.
- `application` depends on `domain` (plus `tracing` for the pipeline layer).
- `infrastructure` depends on `application` and `domain` (plus `serde`, `ron`, `tempfile` in tests).
  SHA-256 region fingerprinting also lives in infrastructure (`sha2`), never in domain.
- `game` depends on all three, plus Bevy, bevy_egui, egui_plot and, optionally, bevy-inspector-egui.
- `cargo test -p domain` and `cargo test -p application` never compile Bevy, so feedback takes
  seconds.

### 4.3 Pinned versions

| Crate | Version | Used in |
|---|---|---|
| bevy | 0.20 | game |
| bevy_egui | 0.43 (egui 0.36) | game |
| egui_plot | 0.37 (egui 0.36) | game |
| bevy-inspector-egui | 0.38 | game (`dev` feature) |
| rand_chacha | 0.10 | domain |
| thiserror | 2.0 | domain, application, infrastructure |
| tracing | 0.1 | application, game |
| serde / ron | 1.0 / 0.12 | infrastructure |
| fluent-bundle | 0.16 | game |
| proptest / proptest-state-machine | 1.12 / 0.9 | dev-dependencies |
| insta | 1.49 | dev-dependencies |
| tempfile | 3.27 | dev-dependencies |
| cargo-mutants / cargo-nextest | 27 / 0.9 | dev tooling (installed binaries) |

## 5. Domain model (`crates/domain`)

### 5.1 Shared kernel: value objects

These are immutable newtypes, and no bare `f64` crosses a module boundary.

- `Megawatts` and `MegawattHours`: `f64` inside, with explicit conversion only via
  `Megawatts × SimDuration`.
- `Hertz`.
- `Money`: `i64` cents.
- `Price`: $/MWh.
- `RampRate`: MW per tick.
- `Ratio`: 0..=1, validated.
- `SimTime` / `SimDuration`: ticks, where 1 tick = 1 game minute and 1,440 ticks = 1 day.
- `SimMonth`: year + month (one played day each), with season and `days_in_month()` (the accounting
  weight) derived from it.
- IDs: `PlantId`, `CityId`, `NodeId`, `LineId`, `RegionId`, `PlantSpecId`, `CellCoord`.

### 5.2 Modules (bounded contexts)

- **`region`**: `RegionDefinition`. It contains:
  - id, name key, nominal frequency and start month;
  - a terrain grid of `Cell`s (height, biome, solar irradiance, cooling water, hydro potential);
  - a plant catalog (`PlantSpec` per type);
  - demand-centre definitions (location, base load, load-profile parameters);
  - the starting territory and fleet, and starting cash;
  - regulator parameters: starting and reference tariff, base allowed return and PBR band,
    reliability target, planning reserve margin, and strike threshold.

  It is built only through validating constructors that return `Result<_, RegionError>`.
- **`generation`**: `Plant`. It has a spec, a location, a `commissioned_on: SimMonth` field (a seam
  for future ageing), an hourly `CommitmentSchedule`, an `auto_follow` flag and a lifecycle enum:
  `UnderConstruction { months_left }`, `Offline`, `Starting { ticks_left }`,
  `Online { output, target }`, `Stopping`.

  The commitment is a validated cyclic pattern (§6.1). The plant also retains whether it was
  offline for the previous whole month, an initial dispatch target, and separate recurring and
  month-boundary start counts. Lifecycle/output/remaining start-up ticks are reinitialised from
  the pattern at `BeginOperating`; they are not restored from the previous representative day.

  Invariants: output stays between minimum stable output and capacity while online; output changes
  by no more than the ramp rate per tick; hydro output never exceeds the remaining budget.
- **`demand`**: `DemandCenter`, `LoadProfile` (a daily curve × a monthly factor) and `Forecast`.
  The forecast holds an hourly mean and band; actual demand is the forecast plus AR(1) noise from
  the session's seeded RNG.
- **`grid`**: the **`PowerSystem` aggregate root**. It owns the plants, demand centres, network
  topology (nodes and lines; connectivity is computed from the starting substation), system
  frequency and `NetworkModel`. It is one aggregate because balancing has to see all of it in the
  same tick. It includes these domain services:
  - **`MeritOrder`**: committed plants sorted by marginal cost.
  - **AGC** (automatic generation control): moves auto-follow plants' targets to close the
    imbalance, raising the cheapest plant first and lowering the most expensive first, within ramp
    limits.
  - **`FrequencyModel`** (§5.4).
  - **`Protection`**: load shedding, curtailment and blackout.
- **`company`**: the **`Company` aggregate**: cash, a `Ledger` of typed cash entries (fuel, start-up,
  O&M, construction instalments, revenue), the rate base and its depreciation schedule, held
  capital portions and consecutive-review history, and satisfaction. Depreciation and write-offs
  are non-cash rate-base changes, not cash ledger entries.
- **`regulation`**: the **`Regulator` aggregate**. The regulator is a separate actor with its own
  rules, so it gets its own bounded context. It holds the current tariff, allowed return, strikes,
  licence status, and the year's performance tallies used in the rate review.
- **`session`**: `GameSession`, which composes the `PowerSystem`, `Company`, clock, `DayPhase`,
  `GameSettings` and the seeded `ChaCha` RNG. It owns the tick order (§6.4).

### 5.3 Settings seams

```rust
pub struct GameSettings { pub fidelity: SimulationFidelity }
pub struct SimulationFidelity { pub network: NetworkModel }
pub enum NetworkModel { CopperPlate }   // later: DcPowerFlow
```

`NetworkModel` uses a one-variant enum rather than `bool`, so adding a variant later forces every
`match` to handle it. Equipment ageing will get its own field in `SimulationFidelity` when it is
built. No placeholder exists today; `Plant::commissioned_on` is the only seam.

### 5.4 Network and frequency model (Milestone 1)

- **Copper plate.** Every plant and city connected to the grid shares a single bus with unlimited
  line capacity. Lines matter only for connectivity. Unconnected plants contribute nothing, and
  unconnected cities are not customers.
- **Frequency** is a *game-level abstraction*, tuned for readability at a 1-minute tick rather than
  for real sub-second physics:
  `Δf(t+1) = Δf(t) + k_inertia × imbalance / system_size − k_damping × Δf(t)`.
- **Protection bands** (region data):

  | Band | Effect |
  |---|---|
  | ±0.05 Hz | Normal |
  | Below −0.5 Hz | Under-frequency load shedding in 5% steps, spread proportionally across connected cities |
  | Below −1.5 Hz, or shedding for N consecutive ticks | **Blackout**: all plants trip to `Offline`, and the player must restart them |
  | Above +0.5 Hz | Solar curtailed first, then AGC plants pushed to minimum stable output |
- **Unserved energy is shed load only:** `served = demand × (1 − shed)` and `unserved = demand × shed`.
  A residual mismatch between supply and served load is a signed `imbalance` absorbed by system
  inertia and visible as frequency deviation; it is neither unserved nor paid for. Unserved energy
  is accumulated and drives reliability, strikes and lessons.

### 5.5 Domain events

Ticks and commands return `Vec<DomainEvent>`. Nothing is pushed through a framework.

- `PlantCommissioned`, `PlantStarting`, `PlantOnline`, `PlantStopped`, `PlantTripped`
- `CityConnected`, `LineCommissioned`
- `FrequencyDeviated { band }`, `LoadShed { mw }`, `SolarCurtailed { mw }`, `Blackout`
- `HydroBudgetExhausted { plant }`
- `LedgerEntryRecorded`, `StrikeIssued`, `StrikeExpired`
- `RateReviewCompleted { tariff, allowed_return }`, `CapitalHeld { asset, amount }`,
  `CapitalReleased { asset, amount }`, `CapitalDisallowed { asset, amount }`
- `PhaseChanged`, `MonthClosed(MonthlyReport)`, `Bankrupt`, `LicenseRevoked`, `MilestoneReached`

### 5.6 Domain policies (event → reaction, inside the domain)

- **Regulator (reliability):** `MonthClosed` with unserved energy above the threshold →
  `StrikeIssued`. Three active strikes → `LicenseRevoked`.
- **Regulator (rate review):** `MonthClosed` for December → compute the revenue requirement,
  the PBR-adjusted return and the forward-looking reserve assessment → hold, release or disallow
  the affected capital portions, then `RateReviewCompleted` using the eligible rate base.
- **Solvency:** cash below zero at `MonthClosed` for 3 consecutive months → `Bankrupt`.

### 5.7 Determinism (a hard invariant)

- All randomness comes from the session's seeded RNG.
- No wall-clock time.
- No iteration order that depends on a `HashMap`; use `BTreeMap`, or `Vec` indexed by ID.
- Single-threaded tick.
- The same seed, settings, region content, simulation rules version and command log must produce
  an identical state hash on the same build and platform (libm transcendentals such as `ln`, `cos`,
  `sin`, `exp` and `hypot` may differ in the last bit between targets). Replay verifies these inputs
  before executing any commands (§7).

## 6. Application layer (`crates/application`)

### 6.1 Commands (write side)

Every command is a plain struct handled as
`handle(&mut GameSession, cmd) -> Result<Vec<DomainEvent>, CommandError>`.

| Command | Phases |
|---|---|
| `StartNewGame { region, seed }` | n/a (creates the session from a `RegionDefinition`) |
| `BuildPlant { spec, cell }`, `BuildLine { from, to }`, `CommitPlant { plant, hours }` | Planning |
| `BeginOperating` | Planning → Operating |
| `SetDispatchTarget { plant, mw }`, `SetAutoFollow { plant, on }`, `StartPlant { plant }`, `StopPlant { plant }` | Planning, Operating |
| `AdvanceTicks { n }` | Operating |
| `FinishReview` | Review → the next month's Planning |

**Commitment is a daily pattern that repeats throughout the month.** Treat the 24 hourly flags as
a circle. `CommitPlant` validates every off-gap between committed runs, including the gap across
midnight. Each must fit both shutdown and start-up. Since the pattern does not constrain the
plant's output at shutdown, reserve the worst-case shutdown duration
`ceil(capacity / ramp_per_tick)` ticks, then `startup` ticks. An insufficient gap returns
`CommitmentInfeasible { gap, shutdown, startup }`, without changing the session. A gap equal to the
required sum is valid. All-day and all-off patterns have no daily restart and are valid; nuclear's
48 h start-up therefore makes it must-run for the month or off for the month.

**Initial state comes from the pattern.** At `BeginOperating`, a plant committed at hour 0 is
already online at its selected initial target (minimum stable output by default). An off plant
whose next run needs a start before midnight begins in `Starting` with the implied remaining
ticks. Shutdown tails crossing midnight are initialised conservatively from capacity and their
elapsed ramp-down time. Other dispatchable plants begin offline. This boundary initialisation is
not an operating tick, and does not carry a 48 h start-up between calendar months. Solar availability
and hydro budgets are initialised independently as before.
Frequency starts at nominal and protection at normal for the new representative day; the previous
month's incidents and reliability totals remain in its report and company/regulator history.

**Start-up charges have two weights.** Each cyclic off→on run has one recurring daily restart,
including a start begun before midnight; charge `daily_starts × days_in_month`. A plant returning
from a whole month offline also incurs a single month-boundary start only when that start is not
already represented by a recurring restart (notably an all-day pattern). Charge
`startup_cost × (daily_starts × days_in_month + boundary_starts)`. Do not count synthetic
initialisation events a second time. The first game's starter fleet is treated as established;
newly commissioned plants are treated as previously offline. A manual restart within the played
day is a recurring event and receives the daily weight.

**Manual commands respect the horizon.** During Operating, a start with
`current_tick + startup_ticks > 1,440` returns `StartUpBeyondDay { remaining, startup }` unchanged;
one that finishes exactly at midnight is allowed. A manual stop or protection trip suppresses that
plant's automatic commitment actions for the rest of the representative day, so it cannot be
immediately auto-started again. These operating overrides do not rewrite the validated base pattern
or carry into the next month. During Planning, `StartPlant` selects the all-day pattern and
`StopPlant` selects all-off; neither advances physical time. Planning target changes set the initial
target. `BeginOperating` freezes the month's pattern and initialises its implied state.

### 6.2 Command pipeline (cross-cutting layers)

```
PlayerCommand ─▶ TracingLayer ─▶ InvariantCheckLayer* ─▶ CommandLogLayer ─▶ handler ─▶ events ─▶ projections
```

Each layer implements the same `CommandHandler` trait. They are assembled in one readable
`build_pipeline()`. `*` marks `InvariantCheckLayer`, which is active only in debug and test builds.

### 6.3 Queries and projections (read side, CQRS)

The presentation layer never reads aggregate internals.

- **Direct queries** (cheap snapshots): `GridSnapshot`, `PlantView`, `CityView`, `ForecastView`,
  `CompanyView`.
- **Projections** (updated from events and per-tick summaries):
  - `TimelineProjection`: a rolling 24 h of supply by plant type, demand, frequency and price.
  - `MonthlyReportProjection`: P&L, the merit order used, incidents and unserved energy.
  - `PlantHistoryProjection`: starts, trips, energy and revenue per plant.
  - `MilestoneProjection`: progress toward milestones.
  - `RateReviewProjection`: a running estimate of the next review (projected allowed return,
    current reserve margin, held capital, capital released, and capital at risk of a second-review
    write-off), so the player can see the consequences before December.
  - `RampWarningProjection`: compares the next 60 minutes of forecast net load with the committed
    *ramp capability* and produces warnings such as "Sunset ramp: solar −6.2 GW in 40 min; fast
    capacity covers 4.9 GW".
  - `LessonsProjection`: builds 2–3 plain-language lessons per month from the above, as typed
    `Lesson` values that the presentation layer localises.

### 6.4 The tick (owned by `GameSession::tick`, in a fixed order)

1. Require an active game in `Operating` (`GameOver`, then `WrongPhase`), even for
   `AdvanceTicks { n: 0 }`.
2. Compute actual demand for connected cities (forecast × (1 + forecast error)).
3. Compute solar and hydro availability; withhold the standing solar curtailment. At hour
   boundaries, re-plan the hydro schedule, and cap auto-follow hydro at its allocation for the
   current hour (§12).
4. Apply scheduled start/stop actions for the current interval, respecting operating overrides.
5. Plants progress (start-up; ramp toward target; hydro budget use). Construction completes at
   month close.
6. AGC re-targets auto-follow plants.
7. Imbalance = supply − served load.
8. Frequency update.
9. Protection (shed, curtail or blackout). The `TickSummary` reports the post-protection state.
10. Accounting: accumulate energy per plant (MWh), recurring daily starts, month-boundary starts
    and *served* energy. Nothing is priced per tick. At month close, apply each total's weight
    (§6.1) and **round to cents once per ledger entry** (round-half-even), so money never accumulates
    fractional cents. Retain only the whole-month offline summary for next month's start charging.
11. Advance the clock and collect events. After tick 1,440: the month's totals are weighted,
    `MonthClosed`, policies run, and the phase moves to Review.

### 6.5 Ports

- `RegionCatalog`: `list() -> Vec<RegionSummary>` and
  `load(RegionId) -> Result<LoadedRegion, RegionLoadError>`. `LoadedRegion` carries the validating
  domain definition, source bytes and opaque fingerprint from the same read (§7.1).
- `CommandLogSink`: `initialize(&LogHeader, region_source: &[u8])` archives the source and records
  the header; `append(&LogRecord)` records subsequent commands. Both return typed errors.
- (Later) `SnapshotStore`.

## 7. Infrastructure (`crates/infrastructure`)

- **`RonRegionCatalog`**: reads `assets/regions/*.ron` into serde **DTOs**, then maps them through
  the domain's validating constructors ("parse, don't validate"). Errors carry file and field
  context.
- **`FileCommandLog`**: writes the seed plus commands, one RON record per line, to a path supplied
  by the caller, with the replay contract below.
- The region data is authored at a coarse cell resolution (on the order of 100 × 60 cells per
  region), with cities as named points and hand-tuned resource layers. Geographic accuracy follows
  real interconnection boundaries at the level of a stylised diorama, not GIS.

### 7.1 Replay inputs and region archive

- The header records `log_format_version`, `SIM_RULES_VERSION`, the full `GameSettings`, seed,
  region ID for display, and `region_fingerprint`. Bump `SIM_RULES_VERSION` whenever changes to
  rules, RNG algorithms, numerical behaviour or region mapping alter outcomes.
- The fingerprint is SHA-256 of the **exact source bytes** of the self-contained region RON file.
  This deliberately treats formatting changes as different content too. Infrastructure computes
  it; the domain gains no I/O, serde or hashing dependency. Region files may not depend on external
  includes or mutable side inputs. The same bytes must supply the parsed `RegionDefinition`, the
  fingerprint and the archive; do not re-read the source separately for each purpose.
- Before accepting commands, save a byte-for-byte copy beside the log as `<log-name>.region.ron`,
  then write the header. Use exclusive creation; never overwrite an archive. Failure to archive or
  initialise the header fails new-game creation with a typed error. The catalog passes source bytes
  and the fingerprint alongside the domain definition through an application-owned load result.
- On replay, validate the log format, rules version and region bytes before creating a session or
  running commands. Unsupported formats return `LogFormatVersionMismatch`; incompatible rules
  return `RulesVersionMismatch`; different bytes return `RegionFingerprintMismatch`. Missing or
  malformed archived files return typed load errors. Never substitute current shipped data silently.
- Replay normally uses the archived region file, so retuning shipped data does not break recovery.
  Explicitly supplying a different region file still performs the fingerprint check. A region
  archive does not migrate old simulation rules: replay after a rules change requires a matching
  engine version or a future explicit migration. The Milestone 1 test harness verifies replay;
  player-facing save/load remains out of scope.

## 8. Presentation (`crates/game`)

### 8.1 Plugins

| Plugin | Responsibility |
|---|---|
| `AppStatePlugin` | `AppState { Boot, MainMenu, RegionSelect, InGame }`. The `DayPhase` sub-state inside `InGame` **mirrors** the domain one way (domain → Bevy). |
| `SimulationPlugin` | The `Session` resource; applies `PlayerCommand` messages through the pipeline; `FixedUpdate` runs `AdvanceTicks { n: speed }`; re-emits domain events as Bevy messages; holds the projections. |
| `WorldViewPlugin` | The terrain mesh, generated from cells (low-poly, vertex-coloured, with height); procedural low-poly plant models; city block clusters sized by demand; `ViewSync` keeps a `PlantId/CityId/LineId → Entity` mapping driven by events. |
| `OverlayPlugin` | `OnEnter(Planning)` turns on the terrain/resource overlay; `OnEnter(Operating)` turns on the control-room overlay; a hotkey hides the overlay. |
| `CameraPlugin` | Orbit, pan and zoom, plus a tilt-shift depth-of-field effect. |
| `HudPlugin` | The egui panels (§8.4). They read only query views and projections, and they write only by sending `PlayerCommand`s. |
| `LocalizationPlugin` | Fluent bundles; translates typed events, errors and lessons into display text. |
| `DevToolsPlugin` (`dev` feature) | The bevy-inspector-egui world inspector, plus a reflected `TuningConfig` resource for live balancing. |

### 8.2 Art direction

- **Base view:** a warm, toy-like tabletop diorama (low-poly, soft shadows, tilt-shift).
- **Planning overlay:** a grounded terrain and resource view (irradiance, cooling water, hydro
  sites, siting suitability).
- **Operating overlay:** a dark control-room hologram (glowing plant output pillars, city lights
  showing supply status, energised lines).

### 8.3 Honesty under copper plate

The operating overlay shows **plant output** and **city supply status** (lit, or flickering when load
is shed). Lines show only an "energised" pulse. It shows no per-line loading until a power-flow
`NetworkModel` exists.

### 8.4 HUD by phase

- **All phases:** a top bar with month and year, season, phase, cash, reliability and satisfaction.
- **Planning:** a build menu (plants and lines), a cell inspector (resources and suitability), a
  tariff panel (current tariff, projected rate review, reserve margin), and a commitment grid with the coming month's forecast band and stacked committed baseload.
  The commitment grid is the primary planning tool.
- **Operating:** speed controls; a frequency gauge with the current imbalance; a selected-plant panel
  (output, target, ramp, cost, AGC toggle, start/stop); a merit-order stacked supply chart against
  actual and forecast demand; predictive ramp warnings.
- **Review:** a modal over the dimmed diorama showing P&L, the merit order used, an incidents
  timeline and the lessons.
- **Errors:** a `CommandError` becomes a localised toast. This is the only place errors are mapped
  for display.

## 9. Cross-cutting concerns

| Concern | Approach |
|---|---|
| Observability | A `TracingLayer` span per command (command, outcome, emitted events); `#[tracing::instrument]` on handlers; Bevy's Tracy feature for frame profiling. The domain stays free of logging. |
| Persistence | Command sourcing with a versioned header, settings, seed, region fingerprint and adjacent source archive (§7.1). `CommandLogLayer` records commands through `CommandLogSink`; aggregates are persistence-ignorant. Snapshots and player-facing save/load come later. |
| Error handling | Typed `Result`s at every layer (`DomainError` → `CommandError` with `thiserror`). The domain never panics on player input; `panic!` and `debug_assert!` are only for bugs. Errors are mapped for display once, at the Bevy boundary. |
| Data integrity | Types first: newtypes, validating constructors and state enums. Debug and test builds run `InvariantCheckLayer` after each command or tick (on the post-protection `TickSummary`: generation = Σ by_plant = Σ by_kind = served + imbalance, 0 ≤ served ≤ demand, curtailed ≥ 0, and generation matches the connected plants' outputs; cash change = ledger sum; plant bounds). |
| Localisation | The domain never produces user-facing text, only typed events, errors and lessons with parameters. Fluent `.ftl` files live under `assets/locales/en-US/`, and region data refers to translation keys. Milestone 1 ships en-US only. |

## 10. Testing strategy

### 10.1 Layers

| Layer | Where | Tooling |
|---|---|---|
| Unit | Inline `#[cfg(test)]` in `domain` and `application` | std |
| Property-based | `domain`, `application` | `proptest`, `proptest-state-machine` |
| Integration and scenario | `application/tests/`: command scripts against small fixture regions built in code | std, `insta` |
| Adapter | `infrastructure/tests/` (parses both *shipped* region files); `game/tests/` (headless `App` with `MinimalPlugins`: command funnel, `ViewSync`, phase mirroring) | std |
| Mutation | `domain`, `application` | `cargo-mutants` |
| Runner | Everything | `cargo-nextest` |

### 10.2 Key properties

- **Ramping:** for any sequence of targets, the per-tick change in output stays within the ramp
  rate, and online output stays between minimum stable output and capacity.
- **Merit order:** AGC never leaves a cheaper auto-follow plant with headroom while raising a more
  expensive one, and the reverse holds when lowering.
- **Conservation:** each `TickSummary` is the post-protection state, with generation = served +
  imbalance (signed), served = demand × (1 − shed), unserved = demand × shed, and
  0 ≤ served ≤ demand. `curtailed` is solar availability withheld (never generated), so it is not
  part of generation.
- **Money:** the change in cash always equals the sum of the ledger entries, exactly.
- **Commitment:** every cyclic off-gap fits worst-case shutdown plus start-up; all-day/all-off are
  valid; invalid patterns leave the session unchanged. Test cross-midnight starts/shutdowns and
  exact-fit gaps. Nuclear is all-day or all-off, never carried in `Starting` across months.
- **Weighting:** daily energy and restarts use `days_in_month`; month-boundary starts are charged
  once, without double-counting a recurring restart. Exactly one rounding step applies per entry.
- **Hydro:** the energy a hydro plant produces in a played day never exceeds that day's budget. The
  peak-shaving schedule's properties are in §12.7.
- **Rules-version guard:** a fixed scripted month's outcome digest (report and ledger figures, not
  `Debug` output) is snapshotted together with `SIM_RULES_VERSION`, so an outcome change without a
  version bump fails review.
- **Determinism:** the same verified replay inputs and commands produce an identical state hash.
  Mismatched rules, format or region bytes fail before any command; the archived file replays after
  shipped data changes.
- **Robustness:** any random sequence of commands (state-machine test) never panics, and all
  invariants hold afterwards.
- **Parsing:** malformed region DTOs produce typed errors, never panics.
- **Rate review:** the new tariff × the past year's served energy = the revenue requirement (within
  one cent); the allowed return always stays within the PBR band. Reserve assessment uses the higher
  of actual and forward coincident peaks, including December connections. Held capital earns no
  return, useful held portions are released, and only portions excess at two consecutive reviews
  are written off, bounded by their remaining book value. Repeated estimates do not age holdings.

### 10.3 Balance snapshots

The canonical scenarios ("East winter peak", "West duck-curve day", "Overreach: city connected
without capacity", "Overbuild: rate review with disallowance") snapshot their `MonthlyReport`s with `insta`. Tuning changes then appear as
reviewable diffs.

### 10.4 Mutation testing

`cargo-mutants` runs on `domain` and `application` on demand and in a scheduled CI job, not on every
commit. The target is zero surviving mutants in `domain`. Any exception is justified in a skip
annotation. The `game` crate is excluded.

### 10.5 Isolation and concurrency (all I/O tests)

- No fixed paths. Each test gets its own `tempfile::TempDir`, and fixtures are copied in, never
  shared.
- No process-global mutable state (`static mut`, environment variables, current-directory changes).
  Configuration is passed explicitly through ports.
- Any future database follows the same rule: one database per test (in-memory or a temp-file
  SQLite), never a shared schema.
- `cargo-mutants` runs build copies in parallel, so hermetic tests are required.

### 10.6 Process

Test-driven development throughout: failing test → minimal code → refactor.

## 11. Future seams (designed for, not built)

| Future feature | Seam in Milestone 1 |
|---|---|
| Transmission congestion | A new `NetworkModel::DcPowerFlow` variant; a settings toggle once it exists |
| Equipment ageing and maintenance | `Plant::commissioned_on`; a new field in `SimulationFidelity` |
| Save/load and replays | Versioned command log, settings, seed, region fingerprint and archived region bytes; a `SnapshotStore` port |
| More regions | A new `.ron` file; the picker already appears for two or more regions |
| More plant types (wind, storage) | `PlantSpec` is data-driven; new behaviour goes in the `generation` module |
| More languages | New `assets/locales/<lang>/` folder |

## 12. Amendment A1: Hydro peak-shaving schedule (2026-10-18)

### 12.1 Why

Hydro has a daily water budget but costs $0/MWh to run, so cheapest-first AGC raises it first and
runs it flat out from midnight. Whenever site potential × monthly flow is below 1, the budget runs
out partway through the day. The plant then drops to almost nothing within a tick, usually during
the evening peak. In the Western region in January this happened at 20:24, and the region's data had
to be inflated to keep the smoke test passing.

This teaches the opposite of reality. Real operators treat stored water as having a **water value**:
the cost of the expensive generation it can displace. So they spend it on the highest-load hours.
Hydro is what grids such as the Pacific Northwest use to ride the evening ramp.

### 12.2 Goals, non-goals, anti-goals

- **Goals:**
  - Under normal operation, auto-follow hydro spends its daily water on the day's highest forecast
    net load ("peak shaving").
  - Hydro never runs dry abruptly while the grid is healthy.
  - The schedule's water level is visible, so the player can learn from it (§12.6).
- **Non-goals (still out of scope):**
  - water carried over between days or months (reservoirs);
  - river cascades between plants;
  - water value expressed in $/MWh;
  - a player-facing allocation editor. The schedule is designed so that one can be added later.
- **Anti-goals:**
  - hydro sitting idle while the grid sheds load;
  - invented tuning knobs, such as a fixed "peak window" parameter. The schedule must follow from the
    forecast alone.

### 12.3 The schedule (water filling)

1. **Inputs.** For each hour `h` of the remaining day, compute the forecast **net load**: the
   connected cities' forecast demand at minute 30 of `h` (the same mean as the hourly forecast)
   minus expected solar at minute 30 of `h` (mean clearness, as in `forecast_net_load`). It is
   deterministic and draws no RNG.
2. **Plants.** Schedule every connected, commissioned hydro plant with AGC on, but only over the hours
   its commitment pattern commits it. Uncommitted hours get an allocation of 0.
   Fill them **in plant-id order against the residual**. The residual starts as net load, and each
   plant's allocation is subtracted before the next plant is filled.
3. **Fill.** A plant has remaining budget `E` (MWh), capacity `C` (MW) and remaining hours `H`. Find
   a water level `λ` such that Σ_{h∈H} clamp(residual_h − λ, 0, C) × 1 h = E. The plant's allocation
   for hour `h` is `clamp(residual_h − λ, 0, C)`. If even λ = min_h(residual_h) − C cannot use all
   of `E` (the plant can't usefully run harder), allocate `min(C, max(0, residual_h))` and let the
   rest of the water spill. Find `λ` by bisection with a fixed iteration count, so the result is
   deterministic.
4. **Unplanned plants.** A hydro plant with no allocation for the current hour is budget-limited
   only (uncapped) until the next hourly re-plan. This covers a plant switched to AGC mid-hour, a
   newly connected plant, and a plant restarted after a trip. Commands never trigger a re-plan
   directly. Connectivity is read at plan time, with no look-ahead for lines still under
   construction. *(Builder's call: being uncapped avoids idling water while the grid may be short;
   re-planning on commands would complicate command-sourced replay for little gain.)*
5. **Re-planning (receding horizon).** Plan at `BeginOperating` over hours 0–23 with the full budget.
   Re-plan at every hour boundary (`tick % 60 == 0`) over the remaining hours, using the remaining
   budget. Water saved because AGC ran hydro below its allocation, or spent in an emergency (§12.4),
   is redistributed over the rest of the day.

### 12.4 How the schedule is used

- **Normal operation.** For auto-follow hydro, the allocation for the current hour caps **AGC's
  headroom and target**: AGC never sets a scheduled hydro plant's target above its allocation. It
  is **not** a hard physical availability cap. The plant step keeps only the budget limit, so when
  the allocation falls at an hour boundary, hydro ramps down at its ramp rate rather than dropping
  in one tick. *(Builder's call: a hard cap would recreate the cliff at every falling hour boundary,
  where three 800 MW units could lose 2.4 GW in one tick.)* The day-boundary dispatch (Ruling R21)
  uses the hour-0 allocation as its upper bound.
- **Emergency override.** The allocation cap is lifted, leaving only the budget limit, whenever the
  frequency band is `Shedding` **or** any load is shed (`shed > 0`, in either grid mode). *(Builder's
  call: keying on `shed > 0` as well as the band keeps the cap off for the whole restoration, rather
  than flapping on and off as the band recovers.)* Avoiding load
  shedding is worth more than any later hour. The next hourly re-plan spreads whatever water remains.
- **Manual hydro.** Hydro with AGC off is neither scheduled nor capped by the schedule. It follows
  the player's target, limited by its budget as before. *(Builder's call: manual hydro output is not
  subtracted from the residual, because the player can change it at any time.)*
- Hydro still sorts at $0/MWh in the merit order, so AGC raises it first, but only up to the cap.
- `HydroBudgetExhausted` is still emitted when a budget reaches zero, which can now happen only
  through emergency use or manual operation.

### 12.5 Data and versioning

- Remove the Western region's winter-flow boost, which was added only to work around the cliff.
  Re-author all twelve monthly flows to a realistic Pacific Northwest hydrograph: a spring freshet
  peaking in May and June, lows from late summer to autumn, and moderate winter flows of about 0.85
  in January and 0.8 in December. *(Builder's call: the earlier boost's exact per-month values were
  never recorded, so all twelve are re-derived.)*
- If a smoke run then misses its reliability target, retune region **data** (fleet mix and size,
  flows) and never the schedule rule. This is the same rule as Task 22.
- Add a late-year Western smoke run (October): reliability ≥ 0.99, and no evening hydro cliff.
- Outcomes change, so bump `SIM_RULES_VERSION` to 2. Regenerate the rules-version guard and the
  scenario snapshots deliberately, reviewing each diff.

### 12.6 Visibility (read side)

`GameSession::hydro_schedule() -> HydroScheduleView` reports, per scheduled plant, the water level
`λ: Option<Megawatts>`, the 24 hourly allocations and the remaining budget.
- `λ` is `None` when nothing is allocated (no water, no committed hours) or when water spills (§12.3,
  step 3). There is no water level in either case.
- Each elapsed hour shows the allocation as it stood when that hour began, so a past hour is never
  rewritten by later re-plans. Future hours show the latest plan.

*(Builder's call: freezing elapsed hours lets the HUD and lessons compare plan with actual.)* It is for the Plan 2 HUD ("hydro covers
everything above 31 GW") and for lessons. *(Builder's call: until the HUD exists, no projection or
lesson is added.)*

### 12.7 Properties (tests)

- **Water conservation.** Σ allocation × 1 h ≤ remaining budget, and it equals
  `min(E, Σ_h min(C, max(0, residual_h)))` within tolerance.
- **Bounds.** 0 ≤ allocation_h ≤ C for every hour.
- **Water-filling optimality.** For any hours `i` and `j`, if allocation_i < C and allocation_j > 0,
  then residual_i − allocation_i ≤ residual_j − allocation_j + ε. In other words, water is never
  spent on a lower hour while a higher hour is left uncovered.
- **Monotonicity.** If residual_i ≥ residual_j, then allocation_i ≥ allocation_j.
- **The cap is not a cliff.** When an hour's allocation falls, scheduled hydro output falls no faster
  than the ramp rate. Because of these ramp tails, an hour's actual energy may exceed its allocation;
  the daily budget is never exceeded.
- **Determinism.** The same inputs give the same schedule, with no RNG draws. Re-planning never
  schedules more than the remaining budget.
- **End to end.** With realistic Western January flows, hydro output stays above 0 through 20:00–21:00
  whenever it started the day with water. The reliability of the existing smoke runs does not fall.

## 13. Amendment A2: Proration of the partial-year rate review (2026-10-18)

### 13.1 Why

The annual review adds a **full year's** return on the rate base to the revenue requirement, but
divides by the energy served over only the months actually tallied. Depreciation, operating cost and
energy already cover the actual months. So a first review period shorter than twelve months
over-recovers return by a factor of 12 ÷ months. A game starting in April (the test fixture)
over-recovers by about a third: the rules-version guard shows the tariff jumping from $95 to
$103.87.

The shipped regions start in January, so play is unaffected today. Requiring January starts would
hide the formula's assumption behind a data rule, and any future mid-year scenario would bring the
bug back.

### 13.2 Rule

- `YearTally` gains `months: u8`, the number of months recorded since the last review (or since the
  start of the game). `record_month` increments it and `apply_rate_review` resets it with the rest
  of the tally.
- The return term becomes
  `round_ties_even(allowed_return × eligible_rate_base × months ÷ 12)`, rounded once in cents, as
  before. Everything else in `compute_rate_review` is unchanged, including PBR return adjustment,
  disallowance and the tariff formula.
- `months` is between 1 and 12 whenever a review runs (a review is triggered by December's close).
  Treat 0 as "no return" rather than as a panic.
- The rate-review estimate (Task 19) shares the same input assembly as the December review (`GameSession::review_inputs`, added in the final fix wave), so it
  prorates automatically. A mid-year estimate reflects the months tallied so far.

### 13.3 Properties (tests)

- **A full year is unchanged:** a 12-month tally gives exactly the same outcome as before.
- **Linear:** the return term for k months equals the 12-month term × k/12, within one cent.
- **No over-recovery at an April start:** a fixture starting in April, with otherwise steady
  months, sets the same first-review tariff (within one cent) as a game whose first year starts in
  January.
- **Versioning:** outcomes change for non-January starts, so this lands under the same
  `SIM_RULES_VERSION` bump to 2 as Amendment A1, with the snapshot diffs reviewed together.
