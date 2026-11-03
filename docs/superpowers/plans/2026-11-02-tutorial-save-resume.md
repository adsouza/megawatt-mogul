# Tutorial save/resume

Persist a versioned optional client record with each manual save and monthly autosave. Keep
simulation checkpoints and hashes unchanged. Restore the saved tutorial independently of the
global first-game flag; missing, malformed or future tutorial data leaves a valid game playable
without a tutorial. Old saves retain their existing behavior.

- [x] Reproduce tutorial loss after saving, leaving and restarting with a failing integration test.
- [x] Add optional client-owned save payload and preserve it through native/browser transports.
- [x] Persist stable lesson identifiers, starter IDs, recommended kind, Review/outlook markers
      and payoff month; rebind session generation on successful load only.
- [x] Restore bookkeeping, restart timers, and re-offer opening an interrupted payoff dialog.
- [x] Cover autosave/Continue, snapshot rewinding, finished/off/legacy/corrupt data and round trips.
- [x] Run focused persistence/tutorial tests, native lint, browser compilation and launch smoke.

Validation: 271 focused checks passed (191 game/tutorial/save/localization tests and 80 save
adapter/codec checks, including arbitrary tutorial-byte decoding). Workspace clippy with
`-D warnings`, formatting and diff checks passed. The browser target compiles with the installed
Rust 1.98.1 WebAssembly toolchain. The native 120-frame windowed launch smoke exited 0 using
temporary save/log directories. The full workspace test suite was not run.
