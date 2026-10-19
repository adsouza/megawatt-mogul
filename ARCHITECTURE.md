# Architecture

Megawatt Mogul uses Clean Architecture and Domain-Driven Design, applied pragmatically. The domain
is a pure Rust core. Bevy is an outer adapter. The full design is in the
[spec](docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md); this page is the short version
for people changing code.

**Status:** Plan 1 (the headless simulation core: `domain`, `application` and `infrastructure`) is
complete. The `game` crate, the Bevy client, is planned for Plans 2-3.

## Layers and the dependency rule

```
game (planned) ──▶ application ──▶ domain
        └────────▶ infrastructure ─┘   (infrastructure implements application ports)
```

| Crate | Role |
|---|---|
| `domain` | Model, rules and events. Pure Rust: no Bevy, no serde, no I/O. May depend only on `rand_chacha` and `thiserror`. |
| `application` | Commands, handlers, pipeline layers, queries, projections and ports. Depends on `domain` (plus `tracing`). |
| `infrastructure` | Adapters that implement ports: the RON region loader (DTOs plus mapping) and the command-log writer. |
| `game` (*planned*, Plans 2-3) | The Bevy binary. Depends on all three. The only crate that may use Bevy. |

The rule is enforced by the compiler: `domain/Cargo.toml` does not list Bevy, so an import is
impossible, and `crates/domain/tests/dependency_rule.rs` fails if a framework or I/O crate appears
there. `cargo test -p domain` and `-p application` never compile Bevy, so feedback takes seconds.

## Where things go

| What | Where |
|---|---|
| Value objects (`Megawatts`, `Money`, `Ratio`, ...), IDs, and time (`SimMonth`, `SimDuration`, the clock) | `domain::units`, `domain::ids`, `domain::time` |
| Game rules (plants, demand, grid balancing, company, regulation, session) | A `domain` module per bounded context |
| Use cases (commands and their handlers), pipeline layers, queries and projections, ports | `application` |
| File formats (RON DTOs, the command-log format) and their mapping to domain types | `infrastructure` |
| Rendering, input, UI and localised text | `game` (*planned*) |

Region data (plant tuning numbers, cities, regulator parameters) lives in `assets/regions/*.ron`,
never as code constants.

## Write side: commands

`Command` is one enum with a variant per player action (`BuildPlant`, `SetDispatchTarget`,
`AdvanceTicks`, ...). Every command is handled as `handle(&mut GameSession, &Command) ->
Result<Vec<DomainEvent>, CommandError>`. Layers wrap the core handler through one `CommandHandler`
trait (`Send + Sync`, so a `Game` can be a Bevy `Resource`) and are assembled in a single
`build_pipeline()`:

```
PlayerCommand ─▶ TracingLayer ─▶ InvariantCheckLayer* ─▶ CommandLogLayer ─▶ handler ─▶ events ─▶ projections
```

`*` marks `InvariantCheckLayer`, which is active only in debug and test builds.

The domain never panics on player input. It returns `DomainError`, and a command that returns `Err`
leaves the session unchanged. Panics are reserved for bugs (invariant violations).

## Read side: queries and projections (CQRS)

The presentation layer never reads aggregate internals. It uses cheap snapshot queries
(`GridSnapshot`, `PlantView`, `CompanyView`, ...) and projections that are updated from domain events
and per-tick summaries (timeline, monthly report, plant history, milestones, rate-review estimate,
ramp warnings, lessons). Milestones are read-side: they are tracked by a projection, not emitted as
domain events.

Ticks and commands return `Vec<DomainEvent>`. Nothing is pushed through an event framework, and
policies (event to reaction, such as strikes or bankruptcy) run inside the domain.

## Command sourcing

Persistence is command sourcing, not event sourcing: `CommandLogLayer` records commands through
the `CommandLogSink` port, and the aggregates know nothing about persistence. The header preserves
log/rules versions, settings, seed and a SHA-256 fingerprint of the exact region source bytes.
Infrastructure archives those same bytes as `<log-name>.region.ron` before the session starts.
Replay verifies versions and fingerprints before executing commands and uses the archive when
shipped data changes. A rules change still needs a matching engine or a future explicit migration.
Event sourcing would force every domain event to be a stable, versioned persistence format, which
fights the goal of keeping the domain free to change. Commands are the smaller, more stable surface.
Save/load and replays build on the log in a later milestone.

## Determinism rules

The same verified versions, settings, region content, seed and command log must produce an identical
state hash. Bit-identical replay is guaranteed **for the same build on the same platform**. A few
places call the platform's libm, whose last-bit results may differ between targets: the Box-Muller
transform in `SimRng::standard_normal` (`ln`, `cos`), the solar day curve (`sin`), line lengths
(`hypot`) and the terrain rasteriser in `infrastructure` (`cos`, `exp`). The region fingerprint
hashes the source bytes, not the rasterised grid, so it cannot detect a rasterisation difference.

- Use `BTreeMap` / `BTreeSet`. Never iterate a `HashMap`.
- No wall-clock time.
- All randomness goes through `SimRng`.
- The tick is single-threaded.
- Break ties by ID.

Time and money follow from this. One tick is one game minute (`TICKS_PER_HOUR = 60`,
`TICKS_PER_DAY = 1_440`). One played day is the representative day of a calendar month, and a game
year is 12 played days. `Money` is `i64` cents. Nothing is priced per tick: physical totals are
priced once per ledger entry at month close as `price x energy x days_in_month`, rounded once with
`f64::round_ties_even` to whole cents.

Commitments are cyclic daily patterns. Every off-gap, including across midnight, must fit worst-case
shutdown from capacity plus start-up; all-day/all-off patterns need no daily restart. At each
`BeginOperating`, initialise physical plant state from the pattern, including cross-midnight starts
and shutdown tails. A 48-hour nuclear start never carries from one representative month to another.
Frequency/protection start at nominal/Normal; prior incidents and reliability remain in history.
Recurring daily starts are weighted by `days_in_month`; a distinct return from a whole month offline
costs one boundary start. Never charge that boundary start again when a recurring restart covers it.
Operating manual starts must finish by midnight; stops/trips suppress automatic actions for the day
without rewriting the validated base pattern. Planning start/stop select all-day/all-off patterns.

Annual reserve assessment uses the higher of the actual year peak and the next 12 months' projected
coincident peak for all cities connected at review time, including December's new connections.
Excess new capital is held without allowed return, re-tested next year, and written off only when
the same portion remains excess at two consecutive reviews. Per-asset held portions and review
history survive annual tally resets; preview queries never age them. Held capital still depreciates;
only eligible remaining book value earns return. Holding and write-off do not change cash.

## The tick

`GameSession::tick` first requires an active game in `Operating` (`require_operating`), then runs
`PowerSystem::tick` over the connected grid and its own bookkeeping, in this fixed order:

1. At each hour boundary after midnight (tick 60, 120, ...), the hydro schedule is re-planned for the
   rest of the day from the remaining budgets (`plan_hydro`; `BeginOperating` plans from hour 0).
   Planning draws no randomness.
2. Demand of connected cities: forecast × (1 + forecast error). The AR(1) error advances by exactly
   one normal draw every tick, in any state.
3. Solar and hydro availability for every plant (hydro from the day's remaining energy budget, reset
   at `BeginOperating`). The standing solar curtailment is withheld from connected online solar, pro
   rata to availability.
4. The commitment actions due at this tick; plants suppressed for the day get none.
5. Every plant steps once: start-up progress, ramp toward target, hydro budget use.
6. AGC re-targets connected auto-follow plants in merit order toward the load to serve,
   `demand × (1 − shed)` (one restoration step higher during a blackout), making room for withheld
   solar. Scheduled hydro is capped at its hourly allocation for the tick the targets take effect
   (`tick + 1`), with a ramp envelope ahead of a falling hour; the cap is lifted while load is being
   shed.
7. Imbalance between connected supply and served load.
8. Frequency update (held at nominal during a blackout).
9. Protection: load shedding, a blackout that trips every plant, solar curtailment for the next
   tick, and an over-frequency trip of the most expensive online producer once frequency has stayed
   high for `trip_after_ticks`. A trip waits while there is no physical surplus, while the rest of
   the fleet could not cover the lost output within one tick, or while another plant is already
   losing output AGC cannot reverse, so protection does not trip the grid into a deficit. After a
   trip AGC re-targets again in the same tick, so the rest of the fleet responds on the next step.
   Tripped solar reconnects once restoration re-energises the grid.
10. The `TickSummary` of the post-protection state, also emitted last as `TickCompleted`.
11. Accounting: the session adds the summary to the month-to-date totals (energy per plant,
    recurring and boundary starts, served, unserved and overgenerated energy, and the Normal-mode
    ticks outside the frequency-performance band). Nothing is priced per tick.
12. The clock advances. After tick 1,440 the month closes: totals are weighted by `days_in_month`
    and booked, construction advances, policies run (including the frequency-performance notice or
    strike), `MonthClosed` is emitted and the phase moves to Review.

Each `TickSummary` satisfies `generation = Σ by_plant = served + imbalance` (signed), with
`served = demand × (1 − shed)` and `unserved = demand × shed`. `curtailed` is solar availability
withheld, which is never generated, so it is not part of `generation`.

## Settings seams

```rust
pub struct GameSettings { pub fidelity: SimulationFidelity }
pub struct SimulationFidelity { pub network: NetworkModel }
pub enum NetworkModel { CopperPlate }   // later: DcPowerFlow
```

`NetworkModel` is a one-variant enum rather than a `bool`, so adding a variant later forces every
`match` to handle it. Each region is a single balancing area (copper plate) in Milestone 1.

## Strings

`domain` and `application` contain no user-facing strings, only typed values and `name_key`
translation keys. Localisation happens in `game` (*planned*).
