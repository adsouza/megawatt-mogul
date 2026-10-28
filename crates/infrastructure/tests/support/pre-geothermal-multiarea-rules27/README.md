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

Do not regenerate these with the current engine. Tests verify their original bytes
before upcasting and compare the baseline continuation receipts exactly.

| Save | SHA-256 |
|---|---|
| `operating.mgsave` | `d536d76c7821224b189964e54ccbab5199e325548e8adbd2c013e2576ac6e9f7` |
| `planning.mgsave` | `36d161e3553e1c5d71aa80bf4bdbbe65ddeaeeac0c2f4c28d11ec4fc58d84694` |
| `review.mgsave` | `5e67e3b9a54e605e781709e2cf990f1d8eba0d56c3c0ac6c93884b999f4da1cc` |
