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
| `operating.mgsave` | `82213ab860c75a490d56731182458cb23ce2b9ef41b805b00c7a0d34fefba541` |
| `planning.mgsave` | `ad3f4c2f1b3683dd6fcd42e9498661074d98404df6f3c269ee2106f8d95948b8` |
| `review.mgsave` | `75fb20a79325d31885980811c2557ec1c211d5aada4c075897731a5c44e2d34d` |
