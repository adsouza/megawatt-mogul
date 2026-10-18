# Megawatt Mogul: Design Spec (Milestone 1)

- **Date:** 2026-10-18
- **Status:** Draft for review
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
  Start-up costs scale too: the representative day stands for that month's every day.
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
  `revenue_requirement = prudent operating costs (fuel, start-up, O&M) + depreciation + allowed_return × rate_base`.
  Year 1 uses a starting tariff from region data.
- **Rate base** is the undepreciated capital cost of plants and lines in service. Depreciation is
  straight-line over each type's book life (region data). Construction capital is paid in cash in
  equal monthly instalments over the build. It enters the rate base when the asset is commissioned.
- **Performance-based return.** The allowed return starts from a base (e.g. 9%). It is adjusted up
  or down by the year's **reliability** (against a target, e.g. 99.9% served) and **cost
  performance** (operating cost per MWh served against the previous year), within a band (e.g.
  7–11%).
- **"Used and useful" disallowance.** If firm capacity exceeds the year's peak connected demand by
  more than the allowed **planning reserve margin** (e.g. 20%), that excess fraction of the *newly
  added* rate base is disallowed. It is written off and never recovered through the tariff.
  Firm capacity is the sum of `capacity × capacity_credit` (§3.2).
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
- Command logging (seed + commands) as the basis for future replays and saves.
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
  the budget runs out, and unused water does not carry over to the next month in milestone 1. Hydro can
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
- **`company`**: the **`Company` aggregate**: cash, a `Ledger` of typed entries (fuel, start-up,
  O&M, construction instalments, revenue, write-offs), the rate base and its depreciation
  schedule, and satisfaction.
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
- Unserved energy is accumulated and drives reliability, strikes and lessons.

### 5.5 Domain events

Ticks and commands return `Vec<DomainEvent>`. Nothing is pushed through a framework.

- `PlantCommissioned`, `PlantStarting`, `PlantOnline`, `PlantStopped`, `PlantTripped`
- `CityConnected`, `LineCommissioned`
- `FrequencyDeviated { band }`, `LoadShed { mw }`, `SolarCurtailed { mw }`, `Blackout`
- `HydroBudgetExhausted { plant }`
- `LedgerEntryRecorded`, `StrikeIssued`, `StrikeExpired`
- `RateReviewCompleted { tariff, allowed_return }`, `CapitalDisallowed { amount }`
- `PhaseChanged`, `MonthClosed(MonthlyReport)`, `Bankrupt`, `LicenseRevoked`, `MilestoneReached`

### 5.6 Domain policies (event → reaction, inside the domain)

- **Regulator (reliability):** `MonthClosed` with unserved energy above the threshold →
  `StrikeIssued`. Three active strikes → `LicenseRevoked`.
- **Regulator (rate review):** `MonthClosed` for December → compute the revenue requirement,
  the PBR-adjusted return and any used-and-useful disallowance → `RateReviewCompleted` (and
  `CapitalDisallowed` if any).
- **Solvency:** cash below zero at `MonthClosed` for 3 consecutive months → `Bankrupt`.

### 5.7 Determinism (a hard invariant)

- All randomness comes from the session's seeded RNG.
- No wall-clock time.
- No iteration order that depends on a `HashMap`; use `BTreeMap`, or `Vec` indexed by ID.
- Single-threaded tick.
- The same seed + the same command log must produce an identical state hash.

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

Starting a plant mid-day is allowed, and start-up time is what makes it costly. This lets realism
replace an arbitrary rule. Committed plants are started automatically so that they are online by
their committed hour (lead time = start-up time).

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
    current reserve margin, and capital at risk of disallowance), so the player can see the
    consequences before December.
  - `RampWarningProjection`: compares the next 60 minutes of forecast net load with the committed
    *ramp capability* and produces warnings such as "Sunset ramp: solar −6.2 GW in 40 min; fast
    capacity covers 4.9 GW".
  - `LessonsProjection`: builds 2–3 plain-language lessons per month from the above, as typed
    `Lesson` values that the presentation layer localises.

### 6.4 The tick (owned by `GameSession::tick`, in a fixed order)

1. Advance the clock.
2. Compute actual demand for connected cities.
3. Compute solar availability.
4. Update hydro budgets.
5. Plants progress (construction completes at month boundaries; start-up; ramp toward target).
6. Auto-start or stop committed plants for the upcoming hours.
7. AGC re-targets auto-follow plants.
8. Imbalance = supply − demand.
9. Frequency update.
10. Protection (shed, curtail or blackout).
11. Accounting: accumulate energy per plant (MWh), start-ups and *served* energy. Nothing is priced
    per tick. At month close, each total is priced, weighted by `days_in_month` and **rounded to
    cents once per ledger entry** (round-half-even), so money never accumulates fractional cents.
12. Collect events. At tick 1,440: the month's totals are weighted, `MonthClosed`, policies run, and the phase moves to Review.

### 6.5 Ports

- `RegionCatalog`: `list() -> Vec<RegionSummary>` and
  `load(RegionId) -> Result<RegionDefinition, RegionLoadError>`.
- `CommandLogSink`: `append(&LoggedCommand)`.
- (Later) `SnapshotStore`.

## 7. Infrastructure (`crates/infrastructure`)

- **`RonRegionCatalog`**: reads `assets/regions/*.ron` into serde **DTOs**, then maps them through
  the domain's validating constructors ("parse, don't validate"). Errors carry file and field
  context.
- **`FileCommandLog`**: writes the seed plus commands, one RON record per line, to a path supplied
  by the caller.
- The region data is authored at a coarse cell resolution (on the order of 100 × 60 cells per
  region), with cities as named points and hand-tuned resource layers. Geographic accuracy follows
  real interconnection boundaries at the level of a stylised diorama, not GIS.

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
| Persistence | Command sourcing: `CommandLogLayer` records seed + commands through the `CommandLogSink` port. Aggregates are persistence-ignorant. Snapshots and save/load come in a later milestone. |
| Error handling | Typed `Result`s at every layer (`DomainError` → `CommandError` with `thiserror`). The domain never panics on player input; `panic!` and `debug_assert!` are only for bugs. Errors are mapped for display once, at the Bevy boundary. |
| Data integrity | Types first: newtypes, validating constructors and state enums. Debug and test builds run `InvariantCheckLayer` after each command or tick (generation = served + curtailed; cash change = ledger sum; plant bounds). |
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
- **Conservation:** each tick, generation = served + curtailed, and unserved energy is never
  negative.
- **Money:** the change in cash always equals the sum of the ledger entries, exactly.
- **Weighting:** each month's ledger entries equal the played day's priced totals × `days_in_month`,
  with exactly one rounding step per entry.
- **Hydro:** the energy a hydro plant produces in a played day never exceeds that day's budget.
- **Determinism:** the same seed + the same commands produce an identical state hash.
- **Robustness:** any random sequence of commands (state-machine test) never panics, and all
  invariants hold afterwards.
- **Parsing:** malformed region DTOs produce typed errors, never panics.
- **Rate review:** the new tariff × the past year's served energy = the revenue requirement (within
  one cent); the allowed return always stays within the PBR band; disallowance is zero whenever firm
  capacity is within the reserve margin, and never exceeds the capital newly added that year.

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
| Save/load and replays | The command log plus seed (already recorded); a `SnapshotStore` port |
| More regions | A new `.ron` file; the picker already appears for two or more regions |
| More plant types (wind, storage) | `PlantSpec` is data-driven; new behaviour goes in the `generation` module |
| More languages | New `assets/locales/<lang>/` folder |
