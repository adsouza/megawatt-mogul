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

Plan 1 (headless simulation core: the `domain`, `application` and `infrastructure` crates)
complete; the Bevy client follows in Plans 2–3, so there is nothing to play yet.

## Build and test

Requires Rust 1.98 and [`cargo-nextest`](https://nexte.st) (`cargo install --locked cargo-nextest`).

```sh
cargo nextest run --workspace                 # run every test
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo mutants -p domain -p application -j 4  # mutation testing (slow; weekly in CI, see TESTING.md)
```

See [TESTING.md](TESTING.md) for the test layers and [ARCHITECTURE.md](ARCHITECTURE.md) for how the
code is organised. Contributors and coding agents should read [AGENTS.md](AGENTS.md).

## Design

The full design is in
[`docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`](docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md).

## Licence

AGPL-3.0. See [LICENSE](LICENSE).
