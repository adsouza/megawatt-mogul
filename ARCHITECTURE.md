# Architecture

Megawatt Mogul uses Clean Architecture and Domain-Driven Design, applied pragmatically. The domain
is a pure Rust core. Bevy is an outer adapter. The full design is in the
[spec](docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md); this page is the short version
for people changing code.

**Status:** only the workspace scaffold exists (three empty crates and the dependency-rule test). The
sections below describe the design Plan 1 implements; the types and modules named here (`GameSession`,
`CommandHandler`, `SimRng`, `Megawatts`, `NetworkModel`, ...) do not exist yet. The `game` crate is
additionally planned for Plans 2-3.

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

## Where things go (planned layout)

| What | Where |
|---|---|
| Value objects (`Megawatts`, `Money`, `SimTime`, IDs, ...) | `domain::units` |
| Game rules (plants, demand, grid balancing, company, regulation, session) | A `domain` module per bounded context |
| Use cases (commands and their handlers), pipeline layers, queries and projections, ports | `application` |
| File formats (RON DTOs, the command-log format) and their mapping to domain types | `infrastructure` |
| Rendering, input, UI and localised text | `game` (*planned*) |

Region data (plant tuning numbers, cities, regulator parameters) lives in `assets/regions/*.ron`,
never as code constants.

## Write side: commands (Plan 1 — not yet built)

Every command is a plain struct handled as `handle(&mut GameSession, cmd) -> Result<Vec<DomainEvent>,
CommandError>`. Layers wrap the handler through one `CommandHandler` trait and are assembled in a
single `build_pipeline()`:

```
PlayerCommand ─▶ TracingLayer ─▶ InvariantCheckLayer* ─▶ CommandLogLayer ─▶ handler ─▶ events ─▶ projections
```

`*` marks `InvariantCheckLayer`, which is active only in debug and test builds.

The domain never panics on player input. It returns `DomainError`, and a command that returns `Err`
leaves the session unchanged. Panics are reserved for bugs (invariant violations).

## Read side: queries and projections (CQRS) (Plan 1 — not yet built)

The presentation layer never reads aggregate internals. It uses cheap snapshot queries
(`GridSnapshot`, `PlantView`, `CompanyView`, ...) and projections that are updated from domain events
and per-tick summaries (timeline, monthly report, plant history, milestones, rate-review estimate,
ramp warnings, lessons). Milestones are read-side: they are tracked by a projection, not emitted as
domain events.

Ticks and commands return `Vec<DomainEvent>`. Nothing is pushed through an event framework, and
policies (event to reaction, such as strikes or bankruptcy) run inside the domain.

## Command sourcing (Plan 1 — not yet built)

Persistence is command sourcing, not event sourcing: `CommandLogLayer` records the seed plus the
commands through the `CommandLogSink` port, and the aggregates know nothing about persistence. Because
the simulation is deterministic, the same seed and command log replay to an identical state hash.
Event sourcing would force every domain event to be a stable, versioned persistence format, which
fights the goal of keeping the domain free to change. Commands are the smaller, more stable surface.
Save/load and replays build on the log in a later milestone.

## Determinism rules

The same seed plus the same command log must produce an identical state hash.

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

## The tick (Plan 1 — not yet built)

`GameSession::tick` runs these steps in a fixed order:

1. Advance the clock.
2. Compute actual demand for connected cities.
3. Compute solar availability.
4. Update hydro budgets.
5. Plants progress (construction completes at month boundaries; start-up; ramp toward target).
6. Auto-start or stop committed plants for the upcoming hours.
7. AGC re-targets auto-follow plants.
8. Imbalance = supply - demand.
9. Frequency update.
10. Protection (shed, curtail or blackout).
11. Accounting: accumulate energy per plant (MWh), start-ups and served energy. Nothing is priced
    per tick.
12. Collect events. At tick 1,440 the month's totals are weighted, `MonthClosed` is emitted,
    policies run, and the phase moves to Review.

## Settings seams (Plan 1 — not yet built)

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
