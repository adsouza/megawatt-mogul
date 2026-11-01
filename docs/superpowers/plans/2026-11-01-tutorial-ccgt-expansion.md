# Tutorial: CCGT expansion after the first month

Implements the requested continuation of the first-game tutorial and design §4.5.

- [x] Reproduce the premature tutorial finish with a failing lifecycle regression.
- [x] Keep the run alive after Review and restrict Planning steps to their chapter.
- [x] Guide site selection, CCGT explanation, build, and grid connection using the real controls.
- [x] Share plant anchors/reveals across battery and CCGT chapters.
- [x] Show the selected site's exact effective build duration in the spotlight and instruction.
- [x] Exclude starter CCGTs; recognize early orders and complete ordered paths to the grid.
- [x] After scheduling the CCGT and connection, spotlight Save and recommend a named snapshot
  before risky moves such as connecting a new city.
- [x] Follow Save with a crystal-ball spotlight explaining the city-demand preview; finish after
  that spotlight or an early day start.
- [x] Extend pure-model, real-widget, small-window and simulation-isolation coverage.

Validation completed:

- 146 focused tutorial, outlook, top-bar and localization tests passed, including the real CCGT
  build/connect widgets, Save → crystal-ball spotlight sequence, opening the actual outlook and
  save dialogs, minimum-window layout at maximum zoom, local-duration overrides, disconnected-line
  regression and tutorial-on/off simulation equality.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo fmt --all --check` and `git diff --check` passed.
- Full workspace tests were not run.
