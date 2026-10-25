# Immutable Plan 6 compatibility baselines

These source regions and rules-15/state-schema-3 saves were captured before the
Plan 6 clock, climate and sun arithmetic changes. Do not regenerate them when
new-region balances or rules change. Both saves are at tick 900 with 800 MWh
stored in a battery. The expected hashes are after another 60 ticks and at review.
The `.z` files are zlib-compressed portable saves.

Terrain hashes were captured before the Task 7 geometry refactor. Encoding is
width and height as u16 little endian, then row-major cells: land as u8, then
height, irradiance, wind potential, cooling water and hydro potential as f64 bits
little endian. The generators are retained for provenance, not automatic refresh.

The separate `portable-saves` directory retains the released schema-1 fixtures.

Capture target: macOS / aarch64 (Darwin arm64). Exact terrain SHA-256 and
future-state literal hashes are asserted on this origin target only, per spec
§5.7's same-build/platform libm guarantee. Every target verifies source bytes,
fingerprint, original portable hash and canonical source-schema bytes, compares
the old independent raster/shape/sun formulas, and checks full same-target
save/resume continuation checkpoints after 60 ticks and at review. Linux is not
excluded from archived-save restoration or continuation.
