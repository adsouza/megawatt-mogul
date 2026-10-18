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
- **Conservation:** each tick, generation = served + curtailed + overgeneration, and unserved energy
  is never negative. (This plan extends the spec's rule with overgeneration: surplus that raises
  frequency and is never paid for.)
- **Money:** the change in cash always equals the sum of the ledger entries, exactly.
- **Weighting:** each month's ledger entries equal the played day's priced totals x `days_in_month`,
  with exactly one rounding step per entry.
- **Hydro:** the energy a hydro plant produces in a played day never exceeds that day's budget.
- **Determinism:** the same seed plus the same commands produce an identical state hash.
- **Robustness:** any random sequence of commands (state-machine test) never panics, and all
  invariants hold afterwards.
- **Parsing:** malformed region DTOs produce typed errors, never panics.
- **Rate review:** the new tariff x the past year's served energy = the revenue requirement (within
  one cent); the allowed return always stays within the PBR band; disallowance is zero whenever firm
  capacity is within the reserve margin, and never exceeds the capital newly added that year.

## Snapshots

Canonical scenarios ("East winter peak", "West duck-curve day", "Overreach: city connected without
capacity", "Overbuild: rate review with disallowance") snapshot their `MonthlyReport`s with `insta`,
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
