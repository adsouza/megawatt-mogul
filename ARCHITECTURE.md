# Architecture

Megawatt Mogul uses Clean Architecture and Domain-Driven Design, applied pragmatically. The domain
is a pure Rust core. Bevy is an outer adapter. The full design is in the
[spec](docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md); this page is the short version
for people changing code.

**Status:** Plan 1 (the headless simulation core: `domain`, `application` and `infrastructure`) and
Plan 2 (the playable client), Plan 2b (save/resume) and Plan 3 (interactive 3D diorama) are
implemented.

## Layers and the dependency rule

```
game ──▶ application ──▶ domain
  │            ▲            ▲
  └──▶ infrastructure ──────┘   (infrastructure implements application ports)
```

`game` also uses `domain` directly, for its typed values and views.

| Crate | Role |
|---|---|
| `domain` | Model, rules and events. Pure Rust: no Bevy, no serde, no I/O. May depend only on `rand_chacha` and `thiserror`. |
| `application` | Commands, handlers, pipeline layers, queries, projections and ports. Depends on `domain` (plus `tracing`). |
| `infrastructure` | Adapters that implement ports: the RON region loader (DTOs plus mapping) and the command-log writer. |
| `game` | The Bevy binary and its library: states, localisation, the simulation bridge and the HUD. Depends on all three (`infrastructure` for the RON region catalog and the file command log). The only crate that may use Bevy. |

The rule is enforced by the compiler: `domain/Cargo.toml` does not list Bevy, so an import is
impossible, and `crates/domain/tests/dependency_rule.rs` fails if a framework or I/O crate appears
there, or if the `domain`, `application` or `infrastructure` manifest lists Bevy or egui (`bevy`,
`bevy_*`, `egui`, `egui_*`) in any dependency table. `cargo test -p domain` and `-p application`
never compile Bevy, so feedback takes seconds.

## Where things go

| What | Where |
|---|---|
| Value objects (`Megawatts`, `Money`, `Ratio`, ...), IDs, and time (`SimMonth`, `SimDuration`, the clock) | `domain::units`, `domain::ids`, `domain::time` |
| Game rules (plants, demand, grid balancing, company, regulation, session) | A `domain` module per bounded context |
| Use cases (commands and their handlers), pipeline layers, queries and projections, ports | `application` |
| File formats (RON DTOs, the command-log format) and their mapping to domain types | `infrastructure` |
| Rendering, input, UI and localised text | `game` (see [The game crate](#the-game-crate)) |

Region data (plant tuning numbers, cities, regulator parameters) lives in `assets/regions/*.ron`,
never as code constants. Shipped files declare `region_schema: 4`, including explicit clock,
climate, calendar, currency, and astronomical-sun data. Legacy region schemas retain frozen
defaults in `infrastructure::region::legacy`; old-source integrity is checked before any
upcast. Unexpected new fields on legacy sources and unsupported schemas fail with typed errors.
Region schema versions are independent of portable state schema 3, written by current saves;
readers retain schemas 1–3.

Seven regions ship in `assets/regions/`, each a single synchronous balancing area (a copper plate)
priced in its own currency:

| Region ID | Name | Grid | Currency |
|---|---|---|---|
| `eastern_interconnection` | Eastern Interconnection | 60 Hz | USD |
| `western_interconnection` | Western Interconnection | 60 Hz | USD |
| `southern_china` | Southern China | 50 Hz | CNY |
| `brazil_sin` | Brazil | 60 Hz | BRL |
| `india_nepal_bhutan` | India–Nepal–Bhutan | 50 Hz | INR |
| `ips_ups` | IPS/UPS | 50 Hz | RUB |
| `continental_europe_maghreb` | Continental Europe–Maghreb | 50 Hz | EUR |

Plan 7 (rules 18) added the five non-North-American regions as region data and tools only; no
simulation rule changed. Multi-area regions, HVDC links and SGCC are Plan 8.

**Offline region authoring.** Two directories hold what is used to write a region file, and none of
it runs in the game or in the release build:

- `tools/region_authoring/` holds Python helpers (see its `README.md`). `natural_earth.py` turns
  Natural Earth v5.1.2 admin-0 and admin-1 features into the RON `land` and `exclude` polygons,
  with a provenance header that records the file hashes, the command and the area lost to
  simplification. `research_svg_extent.py` checks a committed exclusion against the research map.
  Their output is reviewed and pasted into region files by hand; downloaded data is never committed.
- `crates/infrastructure/authoring/` holds committed authoring inputs. The Ukraine–IPS operating
  extent (Crimea and Sevastopol) is the one canonical exclusion that Continental Europe–Maghreb and
  IPS/UPS share. The Europe–Maghreb economic-zone polygons are rasterised into that region's
  `economic_zones.cells` by `cargo run -p infrastructure --example author_economic_zone_cells`,
  and a shipped-regions test checks that the committed cells are reproducible from the polygons.

**Research-row provenance.** Every real-world money or construction-time figure lives in one
place: `docs/research/2026-10-25-plan-7-regional-economics.md`, as rows with IDs of the form
`P7-<area>-<nnn>`. A new region's plant catalog entries, `lines:`, `starting_tariff_per_mwh:`,
`cash:` and economic-zone profiles must each have a `// ... [P7-…]` comment block directly above
that cites a row. `new_region_entries_cite_research_rows` in
`infrastructure/tests/shipped_regions.rs` enforces both directions: an entry without a citation
fails, and so does a citation that names no row of the note. Capacity labels from the older
research map set relative scale only; no figure migrates into game data without a row.

Rules 17 add optional construction economic zones to region schema 4; Europe–Maghreb is the first
shipped region to opt in. The domain's
`region::economics` validates the dense cell map and partial profiles, resolves plant specs at a
cell, and prices AC routes by canonical rational midpoint samples. Exact route work uses a checked
u64 common denominator and u128 sum, rounded once. `plant_quote`/`line_quote` check representable
completion dates before an order mutates state. Orders, starters and commissioning share these
terms; owned plants retain their effective spec, while owned lines resolve fixed geometry against
the immutable archived region. Snapshot validation compares plant specs against their cell's
effective spec. No state fields or schema change: portable state stays 3. Absent economics retains
legacy pricing. These zones do not affect electrical areas, physics, currency or operating costs.
The HUD formats domain quotes and stored construction history; it does not calculate local prices.

The new-game globe is also presentation-only. `hud::region_atlas` caches geographic union meshes
on the unit sphere; `region_globe` supplies front-hemisphere projection, limb clipping and the
24-second rotation clock. Mouse dragging changes longitude while retaining the 22° pole tilt;
automatic rotation resumes two seconds after release. Card bodies select a transient region and
animate its geographic midpoint to the front along the shortest longitude arc, then hold for two
seconds before resuming. Title buttons start games as before. Its transient egui state never enters
simulation time, randomness, commands or saves. The existing region cards remain keyboard-accessible.

Rules 13 accredit commissioned storage by deliverable duration (including disabled batteries),
refresh rate-base firm MW only at annual review, and adjust allowed return by post-close customer
satisfaction. Estimates refresh a clone; restoring a snapshot never refreshes its saved profiles.
New line orders must join separate components across all in-service and ordered lines. This rule
lives in the session quote, so pre-existing cycles can still restore and commission normally.

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

The timeline projection keeps its rolling live buffer and archives each completed day's full
minute samples on `MonthClosed`. It captures the original hourly forecast on entering Operating,
before month-close construction can connect more demand. Review can switch between its report
and this chart; Planning and Operating History tabs select any completed month. These read models
rebuild from the existing command journal on resume. Browsing changes only HUD state, leaving
the simulation, checkpoint, rules version and save format unchanged.

Ticks and commands return `Vec<DomainEvent>`. Nothing is pushed through an event framework, and
policies (event to reaction, such as strikes or bankruptcy) run inside the domain.

## Command sourcing

Persistence is command sourcing, not event sourcing: `CommandLogLayer` records commands through
the `CommandLogSink` port, and the aggregates know nothing about persistence. The header preserves
log/rules versions, settings, seed and a SHA-256 fingerprint of the exact region source bytes.
Infrastructure archives those same bytes as `<log-name>.region.ron` before the session starts.
Replay verifies versions and fingerprints before executing commands and uses the archive when
shipped data changes. The shipped game has no rules-6/7 importer or special upgrade UI. One-time
recovery tools and their historical engines are frozen outside the workspace in
`tools/archive/save-recovery-2026-10-23/`; existing portable snapshots load normally.
Event sourcing would force every domain event to be a stable, versioned persistence format, which
fights the goal of keeping the domain free to change. Commands are the smaller, more stable surface.
`Game` also retains its original header/region bytes and every accepted command, independently of
that best-effort recording. `checkpoint()` captures a pure boundary with count, hash and summary.
Format-1 restoration replays original command batches through the matching `CoreHandler`, applying
projections against each batch's resulting session. Formats 2 and 3 restore complete portable state
without replaying history when present. Format 3 run-length encodes adjacent identical tick commands,
expanding the exact original batches before count/journal-hash verification or command-only replay.
Readers retain formats 1 and 2. All paths verify the boundary, seed a fresh branch recording exactly once,
and return a runnable candidate. Refused commands never enter the journal.

`SaveStore` is an application-owned port. Production snapshots use self-contained format-4
directories: a compressed text manifest plus checksummed binary chart files, one per month.
Infrastructure stamps the saved state with its build fingerprint/platform and owns atomic
publication. Flat formats 1–3 remain readable; no runtime conversion or upgrade UI is involved. The stamp is diagnostics, not a gate: decoding returns the recorded
build, and `SaveBundle::restore` accepts a save from any build or platform whose state verification
reproduces its recorded count, hash and summary. When verification fails, the error is
`OtherVersionDiverged`; when such a save no longer decodes, it is `OtherVersionUnreadable`. Either
way the game can say that this version doesn't reproduce the save, rather than call it corrupt.
A per-slot OS lock protects revision checks. Immutable monthly files are written and synchronised
before atomically replacing the compressed manifest. A new directory is staged and renamed into
place only when complete. Old referenced files remain until a durable receipt; obsolete versions
are cleaned afterwards. File and directory synchronisation precede a success receipt. The directories synced are the save root
and the parent of each directory the store created, until that parent's sync succeeds; on
Windows no directory is synced (NTFS journals the rename; see `sync_directory`).
Post-publication sync failure returns the new revision as `DurabilityUnconfirmed`, not a claim of
rollback. Directory deletion first renames the snapshot out of the library, synchronises its
absence, then removes the hidden directory; uncertain removal remains retryable. `SaveStore::rename` changes display metadata through that same publication path, keeping
its recorded build/platform stamp, saved-at time and checkpoint. It checks the selected revision
again under the OS lock. The game refreshes the list after completion; renaming an active autosave
also updates its expected revision/name for later month closes, without saving the live session.

Save listing has its own metadata-only reader. It verifies the envelope and card identity/version
fields, skips command/snapshot bodies without constructing them, and reads only the archived region
ID/name key without rasterizing terrain. A bounded structural projection accelerates canonical
writer output; noncanonical RON uses the normal metadata deserializer. The selected-save path
always reads fresh bytes and performs full decode, archived-region mapping and state verification.
An in-memory per-store cache retains cards/errors by file size, modification/creation time and,
on Unix, device/inode/change time. Directory scans discover added/deleted slots and evict removed
entries; changed files refresh individually. Directory stamps include the manifest and monthly
file identities/timestamps, so changed or deleted month files invalidate the card too. Reads raced by replacement and transient I/O errors
are not cached. A bounded, versioned, checksummed `.save-library-cache.ron` persists lightweight
rows and stable inspection errors atomically. It is a disposable hint tied to the inspecting build
and platform: missing, damaged, incompatible or unwritable indexes never block listing or loading.
Every restart scans the directory and stats each source before reusing a row; the index cannot
resurrect deleted files. The store survives menu transitions, so opening Load game reuses the
opening menu's index; cached cards paint during refresh. Player saves and backups are never rewritten.

The game's persistence coordinator freezes producers, drains admitted commands and captures manual
saves after application. Autosaves capture inside the funnel at the individual `MonthClosed`
command, before later queued commands. Successful automatic writes run in the background without
a modal or a simulation freeze: Review, its retrospective and Continue remain usable. Immutable
captures are serialized by one worker; queued writes resolve their expected revisions only after
the preceding receipt. A late success never pauses later play or closes a manual dialog. A failed
autosave opens the retry/continue-without-saving barrier and retains its warning. Cancelling a
manual dialog does not cancel a background save. Remaining commands are retained in an owned queue with a
session generation. Workers carry tokens/cancellation; only verified, current results can install a
candidate or fulfil Save-and-menu. Resume stays paused until phase-entry hooks have settled. A
resumed branch gets a fresh run ID, diagnostic log and automatic slot.

`domain::state` provides pure, explicit mementos without serde or I/O. Format-4 snapshot directories
store schema-1 session/projection snapshots and a lossless tick-RLE audit journal in a compressed
text manifest, with full chart history moved to independently compressed monthly binary files.
Application-owned pure chart decomposition/reassembly restores identical canonical projection bytes
before verification, including the live buffer and forecast. Every reference binds a local filename,
month, compressed/expanded size and SHA-256; paths, duplicates, symlinks and size overruns are rejected.
No samples are discarded and no image data is stored. Flat save codec 3 remains the manifest's
internal state representation; canonical bytes replace Debug as the
persisted boundary hash. State schema versions are independent of rule versions. New rules act on
future commands; data changes require defaults/upcasters and retained golden-save CI coverage.
Historical recovery has completed and its engine, converter and fixtures are archived rather than
linked into the shipped game. Portable snapshots retain source slot/revision/rules/hash/count as
provenance. This metadata protects original slots and suppresses duplicate library rows only for
exact converted boundaries; reading or resuming never writes an upgraded copy. Future schema
upcasters remain part of normal snapshot loading, backed by retained golden-save CI coverage.
Snapshot-rooted command recordings retain a verified directory sidecar with the same monthly layout and use the explicit format-2 replay
API, so old command-only replay cannot accidentally ignore a migrated base. Opening-menu preload
also prepares snapshot compression and encoded command prefixes in memory. `PendingRecording` owns
the verified game and its captured boundary, exposing no mutable game until activation; prepared
infrastructure bytes must match that exact checkpoint. Explicit Resume rechecks the source revision,
creates fresh recording files and writes the prepared prefix in batches. Speculation creates no
recordings, saves or live session; cancellation/other menu choices discard all prepared memory.

`PreferencesPlugin` retains window/layout/zoom/speed, visual effects and the supply-chart layout
independently in `preferences.ron`. The stored-energy chart can use a separate pane at half the
supply pane's width, or overlay an independently labelled MWh scale. `UiState` owns this choice
across live, History and retrospective views; `dispatch` handles its UI action without issuing a
simulation command. Session resets retain chart layout and effects while resetting other UI state;
legacy preference files default to separate charts and enabled effects. It observes
actual native/egui geometry, writes changed snapshots on a worker and flushes the latest one on
normal exit. No preference is a simulation input. Headless tests inject paths and can omit the
plugin; there is no ambient working-directory write from `CorePlugins`.

## Determinism rules

The same verified versions, settings, region content, seed and command log must produce an identical
state hash. Bit-identical replay is guaranteed **for the same build on the same platform**. Loading
a portable format-2/3 save restores canonical state bytes instead of replaying its past. Format-1 saves need
a matching historical engine and verify the old session `Debug` hash before migration; a Debug
change can fail that old verification even when no outcome changed. New saves hash stable,
versioned state and projection encodings independently of Rust's Debug implementation. A few
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

1. At each hour boundary after midnight (tick 60, 120, ...), storage plans are recomputed from
   actual inventory before hydro is allocated against residual demand. Storage also replans on
   the next tick after enable changes or restoration. Elapsed allocations stay frozen. Hydro is
   re-planned from remaining water (`plan_hydro`; `BeginOperating` plans from hour 0).
   Then, in Normal mode, the rotating outage is re-sized for the hour starting (`size_outage`): its
   target is the exact forecast shortfall share at the hour's worse end (minute 0 or 59) against
   what the fleet can deliver by then, 0 when the fleet covers both ends. With active batteries,
   evaluate every remaining minute chronologically so depletion inside the hour is visible;
   operating forecasts spend only existing inventory. During a blackout the
   target ordered before it stays frozen. `BeginOperating` sizes hour 0 after a first boundary
   dispatch for the full forecast and starts the day with the outage at that target. Planning and
   sizing draw no randomness.
2. Demand of connected cities: forecast × (1 + forecast error). The AR(1) error advances by exactly
   one normal draw every tick, in any state.
3. Wind advances once per minute after the demand draw (minute 0 uses its initial day sample,
   drawn after solar clearness). The bounded regional AR(1) factor is shared across all farms;
   output is nameplate × authored site potential × factor, including at night. Solar and hydro
   availability retain their resource rules. Standing renewable curtailment is withheld from
   connected online Solar/Wind in proportion to available MW. Queries never draw weather.
4. The commitment actions due at this tick; plants suppressed for the day get none.
5. Every plant steps once: start-up progress, ramp toward target, hydro budget use. If a connected
   dispatchable plant came online in Normal mode, the current hour's outage is re-sized: a tick
   event, not a command.
6. The tick's **load** is `demand × (1 − outage)`; AGC, the imbalance, the frequency model's system
   size and protection all read it where they read demand before. In Normal mode the outage then
   moves toward its target, before AGC runs, no faster than the AGC fleet can follow: a cut by at
   most one `shed_step` and the fleet's net one-tick ramp-down room, a restore by at most
   `max(shed_step, surplus / demand)` and its net one-tick headroom. The surplus term applies only
   while no plant is in a ramp-down AGC cannot reverse (the trip gate's test). A tick emits at most
   one `RotatingOutageOrdered`, with the final target.
7. AGC re-targets connected auto-follow plants in merit order toward the load to serve,
   `load × (1 − shed)` with the moved outage (in a blackout, pre-positioned for the next restoration
   step), making room for withheld solar. Scheduled hydro is capped at its hourly allocation for the
   tick the targets take effect (`tick + 1`), with a ramp envelope ahead of a falling hour. The cap
   is lifted while any load is shed or the frequency band is `Shedding`; a rotating outage alone
   never lifts it. When the lift ends, a recovery phase lets hydro above its ceiling ramp down only
   as fast as the other AGC plants' one-tick headroom (net of the shortfall) can follow, until it is
   at or below every ceiling it meets before the next re-plan.
8. Stage storage flows from actual generation after full customer demand: charging is eligible
   from 00:00 until the authored 18:00 cutoff; gap support remains eligible 17:00–24:00. During
   the overlap, primary surplus chooses charge and deficit chooses discharge, with one inventory
   and no simultaneous/cross-battery flows. Cancel charging before shedding or below the normal
   band. AGC targets full served customer load plus feasible charging, never subtracting battery
   output: storage bridges generator gaps. Requests take effect next tick, wait for generation
   and taper before full inventory/18:00 to absorb thermal ramp-down tails.
   Imbalance is connected supply (including storage discharge) minus charging and served city load,
   `load × (1 − shed)`.
9. Frequency update, with the load as the system size. It is held at nominal during a blackout and
   on a dark grid (a Normal-mode tick whose load is `≤ 0`: the outage at 1, zero demand, or no
   connected city), where over-frequency tracking also resets, so protection never arms on a grid
   with nothing to measure.
10. Protection on the load. Under-frequency shedding is staged: stage `k` sits at
    `shed_below − k × shed_stage_spacing` and drops one `shed_step` the first tick deviation is
    strictly below it, several stages crossed in one tick fire together, and a tick below
    `shed_below` that crosses no new stage sheds one step. Stages re-arm when deviation is back at
    or above `−normal_band` and at `BlackoutRestored`. A blackout trips every plant and holds the
    outage at 0 with its target frozen; black-start restoration steps to
    `max(snapped(shed − step), target)`, each step needing supply, so it ends exactly at the frozen
    target, where Normal mode resumes with the outage at the target and the hour is re-sized.
    Protection also curtails Solar/Wind for the next tick and trips the most expensive online producer
    once frequency has stayed high for `trip_after_ticks`. A trip waits while there is no physical
    surplus, while the rest of the fleet could not cover the lost output within one tick, or while
    another plant is already losing output AGC cannot reverse, so protection does not trip the grid
    into a deficit. After a trip AGC re-targets again in the same tick, so the rest of the fleet
    responds on the next step. Tripped variable generators reconnect once restoration re-energises the grid.
11. Re-evaluate storage flows after protection and settle inventory once. The `TickSummary` of
    that post-protection state includes separate charging/discharge and per-battery flow records,
    and is emitted last as `TickCompleted`. Storage cannot black-start; blackout retains inventory
    and enabled state, with no flow until generating plants restore the grid.
12. Accounting: the session adds the summary to the month-to-date totals (energy per plant,
    recurring and boundary starts, served, unserved, overgenerated and rotating-outage energy, the
    largest outage share ordered, and the Normal-mode ticks outside the frequency-performance band).
    Hour 0's order at `BeginOperating` is recorded as tick 0's. Nothing is priced per tick.
13. The clock advances. After tick 1,440, capture each battery's unused inventory and clear it
    once, before construction, reporting or autosave. Daily input equals output plus both losses
    plus unused inventory. Then the month closes: totals are weighted by `days_in_month`
    and booked, construction advances, policies run (including the frequency-performance notice or
    strike), `MonthClosed` is emitted and the phase moves to Review. The hydro schedule view then
    shows the finished day with every hour frozen; `FinishReview` clears it for the next Planning.

Each `TickSummary` satisfies `generation = Σ by_plant = served + battery_charge + imbalance` (signed), with
`served = demand × (1 − outage) × (1 − shed)` and `unserved = demand − served`; without an outage,
`unserved = demand × shed`. The planned part, `demand × outage`, is part of `unserved`, never added
to it, and the month's `rotating_outage` is its energy. `curtailed` is solar availability withheld,
which is never generated, so it is not part of `generation`. Battery output is positive supply;
charging is separate load and earns no customer-service revenue. Monthly storage reports weight
input, output, losses and unused energy by days; ratings and live inventory remain unweighted.

Storage uses rules version 8 with existing save/log envelope version 1. Command-sourced restore
reconstructs enabled state, inventory, schedules and history from exact archived region data; it
does not initialise a new day. Converted rules-6/7 games are ordinary portable snapshots. Historical
recovery is archived outside the shipped game; diagnostic command-only logs need matching rules.

## Settings seams

The network is anchored at `start.home_city`, an existing connected starter city. Its ordinary city
node is the root; completed starter lines join it to the other starter cities and every starter
plant. Region validation rejects a missing or initially unconnected home city. The home city is
Columbus in the East and Seattle in the West. All additional connectivity follows completed paths
through ordinary city and plant nodes.

```rust
pub struct GameSettings { pub fidelity: SimulationFidelity }
pub struct SimulationFidelity { pub network: NetworkModel }
pub enum NetworkModel { CopperPlate }   // later: DcPowerFlow
```

`NetworkModel` is a one-variant enum rather than a `bool`, so adding a variant later forces every
`match` to handle it. Each region is a single balancing area (copper plate) in Milestone 1.

## Strings

`domain` and `application` contain no user-facing strings, only typed values and `name_key`
translation keys. Localisation happens in `game`, from the Fluent files in
`assets/locales/en-US/*.ftl`.

## The game crate

`crates/game` is the outermost adapter. It owns no rules: it reads query views and projections, and
it changes the game only by sending commands through the `Game` pipeline. It is a library (every
plugin, presenter and system, so the integration tests can reach them) and a thin `main.rs` that
parses the command line and opens the window.

### Plugins and states

`hud/theme.rs` owns the shared dark text palette and the 4.5:1 contrast floor. It runs before any
HUD panel, keeps secondary and disabled labels readable, and adjusts semantic text colours without
changing chart/marker colours. `hud/chart_style.rs` keeps chart ticks at full contrast and adds
opaque backings to bar hover labels; map text uses the same backing helper. Text fades are disabled,
egui's disabled fade (`disabled_alpha`) included, so a disabled control is drawn by
`action::add_enabled` with `theme::disabled_visuals`: buttons without a fill of their own and with
a thin outline, a selected toggle's fill dimmed halfway to the panel, slider rails and handles in
the faint fill, and muted text. Panels use that helper
(through `button` and `toggle_button`), never egui's `Ui::add_enabled`, `add_enabled_ui`,
`disable`, `UiBuilder::disabled` or the `enabled` of `Area`, `Window` and `CollapsingHeader`:
`clippy.toml` disallows them everywhere else. A choice that is current, like the map
layer shown, is a selected toggle, not a disabled button.

| Plugin | Role |
|---|---|
| `AppStatePlugin` | `AppState` (`Boot` → `MainMenu` → `RegionSelect` → `InGame`), the `PhaseState` sub-state and the boot checklist. |
| `LocalizationPlugin` | Loads the en-US Fluent files into the `Localizer` resource. |
| `SimulationPlugin` | The bridge: the `Session(Game)` resource, the region `Catalog`, new games, the command funnel, the phase mirror and the fixed-step driver. |
| `PreferencesPlugin` | Retained window geometry, zoom, running speed and panel/tool layout; separate from game checkpoints. |
| `DailyAudioPlugin` | Presentation-only daily time signals at 1×: rooster at sunrise, metro at 08:30/17:00, gong followed by children laughing at noon, and doorbell at 17:30. Clips finish naturally if the player changes speed. |
| `HudPlugin` | Every egui panel, toast, prompt and modal, plus its non-egui glue (listing regions, collecting toasts, resetting HUD state between games). |

`CorePlugins` groups the first three. They need no window or renderer, so the tests run them on
`MinimalPlugins` plus `StatesPlugin`. `GamePlugins` adds `EguiPlugin`, `HudPlugin` and `PreferencesPlugin`, and
`main.rs` puts it on `DefaultPlugins` with a 1280×640 minimum client area, targeting 1280×720
logical displays with room for desktop chrome, plus `WindowSizingPlugin`
(`window.rs`). Bevy 0.20 spawns the `Monitor` entities only once the winit event loop runs, so
that plugin waits for monitors and preference loading. It restores reachable saved geometry if
available; otherwise it sizes the window once to `opening_size` (80% of the
monitor's logical size, clamped between the minimum window and a reserved client area). Opening
and restored geometry may use the full monitor width and leave at least 40 logical pixels above
and below, budgeting for title bars and system bars. Monitor exposes full display bounds, so this
is a conservative allowance rather than an OS work-area query. Positions account for physical origins and scaling.
The sizing system then never runs again, so the player's own resize wins.

`GamePlugins` also installs `DailyAudioPlugin`. Bevy's audio backend and Vorbis decoder play
the small clips in `audio/`, read once at startup from `AssetRoot` alongside region and locale data.
The daily schedule follows the simulation clock, with sunrise taken from the same monthly
daylight table as the diorama sun. Morning and evening metro cues reuse one asset; the noon
clip combines the louder gong and laughter in sequence with no added gap. Time signals
observe every clock change, including silent fast-forward, so switching back to 1× cannot replay
missed cues. A session generation/month change establishes a fresh boundary without playing past
sounds on load. Audio state belongs only to the client and never changes checkpoints or RNG.
`assets/audio/README.md` records the schedule, sources, licenses and clip preparation; the original
metro/gong/doorbell synthesis is reproducible with `tools/audio/generate_cues.py`.

- **Boot** waits for every `require_for_boot::<R>()` resource: `AssetRoot` and `LogDir` (from the
  command line), `Localizer` and `Catalog`. A `BootError` keeps the app in `Boot`, which shows it.
- **`PhaseState`** mirrors the domain's `DayPhase` one way. `mirror_phase` runs after the commands
  are applied and is its only writer; the game never sets a phase itself. As a sub-state of
  `InGame`, its resource is absent outside a game. It changes at the next frame's state
  transition, a frame after the domain, so it drives the `OnEnter`/`OnExit` hooks while the egui
  panels gate on the domain's phase itself (`in_phase` in `hud/mod.rs`), as their presenters do:
  the frame a phase changes draws the new phase's panels, not none.
- **`Session`** is inserted by a successful `NewGameRequest` and removed on leaving `InGame`. Each
  new game records to its own log under `LogDir`, `<YYYYMMDD-HHMMSS>-<region>-<seed>.log`, beside
  the archived region source.

### Message flow

```
click ─▶ draw_<panel> ─▶ Vec<UiAction> ─▶ dispatch ─┬─▶ HUD resources (Selection, SimSpeed, ...)
                                                    └─▶ PlayerCommand ◀── drive_ticks (FixedUpdate)
                                                              │
                                                              ▼
                                           SimSet::Apply: Game::execute, in order
                                                │                         │
                                                ▼                         ▼
                                       DomainEventMessage           CommandFailed
                                                └────────────┬────────────┘
                                                             ▼
                                               collect_toasts ─▶ ToastQueue
```

- `dispatch` is the single mapping from a click to the rest of the game. It writes
  `PlayerCommand`, `NewGameRequest`, `ReturnToMenu` and `AppExit` messages, and the HUD's own
  resources.
- `apply_player_commands` (in `SimSet::Apply`, in `Update`) runs every queued command in the order
  sent. A success re-emits its events as `DomainEventMessage`s (except `TickCompleted`: the HUD
  reads ticks from the projections). A refusal becomes one `CommandFailed`, and the session is
  unchanged.
- After `SimSet::Apply`: `mirror_phase` copies the phase, `collect_toasts` turns refusals and
  toast-worthy events into toasts, and `settle_commit` clears or reverts the commitment draft that
  was waiting for the game's answer.
- A `CommandError` becomes text only in `collect_toasts`.

### The 3D presentation boundary

`WorldViewPlugin` owns session-scoped meshes/materials and ordered `ViewSync` maps for plant,
city and line IDs. After `SimSet::Apply`, a changed `Session` produces an immutable `WorldModel`
from public queries. New/load session generations and region Arc identity invalidate terrain,
even for the same region ID. Reconciliation adds/removes stable entities; construction completion
updates existing visuals. Leaving removes all entities and owned assets. Terrain layer changes
replace only the existing mesh's colour attribute. Common silhouettes and materials are reused.
The small 2D draw adapter remains for HUD-only consumers/tests; `GamePlugins` always installs the
diorama and uses its transparent interaction surface.

`CameraPlugin` creates an HDR 3D camera restricted to this pass's map rectangle, rounded inward
using egui's actual pixels per point. The full-window 2D camera clears its own texture to transparent
and explicitly alpha-composites the HUD; retaining its previous texture would cover the world with
the previous menu. World viewport/pose updates run after egui and before camera calculations and
transform propagation. The same pose and rounded rectangle project labels and generate picking rays.

In the render world, `viewport_render` allocates the diorama's HDR/depth intermediates at the
map's physical pixel size and rebases their origin to zero. A final one-to-one blit places the
result in the original window rectangle before the HUD; it adds no image asset or resolution
scaling. Bloom sampling uses the local rectangle, and depth-of-field aperture compensates for
the smaller buffer height to retain its blur radius in pixels. The final scissor intersects the
actual output attachment, protecting a frame with stale viewport metadata during a window shrink.
The main-world camera, projection, labels and picking keep their original window coordinates.
Review fills the area below the top bar with an opaque background and omits the map controls.
Its 3D camera is despawned, and the compositor drops its cached texture binding so unused view
targets can be reclaimed. Continue recreates the camera from the preserved orbit and effects
settings when Planning draws its map again. The opaque background also remains behind Save,
Menu and game-over dialogs.

Map controls use the normal presenter/action funnel. Selection, orbit, pan, zoom and toggles pass
through `UiAction`/`dispatch`; systems read those resources through `ActionSink`. Map gestures are
processed after all floating windows register their current layers. The response must own the
pointer, avoid the legend and have no modal/persistence/Review/game-over gate. A drag never becomes
a click. Rays test procedural block bounds and the exact terraced terrain surface using a
front-to-back grid traversal; a property test compares it against the rendered triangles. Ties
follow ordered IDs. Annotation backing rectangles flip/clamp and omit overlapping/occluded labels.

`OverlayPlugin` follows the domain phase, including the frame before Bevy's phase mirror catches up.
Each phase restores its default overlay; resource selection in Operating can override the hologram
until Control room is selected. The supply overlay preserves terrain colours, directional lighting
and building shadows; warm facade windows, the selection marker and bright cyan flow dots are
unlit. Plant output gauges are omitted. City footprints stay fixed while their heights scale
linearly with base demand in Planning and live demand in Operating, using the same regional
base-load reference throughout the day.
Disconnected cities retain forecast demand height; shortages affect lighting, never height.
Rendering, picking and labels share the vertical scale. Presentation
time animates energisation pulses and illustrative source-to-city routes. Four interleaved groups
of city windows go dark as the common served-demand fraction falls; a partially supplied group
dims proportionally. Walls retain their directional lighting and never glow as a whole.
The pure routing presenter uses current output/served demand, shortest paths over in-service
lines with line-ID tie breaks, and net opposing transfers. Pulse entities share one mesh/material,
remain spherical and animate one stream per visible stretch. Collinear overlaps are split at
their endpoints, their signed transfers are summed in a common direction, and zero net transfer
hides the stream. The lowest-ID covering line supplies the clipped terraced geometry; crossing
lines retain their separate routes. Dots follow the deep-blue line geometry and freeze while
paused or persistence is frozen. Routes are labelled illustrative; copper-plate supply remains
one common fraction, without spatial outage rotation or electrical per-line flow/loading.
Bloom/DOF affect only the world camera and can be disabled.

The optional `DevToolsPlugin` (`dev`) uses bevy-inspector-egui 0.38 with the pinned Bevy/egui stack.
F12 opens a filtered inspector over `WorldEntity` and reflected `TuningConfig` (visual scales
and illumination). It exposes no aggregate, command handler or simulation tuning.

### Fixed-step pacing

`FixedUpdate` runs at 4 Hz (`FIXED_STEP_HZ`). Each step, `drive_ticks` sends one
`AdvanceTicks { n: speed }`, where `SimSpeed` is `Paused` (0), `X1`, `X2`, `X4` or `X8` ticks.

- It sends only while the domain's phase is `Operating`, the game is still `Active`, the speed
  isn't paused and the previous batch has been applied (`TicksInFlight`). So a slow frame that
  catches up several fixed steps still sends one batch, and a finished game is never asked for
  more.
- At 1× a 1,440-tick day takes 6 real minutes; at 8×, 45 seconds.
- Small batches keep the projections timely (ruling R22 judges ramp warnings by a batch's end
  state).
- The speed uses the remembered running preference (initially `X1`) on entering Operating and to `Paused` on leaving it, so the month close
  (which ends the batch it falls in) leaves the day paused in Review.

### The presenter pattern

Every panel is split so that its decisions are testable without a window. Each lives in
`crates/game/src/hud/<panel>.rs`, in this order:

1. **A model** (`<Panel>Model`): plain data only, with no egui types. Strings are already localised
   and formatted. Buttons are `ButtonModel`s carrying the `UiAction` they send and whether they are
   enabled.
2. **A presenter** (`<panel>_model(&Game, &Localizer, ..)`): a pure function, with no `Res`, no
   clock and no I/O (the caller passes `now` in). All decisions live here: what a button sends,
   whether it is enabled, what a warning says. State that is hard to reach with a real `Game` (game
   over, say) gets a pure helper over the view values, and the `&Game` wrapper stays thin.
3. **A draw function** (`draw_<panel>(ui, &model) -> Vec<UiAction>`): it only lays out, and returns
   what was clicked. Editable widget state (a text field, a slider) is a `Resource` passed as
   `&mut <Panel>Form`. It doesn't choose its own container.
4. **A system**: it builds the model, opens the container, draws, and passes the actions to
   `dispatch` through its `ActionSink`. It writes nothing anywhere else, and it draws one panel.

Single-line fields submit their associated primary action on Enter, using `action::submitted`
and the same action/validation as the adjacent button. Refresh pure form presenters with the text
edited in the current pass, so typing and Enter together cannot submit a stale value. Respect
busy/disabled states and emit the action once. Save/Rename visually mark that primary button;
destructive deletion keeps Cancel as its default. Seed entry selects no region implicitly.
Escape invokes the active dialog's enabled Cancel/Close action through `action::cancelled`.
Check before drawing text fields so they cannot swallow cancellation by surrendering focus.
An open popup gets Escape first, and dialogs without an enabled cancellation remain open.

Confirmations go through one `OpenPrompt` resource and one `prompt_overlay`: a button sends
`UiAction::Prompt(Some(..))`, and a new prompt is a `Prompt` variant plus its case in
`prompt_model`. A prompt that belongs to one phase, like the shortfall confirmation on *Start the
day*, also gets its case in `prompt_in_phase`, which closes it in the frame the phase changes. A
new HUD mode, like the map layer or the legend's `LegendChoice`, is a `UiAction` variant and a
resource that `dispatch` writes.

**Read through the sink.** `ActionSink` holds `ResMut` of every resource `dispatch` writes. A panel
system must not also take `Res` of one of them: Bevy rejects the conflict (error B0002) when the
egui schedule is first initialised, which is the first frame of a real window. Read them with the
sink's getters (`sink.selection()`, `sink.layer()`, `sink.open_prompt()`, ...), adding one for a new
resource.

### HUD layout

The HUD's systems run in bevy_egui's `EguiPrimaryContextPass`, in chained sets: `HudSet::Begin` →
`Bars` → `Panels` → `Overlays`. egui 0.36 lays panels out inside a `Ui`, not a `Context`, so each
panel system makes its own root `Ui`, and the `HudArea` resource keeps them from overlapping:

- `Begin` resets `HudArea` to the whole screen.
- An edge panel takes `area.root_ui(ctx, id)`, shows its `egui::Panel`, then calls
  `area.claim(&ui)` to record what is left, so the next panel starts inside it.
- The claiming order: the top bar, then (Operating) the speed bar, then the phase's bottom panel
  across the full width, then its side panels (the inspector and build menu in Planning, the
  frequency gauge and plants in Operating), and last the map, a central panel that fills the rest
  and claims nothing.
- These outer panels are resizable (spec §16.7): `HudArea::show_panel` shows an `OuterPanel`
  that opens at its fixed size, which is also its minimum, and grows by a drag of its inner edge
  up to `panel_max`: 40% of the screen's width for a side panel, half its height for a bottom one,
  and never so far that the map's area drops below `MAP_MIN_SIZE` (320×200). The inspector, shown
  before the build menu, reserves the build menu's current width. The Operating bottom panel
  reserves what the side panel's minimum height (`operating_side_min_height`: the gauge, the
  details block and three list rows, from the style) needs beyond the map's own 200. That
  minimum relies on the side panel's selected-object block having one fixed height
  (`plant_details_height`), so every object it can show (a plant, a city, a plant under
  construction) must fit that height. egui keeps each size in its memory for the run. The
  content is laid out over exactly the panel's space, so content can neither grow the panel nor
  shrink it back after a drag: only a drag moves the map.
- Menus are whole-screen central panels. Overlays (windows, toasts, prompts, the Review and
  game-over modals) take the `Context` directly and ignore `HudArea`, except the Review backdrop,
  which reads `free_rect` to leave the top bar uncovered.

### Bevy 0.20 and bevy_egui 0.43.1 notes

Checked against the versions in use (Task 1 of Plan 2, and the lessons since):

- **Messages:** `#[derive(Message)]`, `App::add_message::<M>()`, `MessageWriter::write` and
  `MessageReader::read`.
- **States:** `#[derive(States)]`, and `#[derive(SubStates)]` with
  `#[source(AppState = AppState::InGame)]`, registered with `init_state` and `add_sub_state`. A
  sub-state's `State<S>` resource is absent outside its source state.
- **`StatesPlugin`** (`bevy::state::app::StatesPlugin`) is in `DefaultPlugins` but not in
  `MinimalPlugins`, so headless apps add it.
- **`EguiPlugin::default()`**: there is no `enable_multipass_for_primary_context` any more. UI
  systems take `EguiContexts`; `contexts.ctx_mut()?` returns a `Result`, so they return `Result`.
- **The primary egui context** is explicit on the library's full-window `Camera2d`. Automatic
  context creation is disabled. The separate `Camera3d` has no egui context. A window with no
  primary camera shows no HUD.
- **Init the `EguiPrimaryContextPass` schedule.** If no system was ever added to it, the schedule
  doesn't exist; bevy_egui's `try_run_schedule` fails and drops the egui output, and epaint panics
  on the first frame ("Dropped TexturesDelta with 1 unapplied deltas"). `HudPlugin` calls
  `app.init_schedule(EguiPrimaryContextPass)` so this holds even if every system becomes
  conditional.
- **Features:** Bevy is built with `default-features = false` and an explicit list (commented in
  `crates/game/Cargo.toml`). In 0.20 each `bevy_input` device (`keyboard`, `mouse`) is opt-in.
  bevy_egui's default `picking` feature expects `PickingPlugin`, so `bevy_picking` is on; it adds
  no crates, since bevy_egui compiles them anyway.
- **One egui:** egui 0.36.2 is the only `egui` in `Cargo.lock` (egui_plot 0.37 requires
  `^0.36`). Check with `cargo tree -d -p game --depth 0 | grep egui`, which should print nothing.
- `fn main() -> AppExit` works, and `App::update()` works without `finish()`.


Personal flat-save conversion was performed once with local ignored tooling. Original libraries
are preserved outside the repository; `/tools/archive/` is ignored and contains no shipped code.
No converter, archived recovery source, staged library or backup is added to Git. New games and
monthly/manual saves use the directory store; metadata rename never saves a different live boundary.

## Wind and snapshot evolution (Plan 5, rules 14)

`generation::wind` owns the pure resource transition; `grid::power_system::wind` owns shared
central/indicative forecasts. Planning uses the nominal monthly factor; Operating conditions the
forecast on the latest observation without looking at future RNG. Hydro, battery requests,
shortfall/ramp checks and charts use the same Solar + Wind net load. Wind/storage fleets assess
all minutes; shared weather uncertainty is not reduced as farms are added. Wind is excluded
from thermal commands, commitment, start-up billing and AGC. The 3D model is presentation only.

Portable schema 2 appends regional wind state, battery charge-end ratings and per-kind curtailed
energy. `StateReader` and schema-aware collection/record codecs retain schema-1 field order and
canonical encoding. Restore verifies the source hash, journal, canonical bytes and invariants
before applying in-memory defaults; original files, RNG, inventory, reports and old history remain
intact. New wind fields are neutral and future plans are marked stale separately from legacy
fields. New captures write schema 2; monthly chart split/join carries the source schema until
verification. Directory 4, flat envelope 3 and diagnostic log 2 remain independent and unchanged.
