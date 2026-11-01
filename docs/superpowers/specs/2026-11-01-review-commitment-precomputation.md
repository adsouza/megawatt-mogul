# Precompute next-month commitments during Review

The Continue delay in the latest IPS recording was the synchronous automatic commitment query:
2.53 seconds versus 0.71 milliseconds for FinishReview and 33.75 milliseconds for the supply
forecast in the development build. Move that query away from the UI thread.

A subsequent replay comparison measured 2.62 seconds of preparation and 14.81 milliseconds
for FinishReview plus cached handoff, with identical commands and diagnostic text to a fresh
next-month computation. These are development-build timings, not a frame-time guarantee.

Review is immutable during normal play. Speculate on an owned session copy: call FinishReview
on it, retaining the exact next-month RNG/weather draw, and run the unchanged bounded search.
The live session, journal, diagnostics, autosave and RNG remain untouched. The domain exposes
an incremental pure search, evaluating at most one candidate per step. Native clients use a
worker thread; browser clients yield to the event loop between steps. Cancellation is checked
between steps. The baseline evaluation and each candidate remain synchronous within a step.

Continue always dismisses Review immediately. A ready result is reused at the next-month
Planning boundary. If it is unfinished, Forecast remains interactive with an Updating commitments
hint, and Start the day waits until the result is queued. Apply only after checking session
generation and the predicted Planning state hash. Queue normal planner commands and diagnostic
messages, preserving the existing replay and logging paths. Speculative work itself logs no run.

A committed Planning edit or a commitment draft edit discards late work, preserving player
choices. Disabling automatic commitments, replacing the session, or leaving the game cancels it.
Loading a Planning snapshot never starts a fresh automatic run. Review snapshots may precompute
for their eventual Continue. Native worker failures retain the previous synchronous fallback.

Verify exact next-month plans and diagnostic output, live-state/RNG purity, incremental search
invariants, pending work after Continue, ready-result reuse, manual-edit and session invalidation,
localization, UI parameter compatibility, save/load regressions, native launch, and WASM build.
