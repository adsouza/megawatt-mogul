# Review commitment precomputation implementation

- [x] Measure Continue, the commitment query and supply forecast using the player's IPS recording.
- [x] Add pure incremental search and next-month preparation/matching regressions.
- [x] Run native speculation off-thread and browser speculation with event-loop yields.
- [x] Cache by session/month and verify the exact predicted Planning state before reuse.
- [x] Let Continue show Forecast while unfinished work continues; pause Start until queued.
- [x] Discard results on manual edits, disabled automatic commitments, loads or game exit.
- [x] Preserve ordinary command logging; speculative work does not log or apply anything.
- [x] Finish focused persistence checks, clippy, native smoke and timing verification.

Initial validation: 65 focused planner, cache, localization and UI checks passed; WASM compilation
passed using the installed rustup 1.98.1 toolchain (the Homebrew compiler lacks that target).

Final validation: 72 focused persistence/client checks passed, workspace clippy with warnings
denied passed, formatting/diff checks passed, and the up-to-date native binary drew 120 frames
with isolated temporary save/log directories. The IPS comparison took 2.62 seconds to prepare
off-thread and 14.81 milliseconds for FinishReview plus cached handoff; cached commands and
diagnostic text exactly matched a fresh computation on the actual next-month state.
