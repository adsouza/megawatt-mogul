# AGENTS.md

## Project

Megawatt Mogul is a Rust grid-balancing simulation game: you run a power company and keep supply
equal to demand. Clean Architecture + DDD; see ARCHITECTURE.md, and the spec in `docs/superpowers/specs/`.

## Commands

- Build: `cargo build --workspace`
- Test: `cargo nextest run --workspace` (add `-p domain` for fast feedback; `-p game` for the client)
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all` (CI runs `cargo fmt --all --check`)
- Mutation testing: `cargo mutants -p domain -p application` (slow; on demand)
- Snapshots: `cargo insta review`
- Run the game: `cargo run -p game` (`--assets <dir>`, `--log-dir <dir>`)
- Launch smoke: `cargo run -p game -- --exit-after-frames 120` opens the window, draws 120 frames
  and exits 0. Tests can't open a window, so run it after changing plugins or system parameters.
- Hooks: `git config core.hooksPath .githooks`. pre-commit checks formatting; pre-push always runs
  clippy and runs tests when outgoing changes affect Rust, Cargo metadata, toolchain/build config,
  crate files or assets. New refs or unknown comparison ranges also run tests. Check hook selection
  with `python3 .githooks/tests/test_pre_push.py`. Don't bypass hooks with `--no-verify`.

## Architecture rules

- Dependency rule: `game` -> `application` + `infrastructure` + `domain`, `application` -> `domain`,
  and `infrastructure` -> `application` + `domain`.
  `domain` may depend only on `rand_chacha` and `thiserror`: no Bevy, serde, I/O or tracing.
- No user-facing strings in `domain` or `application`: use typed values and `name_key` translation keys.
- Plant tuning numbers live only in region RON data and the `test-fixtures` fixture, never as constants.
- Determinism: `BTreeMap`/`BTreeSet` only, no wall-clock time, all randomness via `SimRng`,
  single-threaded tick, tie-break by ID.
- Money is `i64` cents. Nothing is priced per tick; price once per ledger entry at month close.
- The domain never panics on player input; it returns `DomainError`. A command that returns `Err`
  leaves the session unchanged. Panics are only for invariant violations (bugs).

## Game crate rules

- The `game` library holds every plugin and system; `main.rs` only parses the command line and
  opens the window.
- HUD panels follow the presenter pattern (ARCHITECTURE.md, "The game crate"): a pure
  `<panel>_model` presenter, a draw function returning `Vec<UiAction>`, and a system that hands
  those to `dispatch`, the only place a click becomes a message or a resource write.
- A panel system reads what `dispatch` writes through `ActionSink`'s getters, never with a second
  `Res` of the same resource (Bevy panics with B0002 on the first windowed frame).
- The game changes the simulation only by sending `PlayerCommand`s; `PhaseState` only mirrors the
  domain's phase.
- New player-facing text goes in `assets/locales/en-US/*.ftl`; a new literal key also goes in
  `crates/game/src/l10n/keys.rs`, which the coverage test (`tests/game/l10n_coverage.rs`) checks.
- Single-line form fields submit their associated primary action on Enter. Validate the current
  text in that same pass, respect disabled/busy states, and emit the action only once. Destructive
  confirmations default to Cancel. Cover keyboard submission when adding or changing forms.
- Escape invokes a dialog's enabled Cancel/Close action, even from a text field. Only the active
  dialog handles it; an open popup gets Escape first. Keep non-dismissible/busy dialogs protected.
- One test binary: game integration tests are modules of `crates/game/tests/game.rs` under
  `tests/game/`, never a new file in `tests/` (each Bevy-linked binary costs hundreds of MB and
  a link step). See TESTING.md for the layers.

## Workflow

- TDD: failing test, minimal code, pass, refactor.
- Specs live in `docs/superpowers/specs/` and plans in `docs/superpowers/plans/`.
- Conventional commits: `feat:`, `fix:`, `test:`, `docs:`, `chore:`.
- Keep `cargo nextest`, clippy (`-D warnings`) and `fmt --check` green before every commit.

## Testing rules

- Isolation: each I/O test uses its own `tempfile::TempDir`. No fixed paths, environment variables,
  current-directory changes or other process-global mutable state.
- Invariants get property tests (`proptest`), not only examples. See TESTING.md.

## Don'ts

- Don't iterate a `HashMap` in the domain.
- Don't `unwrap`/`expect` on player input.
- Don't use Bevy or egui outside the `game` crate.
- Don't add a dependency to `domain` without updating the dependency rule and its test.
