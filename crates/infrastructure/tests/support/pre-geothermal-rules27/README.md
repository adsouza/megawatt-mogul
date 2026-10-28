# Pre-geothermal compatibility capture

Captured from immutable baseline `c1de9d8` (simulation rules 27), before changing the
portable records. The capture recipe is `crates/infrastructure/examples/capture_geothermal_baseline.rs`
(relative to the repository root).
It deliberately refuses to run under rules 28. Generation used an isolated `git archive`
checkout and temporary output directory, with no personal save access.

`minimal.ron` is the exact source. Each phase save carries its original source schema,
canonical session/projection payloads, hashes, command journal and RNG state. Schema-5
folders also contain original-engine continuation receipts: Planning → BeginOperating,
Operating → AdvanceTicks(60), Review → FinishReview. The multi-area source contains an
HVDC link and two electrical areas. The schema-4 folder uses the baseline's schema-4
encoder, without regenerating any earlier historical fixture.

Do not regenerate these with the current engine. Capture target: `aarch64-apple-darwin`.
Tests verify the original source-schema bytes before upcasting on every target, and compare
full checkpoints and events after resaving, restoring and continuing on every target.
The baseline dynamic continuation receipts are compared exactly on the capture target;
other targets can differ in the last bit of libm results after advancing the simulation.

| Save | SHA-256 |
|---|---|
| `operating.mgsave` | `ae1c3d3b3f480467dd6928760f4c36122c144f947dd3c06003fdef895bd9d74d` |
| `planning.mgsave` | `fcefb9f95fe74e69466d52f4070b23bc292d992a7d1c4dc474a37e7fe83ae85d` |
| `review.mgsave` | `7b10a67bc1f6d053919604afa4e8afeab1fcaf48ee84a8abb791206cb5813c09` |
