# Tutorial: renewable siting

Add one final spotlight after the plant-payoff comparison closes, teaching players to optimize
hydro, solar, wind and geothermal yield by comparing resource layers and inspecting candidate
sites. Mention geothermal exploration risk. No simulation, preference or save-schema changes.

- [x] Reproduce premature completion after closing the comparison with a failing lifecycle test.
- [x] Append the localized siting note and anchor it to flat-map and world-view layer controls.
- [x] Keep the tutorial pending until the final note is dismissed or times out; retain early-day skip.
- [x] Extend real-widget dismissal, timeout, compact/wide layout and simulation-isolation coverage.
- [x] Run focused tutorial/localization regressions, formatting and clippy.

Validation: 128 focused tutorial, localization and compact-selector tests passed (the new
world-view timeout test passed on rerun after disabling the fixture's independent automatic
commitment planner). Both compact and wide layer controls are covered, alongside minimum-window
zoom checks and tutorial-on/off simulation equality. Workspace clippy with `-D warnings`,
`cargo fmt --all --check` and `git diff --check` passed. The full test suite was not run.
