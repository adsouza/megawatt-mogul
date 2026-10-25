# Megawatt Mogul: Design Spec (Milestone 1; Milestone 2 begins at §28)

- **Date:** 2026-10-18
- **Status:** Draft for review
- **Amendments:** A1, a hydro peak-shaving schedule (§12, 2026-10-18); A2, proration of the
  partial-year rate review (§13, 2026-10-18); A3, over-frequency protection, a minimum-generation
  warning and frequency-performance strikes (§14, 2026-10-18); A4, rotating outages and staged
  under-frequency shedding (§15, 2026-10-19); A5, the inherited schedule, the shortfall
  confirmation, window and panel sizing, the city card and
  construction progress (§16, 2026-10-20); A6, save/resume before Plan 3 (§17, 2026-10-20);
  A7, faster expansion pacing (§18, 2026-10-22); A8, automatic AGC recovery (§19, 2026-10-22);
  A9, daily-cycle battery storage (§20, 2026-10-23, implemented); A10, portable saved games
  (§21, 2026-10-23, implemented); A11, wind farms and midnight-to-18:00 battery charging
  (§25, 2026-10-23, implemented under rules 14); A12, closing the rate-base loophole and
  making satisfaction count (§26, 2026-10-23, implemented under rules 13; lands before A11);
  A13, battery discharge from 16:00 (§27, 2026-10-24, implemented under rules 15); A14, global
  regions, the first Milestone 2 amendment (§28, 2026-10-24; Plan 6 global foundations
  implemented under rules 16; Plans 7 and 8 pending)
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
   band, the solar and wind forecasts, each hydro plant's water budget and the battery charge/discharge
   preview (§20). They **commit** generating plants hour by hour, queue construction and build lines to connect cities. The current regulated tariff and a
   projection of the next rate review are shown, but the player does not set them (§2.5).
2. **Operating** (runs). The 24 game hours run as 1,440 ticks (1 tick = 1 game minute) at a
   selectable speed: pause, 1×, 2×, 4×, 8×, 16× or 32×. At 1× a day takes about 6 real minutes
   (about 23 seconds at 16× and 11 seconds at 32×). The player
   adjusts plant targets, starts and stops plants, and toggles automatic load-following (AGC) on
   individual plants. Predictive warnings flag trouble ahead.
3. **Review** (paused). P&L, the merit order that actually ran, a timeline of reliability incidents,
   and 2–3 plain-language lessons generated from that day's events. Money and energy totals are
   shown for the whole month (§2.3). Battery reporting separates charging, delivered energy,
   conversion losses and unused energy cleared at midnight (§20.6).

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
- **Batteries use a daily cycle (§20, amended by §25).** Every played day starts empty. They may
  charge immediately from 00:00 until 18:00, independently of daylight, cover evening generation
  gaps from 16:00 (§27), and report then clear
  unused energy at midnight. Their energy flows and midnight remainder receive the daily accounting
  weight; instantaneous power, energy capacity and state of charge do not.
- **Seasons change visibly every few played days.** Months drive the demand shape, solar
  irradiance, wind availability and hydro water budgets, so seasonal swings become central to play.
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
  performance** (operating cost per MWh served against the previous year), and by customer
  **satisfaction** against a target (Amendment A12, §26.5), within a band (e.g. 7–11%).
- **Forward-looking reserve assessment.** The review peak is the higher of the past year's actual
  peak and the projected coincident peak over the next 12 calendar months of all cities connected
  at review time, including connections commissioned at that month's close. The projection uses
  the sum of their hourly mean load profiles, with seasonal/monthly factors and no random noise;
  it is not the sum of individual cities' separate peaks. Unbuilt connections do not count.
  Firm capacity is the sum of `capacity × capacity_credit` (§3.2), and the allowed **planning
  reserve margin** (e.g. 20%) is applied to this review peak. Batteries count by accredited
  duration, `discharge MW × ladder(hours)`, not by nameplate MW (Amendment A12, §26.3).
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
  Lines with no plant endpoint (city-to-city) are never held: each passed the need test when it
  was ordered, because a line whose ends are already joined is refused (Amendment A12, §26.4).
- **Satisfaction** depends on reliability and on the tariff relative to the region's reference
  tariff, and it adjusts the allowed return (§26.5). Overbuilding that raises the tariff above the
  reference therefore hurts twice: through disallowance and through a lower allowed return.

The player profits by running *better than the formula expects*: cheap dispatch, high
reliability, and building only what the reserve margin justifies.

### 2.6 Regions

The player chooses a region when starting a new game. Milestone 1 ships two. Milestone 2
(Amendment A14, §28) adds six more worldwide, with time zones, computed sun, southern and monsoon seasons,
local currencies, a multi-area SGCC region, and a world picker that replaces the North America
overview below:

- **Eastern Interconnection (North America):** the US east of the Rockies plus central and eastern
  Canada, **excluding Quebec** (its own interconnection) and **ERCOT**. The Texas Panhandle belongs
  here because it is synchronised with the Eastern grid.
- **Western Interconnection (North America):** the western US, British Columbia, Alberta and
  **Baja California** (the northern Mexican state that is synchronised with the Western grid).
  El Paso belongs here. Baja California Sur is not included; in reality it is an isolated system.

Each region is a **single balancing area** (copper plate, §5.4). Both run at 60 Hz, and money is
in US$. (Under A14, a multi-area region balances each synchronous island separately, §28.9.)
The two regions play differently:

- **East:** about 4× the load, dense cities, a legacy coal and nuclear fleet, and Great Lakes
  cooling water.
- **West:** long distances, mountains, southwest solar, Pacific Northwest and BC hydro, and a steep
  evening duck curve.

**New game always opens setup**, including when exactly one region is installed. The editable
seed and its validation appear only here, alongside region selection. The opening menu and Load
game have no seed setting. Back preserves the draft seed; invalid seeds disable all region starts
but never prevent opening setup. Region clicks use the current edited value. On launch, scan
snapshots before choosing the first screen: an empty save directory opens this setup directly.
Back still opens the main menu. Existing snapshots, including unreadable ones, keep the main menu
and Load game available; a directory error must not be treated as an empty directory.

The picker shows a North America overview with each available region coloured from its region
file's geographic outline. Hovering a region highlights it and its labelled card; clicking the
region starts a game with the entered seed. Excluded areas and ocean are not selectable. Each
card also offers a keyboard-accessible start button, a short regional description and the home
city. Map labels follow the UI's 4.5:1 text-contrast floor. Custom regions without a geographic
preview remain selectable through their cards; broken entries retain their disabled/error state.

## 3. Milestone 1 scope

### 3.1 In scope

- Main menu, then new-game setup with region selection and an editable seed, then a new game.
- Both regions as data files (`assets/regions/*.ron`).
- A 3D diorama map built from the region's terrain cell grid, with phase-specific overlays (§8.3).
- Seven generating plant types plus battery storage (§3.2, §20, §25), plant construction, and
  transmission-line construction for connectivity. Batteries are implemented under Plan 4;
  wind and the revised charging window are specified under Plan 5, not yet implemented.
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
| Wind (§25) | 300 MW | 0% | n/a (resource) | n/a | n/a | $0/MWh | $0.30B | 3 | Correlated variability and overnight generation |
| Hydro | 800 MW | 0% | 100 MW/min | 5 min | $1k | $0/MWh | $1.2B | 12 | Energy-limited dispatch |

- **Battery storage** is a buildable asset kind alongside the seven generators: separate
  charge/discharge MW and stored MWh, automatic 00:00–18:00 charging and evening gap support,
  and no energy carried past midnight. Its initial tuning is in §20; §25 amends its charging
  contract. It is not a primary energy source.
- **Hydro** has a **daily water budget** (MWh per day, varying by month and site). Output stops when
  the budget runs out, and unused water does not carry over to the next month in milestone 1. Under
  AGC, hydro follows a **peak-shaving schedule** that spends its water on the day's highest net
  load (Amendment A1, §12). Hydro can
  only be built on cells with a hydro resource.
- **Solar** output is irradiance × sun angle × cloud noise. Protection can curtail it but nobody can
  dispatch it up.
- **Wind** output is capacity × site wind potential × the shared regional wind factor (§25).
  It can generate at any hour. Protection can curtail it; thermal dispatch controls do not apply.
- **Regulatory accounting values** (region data): capacity credit, which is the share of capacity
  the regulator counts as firm (thermal 0.95, hydro 0.8, solar 0.3, wind 0.2 under §25; batteries
  by accredited duration under §26.3, 0.85 for the shipped four-hour battery); and book
  life in months (nuclear 480, coal 480, CCGT 360, peaker 360, solar 300, hydro 600, wind 300).
- **Siting constraints** come from cell resources. Nuclear and coal need cooling water, solar output
  scales with irradiance, wind output scales with wind potential, and hydro needs a hydro site.

### 3.3 Out of scope for Milestone 1

- Transmission congestion and power-flow physics.
- Generating plant types beyond the seven in §3.2; storage beyond the daily battery cycle of §20
  (including pumped hydro, overnight/seasonal carry-over and degradation).
- Discrete weather events (storms, damage and weather outages). Ordinary solar clouds and the
  correlated wind availability process in §25 are in scope.
- Competitors and markets.
- Loans.
- Cross-platform command replay. Portable state and supported save migrations are defined in §21.
- Audio.
- A tutorial campaign.
- More regions (Milestone 2: Amendment A14, §28).
- Separate minimum-on/minimum-off dwell timers. Existing commitment feasibility already requires
  each recurring off-gap to fit shutdown ramping plus start-up (§6.1); these restrictions remain.
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
  Batteries reuse plant identity/construction but have dedicated energy state and inverter
  dispatch (§20.2–§20.5); thermal commitment, start-up and ramp rules do not apply to them.
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
  - **Battery scheduling** (§20): daylight valley filling and evening peak shaving, with
    separate charging load, finite stored energy and customer-first dispatch.
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
  unconnected cities are not customers. Under A14 the copper plate is each synchronous island:
  a component joined by AC lines. HVDC links move scheduled power between islands (§28.9).
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
  | Below −1.5 Hz, or shedding for N consecutive ticks | **Blackout**: generating plants trip; eligible AGC units restart under §19. Batteries suspend flows and retain charge under §20.5 |
  | Above +0.5 Hz | Curtail the residual surplus up to available solar; retain AGC's surplus-only, ramp-limited targets |
  | Above +1.5 Hz for N consecutive ticks | **Over-frequency trip:** the most expensive producing plant trips (Amendment A3, §14.2) |
- Battery charging is additional grid load, separate from customer demand; battery discharge is
  a supply contribution. The complete balance and protection ordering are in §20.4–§20.5.
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
- `BatteryDayClosed { plant, unused: MegawattHours }` (daily physical amount, including zero; §20.6)
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
  before executing any commands (§7). Across builds or platforms command replay has no promise.
  Format-1 saves verify that replay with the matching engine; portable format-2/3 saves restore and validate
  portable state without rerunning history (§17.3, §21).

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
| `SetBatteryEnabled { plant, on }` | Planning, Operating (§20.2) |
| `AdvanceTicks { n }` | Operating |
| `FinishReview` | Review → the next month's Planning |

Battery controls are separate from generating-plant controls: commitment, dispatch-target,
AGC and start/stop commands reject batteries with a typed kind error (§20.2).

**Commitment is a daily pattern that repeats throughout the month.** Treat the 24 hourly flags as
a circle. `CommitPlant` validates every off-gap between committed runs, including the gap across
midnight. Each must fit both shutdown and start-up. Since the pattern does not constrain the
plant's output at shutdown, reserve the worst-case shutdown duration
`ceil(capacity / ramp_per_tick)` ticks, then `startup` ticks. An insufficient gap returns
`CommitmentInfeasible { gap, shutdown, startup }`, without changing the session. A gap equal to the
required sum is valid. All-day and all-off patterns have no daily restart and are valid; nuclear's
48 h start-up therefore makes it must-run for the month or off for the month.

This already creates effective minimum off-gaps for both coal and CCGT in the shipped regions:

| Plant | Worst-case shutdown | Start-up | Required off-gap | Shortest gap with hourly commitment flags |
|---|---|---|---|---|
| Coal | 600 MW / 6 MW/min = 100 min | 480 min | 580 min (9 h 40 min) | 10 hours |
| CCGT | 500 MW / 25 MW/min = 20 min | 120 min | 140 min (2 h 20 min) | 3 hours |

These gaps include ramp-down and restarting; they are not periods spent fully offline before
starting again. A two-hour CCGT off-gap is infeasible even though its start-up alone takes two
hours. The same feasibility formula applies to other dispatchable kinds using their region data.
There is no independent minimum-on timer or additional minimum-off dwell after shutdown; those
separate constraints remain out of scope. Start-up, minimum stable output, ramp limits and
commitment feasibility are distinct rules.

**Initial state comes from the pattern.** At `BeginOperating`, a plant committed at hour 0 is
already online at its selected initial target (minimum stable output by default). An off plant
whose next run needs a start before midnight begins in `Starting` with the implied remaining
ticks. Shutdown tails crossing midnight are initialised conservatively from capacity and their
elapsed ramp-down time. Other dispatchable plants begin offline. This boundary initialisation is
not an operating tick, and does not carry a 48 h start-up between calendar months. Solar availability
and hydro budgets are initialised independently as before.
Boundary dispatch must subtract every connected shutdown tail from the load allocated to the
online AGC fleet, regardless of the stopping plant's AGC setting. These plants still generate
but cannot participate in dispatch. Count each tail once, alongside manual online output and
expected solar, both before outage sizing and when balancing against the final outage load.
Cap a hydro shutdown tail by its refilled day's physical water availability before counting it;
resource-limited synthetic output must never displace generation that can actually be delivered.
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
    connected primary generators producing in that tick, `None` when none does. Battery
    discharge has no independent fuel price and is excluded from this marginal-cost calculation.)*
  - `MonthlyReportProjection`: P&L, the merit order used, incidents and unserved energy.
  - `PlantHistoryProjection`: starts, trips, energy and revenue per generating plant. Battery
    history instead separates charge input, delivered output, losses and unused energy (§20.7).
    A generating plant's revenue is its weighted merit-order energy times the tariff in force
    that month, rounded once per plant
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
7. Imbalance = supply − served customer load − battery charging load (§20.4).
8. Frequency update.
9. Protection (shed, curtail or blackout, and an over-frequency trip of one plant, after which AGC
   re-targets again in the same tick; §14.2). The `TickSummary` reports the post-protection state.
10. Accounting: accumulate energy per plant (MWh), recurring daily starts, month-boundary starts
    and *served* energy. Nothing is priced per tick. At month close, apply each total's weight
    (§6.1) and **round to cents once per ledger entry** (round-half-even), so money never accumulates
    fractional cents. Retain only the whole-month offline summary for next month's start charging.
11. Advance the clock and collect events. After the final interval ending at tick 1,440, capture
    each battery's unused energy and clear it exactly once (§20.6). Then weight the month's totals,
    emit `MonthClosed`, run policies, and move the phase to Review.

Amendment A9 (§20.4–§20.5) extends steps 3–10 with coordinated battery/hydro plans, provisional
charge/discharge, post-protection energy settlement and energy-aware outage sizing. Batteries never
run through the ordinary generating-plant step or acquire synthetic opening output.

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

- **Opening screen:** a dark blueprint background, prominent game title and concise premise,
  with a decorative isometric grid at dusk: city blocks, a power station with roof-mounted
  smokestacks, a continuous river surface and a
  continuously turning waterwheel partially submerged in the water, with its underwater paddles
  hidden and three concentric ripples at each waterline crossing: expanding where the wheel
  rises out of the water, contracting where it dips in. Stagger and fade the rings for a
  continuous cycle. The two animated power routes merge just before the city.
  City windows start dark, switch on in shuffled order over three seconds, stay lit for two
  seconds, then switch off in a separately shuffled order over three seconds. Hold dark briefly
  and repeat, with fresh orders each cycle. The thermal station emits rising smoke only while
  at least one window is lit; its power-line pulses pause while every window is dark and resume
  without jumping as the first light returns. The hydro source and shared city feed flow
  continuously with the waterwheel. Use resolution-independent procedural artwork consistent with the in-game
  diorama. When a readable monthly autosave exists, put Resume game first as the primary,
  keyboard-default action and show its name, region, month and saved phase/time. It loads the
  newest automatic monthly snapshot by saved-at time (ties by slot ID), regardless of manual
  saves or what was last loaded. Without one, New game is primary. Retain Load game and Quit; keep the seed field exclusively in new-game setup. At narrow widths simplify to
  the menu column; scroll vertically if needed at extreme UI zoom. Artwork never intercepts input or
  starts/advances a session. Text retains the minimum contrast on opaque surfaces.
- **Save library:** carry the opening screen's navy/teal palette into Load game, with a larger
  header and two independently scrolling columns: large named manual-save cards on the wider
  left, and smaller cards for up to the 12 newest automatic monthly snapshots on the right.
  Sort each group by saved-at time, ties by slot ID; renamed automatic saves remain automatic.
  The cap filters the display only and never deletes snapshots or caps named saves. Retain
  region/month/phase/time, saved timestamp and game-over status, with Resume beside Rename.
  Keep Refresh/Cancel/Retry outside both scrolling columns and visible at minimum window size
  and increased UI zoom. Corrupt entries remain visible in a separate Unreadable saves group
  below named games, with their diagnostic and no resume/rename actions. Each column has an
  empty state; at extreme zoom stack the groups inside one bounded scroll area.
- **Base view:** a warm, toy-like tabletop diorama (low-poly, soft shadows, tilt-shift).
  A terrain cell is one world unit. Fit each city cluster and plant model, including its base,
  inside a centred 0.8 × 0.8-cell footprint. Plant models scale uniformly. Cities retain a fixed
  footprint and scale only their height, linearly with base demand in Planning and live demand
  in Operating. Use the largest regional base load as a stable reference: its base demand gives
  a vertical model scale of 3, and live demand can grow above this. Disconnected cities retain
  the current forecast demand height; supply shortages change lights, not height. Visual tuning
  may shrink models (40–100%) and cannot expand them outside their cell.
  Selection outlines occupy at most 0.9 × 0.9 of a cell and sit just above the terrain. Rendering
  and picking use the same fitted geometry; labels follow the actual scaled roof height.
  Keep the default whole-region framing and allow close zoom to inspect individual cell-sized sites.
- **Planning overlay:** a grounded terrain and resource view (irradiance, cooling water, hydro
  sites, siting suitability). Sunshine uses a sequential purple/blue → teal/green → yellow scale
  with a visible 0–100% solar-potential key outside the map viewport. Use the same absolute scale
  in every region. This is the site's static game resource rating, not the percentage of sunny
  days or current weather; season, sun angle and cloud cover separately affect solar output.
- **Selectable map layers:** terrain, sunshine, cooling water, hydro potential, and suitability
  for nuclear, coal, solar and hydro. Omit gas (CCGT) and peaker suitability overlays: those
  plants accept any land cell, so separate overlays add no siting information. Batteries likewise
  have no suitability overlay (§20.7). The cell inspector
  still reports suitability for every plant kind.
- **Operating overlay:** supply indicators over the warm, lit diorama: subtle segmented output
  gauges around plant bases, city lights showing supply status and energised lines with flowing
  light pulses. Retain terrain colours, building
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
  colours; Control room restores its hologram. Gauge fill represents output/capacity, including
  a fractional final segment; an empty track indicates zero output. Gauges stay below 0.1 cell
  above the terrain, inside the selection outline, and hide outside the Operating control-room
  overlay or for disconnected/unfinished plants. Effects
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
there is no invented geographic rotation. Energised lines retain their pulse and show moving
light pulses along **illustrative supply routes**, labelled explicitly as such. Route each producing,
connected plant's proportional contribution toward cities with positive served demand over
in-service lines, using shortest paths by hop count with line-ID tie breaks. Net opposing
transfers before choosing a travel direction; unused or balanced lines have no pulses. These
routes are a presentation aid, not electrical load flow or a per-line MW/loading estimate.
Recompute directions from current output and served demand. Under §20, discharging batteries
are sources and charging batteries are sinks; these routes do not identify which generator
charged a battery. Hide pulses outside the Operating
control-room overlay and on unfinished/disconnected lines, during blackouts, or without supply
or served demand. Use a steady visual travel speed, freeze during pause/save dialogs, and reuse
render entities/assets across frames. The effects toggle controls bloom/DOF, not these indicators.
Actual per-line loading still requires a power-flow `NetworkModel`.

### 8.4 HUD by phase

- **All phases:** a top bar with month and year, season, phase, cash, reliability and satisfaction.
- **Planning:** a build menu (plants and lines), a selected-object inspector, a
  tariff panel (current tariff, projected rate review, reserve margin), and a commitment grid with
  the coming month's forecast band and stacked supply. The stack includes committed dispatchable
  capacity, water-aware hydro and expected solar from connected, commissioned plants. Solar uses
  site sunshine, the daily sun arc and monthly mean cloud clearness, matching the shortfall check;
  its separate expected-output curve is a reference, not an additional supply contribution.
  All fleets use `GameSession::supply_forecast()`, the same source as the warning: manual plants
  count their initial target and disconnected or unfinished plants supply nothing. Without enabled
  storage the net-supply and demand curves assess minute 0 and 59 of each hour, revealing boundary
  peaks hidden by mid-hour bars. The uncertainty band follows those same demand samples.
  With enabled storage, the forecast uses the shortfall evaluator's sequential, energy-limited
  supply preview. A signed battery contribution is stacked below generation: discharge raises
  supply and charging lowers the stack baseline, so its top is generation + discharge − charging.
  Battery reference curves show feasible flows, not requested allocations. A minute-resolution
  net-supply curve and customer-demand curve expose depletion inside an hour; the bars remain
  mid-hour samples. Disabled, disconnected and unfinished batteries contribute nothing. This
  expected-day preview assumes feasible forecast charging from an empty daily inventory.
  The commitment grid is the primary planning tool. The inspector shows cell resources and
  suitability, city details, construction progress, or a commissioned plant's operating status,
  grid connection, capacity, minimum output, initial target, ramp rate, start-up time and cost,
  marginal cost and fixed monthly O&M. Solar sites show their solar potential; hydro plants show
  the month's daily water budget. Batteries have a dedicated card and schedule preview (§20.7),
  with no thermal commitment controls. Omit dispatch-only figures for solar.
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
  margin. Both the report and retrospective use this full width. Place the merit-order table
  in a right-hand column beside the summary (P&L, reliability, incidents, lessons and regulation)
  on the left. Both columns scroll independently; at extreme UI zoom, stack them in one bounded
  scroll area. A pinned **Supply demand retrospective** button switches the modal to the complete
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
| Persistence | Command sourcing with versioned replay inputs (§7.1). A complete in-memory command journal supplies atomic, self-contained saves through `SaveStore` (§17); `CommandLogSink` remains the diagnostic recording adapter. Aggregates remain persistence-ignorant. Format 2 adds portable state and migrations (§21). |
| Error handling | Typed `Result`s at every layer (`DomainError` → `CommandError` with `thiserror`). The domain never panics on player input; `panic!` and `debug_assert!` are only for bugs. Errors are mapped for display once, at the Bevy boundary. |
| Data integrity | Types first: newtypes, validating constructors and state enums. Debug and test builds run `InvariantCheckLayer` after each command or tick (on the post-protection `TickSummary`: generation = Σ by_plant = Σ by_kind = served + battery_charge + imbalance (§20.4), 0 ≤ served ≤ demand, curtailed ≥ 0, and generation matches the connected plants' outputs; cash change = ledger sum; plant bounds). |
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
  battery_charge + imbalance (signed; §20.4), served = demand × (1 − shed), unserved = demand × shed, and
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
- **Battery conservation:** daily charge input = delivered output + conversion losses + unused
  internal energy at midnight; every tick preserves energy bounds and exclusive charge/discharge
  (§20.8). Stored energy is never credited twice or carried into the next played month.
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
| Further save-schema migrations | Portable snapshots and retained schema readers/golden fixtures (§21) |
| More regions | A new `.ron` file; the picker already appears for two or more regions (Milestone 2, A14, §28, specifies eight) |
| More plant types (beyond §25; storage beyond §20) | `PlantSpec` is data-driven; new behaviour goes in the `generation` module |
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

Amendment A9 (§20.3) subtracts the coordinated battery discharge plan from the residual before
allocating hydro. The water budget, commitment eligibility and hydro emergency rules still apply.

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

With batteries, §20.5 first removes battery discharge and uses feasible daylight charging. Surplus
and headroom gates use the complete bus balance, including charging, and exclude unavailable
storage energy. Batteries are excluded from the generating-plant trip candidate list.

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
- **The existing response comes first.** AGC queues only the required surplus reduction in
  reverse merit order, within each online unit's one-tick ramp and minimum output. At +0.5 Hz,
  curtail the remaining surplus up to available solar before considering an individual trip.
  Curtailment never forces dispatchable outputs or targets to minimum. A residual beyond solar
  waits for the queued ramps or a sustained, safely gated trip; it must not turn a transient
  oversupply into a fleet-wide instantaneous deficit. Recompute curtailment and AGC after a trip
  against the surviving outputs, without stacking a second tick's ramp allowance.
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
      at the November 2029 close under rules 14, after a notice and FrequencyPerformance strikes
      in August, October and November (September stays below the failure threshold), before any
      December review sees the new plants. The original Plan 1 trace ended in October.*
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

For a fleet with batteries, §20.5 supersedes the two-endpoint shortcut below: evaluate every
remaining minute with a sequential stored-energy budget. Charging is optional and cancelled
before shedding customers; battery nameplate MW is never counted as unlimited generation.

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

Battery fleets use the sequential daily preview in §20.5, including feasible forecast charging
from empty. It is an expected-day preview, not a promise that weather or manual actions will
provide the forecast energy. Storage-free fleets retain the existing evaluator.

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
- `GameSession::hydro_forecast()` exposes the pure hour-0 allocations for Planning without
  changing the live `hydro_schedule()`. The forecast chart uses these allocations for connected
  AGC hydro (including the expected battery adjustment), zero for dry or disconnected hydro,
  and initial targets for committed manual hydro, matching the check's hydro rules above.

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
  - the prompt fits at 1280×640.

### 16.7 Window and panel sizing (playtest, presentation only)

Playtesting found two sizing problems. On a large monitor the window opens at Bevy's default
1280×720. The side and bottom panels have fixed sizes, so a player with room to spare can't give
the commitment grid, the charts or the plant list more space.

- **Display target and window minimum.** Support displays of at least 1280×720 logical pixels
  at the default UI zoom. The minimum client area is 1280×640, reserving 80 logical pixels
  vertically for the title bar and top/bottom system bars, with no horizontal allowance.
  This is a conservative cross-platform budget; the engine does not query native OS work areas.
- **The opening size.** The window opens at about 80% of the primary monitor's logical width and
  height, clamped between the 1280×640 minimum client area and the monitor minus that desktop
  budget. Centre within the reserved area: full monitor width, 40 logical pixels from the top
  and 40 from the bottom. Apply the same bounds to restored sizes and positions, respecting
  monitor scale and origin. On unsupported smaller displays, clamp to the available area. If no
  monitor is known, keep the default size.
- **Resizable panels.** The outer panels can be resized by dragging their inner edge:
  - Planning's inspector (left), build menu (right) and bottom panel;
  - Operating's side panel (right) and bottom panel.

  Each panel opens at its current size. That size is also its minimum, so everything that fits at
  1280×640 still fits. Each panel also has a maximum, so the map always keeps a usable area:
  - a side panel takes at most 40% of the window's width, and a bottom panel at most half its
    height;
  - the map's **area**, the space the panels leave it, never drops below 320×200. The map drawn in
    it is a little smaller: it sits inside that area's margins and under the layer buttons;
  - the Operating bottom panel also leaves the side panel above it its minimum height: the gauge,
    the selected plant's details and three rows of the plant list. At about 1280×640 it barely
    grows.

  Content that already fills the available width or height, such as charts, the plant list and
  scroll areas, uses the extra room.
- **The map still never moves on its own.** Only a drag changes a panel's size. A selection
  change, a tab change or a phase change leaves it alone, as before. A panel's size lasts for the
  rest of the run and is saved with client preferences (§17.1).
- **The Operating side panel, from top to bottom** (playtest):
  1. the frequency gauge;
  2. the selected plant's details, in a block of fixed height. With nothing selected, a
     placeholder of the same height stands in;
  3. the plant list, which fills the rest of the panel's height and scrolls, with at least three
     rows at 1280×640.

  The list's position doesn't change when the selection does.
- **Out of scope:** fullscreen. Window geometry is retained by client preferences (§17.1).

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

#### Cancelling an unfinished line (playtest)

Selecting a line on the map selects that line, rather than starting another line drawing.
In Planning its inspector shows construction progress and **Cancel construction** while it is
unfinished. The confirmation defaults to **Keep building**; Escape and clicking outside also keep
the order. Confirming removes the line and all remaining monthly instalments. Costs already paid
remain spent, with no refund. The route can be ordered again. In-service lines cannot be cancelled,
and line construction cannot be cancelled in Operating or Review. **Cancel drawing** in the build
menu only abandons choosing endpoints, without cancelling an existing order.

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
- **Resume game** on the opening screen loads the most recent readable automatic monthly
  snapshot through the normal verified load path, entering paused. Manual saves never determine
  this target. It is the primary button and initial keyboard focus; Enter activates it. While
  loading, keep the opening screen behind a compact Resuming game progress dialog with Cancel;
  do not show the save library during a successful shortcut load. After the first opening-screen
  paint, verify this snapshot in a background worker and retain one candidate in memory. Keep
  the menu interactive; do not install a session, advance simulation, or create recording files
  speculatively. Prepare snapshot encoding/compression and the diagnostic command prefix in memory
  during preload too; retain the immutable game boundary so activation need not capture it again.
  Bind prepared bytes to that exact boundary. Resume writes them to a fresh recording only after
  rechecking the source revision. Resume reuses the verified candidate without replaying it twice, waiting for
  the same worker if necessary. Any other menu choice or cancellation drops the candidate and
  cancels pending work; late results cannot install a game. Bind the cache to the slot revision
  and re-check the source before opening a fresh recording on explicit Resume. A load failure opens the save browser with its normal error/cancel/retry controls and never silently
  loads a different game. Refresh the snapshot index when returning from play. Hide Resume game
  when no readable automatic snapshot exists. Only a successfully scanned, empty save directory
  skips the opening screen on launch and goes directly to new-game setup; errors or unreadable
  files preserve access to the menu and save browser. This shortcut applies only at launch.
- **Load game** on the main menu separates named manual saves into large left-column cards and
  the newest 12 readable automatic monthly snapshots into smaller right-column cards. Both
  groups scroll independently; the display cap never deletes files or limits the Save dialog.
  Entries are newest first, with name,
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
- **Delete** is available on Load game cards for manual and automatic saves. Ask for
  confirmation with the save's name and identifying details, warn that deletion is permanent,
  and default keyboard focus to Cancel. Choosing Delete or cancelling the confirmation changes
  no files. Confirming removes only that snapshot, under the slot's OS lock and expected raw-byte
  revision; a changed slot is rejected. Unreadable snapshots can also be deleted when their valid
  slot ID and raw-byte revision are known. Keep errors visible with Retry and Cancel; disable
  cancellation during the deletion worker. Directory-sync failure reports unconfirmed deletion
  durability, and Retry safely confirms an absent file without deleting a replacement. Refresh
  the library and opening screen's newest-automatic Resume choice after deletion; reveal the next
  snapshot when a capped automatic column has more entries. Never load a game to delete it.
- **Resume** restores the exact accepted-command boundary: all simulation state, including RNG
  progression, construction, plant overrides and hydro water remaining, plus every projection.
  An Operating save at 14:37 returns to Operating at 14:37. A Review save returns to that Review;
  a finished game remains finished and can show its final report. Every load enters **paused**,
  including after the `OnEnter(Operating)` hook, until the player explicitly chooses a speed.
- **Month-close autosave** captures immediately after the successful command that emits
  `MonthClosed`, after its projections have been applied and before a later `FinishReview`.
  Thus it preserves the completed report and any game-over outcome. Each new game or successful
  resume gets a new recording-run ID and its own automatic slot. Later months replace that run's
  autosave; they never overwrite a manual slot or the source of a resumed branch. Capture is
  synchronous at that exact boundary; compression and durable writing run in the background.
  Success opens no dialog and does not block Review, the retrospective, Continue or later play.
  Serialize captures in order and resolve each expected revision after the preceding write.
  Only a failure opens the Retry/Continue-without-saving barrier (§21.5).
- **Menu** offers *Save and return to menu*, *Leave without saving*, and *Stay*. A failed save
  keeps the session and the dialog available for Retry or Cancel; it never performs the leave.
  The game-over modal exposes Save too. **The OS close button still quits without confirmation**
  (the earlier user ruling); it does not promise an exit autosave. Main-menu Quit is unchanged.
- Manual saving and loading show progress and disable conflicting actions. Opening the save/leave
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

A save is a **self-contained checkpoint**, never a reference to a live log. Formats 2 and 3 include
portable domain/projection state; format 1 contains replay inputs only (§21). Format 3 adds lossless
run-length encoding for identical tick batches (§21.7), retaining command-only reader support. `Game` retains the
`LogHeader`, the exact region source bytes loaded at creation (extended only by explicit migration),
and an in-memory `Vec<Command>` containing every successfully executed command, in order. Append
once after the handler succeeds, regardless of the disk sink's health, and update projections in
the same command call. Rejected commands are absent. Preserve original `AdvanceTicks` batches,
including successful zero-tick commands; never merge, split or reinterpret their execution boundaries.
Lossless storage encoding must expand those exact batches before verification or replay.

`Game::checkpoint()` captures those inputs, the accepted-command count, the current
canonical complete-state hash (legacy format 1 uses `GameSession::state_hash()`) and a summary
derived from public queries. It has no I/O and neither
executes a command nor reads installed region files. Capture uses one boundary, never a mixture
of a pre-tick clock, a post-tick RNG and an earlier report. Memory is retained for the current run;
in-memory history remains complete; storage compression never drops or combines commands. Portable snapshots retain
the complete audit trail and all projections (§21).

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

Each new `<opaque-slot-id>.mgsave/` is one independent, self-contained directory. Its
`snapshot.ron.z` manifest is compressed text and references a compressed binary chart file per month.
Copying a save means copying its entire directory. It includes:

- directory format 4, distinct from command-log and portable-state schema versions; standalone
  formats 1–3 remain readable, and flat codec 3 supplies the manifest's internal RLE state (§21);
- a replay compatibility identity: simulation-build fingerprint and target platform, recorded
  for diagnostics only;
- the original `LogHeader` (rules/log versions, settings, seed and region fingerprint);
- a byte-exact embedded copy of the original region RON, and the complete command prefix;
- complete versioned domain/projection state, with monthly chart data split into binary files;
  all references bind file names, months, sizes and SHA-256 digests. Reassembly retains the exact
  canonical projection bytes. The accepted-command journal remains a lossless RLE audit trail;
- command count, canonical snapshot/journal hashes, phase/date/minute/status summary;
- slot ID, manual/automatic kind, recording-run ID, display name and saved-at UTC time.

Use a small version envelope around a checksummed payload. Infrastructure hashes the exact payload
bytes with SHA-256 and verifies length/checksum before decoding its command DTOs. Unsupported save
versions fail before decoding version-specific payloads. IDs are adapter-generated opaque values;
clock times and IDs are injected metadata, never simulation inputs. A slot revision is the SHA-256
of its committed manifest bytes (or historical flat file), used to confirm the specific revision
being overwritten. Explicit Resume also rechecks all referenced month-file digests.

For command-only saves, reproducibility requires the matching simulation: replay must reproduce the
recorded command count, session state hash and phase/date/status boundary. For portable formats 2/3,
load restores the snapshot directly and does not rerun historical simulation/economic choices.
Its canonical memento representation and schema version are independent of Rust Debug output and
simulation rules; changing future rules alone does not invalidate a compatible snapshot. See §21
for the migration and schema-evolution policy.

Keep the simulation-build fingerprint and target platform for diagnostics. Generate the fingerprint
from sorted relative paths and exact bytes of active domain/application/infrastructure
sources/manifests, workspace manifest/lockfile, compiler identity, target,
profile, features and flags. Rendering, checkout location and timestamps remain excluded. A writer
stamps its own build. Region content retains its own fingerprint.

Load verifies the save envelope/limits/checksum and exact embedded region before installing any
candidate. Never substitute current shipped region data. A retuned or deleted shipped region does
not break a save. Rules-6/7 command-only recovery is archived outside the game (§21.1); already
converted snapshots load normally. Rules-8 command-only saves retain their current-engine reader.
Unknown formats or failed verification return an error without changing files or the running game.
The old Debug-hash caveat remains confined to format-1 verification. New saves use explicit portable
state schema 1, with a migration/upcaster required for incompatible data-model changes.

### 17.4 Reconstruct a runnable game, then publish it

`SaveStore` is an application-owned `Send + Sync` port, implemented by infrastructure. Its typed
operations list entries with per-slot errors and optional raw-byte revisions, read a verified
bundle, write a new slot or replace an expected revision, rename, and delete an expected revision. Application types contain no serde, paths or user-facing copy;
infrastructure owns DTOs, bytes, hashing, IDs, filesystem paths and atomic publication. Game owns
localised dialogs and request orchestration. No Bevy, I/O or persistence fields enter the domain.

Restoration builds a **candidate `Game`**, not just the `GameSession` returned by today's `replay`:

For portable formats 2/3, decode and validate the domain/projection state, audit trail, hash and
summary directly (§21), then continue at step 4. For command-only saves (expanding format-3 runs
first), the reconstruction path is:

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

- Opening-screen Resume game chooses the newest automatic save despite newer manual saves,
  stable slot-ID ties and unsorted listings; verify mouse and Enter activation, small-window
  layout, paused restoration, empty-directory routing, manual-only/corrupt/error cases and Back.
  Preloading starts after the first paint, stays interactive, creates no files or live session,
  reuses in-flight verification, and discards ready/late candidates on other choices. A source
  replaced or deleted after preloading cannot silently load the cached revision.
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
  exclusion, codec round trips and corruption rejection. All UI fits at 1280×640 and meets 4.5:1
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
commissioned dispatchable generating plant that had AGC enabled and was Online or Starting immediately before
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

Batteries added under §20 do not use this thermal restart path: they suspend flows during a
blackout, preserve charge, and become eligible again when generation re-energises the grid.

This changes simulation outcomes: advance SIM_RULES_VERSION from 6 to 7. Log and save formats stay
the same, but older rules versions remain rejected by the existing replay compatibility checks;
those command-only logs require their matching previous engine. Rules-6/7 save migration is now
provided by §21, which supersedes that historical save rejection policy. Pin the rules-version outcome digest and
cover automatic restoration, eligibility across mixed plant states and horizons, hydro scheduling,
and once-only start-up billing with regression/property tests.


## 20. Amendment A9: Daily-cycle battery storage (2026-10-23)

**Status:** Implemented in the current working tree under rules version 8. Automated checks,
windowed launch and native cycle/save-resume checks on both regions pass. See the implementation record in
[Plan 4: Battery storage](../plans/2026-10-23-plan-4-battery-storage.md), following implemented
Plans 1, 2, 2b and 3. This amendment governs storage wherever earlier sections assume every plant
only generates electricity. It does not turn hydro water into stored electrical energy.

**Implemented amendment:** §25 supersedes this section's daylight charging window with
00:00–18:00 charging and adds wind to generation forecasts under rules 14. The 17:00 discharge start, residual-gap support (§23), energy conservation, customer
priority and midnight reset remain binding. Read the daylight-only rules below as the historical
Plan 4 contract wherever §25 explicitly replaces them.

### 20.1 Player contract and scope

- Build and connect batteries through the existing plant/line workflow. No battery is added to
  either starter fleet; buying storage is a player investment.
- Every representative day starts with **zero stored energy**, even if the previous month ended
  with a surplus. Construction and the enabled choice persist; electrical energy does not.
- **Morning and afternoon charging are both allowed.** The charging window is regional daylight
  before the configured evening discharge start: `sunrise <= t < min(sunset, discharge_start)`.
  Reuse the region's monthly solar sunrise/sunset calculation. This is a clock window, not a
  cloud threshold, and does not require an installed solar plant. Grid charging is allowed and
  can increase thermal generation; the UI must never label all stored energy as solar energy.
- The automatic charge plan favours the lowest forecast net-load minutes in that window. It can
  therefore use a low-demand morning or a sunny afternoon rather than always filling at sunrise.
  Actual available surplus can be absorbed sooner. Charging is best effort and never causes
  deliberate customer shedding.
- Outside daylight the battery holds its charge until the evening window. During
  `discharge_start <= t < 24:00`, it shaves the highest remaining forecast net-load periods.
  It cannot charge in this window. No overnight charging or morning discharge in this release.
- At midnight, report unused energy before clearing it. This explicit daily reset is a gameplay
  simplification: the representative day stands for every day of its month, not a continuous
  March-to-April physical day. The same charge/discharge/reset cycle is weighted across the month.
- First release has automatic operation and an enabled toggle. Manual MW targets, editable player
  schedules, price arbitrage, degradation, reserve bidding, pumped hydro, and energy carried
  overnight or between months are outside this amendment.

### 20.2 Model, tuning and commands

Add `PlantKind::Battery`, reusing `PlantId`, the plant network node and plant rate-base identity
for construction, lines, selection and accounting. Add dedicated `BatterySpec` and `BatteryState`
inside the generation context; do not represent storage as a free dispatchable generator, a
hydro budget, or negative generation. `PlantSpec` carries an optional battery specification,
required exactly for Battery and forbidden for the six generating kinds.

Battery state contains its enabled choice, internal stored MWh and separate nonnegative charging
and discharging MW. A commissioned battery is ready without a start-up; its operating mode is
Disabled, Disconnected, Holding, Charging, Discharging, Empty, Full or GridSuspended as appropriate.
These typed modes drive the card; the thermal Starting/Stopping state machine does not control it.
Under-construction batteries cannot operate. Generic construction may retain its existing status
representation, but generic plant stepping, AGC, commitment and restart paths must exclude Battery.

Initial shipped tuning, authored in **both region RON files** and the `test-fixtures` fixture:

| Field | Initial value |
|---|---:|
| Maximum grid charging power | 200 MW |
| Maximum grid discharge (`PlantSpec.capacity`) | 200 MW |
| Internal energy capacity | 800 MWh |
| Charge efficiency | 90% |
| Discharge efficiency | 100% |
| Evening discharge start | 17:00 |
| Capital cost / build time | $100M / 2 months |
| Fixed O&M / book life | $100k per month / 180 months |
| Start-up time and cost / minimum stable / marginal fuel cost | Zero |
| Regulatory capacity credit | Generic field zero; firm MW by accredited duration from rules 13 (§26.3) |
| Siting | Any otherwise buildable, unoccupied land cell |

This is 90% round-trip efficiency: apply the loss once, not once on each side. The model supports
separate efficiencies in `(0, 1]`, whose product is round-trip efficiency; the initial discharge
value of 100% makes the 800 MWh rating deliver four hours at 200 MW. These are game tuning values,
not a claim about current commercial battery prices or performance. Batteries can change inverter
power anywhere within their ratings in one minute; they have no thermal ramp/minimum/start costs.
Any inherited ramp field is unused for Battery, not an additional limit or a source of output.

Validating domain constructors reject missing/extraneous battery data, non-finite/nonpositive
power or energy ratings, efficiencies outside `(0, 1]`, an evening hour outside `1..24`, or a
month with no daylight charging interval before it. Generic costs/lifetime/build validation still
applies; require the battery's generic minimum, start-up, fuel cost and capacity credit to be zero.
The generic credit stays zero under §26.3, which derives battery firm MW from duration instead.
New tuning stays in region data/fixtures, never in simulation constants.

`SetBatteryEnabled { plant, on }` is accepted in Planning and Operating for a commissioned battery,
defaulting to enabled at commissioning. Turning it off cancels future flows but retains charge
until midnight. Enabling resumes the appropriate automatic policy at the next tick, with no energy
or start-up fee. The choice carries into the next month. A same-value command is a successful no-op.
Unknown ID, wrong kind, construction, wrong phase and game-over cases return existing typed errors
or an explicit typed extension, leaving state and command history unchanged on failure.
`CommitPlant`, `StartPlant`, `StopPlant`, `SetDispatchTarget` and `SetAutoFollow` reject batteries;
`SetBatteryEnabled` rejects generators. The HUD offers only the applicable controls.

### 20.3 Deterministic automatic schedules

Scheduling is pure, draws no RNG and uses the existing mean city demand and expected solar.
Times refer to minute intervals `[t, t+1)`; sunrise and sunset use the same boundaries as solar
availability. Work in chronological minute order and stable PlantId order, with fixed numerical
iteration counts/tolerances shared by live and preview paths.

1. **Charge plan.** For enabled, connected, commissioned batteries in PlantId order, valley-fill
   the remaining daylight charging minutes against forecast city demand minus expected solar plus
   earlier batteries' planned charging. Let `cap_m = min(charge_max, forecast_headroom_m)` and
   allocate `q_m = clamp(level - residual_m, 0, cap_m)` so `sum(q_m / 60) <= (capacity_MWh - stored_MWh) / charge_efficiency`. Choose the level to fill
   remaining space if the available minutes/power permit it, otherwise request the maximum feasible
   input. Conservatively bound requested headroom by expected solar plus reachable committed
   non-hydro generation after full city demand and earlier charge requests. Do not request extra
   hydro to fill batteries: its stored water can supply the evening directly. Actual surplus from
   any generator may still be absorbed under §20.4. This avoids a circular dependency between
   charging requests and the hydro plan built below. Add each plan to
   the residual before allocating the next battery. This is a request to AGC, not assumed energy.
2. **Discharge preview.** Project remaining evening minutes chronologically against primary
   generation, using one finite inventory per battery in PlantId order. Discharge only into a
   positive generation shortfall, bounded by inverter power and stored energy. Hold reserves
   whenever generation covers demand, even after 17:00. This is a delivery estimate, not a
   request to reduce generation. Do not reserve the same energy twice.
3. **Hydro coordination.** Allocate scheduled hydro against net city load without subtracting
   battery discharge. Batteries cover gaps left by hydro and other generators; they do not make
   those generators back off. Battery charging does not enlarge the hydro water budget or make
   the hydro planner burn water to fill batteries. Hydro's budget, commitment, ramp envelope and
   emergency override still apply. Generator AGC aims at full served customer load plus feasible
   charging requests, without subtracting planned or actual battery output.
4. **Re-plan.** Re-plan at day start, hourly, at the evening-window boundary and on the next tick
   after a battery is enabled/disabled or grid service is restored. Plan only the remaining day;
   use actual stored energy. Freeze elapsed allocations for history; do not rewrite them. A live
   afternoon plan may show expected future charging separately, but neither outage sizing nor
   battery support may spend energy not yet stored. Planning previews may
   forecast the whole cycle from empty, clearly labelled as expected, and use those projected
   charge receipts to plan evening discharge.

Charging schedules are requests; discharge schedules estimate gap coverage. Neither guarantees
energy. Forecast error, generation limits, disconnection,
protection and a player's generator controls can prevent a charge or leave energy unused.

### 20.4 Tick balance and energy settlement

Let `G` be primary generation, `D` battery discharge, `C` battery charging input, and `S` served
**customer** load. The copper-plate balance is:

```
imbalance = G + D - S - C
```

Keep `TickSummary.generation` as total supply including discharge, equal to sums of nonnegative
`by_plant` and `by_kind` output. Add separate charging power totals and per-battery flow/energy
records. Thus `generation = served + battery_charge + imbalance`; primary generation is
`generation - battery_discharge`. The battery kind in the supply stack represents delivered
output only. Neither charging nor losses are negative generation.

For one minute, with internal stored energy `E`:

```
E_next = E + charge_efficiency * C / 60 - D / (60 * discharge_efficiency)
charge_loss = (1 - charge_efficiency) * C / 60
discharge_loss = (1 / discharge_efficiency - 1) * D / 60
```

Maintain `0 <= E <= capacity_MWh`, `0 <= C <= charge_max`, `0 <= D <= discharge_max`, and never
charge and discharge in the same interval. Cap flows using the exact remaining one-minute energy
or headroom, including a fractional final tick; do not clamp an overdrawn result and invent energy.
Floating-point tolerance only normalises roundoff near zero/capacity, not physical overdraw.

Extend §6.4 without changing the seeded demand/solar draws or the generator target timing:

- After generating plants step, compute provisional battery flows from current actual supply,
  charge/discharge plans and grid conditions. Daylight charging takes available surplus after
  **full** customer demand, capped by power/room; it may absorb more than its scheduled request
  when surplus is already available. AGC may request extra generation to meet scheduled charging
  on the following tick. It never treats that request as an actual receipt this tick. Allocate
  actual available surplus/deficit among batteries in ascending PlantId order, consuming shared
  headroom as each allocation is made; fleet totals cannot allocate the same surplus twice.
  Taper charging requests ahead of full capacity and daylight cutoff using generator downward
  ramp capability. Reserve enough room for already-requested generation to ramp down; opportunistic
  absorption must not spend that reserved room and force an avoidable full-battery surplus.
- In the evening, battery output fills the actual deficit after generating plants, bounded by
  power and stored-energy caps. AGC continues asking generators to cover the full served customer
  load: a battery bridges their ramp/start-up gap, then stops as primary generation catches up.
  Do not reduce generation to create a gap for storage or inject discharge into an oversupplied
  bus. A persistent capacity shortfall can still consume the reserve until it is exhausted.
- Frequency/protection use net load including feasible battery charging. After protection changes
  output or served load, cancel charging where necessary and re-evaluate battery delivery/flows
  against the final state. Settle **one** energy change from the final post-protection flow record,
  the same record used by the summary and monthly accumulator. Do not debit a provisional flow
  and debit it again during redispatch. Preserve the existing distinction between the imbalance
  that moved frequency and the final post-protection imbalance (§6.4).
- Account for the final interval ending at 24:00 before performing the midnight close below.
  `AdvanceTicks` batching and simulation speed cannot change any flow, report or reset boundary.

### 20.5 Customers, forecasts and protection

Customer demand, served/unserved energy, reliability, strikes, satisfaction and revenue retain
their city-only definitions. Charging does not inflate peak customer demand, customer energy,
retail revenue or the denominator of reliability. Keep the frequency system-size basis as
`city_demand * (1 - outage)`, before protection shedding and excluding battery charging, with
the existing dark-grid handling. Optional charging must not dilute the severity of a shortage.

**Customer priority.** Charging is zero whenever customer load is shed (planned or unplanned),
while in blackout/restoration, or while frequency is below the normal band. Stop it before asking
protection to shed customers. On over-supply, reduce battery discharge first and allow feasible
charging only in its daylight window, before further solar curtailment/generator protection.
A full battery, a night-time battery or one without room is not downward headroom. Standing solar
curtailment still follows its existing release timing; do not count withheld solar as generated.

**Emergency discharge.** During the evening window an enabled battery covers an actual deficit
before customer shedding, including unexpected deficits beyond its preview, while obeying
energy/power limits. Outside this window it holds charge; it is not a general 24-hour emergency reserve in this
release. Emergency use reduces the next plan's available energy and can leave a later shortage.
A planned rotating outage may use this support to restore customer load only as fast as the
combined feasible headroom allows; do not drain batteries to serve fictional charging load.

**Blackout.** Suspend both flows immediately, retain stored energy and the enabled choice, and
exclude batteries from thermal trip/restart/start-up billing. They do not black-start the grid.
Once generating plants have re-energised it and protection completes restoration, an enabled
battery can resume within its current window. Recovery does not refill it. Disabled or isolated
batteries remain idle; all batteries still take part in the midnight inventory close.

**Energy-limited forecasts.** Any path counting battery contribution—Planning shortfall checks,
operating rotating-outage sizing, ramp warnings, AGC/trip headroom and restoration—must project a
single sequential inventory. A 200 MW battery with 10 MWh remaining cannot cover a whole hour.
For fleets with batteries, evaluate every minute of each assessed horizon rather than just the
hour endpoints; depletion or a window change can create the worst shortage inside an hour.
Operating outage targets use the largest minute shortfall share for that hour, preserving the
existing exact fraction/paced application rules. Retain §15.3's generator reach-by-hour-end
convention for AGC units, so an outage does not hold their forecast output down and ratchet itself.
Storage is always projected chronologically against its finite budget, never treated as one of
those unlimited AGC units. Use only energy already stored, with charging
cancelled in a shortage; future hoped-for charging is not firm cover. Planning's expected-day
preview may include feasible mean-forecast charging, starting from zero, and must expose that
assumption. Queries run on projected state, never consume live energy or RNG.

Minimum-generation warnings remain conservative: batteries add no committed minimum, and optional
charging is not guaranteed cover for an inflexible generator's surplus. Ramp replacement and
falling-output trip gates must include foreseeable depletion and window boundaries. Prevent a
predictable scheduled battery fall from outrunning the committed generator fleet; if the fleet
cannot replace it, warn/size a shortage instead of inventing storage output. Storage-free fleets
retain the earlier forecast and dispatch behavior.

### 20.6 Midnight, monthly accounting and regulation

After the final post-protection tick has settled, and **before** month-close construction,
regulatory work, report publication and autosave:

1. Capture each commissioned battery's internal remaining MWh, including disabled/disconnected
   batteries, and emit `BatteryDayClosed { plant, unused }` once in PlantId order. Include zero
   so an empty battery and a missing close are distinguishable. Construction projects have no
   active inventory and contribute zero report energy.
2. Accumulate the daily remainder as **unused battery energy at midnight**, then clear stored
   energy and both power flows. This is inventory discarded by the daily-cycle abstraction,
   not discharge, solar curtailment, customer unserved energy or a cash penalty.
3. Publish the monthly report with per-battery and fleet charge input, delivered output, charge
   and discharge losses, and unused energy. All are played-day MWh multiplied by `days_in_month`,
   including leap-year February. Preserve the unweighted curves for the retrospective. Every
   Review state and monthly autosave contains zero live stored energy plus the completed report.
   Keep the final tick record immutable, including its pre-close SOC and delivered MW; clearing
   live state must not erase the last minute from energy totals or history. Check tick/output
   invariants before this boundary reset, or explicitly account for the closed-day boundary.

For a day starting empty, within numerical tolerance:

```
charge_input = delivered_output + charge_loss + discharge_loss + unused_at_midnight
```

Fuel is charged for actual primary generation, including generation that served charging.
Battery output has no second fuel charge or start-up charge. Revenue remains tariff × served
customer energy, never tariff × battery throughput. Fixed O&M, construction instalments,
depreciation and allowed return use the ordinary plant rules; no per-tick money rounding.
Do not present a battery's discharge × tariff as extra company revenue. Its benefit is shifted
supply, avoided expensive generation and potentially improved reliability, with charging costs
and losses visible in company totals. Profitable operation is not guaranteed by enabling it.

*Superseded by Amendment A12 (§26.3) from rules 13: batteries are accredited by duration, hold and
write-off apply to them and to their lines, and the exemption below is the rules 8–10 history.*

Under rules 8–10, battery **regulatory capacity credit is zero**: a plant that starts empty and
needs same-day charging is not guaranteed firm supply. Its MW is excluded from the annual reserve
capacity sum, while its commissioned capital and line capital enter normal eligible rate base.
The existing MW-based excess assessment does not hold/write off a zero-firm battery or a line
whose only plant endpoint is that battery merely because the generating fleet is excessive. For
a line also ending at a new positive-firm generator, associate it with that generator (lowest
PlantId among positive-firm candidates), so adding a battery endpoint cannot shield excess
generator line capital. This is a deliberately limited regulator model,
not a duration-based storage accreditation scheme. Do not use nameplate MW or one arbitrarily
chosen evening hour as firm credit. Storage accreditation and prudence tests are future work.

A repeated capture/query, save retry, `AdvanceTicks` extending beyond close, resumed Review or
`FinishReview` cannot close the battery twice. `BeginOperating` also asserts/initialises zero
energy without re-reporting discarded inventory. A battery commissioned at month close starts
empty, and first operates on the next representative day. Its newly finished line cannot alter
energy or city demand in the just-completed day's report.

### 20.7 Queries, HUD, map and persistence

- **Views.** Extend `PlantView` with optional typed battery details: enabled, mode, grid connection,
  stored/capacity MWh, charge/discharge power ratings and current flows, efficiencies, charge window
  and evening start. Expose pure charge/discharge schedule previews with forecast assumptions and
  frozen elapsed allocations. No battery rows in the thermal commitment grid or thermal merit order.
- **Cards and construction.** Build/quote cards show both MW and MWh, cost, instalments, lead time,
  grid connection and automatic cycle. The operating card shows charge level, Charging/Discharging/
  Holding/etc., live MW, today's input/output/losses and Enable/Disable storage. Explicitly say
  that grid charging can use thermal generation. Keep generators' controls and labels unchanged.
- **Charts.** Show battery discharge as a separate positive supply contribution and charging as
  an additional-load curve/band, alongside original customer demand and served load. A separate
  stored-energy trace uses a labelled MWh axis; never stack MWh on a MW scale. Default to two
  side-by-side panes with the supply/demand pane allocated twice the battery pane's width. A
  “Combine graphs” toggle overlays the stored-energy line on the supply/demand plot, retaining an
  independent labelled MWh axis and hover values. Keep its distinct colour and visibility key
  outside the plot. The layout choice is shared by live history, archived months and the Supply
  demand retrospective and retained in client preferences across sessions and launches. Hide the
  control when the chart has no storage data. Short bottom panels use unit-only axis titles so
  rotated text cannot overlap the legend. At midnight show the
  pre-reset remainder as an explicit close marker, not an unexplained fall or discharge spike.
  All legends stay outside the plot; all labels meet the existing 4.5:1 contrast floor.
- **Review.** Add a storage summary and per-battery breakdown with input, delivered energy, losses
  and unused-at-midnight MWh, all clearly month-weighted. Also label the daily midnight remainder
  (for example, 100 MWh per day and 3,100 MWh in a 31-day month), so it is comparable to capacity.
  Show zero unused energy explicitly when
  storage was present. Keep storage throughput separate from the right-side generator merit-order
  table. A typed lesson may explain a nonzero discarded remainder; storage does not displace
  higher-priority reliability lessons or increase the three-lesson cap.
  In the summary view, battery storage has a third independently scrolling column after the
  summary and merit order. Omit the third column without storage. Below 680 logical pixels,
  retain all sections in a single scrolling report. Continue/Back stays pinned below the content.
- **World.** Add a recognisable battery enclosure model/marker within the existing 0.8-cell fitted
  footprint, with construction/selection/connection behavior. Charging batteries are sinks and
  discharging batteries sources for illustrative line routes, including city-to-city paths. Never
  label a route as actual electrical flow or prove a solar source from it. No battery suitability
  overlay: any buildable land cell is already represented by the build tool.
- **Localisation and input.** Follow pure presenter → draw → `UiAction` → `dispatch`, `ActionSink`
  getters and Fluent keys/coverage. Battery UI actions become domain commands; no UI code writes
  stored energy or policy state. Small-window/zoom layouts keep the new controls reachable.
- **Saves and replays.** Retain battery tuning in the exact archived region bytes, enabled commands
  in the complete journal, and battery state/schedules/accumulators in deterministic session state.
  Restore must preserve energy, RNG, reports and all projections at the saved boundary (§21). Loading mid-day preserves charge exactly and enters paused; it must not call
  day initialisation or clear the battery. A Review load preserves the completed unused-energy
  report and zero live inventory. Preload remains a pure candidate build, with no live mutations.

### 20.8 Versioning and acceptance

Implement this under `SIM_RULES_VERSION` **8**, advancing the currently implemented version 7.
The command addition and battery region data are carried by versioned command DTOs and archived
region bytes. Save envelope **3** (superseding envelope 2 under §21.7), portable schema **1** and
snapshot-rooted command-log format **2** are independent versions defined in §21. Rules-6/7 recovery used the archived historical engine
to create separate battery-capable slots; originals stay intact. Command-only historical
logs still need a matching engine. Validate versions and inputs before running commands, and fail
unsupported versions without changing source files. Never replay old years under the new rules.
The save library must retain unavailable entries and deletion access under its existing contract.

Required regression and property coverage:

- Empty at day start; daylight morning/afternoon eligibility, sunset/17:00/midnight exact boundaries,
  cloudy days, no-solar grid charging, full/empty/fractional final ticks and both efficiency factors.
- No simultaneous flows; bounds and conservation for generated state/command sequences, multiple
  batteries and stable ID ordering. Disable/re-enable, construction, disconnection and typed command
  refusals leave energy/state/history correct; refused commands change nothing.
- Actual charging is paid-for generation/load, never customer service or fictitious solar. It stops
  before any customer shedding. Forecast requests alone cannot put energy into a battery.
- Evening discharge displaces peaker energy in a crafted solar/daylight + evening-peak fixture;
  an empty battery does not reduce outages. No new blackout or avoidable frequency trip is introduced
  by charge start/stop, scheduled depletion, hydro coordination or full-capacity transitions on a
  covered, noise-free day. Emergency exhaustion follows ordinary shortage/protection rules.
- A battery that empties mid-hour is not credited for that whole hour. Planning previews, actual
  inventory forecasts, headroom and outage sizing share the same physical limits and consume no RNG.
- Final interval settled once, remainder captured before zeroing, exact month weighting, no duplicate
  midnight close under batched ticks/Review/resume. Customer revenue/fuel/OM/capital and annual reserve
  assessment follow §20.6, with no double fuel or invented firm capacity.
- Mid-charge, mid-discharge, disabled-with-charge, blackout, midnight Review and next-day save/resume
  agree with uninterrupted continuation in state hash, views, projections and RNG-driven outcomes.
  Rules-7 saves fail clearly before archived-region mapping; archived new-rule regions survive retuning.
- Build/select/connect/toggle, charge/discharge graph keys, unused-energy review, history and line
  direction work through actual dispatch/egui/ViewSync paths at minimum size/zoom; localisation is
  complete. Native launch and a full charge/evening/reset/resume playtest pass.
- Existing storage-free shipped-region baseline runs and scenario intent remain intact. Review
  rules-version digest/snapshot changes deliberately; no unrelated tuning is used to conceal a flaw.

## 21. Amendment A10: Preserve saved games across feature updates (2026-10-23)

**Status:** Implemented; verification and player-save publication tracked in
`docs/superpowers/plans/2026-10-23-save-migration-and-portable-state.md`.

### 21.1 Completed one-time battery recovery

All pre-battery player saves were verified against historical rules 6/7, converted into separate
portable snapshot slots and restored again for exact state/history comparison. Original files and
byte-identical backups remain intact. Conversion kept cash/ledger, construction, connections,
commitments, hydro plans, regulator/rate base, RNG, IDs, reports, incidents and charts, appending
only the frozen authored battery catalog entry. New rules affect future play only.

This pre-release recovery is complete. The frozen historical engines, converter, original golden
fixtures and supporting sources are archived with checksums and restore instructions in
`tools/archive/save-recovery-2026-10-23/`. They are outside the active Cargo workspace and shipped
game. There is no in-game upgrade action, recovery prompt, old-rule replay engine or copy
publication on Resume/Load. Converted snapshots use the normal Resume buttons; preload is read-only.
Unsupported historical command saves fail safely, preserving all bytes for archived recovery.

### 21.2 Original preservation and provenance

Retain converted snapshots and original backups without rewriting or deleting them. Snapshot
provenance (source slot/revision/rules/hash/count) remains part of schema 1 and guards against
writing a converted snapshot over its original slot. The library suppresses an unchanged original
only while a readable converted copy of that exact boundary exists. Later progress must not hide
earlier boundaries. Deleting/corrupting the copy, an unsupported schema or a changed source makes
the original visible again as unavailable for normal play. Recovery then uses the archived tool.
Future snapshot-schema migrations run through normal loading and preserve originals; they do not
require a special player-facing upgrade workflow.

### 21.3 Storage and future compatibility

New directory format 4 stores portable state schema 1: complete domain state and projections,
with monthly chart samples in compressed binary files and the remaining state, region and RLE
journal in a compressed text manifest (§21.8). Reassemble the exact original canonical projection
bytes before verification. Retain historical flat formats 1–3 as readers, not a migration flow. Encode fields and enum tags
explicitly, with fixed-width little-endian scalars and ordered collections; do not derive persisted
identity from Debug text, object layout or pointer addresses. Cover both state and history with the
canonical boundary hash and protect the envelope with SHA-256. Bound decoded size/collections;
validate scalars, topology, IDs, session invariants, canonical re-encoding, audit hash and boundary.

A rules bump alone must not discard compatible snapshot saves. Keep schema versions separate from
rule/build versions. When adding fields, provide explicit defaults/upcasters or a new schema reader;
retain readers and golden old-save fixtures for every supported released schema. CI must load,
validate and continue these fixtures. Never silently replay old years under new economics, replace
archived tuning with shipped content, drop unknown state or erase originals to make a load work.
A genuinely changed state meaning still needs migration work; snapshots keep that work independent
of replaying every historical engine. Unknown future schemas fail safely and preserve all bytes.

Snapshot-rooted diagnostic recordings use command-log format 2 and a verified checkpoint sidecar.
Use the snapshot-aware replay API; command-only replay rejects format 2 explicitly. Format-1 logs
and rules-8 command-only saves keep their original reader. Invalid/failed candidates leave no branch
recordings or sidecars behind.

### 21.4 Verification

Keep released portable-schema golden saves for every phase, including a charged battery. Restore,
validate, continue and save them without a historical engine. Cover exact progress/history, RNG,
canonical hashes, bounded malformed input, protected original slots, provenance deduplication,
unknown schemas, ordinary Resume clicks and read-only preload. The archive separately retains
and passes the historical migration fixtures/tests; verify archive checksums and source hashes.
Confirm the shipped Cargo dependency tree contains no historical engine. Hash-check all actual
converted saves/originals/backups before and after changes. Run relevant backend/client tests,
formatting, clippy, build, launch smoke and an isolated native playtest.

### 21.5 Background monthly autosaves

A successful monthly autosave never opens a dialog. Keep Review and its supply/demand retrospective
usable during writing, and let the player continue into Planning/Operating. The immutable snapshot
still represents the exact closing command, even if the live game has advanced. Use one writer,
queue later month-close captures in order, and determine overwrite revisions from the preceding
receipt. Never let a late older receipt pause later play, clear a newer manual capture or fulfil
Save-and-return-to-menu. Manual save/leave requests can wait for the background write; cancelling
their dialog must not cancel that write.

An automatic-write failure pauses play and opens the existing retry/continue-without-saving
barrier. Keep the failed immutable boundary and any newer queued captures. Retry confirms or
publishes that exact boundary before newer queued writes; Continue without saving may skip the
failed write and advance to queued captures. Queued work must not disable the failure controls.
Preserve uncertain-publication revisions and persistent warning semantics (§17.5). Only a confirmed
receipt reports success. OS close and explicit Leave without saving retain their existing contract.


### 21.6 Lightweight save-library metadata

Opening Load game reuses the opening menu's metadata index and refreshes it asynchronously; paint
cached cards immediately rather than showing generic Working text. First-time enumeration may show
Loading saved games. Listing must not construct game sessions, decode command histories, decompress
state/projections, map region definitions or rasterize terrain. Extract only card metadata and the
archived region ID/name key, retaining envelope/version/identity checks and raw revisions for
conflict-safe actions. Metadata readability is not proof that a game can restore. Selected loading
always reads fresh bytes and performs the complete validation/restore path; caches cannot bypass it.

Cache lightweight rows in memory on the save store, surviving menu transitions. Key them by file
identity, size and modification/creation/change timestamps where the platform provides them. Scan
for added/deleted files on refresh, invalidate changed slots individually and evict disappeared paths.
Do not cache reads raced by replacement or transient I/O errors. Full selected reads remain the
safety boundary even if a filesystem stamp is unreliable. Preserve sorting, the newest-12 display
cap, provenance deduplication, unavailable entries, and rename/delete revision checks.

Persist the lightweight index as a disposable `.save-library-cache.ron` beside the saves, containing
only card rows, stable inspection errors, revisions, provenance and filesystem stamps. Version and
checksum it, bound its size/entry count/parse depth, and bind it to the inspecting build/platform.
Publish atomically without rewriting any save or backup; a failed cache write must not fail a scan.
Missing, damaged or incompatible indexes rebuild automatically. A restart must still enumerate the
save directory and stat every source before reusing its row, detecting additions, replacements,
changes and deletions; cached paths never establish file existence. Do not persist transient I/O
errors or raced reads. Full selected validation and save formats remain unchanged.


### 21.7 Lossless tick journal run-length encoding

Save envelope 3 stores the journal as `Command(<original CommandDto>)` entries and
`TickRun(n:<original batch size>,repeat:<number of original commands>)` entries. Only consecutive,
identical `AdvanceTicks` commands may become runs. Any other command or changed tick batch size is
a boundary. Preserve zero-tick commands, all player actions and all command ordering. Singleton
ticks remain ordinary commands. Flat codec writers use format 3, including the state carried inside directory format 4; readers
retain formats 1 and 2 unchanged.
Portable state schema and simulation rules do not change for this storage-only amendment. Format 3
also supports current-rule command-only checkpoints, expanding the journal before verified replay.

The count and audit hash always describe the expanded original command sequence. A repeat count
never means one call with multiplied ticks: expand into the original separate commands before
verification, diagnostic-prefix comparison or replay, preserving projections at every original
boundary. Existing diagnostic log formats and live append semantics remain unchanged; snapshot
sidecars use the new save encoding. Do not rewrite existing player slots or backups automatically.

Reject empty runs, length overflow, unknown records, declared-count mismatches and expanded journals
over `MAX_SAVE_COMMANDS` before allocating expanded storage or installing a game. Expansion remains
cancellable. Keep checksum/region/rules/schema validation and atomic save publication unchanged.
Property tests cover arbitrary batch sizes, repeats and intervening actions. Current-rule format-1
command-only records must re-encode and replay exactly; historical command-only recordings fail
with `RulesVersionMismatch` rather than replaying changed rules. Frozen format-2 portable snapshots
must load, re-encode and continue with exact saved state and projections. Unsupported future
versions fail safely without modifying original files.


### 21.8 Monthly binary chart directories

Each player snapshot is a directory `<slot>.mgsave/`, containing compressed text `snapshot.ron.z`
and monthly `YYYY-MM-<SHA256>.bin.z` files. Names are local, validated references, never arbitrary
paths. Bind each file's month, compressed/expanded size and digest in the manifest; reject duplicate
months, wrong names, traversal, symlinks, corrupt/missing files and aggregate size overruns. Keep
complete minute samples, forecasts and the live rolling buffer, including Planning/Review boundaries.
The pure application split/join must reproduce identical portable projection bytes, canonical hashes,
RNG, reports and audit history. Domain schema 1 and simulation rules remain unchanged.

Writes stage immutable chart files, flush/sync them and their directory, then atomically publish the
new manifest as the commit point. New snapshots stage the whole directory before publishing it.
Keep old referenced files through failed/uncertain publication and reuse unchanged month files;
remove obsolete versions only after success. Retain slot locks, revision-conflict checks, cancellation
and durability Retry semantics. Delete renames the directory out of the library before syncing its
absence and cleaning up; retries cannot delete a recreated slot with a different revision. Rename
preserves all historical game/chart data. Diagnostic base recordings use the same directory layout.

The metadata index remains lightweight and persisted. Check directory, manifest and monthly-file
identity/timestamps on refresh; selected loading verifies every referenced file. The original
historical flat readers remain available, with no implicit conversion at startup or Resume.

Convert the owner's existing saves once, using local ignored tooling outside the workspace. Verify
all canonical state/history and saved metadata before publishing converted copies. Preserve originals
and earlier backups outside the repository, retain preferences and slot IDs, and perform no runtime
upgrade flow. `/tools/archive/` is ignored; recovery archives, converter sources, staged files and
backups are not added to Git. Future users' format/schema evolution still requires retained readers
or explicit schema upcasters backed by compatibility fixtures; this one-off conversion is not shipped.

## 22. Midnight dispatch and proportional surplus response (2026-10-23)

The May 2033 incident exposed two interacting defects: a 500 MW CCGT shutdown tail was omitted
from midnight dispatch, then over-frequency protection forced the entire AGC fleet to minimum,
dropping hydro to zero and causing a blackout. The corrected contracts in §6 and §14.2 require
counting non-dispatchable shutdown output and retaining surplus-only, ramp-limited AGC targets.

Regression coverage uses a reduced, deterministic May 2033 fixture with five hydro units, three
all-day CCGTs, an evening CCGT ending at midnight, and 3,552.12 MW midnight demand. Assert balanced
initial generation, uninterrupted first-hour service, and a full day without blackout. Independently
inject a May-sized surplus: over-frequency must engage without an instantaneous hydro collapse,
ramp violation or induced blackout. Property tests vary evening runs, AGC/manual shutdown tails
and surplus/solar availability; curtailment cannot rewrite physical outputs or queued AGC targets.

Advance `SIM_RULES_VERSION` from 8 to 9 because outcomes change. Keep portable state schema 1 and
save formats unchanged. Rules-8 portable snapshots restore their complete saved boundary, RNG,
progress and history; only subsequent commands use rules 9. Do not rewrite player saves or revise
past monthly reports on load. Historical command-only logs remain version-fenced rather than
silently replaying changed behavior. Retain all immutable compatibility fixtures and verify old
portable saves still load, continue and re-save under the new rules.

## 23. Battery reserves bridge generation gaps (2026-10-23)

The evening battery controller no longer deliberately replaces available thermal generation or
scheduled hydro. Generators continue aiming to meet served customer demand; batteries cover the
remaining real deficit during the existing evening window and stop once generation catches up.
Use the same chronological, energy-limited residual-gap projection for the battery schedule,
Planning supply chart and shortfall warning. Keep daytime charging, efficiencies, ratings,
protection priorities and the midnight inventory close unchanged. A sustained generation shortage
still drains batteries; conserving reserves must not withhold feasible emergency support.

Advance simulation rules from 9 to 10. Keep portable schema 1 and existing save formats unchanged;
restore historical snapshots without rewriting saved state, reports or files, and apply the new
policy to subsequent play. Historical command-only replays remain fenced by their rules version.
Regression tests cover a balanced 17:00 fleet holding charge, transient ramp gaps ending as
generators catch up, later-peak reserve preservation, shared chart/warning/schedule predictions,
finite-inventory exhaustion, and save/resume/batching determinism.

## 24. Planning forecast reuse and idle redraws (2026-10-23)

Cache one Planning analysis for the active session: supply chart, capacity shortfall, committed
minimum warning and dry-hydro findings. Assess the shortfall from the same computed supply forecast
used by the chart, retaining minute selection, arithmetic and earliest-on-ties behavior. Pure
presenters remain available; the live HUD reuses the analysis until the session, session generation
or localiser changes. View-only changes (tabs, highlighted hours, warning expansion, camera and
selection) do not rebuild it. Clear it outside Planning and on session removal. A new game, loaded
game, month transition or applied planning command refreshes before the next Planning panel draws.
Use resource change ticks, not repeated whole-session hashing. The cache is client-only and is
never saved; no additional simulation-rule or portable-schema change is needed.

Use event-driven window updates on static game screens: a 250 ms focused fallback and 500 ms
unfocused fallback, with immediate window/user-event wakeups and no wakeups from unrelated raw
device motion. Menus and active I/O poll at 33 ms while focused. Running Operating keeps the normal
continuous focused rendering and original fixed-step simulation timing. Input-driven camera moves,
scrolling, dialogs, save/load completion and phase transitions must remain responsive. Retain
focused tests of repeated real HUD passes, cache invalidation and shortfall confirmation, plus a
windowed launch smoke and native idle-performance measurement.

## 25. Amendment A11: Wind farms and midnight-to-18:00 battery charging (2026-10-23)

**Status:** Implemented under `SIM_RULES_VERSION` **14**, portable state schema **2** and
region schema **3**. Focused automated checks and isolated native playtests are recorded in
[Plan 5: Wind farms and revised battery charging](../plans/2026-10-23-plan-5-wind-farms.md).
This amendment supersedes the daylight-only charging rules in §20 and the daytime-charging
preservation clause in §23. The latest user decision sets a hard **18:00 charging cutoff**.
The midnight inventory reset, 17:00 discharge start and residual-gap support of §23 remain.
Amendment A12 and its interaction fixes (§26) are implemented as rules 13. Plan 5 advances
to rules 14 and region schema 3, retaining A12 storage accreditation and regulator defaults.

### 25.1 Player contract and scope

- Add buildable **onshore wind farms** in both shipped regions. They use ordinary plant IDs,
  construction, land occupancy, transmission connectivity, selection and rate-base accounting.
  Neither starter fleet changes; wind is a player investment, not free starting capacity.
- Wind can generate throughout the representative day, including at midnight and overnight.
  Site quality and month affect availability. Output changes with a seeded, persistent regional
  wind process; farms share its lulls and surges. More farms add energy, not independent weather.
- Wind follows the available resource. No hourly commitment, dispatch target, thermal start/stop
  or AGC toggle applies. Protection can curtail output, trip a farm and reconnect it under the
  existing variable-generation recovery policy. Low wind is not a thermal shutdown or start.
- A connected, enabled, commissioned battery starts empty and is **eligible to charge immediately
  at 00:00**, on its first operating minute. Eligibility is `0 <= tick < 1080` (18:00), unrelated
  to sunrise, sunset, cloud cover or installed solar/wind. Eligible does not mean guaranteed
  charging: customer priority, available surplus, inverter power and free space still apply.
- Charging stops at **18:00 exactly**. The minute `[17:59,18:00)` can charge; `[18:00,18:01)`
  cannot. Solar sunset does not stop charging early. Grid charging may use thermal generation.
- Evening discharge remains eligible from **17:00 to 24:00**, only into a real primary-generation
  deficit. Between 17:00 and 18:00, surplus permits charging and deficit permits discharge.
  Choose the mode from the bus balance before battery flows; never charge and discharge in the
  same minute, discharge to feed another battery, or reduce generators to manufacture a gap.
- All batteries report and clear unused inventory at midnight exactly as in §20.6. Overnight
  wind after the next 00:00 can charge the newly empty battery; it does not carry yesterday's
  stored energy forward. No charging after 18:00, morning discharge, manual battery schedules,
  price arbitrage, degradation or reservoir/energy carry-over is added.
- Offshore wind, turbine wakes, individual turbine failures, wind-speed/cut-out physics, moving
  weather fronts and discrete storms remain out of scope. The first model is explicitly a
  resource/availability abstraction, not a meteorological or power-flow simulator.

### 25.2 Wind model, region data and validation

Add `PlantKind::Wind` as the eighth asset kind (seven primary generators and Battery), appending
its stable portable enum tag **7** without changing tags 0–6 or existing kind ordering. Explicit
classification distinguishes variable generators (Solar/Wind), dispatchable generators and
storage; `!is_dispatchable()` alone must never classify a battery as a renewable generator.

Add `Cell.wind_potential: Ratio`, a geographic wind-potential field rasterised by infrastructure,
and validated regional `WindParams`. Land with zero potential is a valid but unproductive wind
site; the build overlay/card must make that visible before purchase. Water and occupied cells
remain invalid. Resource maps express game site quality, not measured wind speeds.

Initial wind-farm tuning in both region RON files and the test fixture:

| Field | Initial value |
|---|---:|
| Nameplate capacity | 300 MW |
| Capital cost / build time | $300M / 3 months |
| Fixed O&M / book life | $500k per month / 300 months |
| Minimum stable / start-up time and cost / marginal fuel cost | Zero |
| Regulatory capacity credit | 0.20 |
| Siting | Any otherwise buildable, unoccupied land cell |
| Thermal ramp | Unused; resource availability sets the output limit |

These are game tuning values. Wind's capacity credit is an authored regulator simplification,
not a guarantee of output or an input to instantaneous shortfall sizing. Normal construction,
O&M, depreciation, reserve assessment and line-capital attribution apply; no wind fuel/start-up
entry is created. Delivered primary energy includes wind exactly once. Battery receipts and
discharge never create additional customer revenue or a second primary-energy entry.

Regional parameters contain 12 nominal monthly availability factors `m`, minute persistence
`rho`, innovation standard deviation `sigma`, initial-day standard deviation `initial_sd` and
an indicative forecast-band half-width `band`. Initial authored values:

| Parameter | East | West |
|---|---|---|
| `m`, January–December | .65, .65, .65, .60, .55, .50, .45, .45, .50, .55, .60, .65 | .60, .60, .65, .65, .60, .55, .50, .50, .55, .60, .60, .60 |
| `rho` / `sigma` / `initial_sd` / `band` | .995 / .015 / .15 / .25 | .995 / .015 / .15 / .25 |

Both maps author a 0.4 base potential plus Gaussian resource blobs using the existing geographic
field mechanism, clipped to `[0,1]`: East highlights the Great Plains/Upper Midwest and West
highlights the Columbia Basin and inland plains/ridges. These are illustrative game resources.
Exact blob coordinates/radii are authored and pinned during the region-data task, not inferred
from terrain height at runtime. The tiny fixture uses uniform potential 0.8 and a flat monthly
factor 0.6; deterministic scenarios may set both standard deviations to zero and choose their
own authored profiles. Freeze the first released map/parameter payload for old-save upcasting.

Constructors reject non-finite values, a monthly array of the wrong length, factors/potential
outside `[0,1]`, persistence outside `[0,1)`, negative standard deviations, band outside `[0,1]`,
missing Wind catalog entries and extraneous battery data on Wind. Validate generic positive
capacity/build/lifetime, nonnegative costs, capacity credit and zero thermal minimum/start/fuel
fields. All tuning lives in region RON data and `test-fixtures`, never simulation constants.

### 25.3 Actual availability, forecasts and determinism

Use one regional factor `w` for all farms. At `BeginOperating`, after the existing solar
clearness draw, initialise `w_0 = clamp(m + initial_sd * z_0, 0, 1)`. For minutes `t = 1..1439`:

```
w_t = clamp(m + rho * (w_(t-1) - m) + sigma * z_t, 0, 1)
available_wind(plant, t) = capacity * wind_potential(cell) * w_t
```

Each `z` is exactly one `SimRng::standard_normal()` from the session RNG, including when its
coefficient is zero. Draw once at day initialisation and once before variable generation steps
in each minute after minute 0; the ordinary per-tick demand draw precedes that minute's wind
draw. Draw independently of plant count, connectivity, construction, curtailment, blackout or
whether wind has been built. Pin this ordering with tests. This introduces new random draws
under the new rules; it does not promise identical old-rule demand noise in future play.

Keep factor, initialisation/minute boundary and RNG position in portable state. A new
representative day starts a new regional wind day; resuming a day never reinitialises it.
Paused frames, pure queries, forecast sampling, chart drawing and refused commands draw nothing.
AdvanceTicks batching and speed do not change the sequence. Availability is capped before
curtailment and disconnected/unfinished plants contribute no supply.

Planning uses the month's nominal factor; Operating's central forecast at horizon `h` uses
`m + rho^h * (current_factor - m)`. This is a deterministic central scenario, not a claim to be
the exact statistical mean of a clipped process. Show the resource-factor range central ± band,
clipped to `[0,1]` and scaled by each site's potential/capacity. Aggregate the shared regional
range directly; do not shrink uncertainty as if farms' errors were independent. Label it an
indicative range, with no unsupported confidence percentage. Forecasts never inspect future
random draws. The current-minute point uses the already sampled factor.

Introduce a shared variable-generation forecast (Solar + Wind) while retaining per-kind values.
Every consumer uses it: boundary dispatch, net-load views, hydro allocation, battery charging,
Planning supply/shortfall analysis, operating outage sizing, ramp/headroom warnings and recovery.
Use the same central assumptions in the supply chart and warning, with finite battery inventory;
wind nameplate or regulator credit must never substitute for forecast output. An expected wind
forecast is not guaranteed firm generation. Test unexpected lulls through actual protection too.
For wind fleets, assess all minute points for shortages even without batteries: a conditioned
wind forecast can fall between an hour's endpoints. Retain the generator reach-by-hour-end rule
for outage sizing and actual stored-energy-only cover in Operating (§20.5).

Hydro schedules against customer demand minus expected variable generation, without subtracting
battery discharge (§23). Wind changes residual load, not the water budget. Hourly re-planning
uses the latest wind observation; no player action, forecast or cache can freeze the weather.

### 25.4 Charging plans, overlapping modes and protection

Replace the shared daylight predicate in live flows, previews, validation, headroom and UI
with `0 <= tick < charge_end_hour * 60`. Add `BatterySpec.charge_end_hour`, authored as **18** in
both regions and fixtures; retain `discharge_start_hour = 17`. Require integer hours with
`1 <= discharge_start_hour <= charge_end_hour <= 24`. Equality is valid for custom regions;
overlap is explicitly supported. No solar-daylight validation applies to storage eligibility.

Valley-fill remaining eligible minutes using customer net load after expected Solar + Wind,
bounded by reachable committed non-hydro generation plus variable supply after all customers
and earlier batteries' requests. Request additional thermal generation only through the existing
AGC timing. Do not request extra hydro for charging, count a request as received energy, or
subtract anticipated discharge from the generators' customer-load target.

The old one-fill-then-discharge shortcut is insufficient during 17:00–18:00. Project charge and
discharge chronologically against **one inventory per battery**, using actual stored energy as
the live starting point. Charge may resume after a discharge creates room, until 18:00. Requests
may be conservative about future room; they cannot assume a future discharge has happened.
Never cap the entire day's cumulative input by one battery capacity when earlier discharge has
legitimately freed room. At every minute all batteries use the same pre-storage bus sign; shared
surplus/deficit is allocated once in ascending PlantId order. Histories before a re-plan stay frozen.

Re-plan at day start, hourly, 17:00, 18:00 and after enable/recovery or a feasible storage flow
changes direction during the overlapping window;
ordinary thermal controls invalidate affected future plans on the next tick. Pure Planning
previews start empty and may include feasible forecast receipts before later discharge, labelled
expected. Operating outage/restoration/headroom calculations spend only energy already stored;
they cannot borrow from later wind or charge requests. Forecasts remain pure and share the
physical inventory transition with actual settlement, with independent contract tests.

During the overlapping hour, choose discharge first when an eligible positive customer deficit
exists, even though charging is also eligible. A charging-window `if ... else if discharge`
must not suppress deficit support. Charge only on surplus after **full** customer demand,
not on surplus manufactured by planned shedding. Charging remains zero during any shedding,
blackout/restoration or below-normal frequency. An enabled battery can charge at 00:00 on
existing surplus, but an AGC charge request still needs generation to arrive before receiving it.

Taper AGC charging requests before full inventory and the **18:00 cutoff**, reserving room for
the reachable thermal ramp-down tail as in §20.4. Absorbing already available renewable surplus
does not justify creating an avoidable thermal surplus when storage fills. At/after 18:00,
cancel every charge request/receipt even if wind is abundant or the battery has room. Do not
move the cutoff to sunset or extend it to absorb a ramp tail. If inflexible excess is unavoidable,
ordinary curtailment/protection handles it rather than violating eligibility.

Generalise standing solar curtailment to variable-generation curtailment. The same curtailed
fraction applies to each connected, available Solar/Wind plant, so withheld MW is proportional
to current resource availability. Apply it once, retain next-tick application and the existing
release timing, and expose per-kind withheld MW plus the total. Retain historical solar-only
event records; new events distinguish Solar and Wind with typed kind/MW data. A calm farm
contributes neither supply nor fictitious curtailed energy. Full/no-room batteries are not
downward headroom; after 18:00 no battery is charging headroom. Before curtailment, reduce
discharge and absorb eligible safe surplus under the existing protection ordering.

Wind rejoins through the same renewable reconnection policy as solar after a trip; it does not
join thermal merit-order targeting, start-up billing or AGC restart candidates. Blackout still
suspends battery flows without losing inventory. Resume actual renewable availability only when
the grid's ordinary recovery policy permits it. Preserve the signed balance and once-only,
post-protection energy settlement of §20.4, including the last minute before midnight.

### 25.5 HUD, map and reporting

- Construction offers Wind with cost, lead time, nameplate MW, local potential, expected seasonal
  output and regulatory capacity credit. A wind-resource overlay shows static site potential,
  not a promise of current output; use the same legend/contrast conventions as solar.
- A commissioned farm's card shows connection, current available/delivered MW, curtailment,
  site potential and the wind forecast/range. Omit thermal and battery controls. Add Wind to
  build lists, selection, supply-stack keys, plant histories, reports, event translations and
  the actual-generation review table, consistently distinguishing it from dispatchable capacity.
- Add a recognisable small turbine group to the 3D diorama within the existing 0.8-cell footprint,
  with construction/selection/picking/connection behavior. Any rotor animation is visual only,
  driven by sampled view data and visual-effects settings; it does not advance time or RNG.
- Battery cards and build help say **Charge 00:00–18:00; support generation gaps 17:00–24:00;
  unused energy resets at midnight**. Show current mode and expected versus actual receipts;
  remove daylight/sunrise/sunset implications. Grid energy may come from wind, solar or thermal.
- Supply charts show Wind separately; retain separate battery input, output and MWh axes. Review
  still reports all losses and unused inventory, including late receipts that were discarded.
  Saved earlier charts/reports retain their original recorded values and solar-only event labels.
- Planning analysis caching (§24) includes wind forecasts and invalidates on relevant applied
  commands, session/phase/month/localiser changes and load. Operating observations must not reuse
  a stale Planning result. Keep pure presenter → draw → UiAction → dispatch, ActionSink getters,
  Fluent keys/coverage, Enter/Escape handling and reachable small-window/zoom layouts.

### 25.6 Persistence and compatibility

Advance simulation rules **13 → 14** (after A12 and its interaction fixes, §26) with the first behavior-changing implementation. Introduce
portable state **schema 2** for wind resource/state, the added battery field, forecast/curtailment
records and any projection changes. Directory format 4, flat envelope 3 and snapshot-rooted
command-log format 2 need not change merely for a new state schema; retain all existing readers.
Chart split/join and every metadata/schema gate must recognise both supported schemas.

Keep a schema-1 wire reader and canonical encoder. First verify old state/projection bytes,
journal hash, boundary, original archived region and checksums with schema-1 semantics; only
then upcast in memory. Never compare newly encoded schema-2 bytes to a schema-1 boundary hash.
Upcast original tags unchanged; add zero historical wind output/curtailment, the charge-end
field 18 and a neutral wind state at the saved month's nominal factor without drawing RNG.
Preserve cash, assets, IDs, connectivity, commitments, inventory, water, clock, RNG position,
history and every recorded report. Invalidate future derived plans for recomputation before
the next tick/Planning draw; never rewrite elapsed history or initialise/empty a loaded battery.
An already saved last-minute charge is history even if new rules would forbid it; future flows
obey the new window. Review stays closed and does not reset or bill anything twice.

Pre-wind archived regions require explicit, frozen compatibility defaults for the Wind catalog,
resource map and parameters, preserving every existing tuning value and original source byte.
Carry the extension identity/data in schema 2; do not reread mutable shipped tuning on later loads.
Normal new-format region authoring requires the Wind fields/catalog. Legacy mapping/validation
is version-aware and cannot become a general silent-default escape for malformed new content.
Old games gain the ability to build wind in future Planning without changing their starter fleet.

Loading is a read-only candidate build, with no rewrite, publication over the original, special
upgrade screen or historical gameplay replay. Ordinary future saves encode schema 2 and retain
source/provenance under §21. Protect converted source slots as before. Historical command-only
logs remain version-fenced; unsupported/future schemas fail safely with originals intact.

### 25.7 Acceptance

Required unit/property, independent behavioral, adapter and real-widget coverage includes:

- Coal's 580-minute shutdown/start-up off-gap accepts 10 hourly off flags and rejects nine;
  CCGT's 140-minute off-gap accepts three hourly off flags and rejects two, including a gap
  across midnight. No additional dwell timer is introduced or implied by the documentation
  correction.
- Wind at midnight/noon/night, potential zero/one, shared lulls/surges across multiple farms,
  disconnection/construction, curtailment/trip/reconnection, bounded output and exact RNG ordering.
  Queries/UI/refused commands do not alter state; batch size, speed and resume do not alter ticks.
- Independently computed variable net load, wind supply/indicative ranges, hydro allocation,
  chart/warning agreement and minute-shortage selection, with and without storage. A sustained
  lull and unexpected wind loss exhaust real reserves and engage ordinary protection fairly.
- Battery charging at 00:00, before sunrise, after sunset but before 18:00, and at 17:59; zero
  charging at/after 18:00. Discharge at 17:00 during a deficit, surplus charging during the
  overlap, full/empty/fractional ticks and multiple batteries. No simultaneous or cross-battery
  flows, future-energy borrowing, customer-funded shedding or missed 18:00 ramp-down taper.
- A battery can charge, discharge, then recharge before 18:00 with finite inventory throughout;
  final-minute settlement and exactly-once midnight close obey daily/monthly conservation.
- Schema-1 golden saves for all phases/charged inventory restore without changing originals,
  RNG/history or archived tuning, then continue/re-save under schema 2. Schema-2 wind/overlap/
  cutoff/blackout/Review saves resume identically to uninterrupted new-rule play. Split/join
  chart bytes, unknown schemas, corruption, provenance and read-only preload remain protected.
- Both shipped regions retain hands-off starter service under noise-free contract fixtures;
  do not hide defects by retuning starter fleets/cash. Wind build/connect/select, forecasts,
  battery copy/modes, resource overlay, review and save/resume work at minimum supported size.
  Native launch smoke and an isolated full representative-day wind/battery playtest pass.

## 26. Amendment A12: Close the rate-base loophole and make satisfaction count (2026-10-23)

**Status:** Implemented under `SIM_RULES_VERSION` **13**, after the rules-11/12 interaction fixes;
lands before Plan 5 (§25), which must advance from rules 13.

### 26.1 Why

The tariff includes `allowed_return × eligible_rate_base`, so every dollar of capital that enters
the rate base earns the allowed return (about 9%) for its book life. The only check on overbuilding
is the forward reserve assessment (§2.5). It holds, and after two reviews writes off, capital whose
**firm MW** exceeds the planning reserve margin. Three kinds of capital escape that test:

- **Batteries.** Their regulatory capacity credit is zero (§20.6), so they have no firm MW to be
  excess, and the assessment never holds them or the lines that serve only them.
- **City-to-city lines.** They are never held (§2.5). A line between two already-connected cities
  has no function under copper plate (§5.4), yet it earns the full return.
- **Redundant plant lines.** A second line to a connected plant also adds nothing, and it is held
  only in proportion to that plant's own excess.

Satisfaction, which §2.5 said made overbuilding "hurt twice", is computed and shown but feeds no
rule: no failure test, return, tariff or demand reads it. Together these make building batteries
or redundant lines a guaranteed return for any player with cash. That contradicts the stated
design goal of building only what the reserve margin justifies.

### 26.2 Goals, non-goals, anti-goals

- **Goals:**
  - Every capital asset that earns a return either passes a regulatory need test or can be held.
  - Batteries count toward reserve adequacy by how long they can discharge, as real accreditation
    does, and excess battery capital is held and written off like any other.
  - A redundant line is refused when ordered, with a clear reason, rather than disallowed later.
  - Satisfaction changes the allowed return, so a tariff pushed above the reference costs return
    on the whole rate base.
  - The rules apply to games already in progress from the next command after resume, without a
    save-schema change.
- **Non-goals:**
  - effective-load-carrying-capability (ELCC) studies, or credits that fall as storage
    penetration rises;
  - a full prudence review of operating decisions;
  - revisiting solar or wind credits (§3.2, §25.2);
  - transmission redundancy value, which needs a power-flow `NetworkModel` (§11).
- **Anti-goals:**
  - a new route to a strike or to licence revocation;
  - rewriting saved history, reports or held-capital records on load;
  - silent defaults for region files authored under the new schema.

### 26.3 Battery accreditation by duration

- **Duration.** A battery's accredited duration is the hours it can deliver its full discharge
  rating from full: `energy_mwh × discharge_efficiency / discharge_mw`. The shipped battery
  (800 MWh, 100%, 200 MW) has four hours.
- **The ladder.** `RegulatorParams` gains `storage_accreditation`: a list of
  `(hours, credit)` points, region data. Validation requires at least one point, strictly
  increasing positive finite hours, credits in `[0, 1]` and non-decreasing. A duration is
  interpolated linearly between points, linearly from `(0, 0)` up to the first point, and held
  at the last point's credit beyond it.
- **Shipped values** (both regions and the test fixture). These are the NYISO Final Capacity
  Accreditation Factors for the 2026–2027 Capability Year, Rest of State, CHPE-In, rounded to two
  places: 2 h → 0.65, 4 h → 0.85, 6 h → 0.93, 8 h → 0.96. The shipped battery's credit is
  therefore 0.85, or 170 MW firm. These are real published factors, not a claim that this game's
  daily-cycle battery behaves like a New York resource.
- **One derivation.** A plant's firm capacity is `capacity × capacity_credit` for generating
  kinds and `discharge_mw × ladder(duration)` for Battery. Every consumer uses one domain function:
  the start fleet, commissioning, the fleet's firm capacity, the year's firm additions, held-share
  arithmetic and the rate-review estimate. Battery's generic `capacity_credit` field stays
  required at zero and unused, so archived region bytes still parse. This supersedes §20.2's
  zero-credit rationale and §20.6's battery and battery-line exemptions. With positive firm MW,
  §20.6's line rule associates a battery-endpoint line with that battery like any plant, using
  the lowest positive-firm PlantId among the line's plant endpoints.
- **Assets recorded under rules 10 or earlier.** Batteries commissioned before this amendment
  recorded zero firm MW in their rate-base profile and in the year's tally. Restore keeps those
  bytes unchanged. The first December review under rules 11, and every estimate, derives each plant
  asset's firm MW from its archived spec, and refreshes the stored profile at that December close.
  The year tally's `firm_added` is informational: no review decision reads it. Held-share history is
  otherwise unchanged. *(Builder's call: re-deriving at the review, inside the closing command, applies the
  new rule to subsequent play without changing a restored boundary's hash.)*
- **Why not hold excess storage separately.** Batteries join the existing assessment as new
  additions: existing held portions are re-tested first, then remaining excess is spread across
  the year's additions in proportion and by ID. Nothing else in §2.5 changes.

### 26.4 Line need: a certificate before construction

- **The rule.** `BuildLine { from, to }` is refused when `from` and `to` are already joined by a
  path of lines that are in service or under construction. It returns
  `DomainError::LineNotNeeded { from, to }` and leaves the session and journal unchanged. An exact
  repeat of an existing line keeps returning `DuplicateLine`, which is checked first. The rule
  covers every line, including plant lines, because under copper plate a line's only function is
  connectivity.
- **Islands count.** Two nodes joined to each other but not yet to the home city are still joined:
  a second line between them is refused. A line that joins an island to the rest is allowed.
- **Ordered lines count.** An unfinished route already joins its ends for this test, so the
  player cannot order a parallel route to beat it. To choose a faster route, the player cancels the
  unfinished line (§16.9) and then orders the new one. *(Builder's call: counting ordered lines
  stops a stack of parallel orders, each of which would otherwise look needed.)*
- **Consequence.** New networks are forests: no order can close a cycle. A future
  `NetworkModel::DcPowerFlow` must revisit this rule, because redundancy has value under power
  flow.
- **Grandfathering.** Lines already in service or ordered at resume keep their status, rate base
  and construction schedule. The rule applies only to new orders.
- **City-to-city holding.** §2.5's rule that city-to-city lines are never held stays. Each such
  line passed the need test when it was ordered, so it is never redundant capital.
- **HUD.** The endpoint picker marks endpoints already joined to the chosen origin as unavailable,
  with a one-line reason that both ends are already connected. The quote is not offered. The
  error's toast uses the same copy. Strings go in `ui.ftl` and `errors.ftl`, and new literal keys go
  in `keys.rs`.

### 26.5 Satisfaction as a performance-based return metric

- **The term.** `RegulatorParams` gains `satisfaction_target` (0–100) and
  `return_per_satisfaction_point` (a finite ratio per point). `compute_rate_review` adds
  `return_per_satisfaction_point × (satisfaction − satisfaction_target)` to the raw allowed return,
  alongside the reliability and cost terms, before the existing clamp to the PBR band.
- **Which satisfaction.** The company's satisfaction after December's month-close update, in the
  same closing command, before the review runs. It is already a smoothed EMA, so no new tally is
  added. The estimate (§2.5, Task 19) uses the current value, from the same review inputs.
- **No circularity.** Satisfaction responds to the tariff in force during the year. The new tariff
  takes effect after the review.
- **Shipped values** (our tuning, not a published figure): `satisfaction_target = 90` and
  `return_per_satisfaction_point = 0.0005`, that is 0.05 percentage points per point. Steady good
  service at or below the reference tariff (satisfaction near 100) adds about 0.5 pp. A tariff 10%
  above the reference, with perfect reliability, settles near 80 and costs about 0.5 pp across the
  whole eligible rate base. The PBR band still bounds the total. Retune these values in region
  data if the acceptance runs below fail. Never retune the rule.
- **§2.5 claim, corrected.** Overbuilding hurts twice once it raises the tariff above the
  reference: through disallowance, and through a lower allowed return via satisfaction. Below the
  reference tariff, the satisfaction curve has no price penalty, so only the need tests limit
  overbuilding there.
- **Visibility.** The Planning tariff panel's projected return, the rate-review estimate and the
  Review's rate-review section show the satisfaction term next to the reliability and cost terms.
  It is computed on read, from public state. No new projection field or event field is persisted.

### 26.6 Region schema and compatibility defaults

- **Region schema marker.** Region RON gains `region_schema: 2`. Shipped regions and fixtures
  declare it and must author `storage_accreditation`, `satisfaction_target` and
  `return_per_satisfaction_point`. A schema-2 file missing any of them fails to load with a typed
  error naming the field.
- **Legacy archives.** A region file without the marker is schema 1, as in every existing save and
  log archive. Mapping supplies frozen A12 compatibility defaults equal to the shipped values
  above. The defaults are fixed in the version-aware mapping, never read from current shipped
  files, and are never applied to a schema-2 file. Plan 5 reuses this marker for its own legacy
  defaults (§25.6), as `region_schema: 3`. *(Builder's call: this extends §25.6's frozen-default
  rule. It is the one place where tuning numbers live in code, and only for archives that predate
  the fields.)*
- **Fingerprints.** Shipped files change, so new games get a new region fingerprint. Archived bytes
  are unchanged, so existing saves still match their embedded region.
- **Portable state.** Schema 1 is unchanged: no new session or projection fields. In particular
  `RateReviewEstimate` and `RateReviewOutcome` gain no fields, because both are persisted. Restore
  verifies every saved byte, report and held-capital record. The new rules apply from the next
  accepted command. A saved Review whose December review already ran is not reviewed again.
  A restored checkpoint still equals the saved one byte for byte (§21). So the persisted rate-review
  estimate (`RateReviewProjection`) of a snapshot from an older rules version keeps its old
  figures until its next refresh event, at the latest the next phase change. The satisfaction
  term's own display is a direct query and is current at once. *(Builder's call: refreshing on
  restore would break the restore-equals-save invariant that preload, provenance deduplication and
  the golden-save tests rely on, for a figure that corrects itself within one phase.)*
- **Rules version.** Outcomes change, so bump `SIM_RULES_VERSION` from 12 to 13 (after the interaction fixes). Portable
  snapshots from rules 8–10 restore and continue under rules 11, as §22 and §23 do. Command-only
  recordings stay fenced by `RulesVersionMismatch`. Regenerate the rules-version guard and scenario
  snapshots once, reviewing every diff.

### 26.7 Properties and acceptance

- **Ladder.** Validation rejects each malformed ladder. Interpolation is exact at the points,
  linear between them, linear from zero and flat beyond. The credit is monotone in duration.
  The shipped battery's firm capacity is 170 MW.
- **One derivation.** Commissioning, the fleet's firm capacity, the year's additions and the
  estimate agree for every kind (property test over generated specs).
- **Batteries are tested.** In a fixture whose generating fleet already meets the reserve margin,
  added batteries are held at the next review and written off if still excess at the following
  one. A battery that fills a real reserve gap is not held.
- **Old batteries.** A rules-10 snapshot with a commissioned battery restores byte-identically,
  continues, and at the next December review assesses that battery at 170 MW.
- **Lines.** Over generated networks, `BuildLine` succeeds exactly when the endpoints are in
  different components of the in-service plus under-construction graph. A refusal changes nothing.
  Islands, plant lines, exact duplicates and cancel-then-reorder behave as §26.4 says, and no
  accepted sequence of commands ever creates a cycle. Grandfathered cycles from older saves
  survive resume.
- **Satisfaction term.** The return equals the hand-computed sum of the base and the three terms,
  clamped to the band (property test). Satisfaction at the target adds nothing. Every A2 proration
  and band property still holds.
- **The loophole is closed.** Scripted gold-plating, using batteries or lines beyond what
  reserves or connectivity need, does not raise cumulative cash over three review years compared
  with the same game without it, on both regions, seeds 0–7. This financial matrix disables added
  batteries after commissioning, retaining their accredited capacity and capital while keeping
  storage flows zero. An additional full-application scenario per region on seed 7 leaves storage
  enabled and verifies actual storage operation alongside the same three-review cash comparison.
  With the tariff above the reference, the satisfaction term is negative.
- **Beginner checks still pass.** The no-command and hands-off year-one runs (§14.5, §16.5,
  §18.1) stay Active on both regions, seeds 0–7. The §18.3 early-expansion acceptance still passes.
  Fix a failure by retuning region data, never the rules.
- **HUD.** The endpoint picker's unavailable state, the toast and the satisfaction term display
  through the real dispatch and egui passes, at the minimum window size and above the 4.5:1
  contrast floor. Localisation coverage passes.


## 27. Amendment A13: Battery discharge from 16:00 (2026-10-24)

**Status:** Implemented under `SIM_RULES_VERSION` **15**. This supersedes the default
17:00 discharge start in §§20, 23 and 25 for new games. Both shipped regions and the current
fixtures author `discharge_start_hour: 16`.

- A charged, connected, enabled battery may cover a real generation deficit from **16:00
  exactly** through 24:00. It cannot discharge at 15:59. Having available generation still
  means holding reserves rather than displacing that generation.
- Charging stays eligible from 00:00 until **18:00 exactly**. During 16:00–18:00, charging
  and discharging remain mutually exclusive, chosen from the real primary-generation balance.
  The midnight inventory report/reset, efficiencies, ratings and reserve accreditation stay
  as specified previously.
- Live dispatch, Planning forecasts and shortfall checks read the same configured boundary.
  Regression checks cover 15:59/16:00, an actual ramp gap at 16:00, early-evening forecast
  deficits and discharge followed by recharging before the existing 18:00 cutoff.
- This is a region-data tuning change; the generalized battery-window algorithm and region/
  portable schemas are unchanged. The rules guard records the changed storage energy and fuel
  outcomes under rules 15.
- Existing saves retain their exact archived region bytes and battery timings, including a
  17:00 window where that was saved. Loading does not silently rewrite tuning or recorded
  state. Battery cards and build help render their actual configured charging and discharge
  times, so both old saves and custom regions show accurate windows.

## 28. Amendment A14: Global regions (Milestone 2, 2026-10-24)

**Milestone:** A14 opens **Milestone 2**. Milestone 1 remains as §3 scopes it: the two North
American interconnections, each one 60 Hz copper plate priced in US$. Every earlier amendment
(A1–A13) belongs to Milestone 1.

**Status:** **Plan 6 global foundations is implemented under rules 16** (§28.4–§28.8,
§28.10–§28.11, and the North American schema-4 migration). Automated acceptance, the native
120-frame launch smoke, and the isolated full representative-day native playtests in both North
American regions pass. The dated playtest observations and durable receipts are recorded in
[`native-playtest.md`](../../.superpowers/sdd/2026-10-24-plan-6-global-foundations/native-playtest.md)
and Plan 6 Task 10. **Plan 7** (five single-area regions, B, D, E, F, H in §28.3) and **Plan 8**
(multi-area regions with HVDC links and SGCC, §28.9) remain pending. Each plan bumps
`SIM_RULES_VERSION` once, from whatever the previous implemented amendment left. The source for
boundaries, inclusions and exclusions is the research edition of 18 October 2026, copied to
[docs/research/2026-10-18-world-power-grids-top-8.md](../../research/2026-10-18-world-power-grids-top-8.md).
Its capacity figures set relative region scale only; no capacity label migrates into game data.

### 28.1 Why

Milestone 1 ships the two North American interconnections. The research map identifies the
world's eight leading power systems, and six of them are outside North America. Adding them
teaches lessons that North America cannot:

- **Time zones.** IPS/UPS runs one synchronous grid from Belarus (UTC+3) to Zabaykalsky Krai
  (UTC+9). Its cities peak at different dispatcher hours, so the system peak is flatter than any
  city's peak. Continental Europe spans Lisbon to Istanbul.
- **Southern seasons.** Brazil's summer peak falls in January, and its wet season fills
  reservoirs from December to April.
- **The tropics and the monsoon.** India's demand peaks before the monsoon. During the monsoon,
  wind and hydro rise while solar falls.
- **Polar light.** Murmansk, in the first IPS zone, has polar night in December and midnight sun
  in June.
- **Asynchronous grids.** The five SGCC components exchange power through HVDC links. Each keeps
  its own frequency, and each link carries a scheduled amount, not whatever copper plate allows.
- **Undersea links.** Morocco, Algeria and Tunisia are synchronous with Europe through the
  Spain–Morocco AC cable under the Strait of Gibraltar.

The current model assumes one clock for every city, one regional sun table, northern-hemisphere
seasons (`SimMonth::season()` selects demand shapes), US$ and one frequency per region. Each
assumption is wrong for at least one new region.

### 28.2 Goals, non-goals, anti-goals

- **Goals:**
  - Ship eight map entries (A–H) as playable regions, including the two existing ones.
  - One model of local time, sun and seasons serves every region, including North America for new
    games.
  - Copper plate still decides balancing. Local time enters only through the two physical drivers
    that depend on place: when people use power, and where the sun is.
  - A multi-area region balances each synchronous island separately and moves power between them
    only through player-scheduled HVDC links.
  - Every existing save keeps its archived behaviour exactly.
- **Non-goals:**
  - power flow, congestion or AC transfer limits, including on the Gibraltar cable (§3.3, §11);
  - per-city weather, several wind processes in one region, or basin-specific river regimes;
  - reservoir carry-over between months, which is still out of scope even for Brazil;
  - HVDC links between regions, imports from outside a region, or markets;
  - Morocco's Ramadan clock change, the equation of time, refraction and terrain shading;
  - automatic HVDC frequency support beyond the runback in §28.9.
- **Anti-goals:**
  - a sovereignty position: a region's footprint follows which grid actually operates an area,
    as the research notes record it, never a claim about who governs it (§28.3);
  - presenting the SGCC group as one synchronous machine;
  - tuning numbers in code, except the frozen legacy defaults of §28.11;
  - a silent default for a field that a schema-4 region file must author.

### 28.3 The regions

The map letter is a stable identifier, not a rank. Footprints follow the research notes; the
"excludes" column lists what the notes exclude, not everything outside.

| Map | Region id | Footprint | Excludes | Hz | Currency | System clock |
|---|---|---|---|---|---|---|
| A | `sgcc` | Five areas: North–Central (Beijing, Tianjin, Hebei, Shanxi, Shandong, western Inner Mongolia, Henan, Hubei, Hunan, Jiangxi), East (Shanghai, Jiangsu, Zhejiang, Anhui, Fujian), Northwest (Shaanxi, Gansu, Ningxia, Qinghai, Xinjiang), Northeast (Liaoning, Jilin, Heilongjiang; Hulunbuir, Xing'an, Tongliao, Chifeng), Southwest (Sichuan, Chongqing, Tibet) | Yunnan, region E, Mongolia | 50 | CNY | CST (UTC+8) |
| B | `continental_europe_maghreb` | Continental Europe synchronous area, including DK1, Türkiye, Ukraine and Moldova, the Baltic states, Sicily, Malta and Crete; Morocco, northern Algeria, Tunisia | Nordic area, DK2, Great Britain, Ireland, Sardinia, Corsica, the Balearics, isolated southern Algeria | 50 | EUR | CET/CEST |
| C | `eastern_interconnection` | As §2.6 | As §2.6 | 60 | USD | ET |
| D | `india_nepal_bhutan` | Mainland India, Nepal, Bhutan | Bangladesh, Sri Lanka, Myanmar, isolated systems | 50 | INR | IST (UTC+5:30) |
| E | `southern_china` | Guangdong, Guangxi, Guizhou, Hainan, Hong Kong, Macao | Yunnan | 50 | CNY | CST (UTC+8) |
| F | `ips_ups` | Russia's first synchronous zone, Belarus, Kazakhstan, Uzbekistan, mainland Azerbaijan, Kyrgyzstan, main Tajikistan, Georgia | Russian Far East, Kaliningrad, Armenia, Iran, Turkmenistan, Nakhchivan, Pamir systems, Afghanistan, Mongolia | 50 | RUB | MSK (UTC+3) |
| G | `western_interconnection` | As §2.6 | As §2.6 | 60 | USD | PT |
| H | `brazil_sin` | Brazil's main 60 Hz component, including Roraima, Itaipu's 60 Hz units and Santo Antônio's AC-connected units | Paraguay, Argentina, Uruguay, Venezuela, Jirau and other asynchronous generator groups, isolated Amazon systems | 60 | BRL | BRT (UTC−3) |

- **Disputed territory.** The grid-operation rule decides: an area belongs to the region whose
  grid operates it, as the research notes record, whatever its sovereignty status. The notes'
  generalised mainland footprints settle most cases. For example, D includes the Indian-operated
  north.
- **Areas the notes don't address.** These include Western Sahara, Abkhazia, South Ossetia,
  Transnistria and Kosovo. The region-data task settles each one by the same rule, records the
  operator source in the region file's comments, and never infers membership from a border.
- **The one exception.** Crimea and the occupied parts of Ukraine are left out of both B and F,
  with no cities and no buildable cells. Their boundary is the notes' schematic Ukraine–IPS
  operating extent, which does not follow complete oblasts. Region B's and F's data tasks each
  author that same exclusion polygon once, and the two regions must not overlap.
- **Borders.** The base map's borders express no sovereignty position (research notes,
  cartographic method).
- **Starting territories** (*builder's calls*; each is a few cities with a compact starter fleet,
  as §2.4 requires):

  | Region | Home city | Other starter cities |
  |---|---|---|
  | A | Beijing | Tianjin, Langfang |
  | B | Seville | Málaga, Tangier (joined by the pre-built Gibraltar link, §28.8) |
  | D | Delhi | Gurugram, Noida |
  | E | Guangzhou | Foshan, Dongguan |
  | F | Moscow | Tula, Tver |
  | H | São Paulo | Campinas, Santos |

- **Scale.** City base loads keep their real relative sizes inside a region. A region's total base
  load scales with its capacity relative to the Eastern Interconnection, compressed so that no
  region needs more than about 40 cities. *(Builder's call.)* Plant catalogs use local unit sizes
  and costs, and starter fleets follow each region's real mix.
- **Grids.** Cells stay equirectangular in longitude and latitude. Choose each region's bounds
  and grid so that cells are within 25% of square in kilometres at its middle latitude. Grids can
  be up to 256 × 128 cells. IPS/UPS uses about 1.0° × 0.5° cells. Line costs remain per cell and
  are authored per region.
- **Region lessons.** Each region's description card names its distinctive lessons: HVDC
  scheduling and Northwest curtailment (A), a three-hour time-zone spread and Maghreb solar (B),
  the monsoon and the evening solar ramp (D), Guizhou and Guangxi hydro with coastal nuclear (E),
  six time zones, Siberian hydro and polar light (F), and a hydro system with wet and dry seasons
  plus northeastern wind (H).

### 28.4 Local time: one system clock, a clock per city

- **The system clock.** Every region names one `system_zone`: the dispatcher's clock. The
  representative day runs from 00:00 to 24:00 on this clock. Commitment hours, AGC, the hydro
  schedule, battery windows (§25, §27), outage sizing, charts, the rate review and all reports use
  it. Copper plate has no local deficit: a battery in Novosibirsk covers a Moscow deficit as well
  as one next door. So system-level rules belong on the system clock, and region data authors
  battery windows against the system's aggregate net-load shape.
- **Zones.** Region data lists `time_zones`. Each zone has an id, a short-name key (for example
  `zone-msk`), a standard UTC offset in minutes (a multiple of 15, from −720 to +840) and
  `dst_months: [bool; 12]`. A DST month adds 60 minutes. Mark a month as DST when most of its days
  observe DST under the start year's rules (North America: March to October; the EU: April to
  October). Each city names a zone, and the system zone is one of them. An offset is constant for
  a whole month, so no played day has 23 or 25 hours.
- **A city's local minute.** `local(t) = (t + city_offset(month) − system_offset(month)) mod 1440`.
  The city's demand shape (§28.6) is evaluated at `local(t)`. The regional demand noise is
  unchanged: one AR(1) error for all cities, one normal draw per tick.
- **Zones are civil time, not longitude.** Spain keeps CET at Greenwich longitude, China keeps
  UTC+8 across five solar hours, and Arizona keeps MST all year. Authored zones carry these
  differences, while the sun (§28.5) follows longitude.
- **Shipped zones:**
  - C: ET, CT (with Regina on CST all year) and AT (Nova Scotia, New Brunswick, Prince Edward
    Island);
  - G: PT (including Baja California), MT and Arizona MST without DST;
  - B: WET (Portugal), CET (most of Europe; Morocco, Algeria and Tunisia on UTC+1 without DST),
    EET (Greece, Bulgaria, Romania, Ukraine, Moldova, the Baltic states) and TRT (UTC+3 without
    DST);
  - F: MSK (+3, including Belarus), +4 (Samara, Azerbaijan, Georgia), +5 (Yekaterinburg,
    Kazakhstan, Uzbekistan, Tajikistan), +6 (Omsk, Kyrgyzstan), +7 (Novosibirsk, Krasnoyarsk),
    +8 (Irkutsk, Buryatia) and +9 (Zabaykalsky);
  - D: IST, NPT (+5:45) and BTT (+6);
  - H: BRT (−3), AMT (−4) and ACT (−5), without DST;
  - A and E: CST (+8).
- **Display.** The top bar shows system time with its zone name ("14:20 MSK"). City cards show the
  city's local time and zone, and the commitment grid's hour axis names the system zone.
- **Planning samples.** Shapes are linear between whole local hours, so aggregate demand bends
  wherever any connected city's local hour begins. With +30 and +45 offsets these bends fall at
  :15, :30 and :45. Planning's shortfall check and supply chart, which sample minutes 0 and 59
  without storage (§8.4), also sample every minute where a connected city's local hour begins.

### 28.5 The computed sun

- **Model.** Schema-4 regions drop the authored `daylight` table. The domain computes the sun for
  each cell from its centre's latitude φ and longitude λ, which come from the region's bounds and
  grid:
  - the representative date is the 15th of the month, as a day of a non-leap year `n`;
  - declination `δ = −23.44° × cos(2π(n + 10)/365)`;
  - for system minute `t`, UTC hours are `(t − system_offset(month))/60`;
  - local solar time is `UTC hours + λ/15`, and the hour angle is `H = 15° × (solar time − 12)`;
  - `sun = max(0, sin φ sin δ + cos φ cos δ cos H)`, which is the sine of the solar elevation.
- **What follows from it.** The sun is up at a cell's own solar times, so eastern farms in a wide
  region start earlier on the system clock. Southern-hemisphere days are longest in December.
  Near the equator, day length barely changes. Above the Arctic Circle, a December day can have
  zero sun and a June day can have no sunset. No special cases are needed.
- **Inputs.** Available solar is `capacity × irradiance × sun × clearness`, as in §3.2, with the
  per-cell `sun`. Irradiance now rates site quality without latitude, because latitude is in
  `sun`. Re-author it for every schema-4 region. Clearness stays regional, as one draw per day.
- **Determinism.** The formula is pure. Implementations may cache one 1,440-minute table per cell
  and month. The libm caveat in §5.7 applies.
- **Overlay.** The sunshine overlay shows annual solar potential: irradiance × the annual mean
  daily sun integral at the cell's latitude ÷ the same integral at the equator. It keeps one
  absolute 0–100% scale across regions (§8.2).
- **Lighting.** The diorama sun uses the computed azimuth and elevation at the home city. It sits
  north at solar noon in the southern hemisphere, overhead in the tropics, and stays low all night
  under the midnight sun. During polar night only ambient light remains. Planning previews the
  home city's solar noon. This supersedes the northern-hemisphere arc in §8.2 for schema-4 regions.

### 28.6 Demand climates and the region calendar

- **Climates.** Schema 4 replaces the single `demand` block with `climates`. Each climate has an id
  and 1–4 named hourly shapes (24 values each), `month_shapes: [shape; 12]` and `monthly: [f64; 12]`.
  Each city names a climate. `forecast_error` and `error_persistence` move to a region-level
  `demand_noise` block. A climate lets Porto Alegre have a winter while Manaus has none, and lets
  Tashkent have an air-conditioning summer while Moscow has a heating winter.
- **Season labels.** The region authors `calendar.seasons: [Season; 12]`. `Season` gains
  `Monsoon` (portable tag 4) and `PostMonsoon` (tag 5), appended after Winter, Spring, Summer and
  Autumn. Every reader of `SimMonth::season()` takes the region calendar instead: the HUD label,
  milestones (a "full season" is a maximal run of consecutive months with one label) and lessons.
- **Shipped calendars:**
  - northern regions use meteorological seasons;
  - Brazil uses southern ones (summer from December to February, winter from June to August);
  - India uses the IMD scheme: Winter in January and February, Summer from March to May, Monsoon
    from June to September and PostMonsoon from October to December.
- **Resources.** Monthly wind factors, clearness and hydro flow are already region data. Brazil
  authors its wet season (high flow from December to April) and northeastern wind peaking from
  August to October. India authors monsoon wind and hydro highs alongside low clearness.

### 28.7 Local currency

- **The currency.** Schema-4 regions declare `currency`, one of USD, EUR, CNY, INR, RUB or BRL.
  All six use two minor units, so `Money` stays `i64` minor units, rounded once per ledger entry.
  Any other code fails to load with a typed error until minor-unit exponents are supported.
- **Field names.** Schema-4 money fields lose their `_usd` suffix, for example `capital_cost`,
  `cash`, `cost_per_cell` and `reference_tariff_per_mwh`. Values are in major units of the
  region's currency.
- **Price levels.** Money values (capital, O&M, fuel, start-up, tariffs, cash) come from local
  sources at local price levels, not from converted US figures. Pacing does not: build months,
  line `cells_per_month` and starting-cash headroom follow A7's compressed tycoon pacing (§18),
  so every region plays at North America's tempo. Every value is game tuning, pinned in that
  region's data task. The beginner checks (§26.7) and the early-expansion acceptance (§18.3) are
  the acceptance tests. *(Builder's call: "local" applies to prices, not to real construction
  schedules.)*
- **Display.** `format::money` takes the currency. The symbol and its placement come from Fluent
  (`currency-eur`, …). Compact suffixes stay k, M and B in en-US. Tariffs read "₹/MWh" and so on.
  A glyph test paints each symbol (€ ¥ ₹ ₽ R$) with the shipped fonts. If a glyph is missing, the
  formatter falls back to the ISO code ("INR 1.20M"), never a missing-glyph box.
- **Ratios are unchanged.** Satisfaction's price factor, the PBR terms and cost performance
  compare like with like, so they are currency-neutral.

### 28.8 The Gibraltar link and water crossings

- **Pre-built.** Region B's start network includes the Spain–Morocco interconnector. Tangier is a
  starter city, and its starter line from Seville is the link. It is ordinary: in service from the
  start, in the legacy rate base at its distance (§5.4), and with unlimited capacity under copper
  plate.
- **Naming.** `start.line_names` may give a starter city's line a name key. The Gibraltar link is
  `line-spain-morocco`, so its card and map label read "Spain–Morocco interconnector".
- **Line rules are unchanged.** A player-ordered line may still cross water at the land price.
  That covers Messina, Malta and Crete in B and the Qiongzhou Strait to Hainan in E.
- **Presentation.** Any line whose straight path crosses water cells is drawn as a submarine
  cable over those cells. The cable is a darker, dashed segment that keeps the line's energised
  pulses. A line card shows "Submarine cable" with its water length. A line card states that
  transfer limits are not simulated under copper plate (§8.3).

### 28.9 Multi-area regions: synchronous islands and HVDC links

**The unit of balancing.** A synchronous island is a connected component of energised nodes
joined by in-service AC lines. A node is energised when a path of in-service AC lines and
in-service HVDC links joins it to the home city. Copper plate (§5.4) applies within each island:
its own frequency, imbalance, AGC, merit order, protection, hydro schedule, battery plans, outage
sizing and §19 recovery. A single-area region without HVDC always has exactly one island, the
home city's, so the existing rules are the one-island case and must behave identically. Islands
are recomputed only where topology can change: at month close and at `BeginOperating`. An island's
id is its lowest NodeId, and islands step in ascending id order. Each played day starts every
island at nominal frequency.

**Areas.** Region data may author `areas`: id, name key and polygons. Rasterised, the areas
partition the region's land cells exactly; an overlap or gap fails to load. Each city and starter
plant lies in one area. A region without areas has one implicit area covering its land.

- **No AC between areas.** `BuildLine` between nodes in different areas returns
  `DomainError::LineCrossesAreas` and leaves the session unchanged. Copper plate cannot represent
  the stability limits that make long AC ties between these grids hard. An unlimited AC tie would
  always beat an HVDC link and collapse the region into one bus. So the boundary is a rule, and the
  endpoint picker marks such endpoints unavailable with that reason.
- **AC need test.** §26.4 now counts AC paths only. A path that joins two nodes only through HVDC
  does not make an AC line between them redundant, because the AC line would synchronise them.

**HVDC links.** These are a buildable asset where region data authors an `hvdc` catalog entry. Its
fields are `rating_mw`, `converter_cost` (for both ends), `cost_per_cell`, `converter_build_months`,
`ramp_mw_per_min`, `converter_loss` (per end), `loss_per_cell`, `book_life_months` and
`designed_hours_per_year`. Shipped SGCC values are game tuning pinned in the Plan 8 data task.

- **Build.** `BuildLink { from, to }` works in Planning between two existing nodes. It costs
  `converter_cost + cost_per_cell × cells` in equal instalments over
  `max(converter_build_months, line months)`, and `link_quote` reports exactly that.
- **Need test.** A link's ends must lie in different areas; otherwise `BuildLink` returns
  `LinkWithinArea`. AC lines never cross areas, so a link's terminals can never end up in one
  island, and the case of a link inside one island cannot arise. *(Builder's call: this makes HVDC
  an inter-area asset only, so a region without `areas` can author `hvdc` but never build one.)*
  Parallel links between two areas are allowed, because links have finite ratings. Cancelling an
  unfinished link follows §16.9.
- **Schedule.** In Planning, `SetLinkSchedule { link, mw: [f64; 24] }` sets a signed MW value for
  each hour, positive from `from` to `to`, finite and within ±rating. The default is zero.
- **Override.** In Operating, `SetLinkTarget { link, mw }` overrides the schedule for the rest of
  the day, and `ClearLinkOverride` returns to it. Refused commands leave the session unchanged.
- **Flow.** Each tick, before the islands step, every link's flow ramps toward its target at
  `ramp_mw_per_min`. The receiving island gets
  `flow × (1 − 2 × converter_loss − loss_per_cell × cells)`. Validation keeps that factor at or
  above 0.5. Losses are reported energy; no fuel or revenue entry is created.
- **Blocking.** A link's flow drops to zero, and it resumes at its ramp, while either terminal
  island is blacked out or has no online synchronous generator (thermal, nuclear or hydro). A
  line-commutated converter needs a strong AC grid at both ends. Batteries, solar and wind do not
  count.
- **Runback.** While the sending island is below its under-frequency shedding threshold, the
  link's flow ramps toward zero. It never reverses automatically, and it releases once the island
  is back inside its normal band.
- **Timing.** Blocking and runback read the previous tick's island states and take effect this
  tick, in ascending LinkId order.
- **Balance per island.** Supply is generation, battery discharge and received link energy. Load is
  served customer demand, battery charging and sent link flow. The signed imbalance absorbs the
  difference, and the frequency step uses the island's own system size.

**Protection, reliability and strikes.** Shedding, curtailment, blackout and over-frequency trips
act within one island. A blackout in East China trips only East China's plants and blocks its
links. Reliability and the unserved-energy strike test sum over islands. The frequency-performance
test (§14.4) counts each minute as the demand-weighted share of islands outside their band, so a
one-island region is unchanged.

**Forecasts and warnings.** Each island's forecasts, shortfall checks, rotating outages and
battery and hydro plans treat the scheduled link flows as known injections. A link schedule never
assumes another island's spare capacity. If a schedule exports more than the sender can supply,
the sender's warnings show it, and it is the player's call.

**Regulation.** The company holds one licence and faces one regulator.

- **Reserve assessment.** The §2.5 reserve test stays region-wide: total firm capacity against the
  coincident peak. *(Builder's call: one licence, one test. Reliability penalises a starved island
  directly.)*
- **Links hold no firm MW.** They are judged by use, not by firm capacity. At each December review,
  a link's utilisation is `u = sent MWh in the review period ÷ (rating × designed_hours_per_year ×
  months ÷ 12)`, using weighted monthly energy, and its excess fraction is `1 − min(1, u)`. Here
  `months` counts the link's whole months in service within the review period, its commissioning
  month included, so a link commissioned in November is judged on November and December alone.
  *(Builder's call: this prorates like A2, §13, so a new link is not held for months it could not
  run.)*
- **Hold, release, write-off.** The §2.5 and §26 history applies unchanged: hold, re-test, release,
  and write off a portion only when it is excess at two consecutive reviews.
- **Why.** This follows China's pricing of cross-regional dedicated transmission, which is set from
  each project's costs and a designed annual transfer volume, so an underused line under-recovers.
  *(Builder's call: the hold-and-write-off adaptation is ours.)*

**HUD and map.**

- **Island chips.** A row of chips in the top bar shows each island's area name, with a suffix when
  an area has more than one island, plus a frequency dot. Selecting a chip scopes the frequency
  gauge, supply charts, warnings and incidents. "All" shows region totals and no gauge.
- **Link card.** It shows rating, current flow and target, a 24-hour signed schedule editor, the
  blocked or runback state, losses, and year-to-date utilisation against the designed hours.
- **Map.** Area boundaries are drawn on the terrain. Links are drawn as DC corridors with converter
  glyphs at both ends. Link pulses show actual DC flow and direction, which is exact, unlike the
  illustrative AC routes of §8.3, and the legend says so.
- **Conventions.** The presenter and dispatch rules, Fluent coverage, Enter and Escape handling and
  the minimum-window layout rules apply.

**Persistence.** Plan 8 adds island frequency states, link assets, schedules, overrides and flows,
plus each island's report rows (served, unserved, imports, exports, losses and minutes outside the
band) to portable state. It bumps the state schema once, with readers for every earlier schema. An
upcast gives an older save one island with its existing frequency and no links. Region totals in
old reports stay as recorded.

### 28.10 The world picker

- **Map.** The region picker becomes a world map in the Equal Earth projection, cropped to
  60°S–75°N. Each shipped region is filled from its own outline with the research legend's colour
  and letter: A `#CF414C`, B `#226EC7`, C `#E4832F`, D `#B29400`, E `#008F9D`, F `#7652B2`,
  G `#936A45`, H `#1E8F59`.
- **Atlas metadata.** The letter and colour are optional `atlas` metadata in region RON. Only the
  infrastructure outline reader uses them; the domain never sees them. A custom region without
  them takes the next colour from the fallback palette.
- **Areas.** SGCC is one entry. Its area boundaries are drawn, and hovering shows the area names.
- **Text.** Letters and labels sit on opaque backings at the 4.5:1 contrast floor (§8.4).
- **Region cards.** Each card adds frequency, currency, time-zone span and hemisphere to the
  existing description and home city.
- **Unchanged behaviour.** Hover, click, keyboard start buttons, the seed rules and disabled entries
  stay as in §2.6. Grey land is unselectable, and the picker does not imply that grey land lacks
  electricity.
- **Small windows.** At the minimum window size, Europe and the Chinese regions are too small to
  aim at. The picker may zoom or show an inset, but every region stays startable from its card.

### 28.11 Region schema 4, compatibility and versioning

- **Schema 4.** New authored fields: `system_zone`, `time_zones`, a zone per city, `climates`,
  `demand_noise`, `calendar`, the computed-sun `solar` block (no `daylight`), `currency` with
  unsuffixed money fields, optional `areas`, optional `hvdc`, optional `start.line_names` and
  optional `atlas`. A schema-4 file missing a required field fails to load with a typed error that
  names the field.
- **Schema-4 irradiance authoring.** `irradiance` is either the existing Gaussian field
  `(base: value, blobs: [...])` or an exact cell field `(cells: [...])`. The forms are mutually
  exclusive; partial Gaussian fields and mixed forms are rejected. The cell list has exactly
  `grid.width × grid.height` finite values in `[0, 1]`, row-major from the northwest cell.
  These are authored site-quality values, not annual potential: the usual computed sun and
  annual latitude factor still apply unchanged. An offline data tool may invert that factor to
  preserve an existing field's relative solar-site ranking; no runtime compensation or
  calibration is added. Schemas 3 and earlier keep only their existing Gaussian syntax and math.
- **Schemas 3 and earlier.** Version-aware mapping supplies frozen legacy behaviour: one zone at
  offset zero without DST, the authored `daylight` table (the domain keeps
  `SolarModel::DaylightTable` beside `SolarModel::Astronomical`), one climate with northern
  meteorological seasons, USD, one area and no HVDC. These defaults live only in the legacy
  mapping and never apply to a schema-4 file.
- **North America.** Both shipped North American files move to schema 4 for new games, with zones
  (§28.4), the computed sun, re-authored irradiance and climates. Their balance changes. Fix any
  failing beginner check by retuning region data, never the rules. Their fingerprints change.
  Archived bytes do not, so existing saves keep their single clock and daylight table.
  The migrated annual potential preserves every strict ordering between playable cells in the
  frozen legacy irradiance field; acceptance groups equal old values and requires each group's
  largest new value to be below the next group's smallest, including rejection of collapsed ties.
- **Portable state.**
  - **Plan 6:** the new `Season` tags enter persisted milestone state, so it bumps the state schema
    once.
  - **Plan 8:** bumps it again (§28.9).
  - **Both:** keep every older reader. Verify old bytes with old semantics before any upcast, and
    never rewrite history (§21, §25.6).
- **Rules versions.** Each plan bumps `SIM_RULES_VERSION` once and regenerates the rules guard and
  the scenario snapshots, reviewing every diff. Command-only logs stay fenced by
  `RulesVersionMismatch`.

### 28.12 Acceptance

- **Time.**
  - A city's demand at system minute `t` equals its climate shape at `local(t)` for every shipped
    zone, including +5:30, +5:45, −3 and Arizona.
  - DST months shift by exactly 60 minutes, and no month has a mid-day change.
  - Demand noise draws exactly one normal per tick, whatever the number of cities or zones.
  - Planning samples hit every quarter-hour bend that offset cities create.
- **Sun.**
  - Hand-computed elevations agree at Quito, Murmansk (December zero and June positive at local
    midnight), São Paulo (December longer than June) and Novosibirsk (sunrise earlier than
    Moscow's on MSK).
  - Overlay potential is monotone in irradiance and stays within [0, 1].
  - Lighting points north at solar noon in Brazil.
- **Seasons.** Brazil's January uses its summer shape. India's labels and milestones follow its
  calendar. Legacy regions reproduce `SimMonth::season()` exactly.
- **Currency.** Every shipped currency formats with its glyph or the ISO fallback, and cash change
  still equals the ledger sum exactly. A schema-4 file with a `_usd` field or an unknown currency
  fails with a typed error.
- **Gibraltar.** Region B starts with Seville, Málaga and Tangier served, and the named cable is
  drawn as a submarine segment.
- **Islands.**
  - Every existing property and scenario holds unchanged in one-island regions; the rules guard
    shows only intended changes.
  - Over generated networks, islands equal the AC components of the energised graph.
  - AC lines never cross areas, links never stay within one, and a refused `BuildLine` or
    `BuildLink` changes nothing.
- **Links.**
  - Flow stays within ±rating and its ramp.
  - Received energy equals sent energy minus losses.
  - Blocking, runback and next-tick timing behave as §28.9 says.
  - A blackout in one island leaves the other islands' plants untouched.
  - Island and total conservation hold: generation = served + charging + imbalance + link losses.
  - Utilisation holding matches a hand-computed review and its two-review write-off.
- **Saves.** Golden saves from every supported state schema and from schema-3 regions restore
  byte-identically under their archived behaviour, continue, and re-save under the new schema. A
  new game saved mid-day in each new region resumes identically to uninterrupted play.
- **Regions.**
  - Every shipped region loads, and its starter territory passes the no-command and hands-off
    year-one runs (§26.7) and the early-expansion acceptance (§18.3) on seeds 0–7.
  - The terrain mesh, picking and the world picker meet frame budgets at 256 × 128 cells.
  - Native launch smoke passes, and an isolated full-day playtest runs in IPS/UPS, Brazil and
    SGCC (with one scheduled link).
