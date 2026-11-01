# Crystal-ball implementation

- [x] Add a pure twelve-month session query using existing prospective construction/service.
- [x] Add a localized presenter, plot and table with a selected-city chooser and estimated route.
- [x] Add an accessible vector crystal-ball top-bar button and dispatch-controlled dialog state.
- [x] Cover domain arithmetic, determinism, construction timing, widgets, keyboard and layout.
- [x] Run focused tests, workspace clippy, formatting, and a windowed launch smoke.

Validation completed:

- `cargo nextest run -p domain --features test-fixtures -E 'test(outlook)'`: 5 passed, including
  the property covering plant/connection completion boundaries.
- `cargo nextest run -p game -E 'test(outlook) | test(l10n_coverage) | test(top_bar) | test(tutorial)'`:
  145 passed. Includes all shipped regions, the actual icon/chooser/Close/Escape flow, maximum UI
  zoom at minimum window size, opening over Review, and preservation of simulation state.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, and
  `git diff --check` passed.
- Native launch drew 120 frames and exited successfully, with logs/saves in a temporary directory.
- Full workspace tests were not run.

Follow-up: the crystal ball sits left of Milestones with the same vertical button alignment.
The preview chooser excludes every city with an existing or ordered grid path and falls back to
the baseline when the selected map city is ineligible. Scheduled city demand is verified to enter
the baseline at the connection's completion date. Focused follow-up checks: 24 game tests and
5 domain outlook tests passed; formatting and diff checks passed.
