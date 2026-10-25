# Megawatt Mogul

Megawatt Mogul is a single-player 3D simulation. You run a power company that connects
electricity suppliers with consumers and grows it, while keeping the grid balanced, customers
happy and costs down. Supply has to match demand at every moment. Cheap baseload plants ramp
slowly, while fast-ramping plants are expensive, so you learn to cover predictable demand with
cheap, slow plants committed ahead of time and to patch the margins with expensive fast ones. This
is the **merit order**, as real grid operators use it.

## Goals

These goals carry equal weight:

1. **Teach how real grids work.** Realistic mechanics teach through play, not text dumps.
2. **Fun to play.** It should feel like a real game instead of a tech demo.
3. **Architectural elegance.** Clean Architecture & Domain-Driven Design, applied pragmatically.


## Regions

You choose a region when you start a new game. Milestone 1 ships two, each modelled as a single
balancing area:

- **Eastern Interconnection (North America):** the US east of the Rockies plus central and eastern
  Canada, excluding Quebec and ERCOT.
- **Western Interconnection (North America):** the western US, British Columbia, Alberta and Baja
  California.

## Status

Plans 1, 2, 2b, 3, 4 and 5 are implemented: the simulation core, playable client, save/resume,
interactive 3D tabletop, daily-cycle battery storage and onshore wind farms. Pick a region and
seed, plan the day's commitments, operate the grid at
up to 8× and read the month's review. The diorama shows terrain, resource layers, cities, plants
and transmission construction. The [playtest checklist](docs/superpowers/plans/2026-10-19-plan-2-playtest.md)
covers the original shell; [Plan 3](docs/superpowers/plans/2026-10-20-plan-3-diorama.md) records the 3D checks.
[Plan 4](docs/superpowers/plans/2026-10-23-plan-4-battery-storage.md) records battery storage;
[Plan 5](docs/superpowers/plans/2026-10-23-plan-5-wind-farms.md) adds wind farms and extends battery
charging to 00:00–18:00.

## Run

```sh
cargo run -p game                                  # assets from ./assets or the workspace
cargo run -p game -- --assets <dir> --log-dir <dir> --save-dir <dir>
```

- `--assets` is the directory holding `regions/` and `locales/`. Without it the game uses the
  first of `./assets`, `assets/` next to the executable, and the workspace's `assets/`.
- `--log-dir` is where command logs are written (default `./logs`, which git ignores). Each game
  records its own log there, named for the time, region and seed.
- `--save-dir` holds game saves and `preferences.ron` (default `./saves`). Storage errors do not
  prevent a new game from starting.
- `--exit-after-frames <n>` (for developers) closes the game after `n` frames: a quick check that
  the window opens and the menu draws.
- On Linux, install Bevy's build dependencies first; see
  [Bevy's Linux dependencies](https://github.com/bevyengine/bevy/blob/v0.20.0/docs/linux_dependencies.md)
  or the list in `.github/workflows/ci.yml`. The first build compiles Bevy and takes a few minutes.

## Diorama controls

Click a plant, city or terrain cell to inspect it. Drag with the left button to orbit; right or
middle drag pans; scrolling zooms. **Reset view** or **R** fits the region again. **O** hides or
restores the overlay. Keyboard shortcuts work while pointing at the map and leave text entry and
Space-to-pause alone. Panels, legends and dialogs capture their own input.

Planning opens with terrain/resource colours. Operating keeps the lit terrain and shaded buildings,
with pillars showing each plant's output as a fraction of its capacity. Connected city lights share
the same
served proportion; partial shedding makes them dim/flicker together. Lines pulse only when
energised: they show no direction, congestion or MW flow. Choosing a resource layer temporarily
returns to resource colours; **Control room** restores the supply indicators. Each phase starts
with its default overlay. **Legend** opens the compact key; it starts collapsed so it cannot hide objects.
Narrow maps use a compact layer dropdown. Build transmission lines through the existing endpoint menu.

**Effects off** disables bloom and depth of field while preserving the world and input. Bevy skips
depth of field on platforms without supported depth textures. Labels and HUD render in a separate
unblurred pass, with opaque text backing. Labels flip/clamp at edges and skip occluded objects.

For development, run `cargo run -p game --features dev` and press **F12** to open the diorama
inspector. Only render entities and reflected visual tuning are exposed. Simulation tuning stays
in region RON and applies to a new game; the inspector cannot edit the session or simulation rules.

## Expansion pacing

Cities and starting fleets are scaled for growth in a few plant-sized steps. Peakers and solar take
2 played months to build, combined-cycle gas 4, coal 8, hydro 12 and nuclear 18. Transmission builds
at 4 cells per month. Build quotes show approximate monthly payments alongside total capital cost;
construction is paid at month close, not upfront. First-city acceptance tests cover generation and
lines together, a full year of seasonal demand and a retained cash buffer.

This balance applies to new games. Existing saves retain their original region tuning and build times.

## Saving and resuming

Use **Save game** in any phase, including the final review. Give a new slot a name or confirm an
existing manual slot to overwrite it. Saving pauses play and includes applied changes; schedule
edits still in the form are excluded. A separate autosave preserves each month-close review.
Every new game and resumed branch has its own autosave, so loading never overwrites its source.

Choose **Load game** on the main menu. A restored game always opens paused; use a speed button or
Space to continue at the remembered speed. Choose **Rename** beside a save in either save list to
change its name without loading it or changing its progress or saved-at date.

**Menu** offers Save and return, Leave without saving, or Cancel. Failed saves keep the game open and offer Retry. The OS close button still quits
immediately without saving game progress; client preferences are flushed on normal exit.

Window size/position, UI zoom, running speed, sidebar/bottom-panel sizes, the Incidents divider,
and the movable Milestones window's position are retained automatically, separately from save
slots. Restored geometry is constrained to the current monitor/layout. Missing or malformed
preferences use defaults. Preference write errors are recorded in the application log.

Each `.mgsave` is self-contained and includes the original region and complete accepted command
history. A diagnostic log failure does not disable saving. Loads verify checksums and the rules
version, then replay the history and check the reconstructed state. A save loads on any build or
platform whose replay reproduces it. Otherwise loading fails safely, with an explanation, and the
save file is unchanged; there are no automatic migrations. Because the state check hashes the
session's debug representation, a code change that alters it, such as a new session field, can
fail older saves even when no outcome changed. Files are limited to 64 MiB and one million
commands; exceeding a limit fails without truncating history. Corrupt or incompatible slots remain
visible with their reason.

## Build and test

Requires Rust 1.98 and [`cargo-nextest`](https://nexte.st) (`cargo install --locked cargo-nextest`).

```sh
cargo nextest run --workspace                 # run every test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo mutants -p domain -p application -j 4  # mutation testing (slow; weekly in CI, see TESTING.md)
```

Enable the git hooks once per clone:

```sh
git config core.hooksPath .githooks
```

- **pre-commit** runs `cargo fmt --all --check` (instant).
- **pre-push** runs clippy and the full test suite (about 25 s on a warm build).
- Both match CI. `--no-verify` skips them in an emergency.

See [TESTING.md](TESTING.md) for the test layers and [ARCHITECTURE.md](ARCHITECTURE.md) for how the
code is organised. Contributors and coding agents should read [AGENTS.md](AGENTS.md).

## Design

The full design is in
[`docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`](docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md).

## Licence

AGPL-3.0. See [LICENSE](LICENSE).
