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

Every layer above has tests except the `game` adapter (the crate does not exist yet, so its headless
`App` tests are still planned). Beyond the inline unit and property tests, the integration suites are
`application/tests/{scenarios,determinism,state_machine,pipeline,rules_version_guard}.rs`, the adapter
suites are `infrastructure/tests/{region_catalog,shipped_regions,command_log}.rs`, and
`domain/tests/dependency_rule.rs` enforces the dependency rule.

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

`cargo-mutants` mutates each function in `domain` and `application` (flipping operators, replacing
bodies with defaults) and reruns the tests; a mutant the tests do not fail on is "missed" and points at
an assertion that is missing or too weak. The `game` crate is out of scope. It is never run on every
commit.

**Target:** zero missed mutants in `domain`. The only exceptions are `exclude_re` entries in
`.cargo/mutants.toml`, each with a `#` comment proving the mutant equivalent (or the code untestable).
`application` is held to the same standard.

### Commands

```sh
cargo mutants -p domain -p application -j 4          # the full run (what the weekly job does)
cargo mutants -p domain --list | wc -l               # size the job (about 1,500 mutants for domain)
cargo mutants -p domain -j 4 -F "grid/dispatch.rs"   # one file while fixing survivors
cargo mutants -p domain -j 4 --shard 3/8 --output target/mutants-domain-3   # one slice of eight
```

A full local run takes roughly 20 minutes at `-j 4`. To keep each command short, run the shards
`0/8` through `7/8` one after another, each with its own `--output` directory. Use `-F` (an examine
regex), not `-f`, to narrow to a file: `-f` is merged with the configured `examine_globs` and still
runs everything. Mutants run in scratch copies of the tree, so `cargo mutants` never touches your
working directory; tests are hermetic (see below), which is what makes the parallel runs safe.

`.cargo/mutants.toml` selects nextest and the `mutants` nextest profile (`.config/nextest.toml`). That
profile stops at the first failure and terminates running tests immediately, so a mutant that makes a
loop spin forever fails fast instead of waiting out `timeout_multiplier`.

### Reading `mutants.out/`

| File | Meaning |
|---|---|
| `missed.txt` | Mutants every test passed against. Each one needs a stronger test (or a justified exclusion). |
| `caught.txt` | Mutants at least one test failed against. |
| `timeout.txt` | Mutants whose tests did not finish. They count as detected (an infinite loop is a failure), but check they are not hiding a slow test. |
| `unviable.txt` | Mutants that do not compile; nothing to do. |
| `outcomes.json`, `log/`, `diff/` | Per-mutant detail: the test output and the exact source change for each mutant. |

To kill a survivor, open its file under `diff/` to see the change, then write a test that fails against
it: prefer a property test with boundary values (a ramp exactly at the limit, a reserve margin exactly
at its threshold) over a one-off example. Before excluding a mutant, show that no input can tell it
apart from the original; an operator that is only unreachable at one boundary is usually still
testable with a hand-built state.

### Scheduled job

`.github/workflows/mutants.yml` runs the full `cargo mutants -p domain -p application -j 4` every
Monday at 06:00 UTC and on demand (`workflow_dispatch`), then uploads `mutants.out/` as the
`mutants-out` artifact. The job goes red when anything is missed; download the artifact and read
`missed.txt`. Output directories (`mutants.out*/`) are git-ignored.

### Mutation testing status

Last full run (this branch, 8 shards of `domain`, 2 of `application`):

| Crate | Mutants | Caught | Missed | Timeout | Unviable |
|---|---|---|---|---|---|
| `domain` | 1,476 | 1,110 | 0 | 0 | 366 |
| `application` | 139 | 101 | 0 | 0 | 38 |

Both crates are at the zero-missed target, so there is no application backlog to record. The first
baseline found 123 missed (and 10 timed-out) mutants in `domain` and 6 in `application`; all were
killed with tests except the equivalent mutants excluded in `.cargo/mutants.toml`. Those are the
exceptions to keep an eye on:

- `GameSession::settings` and `PowerSystem::model` return a value of a type with one inhabitant
  (`NetworkModel::CopperPlate`); they become testable when a second network model exists.
- `HeldSlice::consecutive_excess_reviews` returns a count that is always 1 for any slice that
  survives a review, because a second consecutive review writes the slice off. The counter is
  vestigial state the write-off rule never reads past 1.
- The remaining entries are arithmetic that cannot change a result: expected solar at tick 0 is zero,
  a zero shortfall allocates zero, `% 24` on an hour already reduced modulo 24, a float distance that
  can never equal `1e-9`, and a zero-weight allocation.

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
