# Megawatt Mogul: Design Spec (Milestone 1)

- **Date:** 2026-10-18
- **Status:** Draft for review
- **Amendments:** A1, a hydro peak-shaving schedule (§12, 2026-10-18); A2, proration of the
  partial-year rate review (§13, 2026-10-18); A3, over-frequency protection, a minimum-generation
  warning and frequency-performance strikes (§14, 2026-10-18); A4, rotating outages and staged
  under-frequency shedding (§15, 2026-10-19); A5, the inherited schedule, the shortfall
  confirmation, window and panel sizing, the city card and
  construction progress (§16, 2026-10-20); A6, save/resume before Plan 3 (§17, 2026-10-20);
  A7, faster expansion pacing (§18, 2026-10-22)
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
   selectable speed: pause, 1×, 2×, 4×, 8×, 16× or 32×. At 1× a day takes about 6 real minutes
   (about 23 seconds at 16× and 11 seconds at 32×). The player
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
- **Build times are counted in months** (= played days, §3.2). They are compressed for tycoon
  pacing (§18), while nuclear and hydro remain longer-term bets than gas or solar.
- One game year takes about 72 minutes of operating at 1×.

### 2.4 Service territory and growth

The player does not start out operating an entire interconnection. Each region defines a
**starting service territory**: a few connected cities and a starter fleet, with one of those
cities designated the **home city**. Its ordinary city node anchors network connectivity. The
home city is Columbus in the East and Seattle in the West. It remains a normal demand centre,
with the same city marker, selection and line-building controls as other cities.
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
  exceeds a threshold share of that month's demand (default 0.5%, region data), **or** frequency
  spends too long outside its performance band (Amendment A3, §14.4). A frequency-only failure
  first draws a **notice** (at most one per rolling 12 months) before it can strike. A month earns
  at most one strike. One strike expires after each 3 consecutive clean months, and a clean month must pass
  both tests.
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
  Lines with no plant endpoint (city-to-city) are never held.
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

The picker shows a North America overview with each available region coloured from its region
file's geographic outline. Hovering a region highlights it and its labelled card; clicking the
region starts a game with the entered seed. Excluded areas and ocean are not selectable. Each
card also offers a keyboard-accessible start button, a short regional description and the home
city. Map labels follow the UI's 4.5:1 text-contrast floor. Custom regions without a geographic
preview remain selectable through their cards; broken entries retain their disabled/error state.

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
- Command logging with versioned replay inputs and an archived region file (§7).
- Player-facing manual saves, month-close autosaves and resume, implemented before Plan 3 (§17).
- Localisation scaffolding with en-US only.

### 3.2 Plant types (shipped-region tuning, stored as region data and not in code)

| Type | Unit size | Min stable | Ramp | Start-up | Start-up cost | Marginal cost | Capital cost | Build (months) | Lesson |
|---|---|---|---|---|---|---|---|---|---|
| Nuclear | 1,100 MW | 70% | 5 MW/min | 48 h | $1.0M | $10/MWh | $2.4B | 18 | Commitment is a promise |
| Coal | 600 MW | 40% | 6 MW/min | 8 h | $150k | $30/MWh | $0.7B | 8 | Cheap baseload, start-up costs |
| Combined-cycle gas (CCGT) | 500 MW | 30% (40% in the test fixture) | 25 MW/min | 2 h | $40k | $45/MWh | $0.3B | 4 | The flexible middle |
| Gas peaker | 200 MW | 20% | 40 MW/min | 10 min | $5k | $150/MWh | $0.1B | 2 | Pay dearly for agility |
| Solar | 300 MW | 0% | n/a (weather) | n/a | n/a | $0/MWh | $0.15B | 2 | The duck curve |
| Hydro | 800 MW | 0% | 100 MW/min | 5 min | $1k | $0/MWh | $1.2B | 12 | Energy-limited dispatch |

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
- Fast-load aggregate snapshots, cross-platform replay and migration of incompatible saves.
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
- `infrastructure` depends on `application` and `domain` (plus `serde`, `ron`, and `tempfile`
  for atomic save-file publication as well as tests).
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
| tempfile | 3.27 | infrastructure, dev-dependencies |
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
  topology (nodes and lines; connectivity is computed from the starting home city), system
  frequency and `NetworkModel`. It is one aggregate because balancing has to see all of it in the
  same tick. It includes these domain services:
  - **`MeritOrder`**: committed plants sorted by marginal cost.
  - **AGC** (automatic generation control): moves auto-follow plants' targets to close the
    imbalance, raising the cheapest plant first and lowering the most expensive first, within ramp
    limits. After a system-wide trip it automatically restarts eligible units (§19).
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
- **Home-city anchor.** Region data specifies `start.home_city: CityId`, which must identify an
  existing city in `start.connected_cities`; otherwise loading returns
  `RegionError::InvalidHomeCity(CityId)`. The home city's existing node is the network root and is always connected. At game creation, completed lines join it to every other
  starter city and every starter plant. There is no extra root node, self-line or separate anchor
  asset. Paths of completed lines connect additional cities and plants, and may pass through any
  city or plant. The anchor establishes connectivity; generation still comes from connected plants.
  Startup lines enter the legacy rate base once, at their actual home-city distances. The map,
  legend and endpoint picker contain only cities and plants; the home city is listed once, first
  among line origins unless a selected city or plant takes precedence.
- **Frequency** is a *game-level abstraction*, tuned for readability at a 1-minute tick rather than
  for real sub-second physics:
  `Δf(t+1) = Δf(t) + k_inertia × imbalance / system_size − k_damping × Δf(t)`.
- **Protection bands** (region data):

  | Band | Effect |
  |---|---|
  | ±0.05 Hz | Normal |
  | Below −0.5 Hz | Under-frequency load shedding in 5% steps, spread proportionally across connected cities; staged relays drop one more step per 0.2 Hz of depth (Amendment A4, §15.4) |
  | Forecast shortfall | **Rotating outage:** a planned share of load is cut, sized hourly to the shortfall (Amendment A4, §15.3) |
  | Below −1.5 Hz, or shedding for N consecutive ticks | **Blackout**: all plants trip to `Offline`, and the player must restart them |
  | Above +0.5 Hz | Solar curtailed first, then AGC plants pushed to minimum stable output |
  | Above +1.5 Hz for N consecutive ticks | **Over-frequency trip:** the most expensive producing plant trips (Amendment A3, §14.2) |
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
  before executing any commands (§7). Across builds or platforms there is no promise, so a save
  load does not trust one: it replays and accepts the result only if it reproduces the recorded
  end state (§17.3).

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
plant's scheduled commitment actions for the rest of the representative day. System-wide AGC
black-start recovery (§19) can restart a unit interrupted by that trip; it never overrides a manual
stop or an individual over-frequency trip. These operating overrides do not rewrite the validated base pattern
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
  - `TimelineProjection`: a rolling 24 h of supply by plant type, demand, frequency and price,
    plus every completed played day's full 1,440 samples, labelled by its represented month.
    Capture that day's hourly forecast on entering Operating, before month-close commissioning
    can change connected demand. Archive on `MonthClosed`; later ticks never overwrite archives.
    Both live and archived data rebuild from the command journal on save/resume, with no change
    to the save format or simulation rules.
    *(Builder's call: under a regulated tariff the per-tick "price" the merit order teaches is the
    system marginal cost: `TimelinePoint.marginal_cost` is the highest marginal cost among the
    connected plants producing in that tick, `None` when none does.)*
  - `MonthlyReportProjection`: P&L, the merit order used, incidents and unserved energy.
  - `PlantHistoryProjection`: starts, trips, energy and revenue per plant. A plant's revenue is
    its weighted merit-order energy times the tariff in force that month, rounded once per plant
    per month; a read-side figure, not a ledger entry.
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
   boundaries, re-plan the hydro schedule (§12.3). The schedule is not an availability cap: it caps
   what AGC asks of auto-follow hydro in step 6, by the hour in which the targets take effect
   (§12.4).
4. Apply scheduled start/stop actions for the current interval, respecting operating overrides.
5. Plants progress (start-up; ramp toward target; hydro budget use). Construction completes at
   month close.
6. AGC re-targets auto-follow plants, with scheduled hydro capped as in §12.4.
7. Imbalance = supply − served load.
8. Frequency update.
9. Protection (shed, curtail or blackout, and an over-frequency trip of one plant, after which AGC
   re-targets again in the same tick; §14.2). The `TickSummary` reports the post-protection state.
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
- `SaveStore` for self-contained command checkpoints (§17); aggregate `SnapshotStore` comes later.

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
  The home-city topology and region schema use rules version 6: node/line identities, distances,
  construction quotes and legacy line capital change, so older logs require their matching engine.
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
  engine version or a future explicit migration. Player-facing save/resume (§17) packages these
  inputs into atomic checkpoints and adds end-state verification; the recording build and
  platform are kept for diagnostics.

## 8. Presentation (`crates/game`)

### 8.1 Plugins

| Plugin | Responsibility |
|---|---|
| `AppStatePlugin` | `AppState { Boot, MainMenu, RegionSelect, InGame }`. The `DayPhase` sub-state inside `InGame` **mirrors** the domain one way (domain → Bevy). |
| `SimulationPlugin` | The `Session` resource; applies `PlayerCommand` messages through the pipeline; `FixedUpdate` runs `AdvanceTicks { n: speed }`; re-emits domain events as Bevy messages; holds the projections. |
| `WorldViewPlugin` | The terrain mesh, generated from cells (low-poly, vertex-coloured, with height); procedural low-poly plant models; city block clusters sized by demand; `ViewSync` keeps a `PlantId/CityId/LineId → Entity` mapping reconciled from query snapshots after commands and session loads. |
| `OverlayPlugin` | `OnEnter(Planning)` turns on the terrain/resource overlay; `OnEnter(Operating)` turns on the control-room overlay; a hotkey hides the overlay. |
| `CameraPlugin` | Orbit, pan and zoom, plus a tilt-shift depth-of-field effect. |
| `HudPlugin` | The egui panels (§8.4). They read only query views and projections, and they write only by sending `PlayerCommand`s. |
| `LocalizationPlugin` | Fluent bundles; translates typed events, errors and lessons into display text. |
| `DevToolsPlugin` (`dev` feature) | A bevy-inspector-egui inspector filtered to render entities, plus reflected visual `TuningConfig` scales/colours. Simulation tuning remains region data for a new session. |

### 8.2 Art direction

- **Base view:** a warm, toy-like tabletop diorama (low-poly, soft shadows, tilt-shift).
  A terrain cell is one world unit. Fit each city cluster and plant model, including its base,
  inside a centred 0.8 × 0.8-cell footprint; scale all three axes together to preserve silhouettes.
  Demand may shrink a city's model relative to the largest city, but never enlarge it beyond this
  footprint. Visual tuning may shrink models (40–100%) and cannot expand them outside their cell.
  Selection outlines occupy at most 0.9 × 0.9 of a cell and sit just above the terrain. Rendering
  and picking use the same fitted geometry; labels and output pillars follow its actual roof height.
  Keep the default whole-region framing and allow close zoom to inspect individual cell-sized sites.
- **Planning overlay:** a grounded terrain and resource view (irradiance, cooling water, hydro
  sites, siting suitability). Sunshine uses a sequential purple/blue → teal/green → yellow scale
  with a visible 0–100% solar-potential key outside the map viewport. Use the same absolute scale
  in every region. This is the site's static game resource rating, not the percentage of sunny
  days or current weather; season, sun angle and cloud cover separately affect solar output.
- **Selectable map layers:** terrain, sunshine, cooling water, hydro potential, and suitability
  for nuclear, coal, solar and hydro. Omit gas (CCGT) and peaker suitability overlays: those
  plants accept any land cell, so separate overlays add no siting information. The cell inspector
  still reports suitability for every plant kind.
- **Operating overlay:** supply indicators over the warm, lit diorama: glowing plant output
  pillars, city lights showing supply status and energised lines. Retain terrain colours, building
  face shading and cast shadows; the overlay must not blacken the map or flatten the buildings.
- **Sunlight and shadows:** use the region's monthly sunrise/sunset times and the simulation
  clock. Sunlight comes from the east in the morning, south at solar noon and west in the evening;
  longer days have a higher illustrative northern-hemisphere arc. This is a representative regional
  sun, not per-cell astronomical positioning. Direction and intensity freeze when simulation time
  pauses and restore from saved simulation time. After sunset, disable solar light/shadows and
  retain readable ambient illumination. Planning previews solar noon for building visibility;
  Operating and Review use the actual clock. Resource overlays and the effects toggle do not
  change this lighting rule. No simulation tuning, solar generation or RNG changes.
- Click selects; left drag orbits; Shift + left drag or right/middle drag pans without orbiting
  or selecting. Scroll zooms about the visible terrain or object beneath the pointer, keeping that
  point fixed on screen within the camera's pan bounds. Scrolling over empty background zooms
  about the camera target. Zoom limits must not cause extra panning. Reset view/R fits the region.
  O hides/restores the overlay; map shortcuts
  operate while pointing at the map without taking keyboard focus from text entry. HUD panels,
  legends and modals capture their own input.
- Each phase restores its default overlay. Choosing a resource layer in Operating shows resource
  colours; Control room restores its hologram. Pillar height represents output/capacity. Effects
  add bloom (a soft glow around bright objects) and a subtle camera-focus blur, capped at three
  pixels. They default on. The button names its action explicitly: **Disable effects** while on,
  **Enable effects** while off, with a hover explanation. Remember this application preference
  across new/load games, phase changes and restarts; session reset must not reset it. Disabling
  removes bloom/DOF; unsupported depth-texture platforms skip DOF. HUD and backed labels remain
  outside post-processing. The perspective legend starts collapsed and can be opened explicitly;
  it scrolls within narrow maps. Narrow map areas use a compact resource-layer dropdown, leaving
  room for the terrain at increased UI zoom.

### 8.3 Honesty under copper plate

The operating overlay shows **plant output** and **city supply status** (lit, or flickering when load
is shed). Every connected city's lights show the same served fraction, dimming/flickering together;
there is no invented geographic rotation. Lines show only an "energised" pulse. It shows no
per-line loading until a power-flow `NetworkModel` exists.

### 8.4 HUD by phase

- **All phases:** a top bar with month and year, season, phase, cash, reliability and satisfaction.
- **Planning:** a build menu (plants and lines), a selected-object inspector, a
  tariff panel (current tariff, projected rate review, reserve margin), and a commitment grid with the coming month's forecast band and stacked committed baseload.
  The commitment grid is the primary planning tool. The inspector shows cell resources and
  suitability, city details, construction progress, or a commissioned plant's operating status,
  grid connection, capacity, minimum output, initial target, ramp rate, start-up time and cost,
  marginal cost and fixed monthly O&M. Solar sites show their solar potential; hydro plants show
  the month's daily water budget. Omit dispatch-only figures for solar.
- **Operating:** speed controls; a frequency gauge with the current imbalance; a selected-plant panel
  (grid connection, output/capacity, target, ramp, cost, energy and revenue, hydro water, AGC toggle,
  start/stop, and expandable specifications); a merit-order stacked supply chart against
  actual and forecast demand; predictive ramp warnings. Ramp warnings and Incidents share a
  column opening at 480 pixels, with a draggable divider. The column keeps at least 300 pixels
  and leaves at least 420 pixels for the Supply/Hydro content. The supply key wraps above the
  plot, outside its data area, so it never obscures the daily curve. Key entries retain click and
  keyboard show/hide controls, matching fill/line swatches and hover highlighting.
- **Plant connection:** every plant card, including construction cards, explicitly shows
  `Connected` or `Not connected` from `PlantView.connected`. This is independent of operating
  status and commissioning: an idle plant can be connected, a completed plant can be isolated,
  and an ordered line does not connect it before that line enters service. Connection appears
  near the top; longer specifications scroll without moving the surrounding panels.
  When increased UI zoom leaves less height than the Operating sidebar needs, the column itself
  scrolls so the full card and plant list remain reachable.
- **Review:** a modal filling the available window width below the top bar, with a 16-point outer
  margin. Both the report and retrospective use this full width. It shows P&L, the merit order
  used, an incidents timeline and the lessons. A pinned **Supply demand retrospective** button switches the modal to the complete
  finished-day graph; **Review summary** returns to the report. Continue (or Back after game over)
  remains visible in either view. Viewing a graph never advances or finishes the month.
- **Supply history:** History tabs in Planning and Operating revisit any completed month through
  a month selector, defaulting to the latest. Show that month's complete played-day supply stack,
  actual demand, served load, original forecast and marginal cost, using the same chart and key as
  the live day. These are daily MW/$ per MWh curves, not month-weighted energy totals. The History
  view in Operating uses the full bottom-panel width and omits today's incidents/ramp warnings
  alongside historical curves. Before the first close, explain that history needs a completed day.
- **Errors:** a `CommandError` becomes a localised toast. This is the only place errors are mapped
  for display.
- **Text contrast:** every visible UI text size uses a minimum **4.5:1** contrast ratio against
  its backing, measured with WCAG's sRGB relative-luminance formula, without rounding up. This
  includes secondary/help text, disabled controls, semantic status text, chart axes, legends and
  hover labels, and map glyphs and construction labels. The shared dark palette is applied before
  every screen draws. Disabled labels retain contrast; status hues are adjusted independently of
  the chart/marker palette. Text over terrain or data gets an opaque backing. Chart ticks appear
  fully legible or are omitted for spacing; opacity fades never make visible labels unreadable.
  Palette/state checks, property tests over arbitrary colour pairs and tests of actual paint
  output guard the requirement. See [WCAG 2.2 contrast guidance](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum).

## 9. Cross-cutting concerns

| Concern | Approach |
|---|---|
| Observability | A `TracingLayer` span per command (command, outcome, emitted events); `#[tracing::instrument]` on handlers; Bevy's Tracy feature for frame profiling. The domain stays free of logging. |
| Persistence | Command sourcing with versioned replay inputs (§7.1). A complete in-memory command journal supplies atomic, self-contained saves through `SaveStore` (§17); `CommandLogSink` remains the diagnostic recording adapter. Aggregates remain persistence-ignorant. Fast-load snapshots come later. |
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
reviewable diffs. *(Builder's call: under A3 the overbuild scenario is split in two. Committing
the new plants ends in licence revocation before the first post-commissioning review. Leaving them
uncommitted keeps the hold-then-disallowance case; see §14.5.)*

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
| Fast-load snapshots and save migrations | The verified command checkpoints of §17; a future `SnapshotStore` and explicit migration path |
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
   During Operating, a stopped or tripped plant whose commitment is suppressed reserves no water
   allocation until restarted manually or by AGC black start (Starting or Online). Unsuppressed plants with future
   scheduled starts still count. Planning previews ignore the previous day's physical overrides.
   Fill them **in plant-id order against the residual**. The residual starts as net load, and each
   plant's allocation is subtracted before the next plant is filled.
3. **Fill.** A plant has remaining budget `E` (MWh), capacity `C` (MW) and remaining hours `H`. Find
   a water level `λ` such that Σ_{h∈H} clamp(residual_h − λ, 0, C) × 1 h = E. The plant's allocation
   for hour `h` is `clamp(residual_h − λ, 0, C)`. If even λ = min_h(residual_h) − C cannot use all
   of `E` (the plant can't usefully run harder), allocate `min(C, max(0, residual_h))` and let the
   rest of the water spill. Find `λ` by bisection with a fixed iteration count, so the result is
   deterministic. *(Builder's call: the spill test is `E ≥ Σ_h min(C, max(0, residual_h))`, the
   usable water in §12.7's conservation formula, so hours with a negative residual stay dry.)*
4. **Unplanned plants.** A hydro plant with no allocation for the current hour is budget-limited
   only (uncapped) until the next hourly re-plan. This covers a plant switched to AGC mid-hour, a
   newly connected plant, and a plant restarted after a trip. Commands never trigger a re-plan
   directly. Connectivity is read at plan time, with no look-ahead for lines still under
   construction. *(Builder's call: being uncapped avoids idling water while the grid may be short;
   re-planning on commands would complicate command-sourced replay for little gain.)* *(Builder's
   call: a plant becomes unplanned the moment it trips, and when its AGC is turned off; elapsed
   hours keep their frozen allocations, so turning AGC back on mid-hour leaves it unplanned until
   the next re-plan.)*
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
  uses the hour-0 ceiling (the hour-0 allocation within the ramp envelope below) as its upper bound.
  - *(Builder's call, effect hour: the cap uses the allocation of the hour in which the target takes
    effect, `(tick + 1) / 60`, because plants step before AGC retargets. Capping by the current
    tick's hour leaked one tick of the previous hour into each new hour and overdrew a final hour
    planned to spend exactly the remaining budget.)*
  - *(Builder's call, look-ahead ramp envelope: the cap is
    `min(hourly[h], hourly[h+1] + ramp × ticks to the next boundary)`, with no next-hour term for
    hour 23. A foreseen fall therefore ramps down early and an hour's energy stays within its
    allocation, which prevents a budget-limit cliff after the last allocated hour on budget-tight
    days.)*
- **Emergency override.** The allocation cap is lifted, leaving only the budget limit, whenever the
  frequency band is `Shedding` **or** any load is shed (`shed > 0`, in either grid mode). *(Builder's
  call: keying on `shed > 0` as well as the band keeps the cap off for the whole restoration, rather
  than flapping on and off as the band recovers.)* Avoiding load shedding is worth more than any
  later hour. The next hourly re-plan spreads whatever water remains.
  - *(Builder's call: the cap returns at a rate the rest of the fleet can follow. When a lift ends,
    the power system enters a recovery phase. While it lasts, scheduled hydro above its AGC ceiling
    ramps down by no more than the other AGC plants' aggregate one-tick headroom (the headroom
    gate's definition in §14.2) less the shortfall they already face, never below zero; a surplus
    counts as cover, as at the gate. Several plants share it in plant-id order. Recovery lasts until
    every scheduled plant is at or below the lowest ceiling it meets before the next re-plan (the
    falling-output withhold's look-ahead, §14.2), so a foreseen fall soon after is limited too, and
    it ends at the day boundary. A new emergency lifts the cap again. Without it, hydro fell back at
    its own ramp on the first tick nothing was shed, slow thermal could not follow, and load was shed
    again, up to a blackout. Recovery may spend water beyond the plan; the budget limit still
    binds.)*
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

*(Builder's call: freezing elapsed hours lets the HUD and lessons compare plan with actual.)*
*(Builder's call: the view is valid per phase. While Operating it shows today's plan; during Review,
the finished day with every hour frozen; from the next Planning phase (`FinishReview`) it is empty
until `BeginOperating` plans the new day.)* It is for the Plan 2 HUD ("hydro covers
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
  the daily budget is never exceeded. *(Builder's call: with the look-ahead envelope of §12.4, a
  foreseen fall ramps before the boundary, so this overrun now happens only for an unforeseen drop
  at a re-plan.)*
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

## 14. Amendment A3: Over-frequency protection, minimum-generation warning and frequency-performance strikes (2026-10-18)

### 14.1 Why

Under-supply has real consequences (shedding, strikes, blackout); over-supply has almost none.
When the committed fleet's minimum stable output exceeds demand, typically overnight:

- AGC lowers everything to minimum and can go no further;
- `Curtail` finds no solar to withhold;
- frequency settles at roughly k_inertia × surplus ÷ (size × k_damping) above nominal (about
  +2.5 Hz for a 10% surplus) and stays there.

The only cost is fuel burned for unpaid energy. The Task 20 "overbuild" scenario shows +230 GWh of
surplus in one month with perfect reliability.

On real grids this is the **minimum-generation problem**: committing too much inflexible capacity.
Real systems respond to it, and the game should model two of those responses:

- generators protect themselves by tripping on sustained over-frequency;
- control-performance standards (NERC's BAL family) penalise balancing authorities that let
  frequency wander.

The teaching goal is to make the trap visible before the player falls into it.

### 14.2 Over-frequency generator protection

- **New `FrequencyParams` fields** (region data): `trip_above: Hertz` (default +1.5 Hz) and
  `trip_after_ticks: u32` (default 5). Validation requires `trip_above > curtail_above` and
  `trip_after_ticks ≥ 1`.
- **`ProtectionState` gains `over_ticks: u32` and `over_armed: bool`.** In Normal mode:
  - when not armed, each tick with `deviation > trip_above` increments `over_ticks`, and a tick at or
    below `trip_above` resets it to 0;
  - once `over_ticks` first reaches `trip_after_ticks`, protection becomes **armed**;
  - while armed, `over_ticks` counts every tick with `deviation > curtail_above`;
  - protection disarms, resetting both fields, as soon as `deviation ≤ curtail_above`.

  *(Builder's call: with the default k_inertia ÷ k_damping of 25 Hz per unit of surplus, a 2–6%
  surplus would otherwise settle between +0.5 and +1.5 Hz and never trip, contradicting §14.6.)*
- **When `over_ticks` reaches `trip_after_ticks`,** protection returns `OverFrequencyTrip`.
  `PowerSystem` then trips **one** plant:
  - the connected, online, dispatchable plant with output > 0 and the **highest marginal cost**
    (ties broken by plant id);
  - solar is never tripped, because curtailment covers it;
  - the trip uses `Plant::trip()` plus `suppress_commitment_for_day()`, exactly as after a blackout;
  - `over_ticks` resets to 0 after each trip, so at most one plant trips per `trip_after_ticks`
    window and the fleet sheds generation gradually until frequency is back at or below
    `curtail_above`.
- **The existing response comes first.** Curtailment at +0.5 Hz (solar first, then AGC to minimum)
  still runs before any trip, and nothing changes below `trip_above`.
- **New event `OverFrequencyTrip { plant }`** (alongside the usual `PlantTripped`), and a new
  `IncidentKind::OverFrequencyTrip { plant }` in the monthly report.

*(Builder's call: tripping the most expensive producer first is the economically least harmful
choice, and it is deterministic. Real trip order follows relay settings, which the game doesn't
model.)*

The builder's calls below keep protection from tripping the grid into a deficit:

- **Surplus gate.** *(Builder's call: a tick counts toward a trip window only while a physical
  surplus exists, `supply − demand × (1 − shed) > 0`. A tick without one resets the count but does
  not disarm. The 5-tick damping lag equals `trip_after_ticks`, and without the gate the literal
  rule over-tripped into blackouts.)*
- **Headroom gate.** *(Builder's call: the candidate trips only if its `output − surplus` is at most
  the sum, over the other auto-follow plants, of `min(min(available, hydro ceiling) − output, one
  tick of ramp)`. Otherwise the trip is withheld and the count held.)*
- **Redispatch.** *(Builder's call: AGC retargets again in the same tick after a trip, so the rest
  of the fleet responds on the next tick.)*
- **Falling-output withhold.** *(Builder's call: no trip while any other plant is losing output that
  AGC cannot reverse: scheduled hydro above the lowest AGC ceiling it meets up to the first tick of
  the next hour, a commitment-driven shutdown, or a manual plant above its target.)*
- **Restoration.** *(Builder's call: a trip that falls due while shed load is being restored waits
  for the first tick that does not restore.)*

### 14.3 Minimum-generation warning (read side)

- **A direct query (CQRS read side), not a projection:**
  `GameSession::minimum_generation_check() -> Option<MinimumGenerationWarning>`. It returns
  `MinimumGenerationWarning { hour: u8, committed_minimum: Megawatts, net_load: Megawatts, excess: Megawatts }`.
  Commitment edits during Planning emit no events, so a projection would go stale; the Plan 2 HUD
  reads this query directly while in Planning.
- **For each hour `h` of the coming day,** compute two quantities:
  - `committed_minimum_h` is a sum over connected, commissioned, dispatchable plants committed in
    hour `h` by their pattern. Each contributes its minimum output if it is under AGC, or its
    initial target if it is manual.
  - `net_load_h` is the forecast net load at the hour's two ends, minute 0 and minute 59. Demand
    within an hour is interpolated linearly, so its extremes sit at the hour's ends, and a trough
    on an hour knot is invisible to a mid-hour sample (as with §15.5). Each end is evaluated
    through the same per-hour evaluator as §15.5 and the operating-time sizing (§15.3), with the
    committed minimum as the fleet.
- **The excess** of an hour is `committed_minimum_h − net_load_h` at its worse end, the one with
  the lower net load (minute 0 on a tie); the warning's `committed_minimum`, `net_load` and
  `excess` are that end's. The warning reports the hour with the largest positive excess (ties go
  to the earliest hour), or `None` if no hour has a positive excess.
- **Pure:** no RNG and no state change.
- **A new lesson:** `Lesson::Overgeneration { energy: MegawattHours, trips: u32 }`. It fires when the
  month's positive imbalance exceeds 2% of served energy, or when at least one over-frequency trip
  happened.
  - Its priority sits directly after `UnservedEnergy`.
  - `MonthlyReport` gains `overgeneration: MegawattHours` (the weighted Σ of positive imbalance) and
    `over_frequency_trips: u32`.

### 14.4 Frequency-performance strikes

- **New `RegulatorParams` fields** (region data): `frequency_band: Hertz` (default 0.2 Hz) and
  `frequency_violation_share: Ratio` (default 0.05, about 72 minutes per day). Validation requires
  `frequency_band > 0`.
- **Counting.** `MonthAccumulator` counts Normal-mode ticks with `|deviation| > frequency_band`,
  out of all Normal-mode ticks. Blackout ticks are excluded: frequency is held at nominal during a
  blackout, and blackouts are already penalised through unserved energy.
- **Month close.** If the out-of-band share exceeds `frequency_violation_share`, the month fails
  frequency performance. *(Builder's call: `MonthlyReport` also carries `frequency_failed: bool`,
  computed by the same test as the verdict, which the `FrequencyPerformance` lesson reads.)*
- **Notice before strike.** A month that fails **only** frequency performance draws a formal
  **notice** instead of a strike, unless a notice was already issued in the previous 12 months.
  - The regulator stores `last_frequency_notice: Option<SimMonth>`.
  - A notice is due when that is `None`, or when `month.months_since(last) ≥ 12`.
  - The notice emits `FrequencyNoticeIssued { month }`.
  - A month with a notice is not clean (it resets the clean streak) but adds no strike.
  - A further frequency-only failure while a notice is still recent earns a strike.
- **One strike per month.** A month that fails both tests earns **one** strike (reason
  `UnservedEnergy`) and does not use up the notice. A month counts as clean for strike expiry only if
  it passes **both**.
- **The event gains a reason:** `StrikeIssued { strikes, reason: StrikeReason }`, where
  `StrikeReason` is `UnservedEnergy` or `FrequencyPerformance`. When both fail, the reason is
  `UnservedEnergy`.
- **Projections that read strike events adapt to the new event shape.** `FullSeasonWithoutStrike`
  still counts any strike; notices don't break it.
- **New lesson:** `Lesson::FrequencyPerformance { out_of_band: Ratio, notice: bool }`. It fires for
  any month that failed frequency performance (`notice` is true when the month drew a notice rather
  than a strike), with priority directly after `Overgeneration`.

### 14.5 Data and versioning

- **Beginner acceptance check.** A **hands-off plan** commits every dispatchable plant all day with
  AGC on and issues no other commands. Run it for 12 months from each shipped region's start, on
  seeds 0–7, after A1–A3 have landed. The game must still be `Active` at the end of month 12.
  - If the check fails, retune region **data** (fleet mix, flows, the new defaults) and never the
    rules, as in A1 §12.5.
  - Test scripts that deliberately exercise failure paths, such as the overbuild scenario, may still
    end in game over.
  - *(Historical tuning before A7 (§18): the check passed on both regions, but the East's margin was thin. 6 of 8 seeds
    draw a frequency notice in a shoulder month, and none has more than one strike outstanding,
    because the April and October committed minimum sits about 50 MW above the 02:30 net load.)*
  - *(Builder's call: the CCGT minimum stable level is 30% in both shipped catalogs, the turndown of
    a modern plant, which supersedes §3.2's 40%. The test fixture keeps 40%.)*
  - *(Historical tuning before A7 (§18): the Eastern start fleet retired four coal plants that closed by 2022 and adds
    four CCGTs built since 2020 in the same footprint, leaving four coal and nine CCGT units.)*
  - *(Builder's call: the overbuild scenario is split in two.*
    - *`overbuild_overcommitment_loses_licence` commits the four new CCGTs. The licence is revoked
      at the October 2029 close, after a notice and three FrequencyPerformance strikes, before any
      December review sees the new plants.*
    - *`overbuild_idle_capacity_disallowance` builds the same CCGTs but leaves them uncommitted. The
      December 2029 review holds the excess capital, and the December 2030 review disallows it.)*
- **Delivery.** A1, A2 and A3 are implemented together as Plan 1a, before Plan 2, with a single
  `SIM_RULES_VERSION` bump to 2. Snapshots are regenerated once, after all three have landed, and
  the diffs are reviewed together.

- Both shipped regions and the test fixture gain the four new fields with the defaults above.
- Outcomes change: overgeneration now trips plants and can earn strikes. This lands under the same
  `SIM_RULES_VERSION` bump to 2 as A1 and A2, with all snapshot diffs reviewed together. The
  "overbuild" scenario is expected to change materially, to trips and frequency strikes instead of a
  silent surplus.

### 14.6 Properties (tests)

- **Trips are sustained and gradual.**
  - Protection never arms while `deviation ≤ trip_above`.
  - It arms after exactly `trip_after_ticks` consecutive ticks above `trip_above`.
  - While armed, it trips one plant per `trip_after_ticks` window while `deviation > curtail_above`.
  - It disarms at or below `curtail_above`.
  - At most one plant trips per window.
- **Trip order.** Always the highest-marginal-cost connected producing dispatchable plant (ties by
  id). Never solar.
- **The surplus clears.** In the overbuild fixture with no player action, frequency returns below
  `curtail_above` within a bounded number of ticks, and stays there for the rest of the day.
  *(Builder's call: the surplus clears as far as safe trips allow. Frequency returns to
  `curtail_above` or below unless no qualifying candidate can trip safely (§14.2). A residual
  surplus that no unit can safely trip stays as over-frequency: §14.4 penalises it, and §14.3 warns
  of it in advance.)*
- **Armed protection never sheds more.** Armed protection never increases shed energy or causes a
  blackout relative to the same run unarmed. *(Builder's call: added with the §14.2 gates. The
  accepted residuals are a hydro drop at an hour-boundary re-plan or past the next boundary, a
  commitment stop that starts after the trip, and a player cutting a manual target after it.)*
- **The warning is exact.** On a crafted fleet, `minimum_generation_check` reports the
  hand-computed hour and excess, returns `None` when committed minimums fit under net load, counts
  only committed hours, and leaves `state_hash` unchanged.
- **Strikes.**
  - the out-of-band share counts Normal-mode ticks only;
  - the first frequency-only failure in any rolling 12 months draws a notice, not a strike;
  - a second frequency-only failure within 12 months of that notice strikes with reason
    `FrequencyPerformance`;
  - a notice month resets the clean streak;
  - both tests failing still gives one strike, with reason `UnservedEnergy`;
  - clean-month expiry requires passing both;
  - three strikes from any mix revoke the licence.
- **Validation.** Each new field has a negative validation test.

## 15. Amendment A4: Rotating outages and staged under-frequency shedding (2026-10-19)

### 15.1 Why

Connecting load far beyond the fleet collapses the grid in the first minute (open question Q1 in
the Plan 1 follow-ups):

- frequency moves by about k_inertia × the deficit share each tick, so a 30% shortfall drops it
  1.5 Hz in one minute, which is the blackout threshold;
- shedding removes only one 5% step per tick, so any shortfall above about 10% outruns it (a 20%
  step deficit blacks out within two ticks);
- a blackout trips every plant and suppresses its commitment for the day, and the played day
  stands for the whole month. The overreach scenario serves nothing for a month (reliability 0).

Real grids do not work this way, for two reasons the game should model:

- **A forecast shortfall is shed on purpose.** When operators can see that supply won't cover
  demand, they declare an energy emergency and order **rotating outages**: a controlled share of
  load is cut in turns while frequency stays near normal (California, August 2020; Texas,
  February 2021).
- **A sudden loss is arrested by staged relays.** Under-frequency load-shedding (UFLS) relays sit
  at stepped thresholds (for example 0.2 Hz apart in the Western interconnection). Each stage
  drops a block of load once, so a deeper dip sheds more, at once.

The teaching goal: a shortage costs you the load you could not serve and its strikes, not the
whole grid.

### 15.2 Goals, non-goals, anti-goals

- **Goals:**
  - A shortfall the forecast can see is shed as a rotating outage, with frequency kept near
    normal and the fleet left running.
  - A sudden step deficit of up to about 29% of demand (just under the one-tick blackout line) is
    arrested by staged shedding without a blackout, even with no help from AGC.
  - The shortfall is visible during Planning, before the day starts.
- **Non-goals:**
  - physical black-start infrastructure or cranking-power dependencies; automatic restart policy
    is specified in §19;
  - demand response, imports, or prices that ration load;
  - an operating-reserve requirement or a "reserve margin" knob for outages;
  - choosing which city goes dark: the domain sheds proportionally across connected cities, as
    UFLS does. The presentation shows that shared fraction; it does not assign outages to specific
    cities or invent a geographic rotation.
- **Anti-goals:**
  - outages that cut more than the hour's worst forecast minute requires;
  - staged shedding that over-sheds a step deficit by more than three shed steps;
  - invented tuning knobs: outage sizing follows from the forecast and the existing `shed_step`,
    and stage spacing is a real relay setting.

### 15.3 Rotating outages (forecast-based, planned shedding)

- **State.** `PowerSystem` gains `outage: Ratio`, the current rotating-outage share, and
  `outage_target: Ratio`, the share ordered for the current hour. Both are 0 outside a shortage.
- **Protection acts on what the outage leaves.** The tick's **load** is
  `demand × (1 − outage)`. Everything that today reads demand reads the load instead:
  - the system size in the frequency model (a dark grid, below, holds frequency instead);
  - AGC's requirement;
  - the imbalance;
  - `protection::evaluate`, including its surplus gate.

  Protection's `shed` is a share of that load, so
  `served = demand × (1 − outage) × (1 − shed)` and `unserved = demand − served`. Unserved energy
  is still shed load only (Ruling R16). *(Builder's call: composing the two shares this way leaves
  protection's internals and thresholds unchanged.)*
- **Sizing.** At `BeginOperating` and at every hour boundary, immediately after the hydro re-plan
  (§12.3), size the hour `h` that is starting. Demand within an hour is interpolated linearly, so
  its worst minute is one of the two ends. Evaluate both minute 0 and minute 59 of `h`, and at each
  instant `m`:
  - `demand_m`: connected demand forecast at `m` (the same forecast as `hourly_net_load`, at
    that minute).
  - `deliverable_m`: what the fleet can actually supply at `m`, as the sum of:
    - expected solar at `m`;
    - each connected dispatchable non-hydro plant that is `Online` now:
      - under AGC, `min(capacity, output + ramp × 59)` at both ends: its reach by minute 59;
      - on a manual target, `min(target, output + ramp × m)`: ramp-limited to `m`;
    - each scheduled hydro plant: its AGC ceiling for the minute being sized, which is its
      allocation for `h` (§12.3), lowered to the ramp envelope ahead of a falling next hour;
    - each manual hydro plant: its target, limited by its remaining budget.

    Plants that are `Offline`, `Starting`, `Stopping` or tripped count 0, and so does a plant whose
    commitment stops it at the start of `h`.
  - `share_m = (demand_m − deliverable_m) / demand_m`, floored at 0.

  `outage_target = clamp(max(share_0, share_59), 0, 1)`, exactly.

  *(Builder's call: sizing for the hour's worse end keeps protection from shedding inside a rising
  or falling hour. There is no rounding up to `shed_step`: a whole step over-cut marginal
  shortages, and a 1–2% shortfall cost 5% of load for the hour.)* *(Builder's call: AGC plants
  count their reach by minute 59 at both ends. A load-following plant's output is held down by the
  outage itself, so counting only its current output at minute 0 would ratchet the outage: every
  hour would size at least at the outage in effect.)* *(Builder's call: scheduled hydro counts its
  ceiling, which is what AGC will actually allow, so a foreseen fall is not over-counted.)*

  At `BeginOperating`, hour 0 is sized after a first boundary dispatch for the full forecast.
  *(Builder's call: the pattern's initial outputs leave AGC plants at minimum, and sizing from them
  would order outages on days the fleet covers.)*
- **Re-sizing.** Besides every hour boundary, the current hour is re-sized:
  - on any Normal-mode tick where a connected dispatchable plant comes `Online` (a tick event,
    not a command);
  - on the `BlackoutRestored` tick.

  At most one `RotatingOutageOrdered` is emitted per tick, carrying the final target.
- **Applying it.**
  - **At `BeginOperating`,** the day starts with `outage = outage_target` for hour 0, and the
    day-boundary dispatch (Ruling R21) balances against the reduced load. *(Builder's call: this
    is the cyclic-boundary principle. Ramping in from 0 would black out a large shortfall on
    tick 1.)*
  - **During the day,** `outage` moves toward the target on each Normal-mode tick, before AGC
    runs, so AGC aims at the load its targets will meet.
  - **Cutting load.** `outage` rises by `min(gap, shed_step, cover_down / demand)`, where
    `cover_down` is the AGC fleet's net one-tick ramp-down room. A deficit counts as cover.
  - **Restoring load.** `outage` falls by `min(gap, max(shed_step, surplus / demand),
    cover_up / demand)`, where:
    - `cover_up` is the net one-tick headroom: `agc_headroom` (the same headroom as the trip gate,
      §14.2, and hydro recovery, §12.4) minus the shortfall, so a surplus counts;
    - `surplus = max(0, supply − load)`;
    - the surplus term applies only when no plant is in an unrecoverable ramp-down
      (`has_unrecoverable_ramp_down`, the trip gate's predicate and lookahead).

  *(Builder's call: cutting in fixed 5% blocks outran a small fleet's ramp-down. It forced AGC to
  minimum and blacked out the overreach grid. Restoring only a full step at a time stalled, and a
  unit larger than one step coming online into a full outage spiked frequency. A surplus from an
  unrecoverable fall is about to vanish, so restoring it as load would only be shed again.)*
- **Blackouts.**
  - **During a blackout,** sizing is frozen: hour boundaries keep the target ordered before the
    blackout, and `outage` is held at 0. All dark load is protection's `shed`, so it counts as
    unplanned.
  - **Restoration** completes when protection's `shed` reaches that frozen target rather than 0.
    Each step lands at `max(snapped(shed − step), restore_to)`, with `restore_to` the frozen target
    and AGC pre-positioning to the same figure, so restoration ends exactly at the target without
    a stall or an overshoot. A restoration step requires `supply > 0`, so black start needs
    generation. Normal mode then resumes with `outage` equal to the frozen target, so a shortage
    day can come back from a blackout.
  - **After restoration,** the hour is re-sized on the `BlackoutRestored` tick and at every
    boundary after it, counting any plants the player has restarted that are online by then.
- **Dark grid.** On a Normal-mode tick whose load is `≤ 0` (the outage at 1, zero demand, or no
  connected city), deviation is held at 0 and over-frequency tracking resets, so protection
  disarms. One first-lit tick may exceed `trip_above`, but protection never arms. *(Builder's
  call: a fully cut grid has no frequency to measure, and the 1 MW system-size floor is there for
  numerical safety, not physics.)*
- **Hydro.** The emergency lift (§12.4) keys on protection's `shed > 0` and the `Shedding` band
  only, not on `outage`. *(Builder's call: the hydro plan already spends water on the shortage
  hours, and outage sizing counts that allocation. Lifting the cap for a planned outage would
  spend the water early and deepen later hours' outages.)*
- **Commands** never re-size the outage directly; the next hour boundary, or a tick on which a
  plant comes online, does. This matches the hydro schedule (§12.3, step 4) and keeps
  command-sourced replay simple.
- **Events and report.**
  - `DomainEvent::RotatingOutageOrdered { hour: u8, share: Ratio }` is emitted on a tick that
    ends with `outage_target` different from where it began, once and with the final target,
    including a change to 0 ("outage lifted"), and at `BeginOperating` when hour 0's target is
    not 0.
  - `IncidentKind::RotatingOutage { share: Ratio }` is recorded when a non-zero target is ordered.
  - `MonthlyReport.rotating_outage: MegawattHours` is the weighted energy cut by `outage`. It is
    part of `unserved`, never added to it.

### 15.4 Staged under-frequency shedding

- **New `FrequencyParams` field** (region data): `shed_stage_spacing: Hertz`, default 0.2 Hz.
  Validation requires a finite value > 0.
- **Stages.** Stage `k` (k = 0, 1, 2, …) has threshold `shed_below − k × shed_stage_spacing`. A
  stage fires at most once per under-frequency event, dropping one `shed_step` block the first tick
  `deviation` is strictly below its threshold, so a deviation exactly at a threshold has not
  crossed that stage. Several stages crossed in one tick fire together.
- **Slow stage.** On a tick below `shed_below` that crosses no new stage, one `shed_step` is shed,
  as today. This keeps the existing time-delayed behaviour for a deficit that persists.
- **Re-arming.** The event ends, and every stage re-arms, when deviation is back at or above
  `−normal_band`, at the same moment shed starts to be restored and `shed_ticks` resets. Stages
  also re-arm on `BlackoutRestored`. `ProtectionState` gains `stages_fired: u8`.
- **Blackout rules are unchanged:** below `blackout_below`, or `blackout_after_shed_ticks`
  consecutive shedding ticks. *(Builder's call, measured on the game's frequency model with
  0.2 Hz stages, step deficits from 0.1% to 29.9% in 0.1% steps, and AGC ramps from 0 to 2% of
  demand per tick: there was no blackout, peak shed stayed within 3 shed steps of the deficit, and
  the rebound peaked at +0.59 Hz, measured on a finer grid of 0.01% deficits and 0.05% ramps. A
  rebound that high can briefly curtail but never reaches the over-frequency trip line. A rule
  that scaled every tick's shed by depth arrested the same deficits but rebounded to +1.6 Hz.)*

### 15.5 Capacity-shortfall warning (read side)

- **A direct query, like §14.3:** `GameSession::capacity_shortfall_check() -> Option<CapacityShortfallWarning>`,
  returning `CapacityShortfallWarning { hour: u8, demand: Megawatts, deliverable: Megawatts, shortfall: Megawatts }`.
  It reports the coming day's worst hour (ties to the earliest), or `None` when every hour is
  covered. It is pure: no RNG and no state change.
- **Both ends of each hour, like §15.3.** Demand within an hour is interpolated linearly, so its
  extremes sit at the hour's ends, and a peak on an hour knot is invisible to a mid-hour sample.
  The check evaluates minute 0 and minute 59 of every hour `h`, taking demand, expected solar and
  each plant's availability at the same minute, through the same per-hour evaluator as the
  operating-time sizing; only the deliverable rule differs. An hour's shortfall is that of its
  worse end (minute 0 on a tie), and the warning's `demand`, `deliverable` and `shortfall` are
  that end's.
- **For the coming day,** `deliverable_h` counts each plant committed in hour `h` by its pattern
  as follows:
  - dispatchable non-hydro under AGC: its availability;
  - manual: its initial target. Manual hydro counts its `initial_target` too, with no limit from
    the day's water, consistent with "manual counts its target";
  - scheduled hydro: its allocation from a **pure preview** of the hour-0 hydro plan, computed by
    the same water-filling code path as `plan_hydro` with the coming day's budgets;
  - expected solar.

  It is an upper bound: it ignores start-up and ramp limits that the operating-time sizing
  (§15.3) applies. *(Builder's call: the Planning question is "can the committed fleet cover the
  peak at all?". Ramp-limited shortfalls show up as outages during the day.)* So on a noise-free
  day whose committed fleet runs on AGC and ramps without limit, no warning means no outage is
  ordered. Scheduled hydro is the one exception to "upper bound", and a safe one: each boundary
  re-plan pours water left unused earlier in the day, so the live allocation can exceed the
  preview and an hour the check warns of may not be cut.
- The pure hydro preview is `pub(crate)`. Exposing it as a Planning view of `hydro_schedule()` is
  left to Plan 2 (§12.6).

### 15.6 Lessons

- `Lesson::RotatingOutages { energy: MegawattHours, peak_share: Ratio }` fires when
  `rotating_outage > 0`. `peak_share` is the month's largest ordered share, from a new
  `MonthlyReport.peak_outage_share: Ratio`. Its priority sits directly before `UnservedEnergy`.
- `Lesson::UnservedEnergy` now fires only for **unplanned** unserved energy,
  `unserved − rotating_outage`, and reports that amount. It fires when that amount exceeds a
  relative tolerance of `1e-9 × unserved`, so float noise in an outage-only month does not. *(Builder's call: a planned outage
  and a protection-driven shed teach different things: build or commit capacity, versus keep
  reserves and ramp.)*
- The cap of three lessons and the `[AllClear]` fallback are unchanged.

### 15.7 Data and versioning

- **Defaults.** Both shipped regions and the test fixture gain `shed_stage_spacing = 0.2 Hz`.
- **The strike rules are unchanged,** but a marginal-shortfall month can trade a frequency notice
  for an unserved-energy strike, because load the fleet couldn't serve is now cut and counted
  rather than carried as frequency sag. Rotating outages are unserved energy, so they count toward
  the reliability strike (§2.5). *(Controller ruling: this is intended. The v2 model counted a
  standing sub-2% deficit as frequency sag, with energy "served" that was never generated. In the
  rules-guard script, August 2027 moves from a frequency notice at 0.436% unserved to an
  unserved-energy strike at 0.625%.)*
- **Versioning.** Outcomes change, so bump `SIM_RULES_VERSION` to 3 and regenerate the snapshots
  once, reviewing every diff. The overreach scenario is expected to change materially: from a
  month-long blackout to a rotating outage with the fleet running and reliability above 0.
- **The beginner acceptance check (§14.5) must still pass.** Retune region data, never the rules,
  as before.

### 15.8 Properties (tests)

- **Staged shedding arrests a step deficit.** In the protection model, take every step deficit
  `d` with `k_inertia × d < −blackout_below` (up to 29.9% with shipped data) and every AGC ramp
  from 0 to 2% of demand per tick. Each case:
  - ends with no blackout;
  - keeps its minimum deviation above `blackout_below`;
  - peaks at a shed of `≤ d + 3 × shed_step`;
  - rebounds by less than `trip_above`, so no over-frequency trip can follow.
- **Stages fire once.** Each stage sheds at most once per event and re-arms only at or above
  `−normal_band`.
- **Outage sizing is exact.**
  - `outage_target` is exactly the worse-end share, `max(share_0, share_59)`. It is 0 when the
    fleet covers both ends of the hour.
  - During the day `outage` moves at the paced rates of §15.3: a cut by at most
    `min(shed_step, cover_down / demand)`, a restore by at most
    `min(max(shed_step, surplus / demand), cover_up / demand)`, never past the target.
  - It never restores a surplus from an unrecoverable fall.
  - Sizing draws no RNG.
- **Outages start at the target.** At `BeginOperating`, `outage` equals hour 0's target.
- **Blackouts freeze sizing.** During a blackout `outage` is 0 and the target is frozen.
  Restoration completes at the frozen target.
- **Overreach no longer collapses.** In the overreach fixture:
  - there is no blackout;
  - plants stay online;
  - reliability is above 0;
  - deviation stays above `blackout_below` throughout;
  - with demand noise, `0.95 ≤ rotating_outage / minute-by-minute forecast shortfall ≤ 1.10`;
  - noise-free, `shortfall − ε ≤ rotating_outage ≤ shortfall + shed_step × peak demand × hours
    with an outage × days in the month`;
  - protection shed from demand noise is allowed.
- **No change without a shortage.** On a day whose forecast is covered and that has no sudden
  loss, `outage` is 0 throughout, so outcomes depend on the staged-shedding change only through
  events that actually shed.
- **A large unit coming online into a full outage never arms over-frequency protection.**
- **Black start needs generation.** A restoration step with no supply online does not complete,
  even to a frozen target of 1.
- **The shipped regions order no outage over a hands-off year** (seeds 0–7).
- **Hydro is not lifted by an outage.** A rotating outage alone (`shed == 0`) leaves the hydro cap
  in force.
- **The warning is exact.** On a crafted fleet, `capacity_shortfall_check` reports the
  hand-computed hour and shortfall, returns `None` when the committed fleet covers every hour, and
  leaves `state_hash` unchanged.
- **No warning, no outage.** Over a table of shaped days (seasons, fleets with and without solar
  and scheduled hydro, loads scaled across the point of cover), a noise-free day whose committed
  fleet runs on AGC with unlimited ramps orders no outage when the warning is `None`. Without
  hydro, the warned hour is cut.
- **Validation.** `shed_stage_spacing` has a negative validation test.

## 16. Amendment A5: The inherited schedule and the shortfall confirmation (2026-10-20)

### 16.1 Why

Playtesting found that a new game starts with every plant uncommitted and off AGC, so a new
player's first day goes dark unless they commit each plant by hand. A real utility doesn't change
hands that way: whoever takes over a going concern takes over its running fleet and the schedule
it was operating to. The previous operator's schedule is also the spec's own beginner baseline,
the **hands-off plan** of §14.5, which the shipped regions are tuned to survive for a year.

The confirmation covers the other half. A player who has edited the schedule into a shortfall
should be asked before starting a day that the forecast already says will need rotating outages.

### 16.2 Goals, non-goals, anti-goals

- **Goal:** a new game, with no player commands, plays its first day without a forecast shortfall
  in the shipped regions. It runs the hands-off plan.
- **Goal:** Start the day asks for confirmation when `capacity_shortfall_check()` (§15.5) reports
  a shortfall.
- **Non-goal:** committing newly built plants automatically. A plant the player builds still
  commissions uncommitted, as today.
- **Non-goal:** changing commitments between months. Patterns already persist.
- **Anti-goal:** a confirmation that nags. It appears only on a forecast shortfall, never on a
  minimum-generation or dry-hydro warning, and never in Operating.

### 16.3 The inherited schedule (domain)

- **The rule.** `GameSession::new` gives every dispatchable start-fleet plant the all-day pattern
  (`CommitmentSchedule::all_day()`) and `auto_follow = true`. Solar is not dispatchable and is
  unchanged.
- **Unchanged rules:**
  - `previous_month_offline` stays `false` for the start fleet, so month 1 charges no boundary
    start (§6.1);
  - the player can change any pattern or AGC flag in Planning as before;
  - the end state is identical to the hands-off plan's commands applied to a fresh game.
- **Read side.** `plant_views()` reports the inherited pattern and AGC, so the commitment grid
  opens already filled.

### 16.4 The shortfall confirmation (game)

- **When it shows.** In Planning, Start the day checks `capacity_shortfall_check()`.
  - If it returns `None`, the button sends `BeginOperating` directly, as today.
  - If it returns a warning, a prompt opens instead. It names the worst hour and the shortfall,
    using the same figures as the warnings strip, and offers two buttons:
    - **Start anyway** sends `BeginOperating`;
    - **Keep planning** closes the prompt.

    Clicking outside the prompt counts as Keep planning.
- **Copy.** The prompt's text must make clear that a rotating outage is expected, not certain.
  The check is an upper bound (§15.5).
- **What it doesn't do.** It neither blocks the player nor changes the domain. `BeginOperating`
  stays valid with a shortfall.

### 16.5 Data and versioning

- **Rules version.** The same seed and commands now produce a different game, so bump
  `SIM_RULES_VERSION` to 4 and regenerate the snapshots once, reviewing every diff.
- **Scenario intent.** Scenarios that relied on the empty start must keep what they were written
  to show. Where needed, a scenario issues explicit `StopPlant` (Planning) and
  `SetAutoFollow { on: false }` commands to recreate it. The overbuild and overreach scenarios
  must still end the way they were written to end.
- **The beginner acceptance check (§14.5)** now also passes with no commands at all. Keep the
  explicit hands-off variant as well.

### 16.6 Properties (tests)

- **Inheritance.** A new game's dispatchable start-fleet plants are committed all day with AGC
  on. Solar is untouched. Month 1 charges no boundary start.
- **Equivalence.** Applying the hands-off commands to a new game changes nothing: its
  `state_hash` is the same before and after.
- **No commands, no shortfall.** Each shipped region, at seeds 0–7, has no shortfall warning on
  month 1 with no commands.
- **The confirmation, through the real `dispatch` and the real egui pass:**
  - with no shortfall, Start the day begins operating;
  - with a shortfall, the prompt opens and no command is sent;
  - Keep planning sends nothing and closes the prompt;
  - Start anyway begins operating;
  - the prompt fits at 1024×640.

### 16.7 Window and panel sizing (playtest, presentation only)

Playtesting found two sizing problems. On a large monitor the window opens at Bevy's default
1280×720. The side and bottom panels have fixed sizes, so a player with room to spare can't give
the commitment grid, the charts or the plant list more space.

- **The opening size.** The window opens centred on the primary monitor at about 80% of its
  logical width and height. It is clamped to at least the 1024×640 minimum window and at most the
  monitor. If no monitor is known, it keeps the default size.
- **Resizable panels.** The outer panels can be resized by dragging their inner edge:
  - Planning's inspector (left), build menu (right) and bottom panel;
  - Operating's side panel (right) and bottom panel.

  Each panel opens at its current size. That size is also its minimum, so everything that fits at
  1024×640 still fits. Each panel also has a maximum, so the map always keeps a usable area:
  - a side panel takes at most 40% of the window's width, and a bottom panel at most half its
    height;
  - the map's **area**, the space the panels leave it, never drops below 320×200. The map drawn in
    it is a little smaller: it sits inside that area's margins and under the layer buttons;
  - the Operating bottom panel also leaves the side panel above it its minimum height: the gauge,
    the selected plant's details and three rows of the plant list. At about 1024×640 it barely
    grows.

  Content that already fills the available width or height, such as charts, the plant list and
  scroll areas, uses the extra room.
- **The map still never moves on its own.** Only a drag changes a panel's size. A selection
  change, a tab change or a phase change leaves it alone, as before. A panel's size lasts for the
  rest of the run. It isn't saved to disk, because there is no settings persistence yet.
- **The Operating side panel, from top to bottom** (playtest):
  1. the frequency gauge;
  2. the selected plant's details, in a block of fixed height. With nothing selected, a
     placeholder of the same height stands in;
  3. the plant list, which fills the rest of the panel's height and scrolls, with at least three
     rows at 1024×640.

  The list's position doesn't change when the selection does.
- **Out of scope:** remembering the window size between runs, and fullscreen.

### 16.8 The city card (playtest, read side and presentation)

Playtesting found that selecting a city on the map shows neither its name nor anything about its
demand. Planning's inspector only describes cells, and Operating's side panel only describes
plants.

- **A read-only domain query.** `GameSession::city_detail(id) -> Option<CityDetail>` returns the
  following figures for the coming day, or for the day being played:
  - the city's `base_load`;
  - whether it is connected;
  - its share of the connected base load;
  - its expected demand at the forecast peak, and the hour of that peak;
  - in Operating, its current demand and the load served to it.

  All cities share one demand shape and one noise process, and rotating outages and protection
  shed spread proportionally across connected cities (§15.3, §15.4). So a city's figures are the
  system figures scaled by its share of base load. An unconnected city has no current demand. The
  card shows the demand it **would** add once connected (`base_load` scaled by the forecast
  shape), so that connecting it is an informed choice. The query holds no strings and no prices.
- **Where the card shows.** It shows wherever a plant's details show, and the selected object
  drives it:
  - In Planning, the inspector shows a city card when a city is selected. A cell keeps its cell
    card.
  - In Operating, the side panel's selected-object block (above the plant list, §16.7) shows the
    city card instead of plant details. It keeps the same fixed height, so the list doesn't move.
- **The card's contents:**
  - the localised city name;
  - Connected or Not connected. When not connected, a one-line hint to build a line to it from the
    build menu. While a line that ends at the city's node is under construction, the hint says
    instead when it connects: the earliest in-service month of those lines, by the Construction
    tab's rule (§16.9). Checking the line's endpoint is enough for M1;
  - base load;
  - the expected peak today and its time;
  - in Operating, demand now and served now, with the share cut when an outage or shed is
    active.
- **Strings.** Every string goes in `ui.ftl`. The l10n coverage test includes the city name keys,
  as it already does.

### 16.9 Construction progress (playtest, presentation)

Playtesting found that after a plant or line is ordered, the player sees no sign of its progress.
The map draws the project faded or dashed, but nothing says how long it has left or when it will
be in service. The commissioning toasts arrive with no warning.

- **The Construction tab.** Planning's bottom panel gets a fourth tab, after Commitments,
  Forecast and Tariff. It lists every plant and line under construction, soonest first and then
  by id. Each row shows:
  - the project's localised name;
  - months left of its total build time, with a progress bar;
  - the month it enters service.

  With nothing under construction, the tab says so in one line. The tab's label shows the number
  of projects, for example "Construction (2)", so the player can see it without opening the tab.
- **On the map.**
  - A plant or line under construction shows its months left (for example "3 mo") next to its
    faded marker or dashed line. Like the plant letters, the label is drawn only at 9 pt or more
    (§8, Plan 2 ruling N1).
  - The map legend gains the two under-construction marks.
  - The legend can be collapsed (playtest, final fix wave): a *Legend* toggle ends the
    layer-button row. Until the player uses it, the legend opens in the first corner of the drawn
    map, trying top right, bottom right, bottom left and top left, whose rectangle covers no
    pickable mark: no city centre and no plant centre, plants under construction included. With no
    clear corner it is collapsed. So by default it never hides a city or plant, and it shows at
    common window sizes (1280×720 included). Once the player chooses, their choice wins: opened,
    it goes in the clear corner if there is one, else the top right, where it may cover marks. The
    choice lasts the rest of the game, through every phase, and a new game starts undecided. An
    open legend drops to fewer columns to stay inside the drawn map, and a click on it still
    selects nothing.
- **Selecting a plant under construction.** Plants under construction stay out of the commitment
  grid, as today. Operating's plant list shows them, as it always has, with their status (*Under
  construction*, with the months left). In Planning, the inspector shows a construction card:
  kind, capacity, grid connection, capital cost, approximate monthly payment, months left of the
  total, and the month it enters service. In Operating,
  selecting one, in the list or on the map, shows the same card in the selected-object block.
- **Data and arithmetic.**
  - Months left come from `PlantStatus::UnderConstruction { months_left }` and
    `LineStatus::UnderConstruction { months_left }`.
  - The total is the kind's `build_months`, or, for a line, `GameSession::line_build_months`: the
    months its quote said, from its stored length by the rule the order and the month close use.
  - The in-service month is derived from the current month and the months left, by the same rule
    the month close applies. Read the domain to get the off-by-one right, and pin it with a test
    that plays to commissioning.
  - No new domain state. A read-only query is added only if the in-service month can't be derived
    from public views.

## 17. Amendment A6: Save/resume before Plan 3 (2026-10-20)

### 17.1 Scope and player experience

Save/resume is [**Plan 2b**](../plans/2026-10-20-plan-2b-save-resume.md), delivered before the 3D
diorama work of Plan 3. It changes persistence and presentation, not simulation rules. The initial
release includes manual save slots and a
separate autosave at each month close, as selected by the user.

- **Save** is available in Planning, Operating and Review, including the final game-over Review.
  Its dialog creates a named slot or overwrites a selected manual slot after confirmation. Names
  are trimmed, contain 1–80 Unicode scalar values and no control characters. Names are display
  metadata, never file paths. Autosave slots cannot be selected for manual overwrite.
- **Load game** on the main menu lists manual saves and autosaves, newest first, with name,
  region, calendar month, phase, game minute, saved-at time and game-over status. Ties sort by
  opaque slot ID. An empty list says so. A bad slot remains visible with a localised reason and
  does not hide good slots; if its metadata cannot be decoded, show its file ID and use filesystem
  modification time only for list ordering. Failure to read the directory is a separate, retryable
  error; a nonexistent directory is an empty list. Loading is offered from the menu, not as an
  in-game session replacement in this release.
- **Rename** is available beside each readable manual save or autosave in both save lists. Its
  dialog starts with the existing display name and uses the same name validation as saving.
  Confirming changes only the name: slot/run IDs, kind, saved-at time, build/platform stamp,
  embedded region and all saved progress stay intact. It does not load or replay the save, save
  the current game, or change its last-saved boundary. Cancel returns to the list. Publish through
  the same atomic replacement, OS lock and revision check as a save; conflicts never overwrite
  another writer's change. Errors remain visible with Retry, including unconfirmed directory
  durability. A renamed active run's autosave retains its chosen name at later month closes.
- **Resume** restores the exact accepted-command boundary: all simulation state, including RNG
  progression, construction, plant overrides and hydro water remaining, plus every projection.
  An Operating save at 14:37 returns to Operating at 14:37. A Review save returns to that Review;
  a finished game remains finished and can show its final report. Every load enters **paused**,
  including after the `OnEnter(Operating)` hook, until the player explicitly chooses a speed.
- **Month-close autosave** captures immediately after the successful command that emits
  `MonthClosed`, after its projections have been applied and before a later `FinishReview`.
  Thus it preserves the completed report and any game-over outcome. Each new game or successful
  resume gets a new recording-run ID and its own automatic slot. Later months replace that run's
  autosave; they never overwrite a manual slot or the source of a resumed branch.
- **Menu** offers *Save and return to menu*, *Leave without saving*, and *Stay*. A failed save
  keeps the session and the dialog available for Retry or Cancel; it never performs the leave.
  The game-over modal exposes Save too. **The OS close button still quits without confirmation**
  (the earlier user ruling); it does not promise an exit autosave. Main-menu Quit is unchanged.
- Saving and loading show progress and disable conflicting actions. Opening the save/leave
  dialog freezes new ticks and simulation input. Cancel restores the preceding speed only if
  the game is still active in Operating; successful manual saving leaves it paused. An autosave
  error is visible and retryable, but the player may acknowledge it and continue without saving.
  That dismisses the error barrier; moving on from Review still uses the normal Continue action.
  Last-saved status changes only on a confirmed successful write, and says what boundary it saved.
- Saves contain applied game state. Game-specific transient state (unapplied commitment drafts,
  selection, map view, tabs, prompts and toast queues) is excluded and resets on resume. Window,
  layout, zoom and chosen running speed are separate application preferences below; Operating
  resume still starts paused. Replay does not re-emit historical toasts or milestone notifications.
- **Application preferences** are stored separately from game checkpoints and restored on launch:
  window size/position, UI zoom, the last chosen running speed, visual effects, sidebar widths, bottom-panel
  heights, the Incidents divider and movable tool-window positions. Observe actual resized/moved
  state; save automatically after changes and flush pending changes on normal OS/menu exit.
  Pausing or entering Review never overwrites the remembered running speed. New days use that
  preference; loaded Operating games still enter paused, and unpausing uses it. Clamp restored
  geometry to available monitors and the current HUD's minimum/maximum sizes, so a missing monitor
  or changed zoom cannot strand the window or panels. Missing preferences use defaults; malformed
  preferences do not block boot. Tests inject the preferences path, never mutate global settings.

A save failure never changes simulation state or deletes another save. The old manual slot remains
unchanged on a failure **before publication**; the post-publication durability case is specified in
§17.5. Disk-recording failure is distinct from save failure: the existing recording warning remains,
but a complete manual save can still succeed (§17.2).

### 17.2 Authoritative capture and complete history

A save is a **self-contained command checkpoint**, not a serialised aggregate or a reference to a
live log. `Game` retains the original `LogHeader`, the exact region source bytes loaded at creation,
and an in-memory `Vec<Command>` containing every successfully executed command, in order. Append
once after the handler succeeds, regardless of the disk sink's health, and update projections in
the same command call. Rejected commands are absent. Preserve original `AdvanceTicks` batches,
including successful zero-tick commands; never merge, split or reinterpret them when saving.

`Game::checkpoint()` captures those inputs, the accepted-command count, the current
`GameSession::state_hash()` and a summary derived from public queries. It has no I/O and neither
executes a command nor reads installed region files. Capture uses one boundary, never a mixture
of a pre-tick clock, a post-tick RNG and an earlier report. Memory is retained for the current run;
history compaction and aggregate snapshots are deferred, not approximated by dropping commands.

The bridge establishes a barrier: stop command producers, drain commands already admitted to the
session, and capture after `SimSet::Apply`. A queued `AdvanceTicks` is applied and its in-flight
flag cleared before manual capture, not discarded or applied again on resume. Autosave captures
at the individual closing-command boundary inside that funnel; later admitted commands must not
leak into its immutable checkpoint. Hold any remaining commands while a save dialog/write barrier
is active, and release them once on cancellation/continuation. They cannot cross a session change.
If draining reaches month close while a manual save/leave request is waiting, finish that autosave
(or acknowledge its error) first, then service the queued manual intent for the same session. An
autosave receipt cannot fulfil a separate Save-and-menu request.

Saving encodes an immutable capture on a worker, outside the frame/tick path. The active session
remains available and frozen until a manual save completes or is cancelled. Autosave capture is
already in paused Review. File logging continues as diagnostic command recording; its append
failure cannot make the retained history incomplete. No attempt to copy an unflushed or partially
written live `.log` is used as a save.

### 17.3 Save contents, identity and compatibility

Each `<opaque-slot-id>.mgsave` is one independent file. It includes:

- `SAVE_FORMAT_VERSION`, initially 1, distinct from `LOG_FORMAT_VERSION`;
- a replay compatibility identity: simulation-build fingerprint and target platform, recorded
  for diagnostics only;
- the original `LogHeader` (rules/log versions, settings, seed and region fingerprint);
- a byte-exact embedded copy of the original region RON, and the complete command prefix;
- command count, expected end-state hash, phase/date/minute/status summary;
- slot ID, manual/automatic kind, recording-run ID, display name and saved-at UTC time.

Use a small version envelope around a checksummed payload. Infrastructure hashes the exact payload
bytes with SHA-256 and verifies length/checksum before decoding its command DTOs. Unsupported save
versions fail before decoding version-specific payloads. IDs are adapter-generated opaque values;
clock times and IDs are injected metadata, never simulation inputs. A slot revision is the SHA-256
of its committed file bytes, used to confirm the specific revision being overwritten.

The deterministic guarantee is same simulation build/platform (§5.7). Do not assume
`SIM_RULES_VERSION` alone makes arbitrary builds bit-identical: libm and the Debug-based
`state_hash` can change without an outcome-rule change. So loading does not trust a build
identity; it trusts a replay that reproduces the recorded end state. The simulation-build
fingerprint and target platform are recorded for diagnostics: the identity is available to the
save listing, and loading logs it next to the running one when they differ. A writer always
stamps its own build, so a game resumed from another build's save saves as the current build.
Generate the fingerprint from sorted, relative paths and exact bytes of
`domain`/`application`/`infrastructure` sources and manifests, the workspace manifest/lockfile,
compiler identity, target, profile, enabled simulation features and compilation flags. Do not
include checkout location, timestamps, game rendering sources or assets; region content has its
own fingerprint.

Load checks save format/integrity, size limits, log/rules versions, and the embedded region
fingerprint before replay; the rules version is a fast, clear early failure. Hash the embedded
bytes before mapping them, then construct the region from those same bytes; never fall back to
installed region data. Load accepts any build or platform whose replay reproduces the recorded
command count, end-state hash and summary; verify them after replay, before installing anything.
A retuned or deleted shipped region therefore does not break a save. A replay that doesn't
reproduce them fails explicitly and installs nothing, leaving the running state and the save file
unchanged. When the save came from another build or platform, the player is told that this
version of the game doesn't reproduce it and that the file is unchanged; on the same build the
existing state/replay copy stays. There is no migration in this release.

The caveat is honest: `state_hash` hashes the session's `Debug` form, so a refactor that changes
that form, such as adding or renaming a session field, also makes older saves fail verification
even when no outcome changed. That is a safe failure, never a silent wrong load. A canonical state
hash or a stored snapshot would remove it; both are future work.

The initial implementation leaves `SIM_RULES_VERSION` at 6 and `LOG_FORMAT_VERSION` at 1. Changing
those is justified only by an independent rule or raw-log-format change. Save codec changes use
their own version. Raw diagnostic-log import, cloud sync, deletion/rename tools, UI-state saves,
aggregate snapshots and migration of incompatible saves are outside Plan 2b.

### 17.4 Reconstruct a runnable game, then publish it

`SaveStore` is an application-owned `Send + Sync` port, implemented by infrastructure. Its typed
operations list entries with per-slot errors, read a verified bundle, and write a new slot or
replace an expected revision. Application types contain no serde, paths or user-facing copy;
infrastructure owns DTOs, bytes, hashing, IDs, filesystem paths and atomic publication. Game owns
localised dialogs and request orchestration. No Bevy, I/O or persistence fields enter the domain.

Restoration builds a **candidate `Game`**, not just the `GameSession` returned by today's `replay`:

1. Start a fresh session from the verified region, original seed and settings, and seed
   `Projections::new` from that initial session.
2. Replay each stored command through the core handler. Immediately feed its entire returned event
   batch into `Projections::apply_all` using the session after **that command**, exactly as live
   execution does. Preserve command boundaries. Do not fold every event against the final session.
3. Check the expected state hash, count and summary. Return an indexed typed error on a rejected
   historical command. Check cancellation between commands. Do not send replay events to the HUD.
4. Initialise a fresh diagnostic log and adjacent region archive for this resumed branch; seed it
   with the complete historical prefix without re-executing it. Failure prevents installation.
   Attach the normal command pipeline and retained journal. Future commands and future saves have
   the whole original prefix plus new commands, exactly once, without depending on a parent file.
5. Install the candidate only after all checks and log initialisation succeed. Reset transient HUD
   resources and pending message/in-flight state; mirror the domain's actual phase and keep speed
   paused after entry hooks. Historical message queues never reach this new session.

The load worker never mutates the Bevy world or the live session. Each job has an operation token;
a cancelled or superseded completion cannot install a game, trigger a save/leave, or emit toasts.
A save's atomic publication, once started, is allowed to finish; cancelling its UI does not undo
an already published checkpoint. Never leave the game merely because a stale job reports success.
Only one persistence operation is active per app. Errors leave the menu/session and source file
usable. A failed restore deletes the branch log and archive it created; at worst a failed
deletion leaves a diagnostic partial branch log. A candidate cancelled or superseded after it
verified may keep its branch log. Neither ever changes a save.

### 17.5 Durable storage and failure contracts

`SaveDir` is an explicitly injected resource: `--save-dir <dir>`, otherwise `./saves`. It is separate
from `LogDir`, and a failure to access it does not prevent boot or a new game. Tests use independent
`TempDir`s and inject time, IDs and compatibility identity; they never mutate environment or cwd.

- Create a same-directory unique temporary file, encode the full bundle, flush and `sync_all` it,
  then atomically publish it over the destination. Infrastructure may promote the existing
  `tempfile` dependency to runtime use (`NamedTempFile::persist`); persistence alone does **not**
  sync contents or directories. Sync new directory entries and the destination's parent directory
  as required by each supported platform before reporting durable success. Never delete the old
  destination first. Abandoned temporary files are ignored by listing/loading.
- Use an OS-backed per-slot exclusive lock for revision check and publication. New-slot creation
  requires absence; overwrite requires the revision the player confirmed. A changed revision or
  busy lock returns a typed conflict, rather than overwriting another app's work. OS lock release
  on process death must not leave a permanent stale-lock failure.
- A write/flush/file-sync/rename failure before publication preserves the previous slot byte for
  byte. If publication succeeds but directory synchronisation fails, return
  `DurabilityUnconfirmed { slot, revision }`: the new complete file may already be visible. Do not
  report "saved", claim the old file is unchanged, or leave the session. Re-read that revision on
  Retry under the lock, then finish synchronisation or report conflict; do not blindly overwrite.
- Only a confirmed receipt updates last-saved metadata or completes *Save and return to menu*. A
  failed autosave preserves prior checkpoints under the same publication rules, shows a persistent
  warning and offers Retry or Continue without saving. The warning stays after Continue without
  saving. It is cleared by any confirmed save (an autosave, Retry or a manual save) whose boundary
  is at or after the failed one, and by leaving the run: a new game, a load or returning to the
  menu. A failed or unconfirmed save does not clear it, and while it shows, Save never says
  "(saved)". Retry uses the failed immutable boundary and its expected revision, not a newly
  captured state or an unconditional overwrite. A newer autosave supersedes older pending retries,
  which must never replace it. Manual saves and other runs' autosaves are never rotated away.
- Bound decoding to 64 MiB per save file and 1,000,000 commands, with checked lengths and nesting
  limits. Oversized captures fail before publication with `SaveTooLarge`; oversized, truncated,
  malformed or checksum-invalid files return typed errors, never a partial replay or panic.
  Limits are storage limits, not permission to truncate history.

Typed failures include `SaveFormatVersionMismatch`, `ChecksumMismatch`, existing
rules/log/region mismatches, `ReplayCommandFailed { index, error }`, `StateHashMismatch`,
`SummaryMismatch`, `OtherVersionDiverged` (one of those verification failures, for a save another
build or platform recorded), `OtherVersionUnreadable` (such a save this build can't decode: its
payload no longer parses, for example after a command variant, header or settings field changed,
or it parses but its embedded region or a command no longer converts; the recorded build is read
from the payload's stamp alone), `InvalidSaveName`, `SaveTooLarge`, `SlotChanged`, `SlotBusy`,
`DurabilityUnconfirmed`, `Cancelled`, and stage-specific I/O/log errors. Localise them at the game
boundary; distinguish "recording stopped" from "save failed".

### 17.6 Acceptance

- Save and resume Planning, mid-Operating, Review and game over; compare state hash **and every
  public projection/view**, not only cash or phase. Apply the same subsequent commands to an
  uninterrupted game and the resumed one; all outcomes, RNG-driven values and reports agree.
- Round-trip a second save made after resume, delete its parent save/log and installed region,
  and resume it again. History and historical projections appear exactly once.
- Autosave the closing command's Review state, including December's rate review and a terminal
  month. No autosave is triggered by replay or a refused command. Source/other slots stay intact.
- Reproduce a disk-log append failure after play has begun, then save successfully from memory
  and resume the exact current state. Inject a save write failure independently and retain play.
- Fault-inject every publication stage, including post-rename directory sync and two stores
  targeting the same revision. Assert the durable bytes and receipt/error, not just a mocked call.
- Exercise queued ticks, entry hooks, Cancel/Retry, stale jobs and Save-and-menu failures through
  headless Bevy integration and real egui passes. Nothing runs until explicitly unpaused after load.
- Property tests cover arbitrary successful command prefixes and continuation, failed-command
  exclusion, codec round trips and corruption rejection. All UI fits at 1024×640 and meets 4.5:1
  text contrast. Run workspace tests, clippy, format checks and the native launch/playtest checks
  in the Plan 2b implementation plan before declaring it complete.


## 18. Amendment A7: Faster expansion pacing (2026-10-22)

Playtesting found that whole-city demand jumps, 12–84 played months of construction and large
capital commitments delayed the first expansion too long. The selected direction is faster tycoon
pacing with smaller expansion steps and shorter build times. This supersedes the earlier shipped
capital/build tuning and starting-fleet counts; simulation physics and accounting rules are unchanged.

### 18.1 Demand and starting fleets

Every shipped city's base demand is halved. City identities, positions, relative sizes, load shapes,
weather, the three starting cities and the home-city anchors stay the same. Connections still serve
the whole city; no districts, service tiers, imports or borrowing are introduced.

Reduce the starting fleet alongside demand so the existing territory stays balanced rather than
inheriting excessive spare supply. Sites are a compact selection of the existing legacy locations:

| Region | Starting fleet | Nameplate | Starting base demand | Starting cash |
|---|---|---:|---:|---:|
| East | 1 nuclear, 1 coal, 5 CCGT, 7 peakers | 5,600 MW | 3,400 MW | $5B |
| West | 1 hydro, 2 CCGT, 4 peakers | 2,600 MW | 1,400 MW | $3B |

The inherited daily plan remains the default. Both regions must still pass the existing no-command
and hands-off year-one acceptance runs on seeds 0–7, including no planned rotating outages.

### 18.2 Construction and affordability

The authoritative shipped costs and lead times are in §3.2: peakers and solar take 2 played months,
CCGT 4, coal 8, hydro 12 and nuclear 18. Capital costs are tuned with these durations; shortening a
build alone would increase its monthly instalment. Unit capacities, operating costs, start-up times,
ramping, capacity credits and book lives are unchanged.

Transmission costs $20M per whole grid cell and builds at 4 cells per played month, with a minimum
of one month. All construction remains paid in equal monthly instalments, with any rounding cents
in the last payment. Plant and line build quotes show total capital, approximate monthly payment
and build duration so the player can distinguish project cost from immediate cash requirements.

### 18.3 Acceptance and compatibility

For both regions, test an early expansion to a nearby city (Cleveland in the East, Vancouver in the
West), including the new generation's connections and the city line. Count the existing fleet's
spare capacity against the expanded territory's highest forecast demand over all seasons, with
10% operating headroom using capacity credits. It must require no more than three new CCGTs,
finish within 12 played months and commit at most half the starting cash in total project capital.
Schedule the lines to finish with the new generation, then commit the commissioned plants under
AGC. Across a full year, including the new city's seasonal peak, reliability must stay at least
99.9%, cash must stay above one third of the starting amount and the game must remain Active.
Exercise this with property-generated simulation seeds, in addition to the existing start checks.

This is region-data tuning, not a new simulation rule or save schema. Keep `SIM_RULES_VERSION` and
the save/log formats unchanged. New games load the new region fingerprint. Existing saves and
archived command logs keep their original region definitions, costs, city loads, fleets and build
times; they replay as before. Applying this balance to an existing run would require an explicit
migration, which is outside this amendment. Start a new game to use the faster pacing.

## 19. Amendment A8: Automatic AGC recovery after a system-wide trip (2026-10-22)

After protection enters Blackout and trips the fleet, AGC immediately restarts each connected,
commissioned dispatchable plant that had AGC enabled and was Online or Starting immediately before
that trip. Restart candidates are captured before any unit trips, all trips happen first, and
restarts then happen once in ascending PlantId order at the same tick's ending instant. There is
no extra waiting tick and no repeated start attempt while a unit is already Starting.

A restart uses the plant's full normal start-up duration and emits the existing PlantStarting and,
for zero-duration starts, PlantOnline events. It does not bypass ramp limits, reset hydro water,
or instantly restore load. The existing staged black-start restoration resumes load as output
becomes available; solar reconnects when that restoration first re-energises the grid. A tripped
hydro plant remains unplanned until the next normal hourly re-plan, just as after a manual restart.
Start-up costs are counted once per actual start and receive the normal daily/month weighting.

The horizon is the same as for a manual Operating start: a restart is eligible only when
`ending_tick + startup_ticks <= 1,440`. Finishing exactly at midnight is allowed. Units that cannot
finish within the representative day, including a 48-hour nuclear start, remain offline. No
physical start-up state carries into the next representative month.

AGC does not start previously Offline plants, units already Stopping, disconnected units, plants
under construction, or units whose AGC was off at the trip. It does not auto-restart an individual
over-frequency trip. A manual stop after an automatic restart cancels that start normally and is
not retried. Scheduled commitment actions remain suppressed for the day after a protection trip;
recovery does not rewrite the month's cyclic pattern or AGC choices.

This changes simulation outcomes: advance SIM_RULES_VERSION from 6 to 7. Log and save formats stay
the same, but older rules versions remain rejected by the existing replay compatibility checks;
those saves/logs require their matching previous engine. Pin the rules-version outcome digest and
cover automatic restoration, eligibility across mixed plant states and horizons, hydro scheduling,
and once-only start-up billing with regression/property tests.
