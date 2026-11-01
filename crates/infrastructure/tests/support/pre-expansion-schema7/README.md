# Pre-expansion-tracking compatibility capture

Captured from baseline `97b3ac0` (state schema 7, simulation rules 31), before
`ExpansionProjection` and state schema 8, by
`crates/infrastructure/examples/capture_expansion_baseline.rs` (relative to the repository root).
That example was written later, so it isn't in the baseline; it refuses to run under any other
state schema and never overwrites a file. To reproduce the capture, from the repository root,
with `<scratch>` any temporary directory outside the repository (no personal saves are read):

```sh
git worktree add --detach <scratch>/baseline 97b3ac0
cp crates/infrastructure/examples/capture_expansion_baseline.rs \
   <scratch>/baseline/crates/infrastructure/examples/
(cd <scratch>/baseline &&
 cargo run -p infrastructure --example capture_expansion_baseline -- <scratch>/capture)
shasum -a 256 <scratch>/capture/*.mgsave   # must match the table below
cmp <scratch>/capture/minimal.ron crates/infrastructure/tests/support/pre-expansion-schema7/minimal.ron
git worktree remove --force <scratch>/baseline
```

`<scratch>/capture` must not exist yet. The capture is deterministic: rerunning it gives the
same bytes.

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
