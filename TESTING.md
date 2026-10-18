# Testing

Development is test-driven: a failing test, then the minimal code to pass it, then refactor. Every
commit leaves `cargo nextest run --workspace`, clippy and `cargo fmt --check` green.

## Layers

| Layer | Where | Tooling |
|---|---|---|
| Unit | Inline `#[cfg(test)]` in `domain` and `application` | std |
| Property-based | `domain`, `application` | `proptest`, `proptest-state-machine` |
| Integration and scenario | `application/tests/`: command scripts against small fixture regions built in code | std, `insta` |
| Adapter | `infrastructure/tests/` (parses both shipped region files); `game/tests/` (planned: headless `App` with `MinimalPlugins`) | std |
| Mutation | `domain`, `application` | `cargo-mutants` |
| Runner | Everything | `cargo-nextest` |

Only the dependency-rule test (`crates/domain/tests/dependency_rule.rs`) exists so far; the other
layers arrive with the code they test.

## Running each layer

```sh
cargo nextest run --workspace                  # everything
cargo nextest run -p domain                    # domain only (never compiles Bevy)
cargo nextest run -p application               # unit, property and scenario tests
cargo nextest run -p infrastructure            # adapter tests
cargo nextest run --workspace --profile ci     # what CI runs (no retries)
cargo insta review                             # review snapshot changes
cargo mutants -p domain -p application         # mutation testing
```

Property tests use `proptest`; set `PROPTEST_CASES` to run more cases locally. Note that nextest does
not run doctests; run `cargo test --doc --workspace` for those.

## Property catalogue

The invariants the property tests must cover:

- **Ramping:** for any sequence of targets, the per-tick change in output stays within the ramp rate,
  and online output stays between minimum stable output and capacity.
- **Merit order:** AGC never leaves a cheaper auto-follow plant with headroom while raising a more
  expensive one, and the reverse holds when lowering.
- **Conservation:** each tick, generation = served + curtailed + imbalance (signed), unserved =
  demand × shed, and 0 ≤ served ≤ demand. Unserved energy is shed load only; a residual mismatch is
  absorbed by system inertia and shows up as frequency deviation.
- **Money:** the change in cash always equals the sum of the ledger entries, exactly.
- **Commitment:** validate cyclic off-gaps against shutdown plus start-up, including midnight and
  exact-fit boundaries. Nuclear accepts all-day/all-off only. Initial state follows the pattern;
  physical start-up state does not cross representative months. Operating starts beyond midnight
  fail unchanged; stops/trips suppress automatic actions without mutating the base pattern.
- **Weighting:** daily energy and restarts use `days_in_month`; distinct boundary starts use a
  weight of one. No recurring/boundary duplicate or synthetic-initialisation charge; exactly one
  rounding step per ledger entry.
- **Hydro:** the energy a hydro plant produces in a played day never exceeds that day's budget.
- **Determinism:** the same verified versions, settings, region content, seed and commands produce
  an identical state hash. Test version/fingerprint mismatches before command execution, exact-byte
  region archival, and recovery after shipped data is retuned or removed. Missing/corrupt archives
  never silently fall back to installed data; failed archive/header initialisation prevents new games.
- **Robustness:** any random sequence of commands (state-machine test) never panics, and all
  invariants hold afterwards.
- **Parsing:** malformed region DTOs produce typed errors, never panics.
- **Rate review:** the new tariff x the past year's served energy = the revenue requirement (within
  one cent); the allowed return always stays within the PBR band. Assess reserves against the higher
  of actual and projected coincident peaks, including December connections. First excess review
  holds capital without return; useful portions are released; only portions still excess at a second
  consecutive review are written off, bounded by remaining book value. Held history survives annual
  resets, partial releases preserve the right history, previews never mutate it, and capital
  adjustments never change cash. Held portions continue book depreciation.

## Snapshots

Canonical scenarios ("East winter peak", "West duck-curve day", "Overreach: city connected without
capacity", "Overbuild: hold followed by second-review disallowance") snapshot their `MonthlyReport`s with `insta`,
so tuning changes show up as reviewable diffs.

1. Run the tests; a changed snapshot fails and writes a `.snap.new` file.
2. Run `cargo insta review` and accept or reject each diff.
3. Commit the accepted `.snap` files with the change that caused them.

## Mutation testing

`cargo-mutants` runs on `domain` and `application` on demand; a scheduled CI job is planned (it does
not exist yet). It is never run on every commit. The target is zero surviving mutants in `domain`.
Exceptions are `exclude_re` entries in `.cargo/mutants.toml`, each with a justification comment. The `game` crate is excluded.

## Isolation rules (all I/O tests)

`cargo-mutants` runs build copies in parallel, so tests must be hermetic.

- No fixed paths. Each test gets its own `tempfile::TempDir`; fixtures are copied in, never shared.
- No process-global mutable state: no `static mut`, no environment variables, no current-directory
  changes. Configuration is passed explicitly through ports.
- Reading shipped `assets/` by absolute path from `CARGO_MANIFEST_DIR` is allowed (read-only).
- Any future database follows the same rule: one per test (in-memory or a temp-file SQLite), never a
  shared schema.

## Writing a new test: checklist

- [ ] Write it first and watch it fail for the right reason.
- [ ] It sets up its own state and shares nothing with other tests.
- [ ] Any file I/O goes through a fresh `TempDir`.
- [ ] Randomness comes from an explicit seed.
- [ ] Domain tests use `domain`'s `test-fixtures` feature or in-code fixture builders, not plant
      numbers copied into the test.
- [ ] Invariants get a property test, not only hand-picked examples.
- [ ] Error paths assert both the typed `Err` and that state is unchanged.
- [ ] Snapshot tests have a descriptive name, and the diff was reviewed.
- [ ] A change that alters outcomes for the same inputs changes the rules-version guard snapshot
      (`application/tests/snapshots/rules_version_guard__*.snap`): either it is a bug, or
      `domain::SIM_RULES_VERSION` is bumped in the same commit.
