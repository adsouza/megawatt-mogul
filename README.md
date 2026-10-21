# Megawatt Mogul

Megawatt Mogul is a single-player 3D simulation. You run a power company that connects
electricity suppliers with consumers and grows it, while keeping the grid balanced, customers
happy and costs down. Supply has to match demand at every moment. Cheap baseload plants ramp
slowly, while fast-ramping plants are expensive, so you learn to cover predictable demand with
cheap, slow plants committed ahead of time and to patch the margins with expensive fast ones. This
is the **merit order**, as real grid operators use it.

## Goals

Three goals carry equal weight. A trade-off between them is a design problem to solve, not a choice
to make quietly.

1. **Fun and polish.** It feels like a real game, not a tech demo.
2. **Architectural elegance.** Clean Architecture and Domain-Driven Design, applied pragmatically.
3. **Teach how real grids work.** Realistic mechanics teach through play, not text dumps.

## Regions

You choose a region when you start a new game. Milestone 1 ships two, each modelled as a single
balancing area:

- **Eastern Interconnection (North America):** the US east of the Rockies plus central and eastern
  Canada, excluding Quebec and ERCOT.
- **Western Interconnection (North America):** the western US, British Columbia, Alberta and Baja
  California.

## Status

Plan 1 (headless simulation core: the `domain`, `application` and `infrastructure` crates) and
Plan 2 (the Bevy client, `crates/game`) are complete, and the game is playable. From the main menu
you pick a region and a seed, then play month after month: plan the day's commitments on a 2D map,
operate the grid through the day at up to 8×, and read the month's review. Plan 2b adds manual
save/resume, monthly autosaves and retained window/UI preferences. Plan 3 adds the 3D
diorama. The [playtest checklist](docs/superpowers/plans/2026-10-19-plan-2-playtest.md) walks
through every screen.

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

## Saving and resuming

Use **Save game** in any phase, including the final review. Give a new slot a name or confirm an
existing manual slot to overwrite it. Saving pauses play and includes applied changes; schedule
edits still in the form are excluded. A separate autosave preserves each month-close review.
Every new game and resumed branch has its own autosave, so loading never overwrites its source.

Choose **Load game** on the main menu. A restored game always opens paused; use a speed button or
Space to continue at the remembered speed. **Menu** offers Save and return, Leave without saving,
or Cancel. Failed saves keep the game open and offer Retry. The OS close button still quits
immediately without saving game progress; client preferences are flushed on normal exit.

Window size/position, UI zoom, running speed, sidebar/bottom-panel sizes, the Incidents divider,
and the movable Milestones window's position are retained automatically, separately from save
slots. Restored geometry is constrained to the current monitor/layout. Missing or malformed
preferences use defaults. Preference write errors are recorded in the application log.

Each `.mgsave` is self-contained and includes the original region and complete accepted command
history. A diagnostic log failure does not disable saving. Loads verify checksums, rules, the
simulation build, platform and reconstructed state. Keep the matching executable for older saves:
backend/toolchain/lockfile changes can make saves incompatible; automatic migrations and
cross-platform replay are not included. Rendering-only source edits preserve compatibility.
Files are limited to 64 MiB and one million commands; exceeding a limit fails without truncating
history. Corrupt or incompatible slots remain visible with their reason.

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
