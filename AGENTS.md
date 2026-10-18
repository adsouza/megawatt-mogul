# AGENTS.md

## Project

Megawatt Mogul is a Rust grid-balancing simulation game: you run a power company and keep supply
equal to demand. Clean Architecture + DDD; see ARCHITECTURE.md, and the spec in `docs/superpowers/specs/`.

## Commands

- Build: `cargo build --workspace`
- Test: `cargo nextest run --workspace` (add `-p domain` for fast feedback)
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all` (CI runs `cargo fmt --all --check`)
- Mutation testing: `cargo mutants -p domain -p application` (slow; on demand)
- Snapshots: `cargo insta review`

## Architecture rules

- Dependency rule: `game` -> `application` -> `domain`, and `infrastructure` -> `application` + `domain`.
  `domain` may depend only on `rand_chacha` and `thiserror`: no Bevy, serde, I/O or tracing.
- No user-facing strings in `domain` or `application`: use typed values and `name_key` translation keys.
- Plant tuning numbers live only in region RON data and the `test-fixtures` fixture, never as constants.
- Determinism: `BTreeMap`/`BTreeSet` only, no wall-clock time, all randomness via `SimRng`,
  single-threaded tick, tie-break by ID.
- Money is `i64` cents. Nothing is priced per tick; price once per ledger entry at month close.
- The domain never panics on player input; it returns `DomainError`. A command that returns `Err`
  leaves the session unchanged. Panics are only for invariant violations (bugs).

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
- Don't use Bevy outside the `game` crate.
- Don't add a dependency to `domain` without updating the dependency rule and its test.
