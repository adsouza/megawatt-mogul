# Pre-expansion-tracking compatibility capture

Captured from baseline `97b3ac0` (state schema 7, simulation rules 31), before
`ExpansionProjection` and state schema 8. The capture recipe is
`crates/infrastructure/examples/capture_expansion_baseline.rs` (relative to the repository root).
It refuses to run under any other state schema. Generation used a temporary `git worktree` of
the baseline and a temporary output directory, with no personal save access.

`minimal.ron` is the exact source (`tests/support/economic_region_v4.ron`). Both saves are in
Planning, seed 7, and carry their original schema-7 session and projection payloads, hashes,
command journal and RNG state:

- `expanded.mgsave`: a line from the anchor to city 3, outside the starter territory, was built
  and the city connected (the months it took to build were played).
- `starter.mgsave`: a new game, serving only its starter cities.

Do not regenerate these with the current engine. Tests verify their original payloads, then
check that restoring seeds `ExpansionProjection` as the first-game tutorial spec §6 says: an
extra connected city starts the count at zero from the loaded month; otherwise it stays unset.

| Save | SHA-256 |
|---|---|
| `expanded.mgsave` | `ad912af2da3429be5f4e0de39478e77daebdc00f32ea111894e939ec82bcdaa6` |
| `starter.mgsave` | `1107f53e330f1f2e97fad65bdc71712eac7bae90ad78986e53fe2b8cae5cc464` |
