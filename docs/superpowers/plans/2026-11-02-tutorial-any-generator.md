# Tutorial: accept any connected new generator

Let the Save reminder trigger after the player orders and connects any generating plant,
excluding battery storage. Capture every starter generator so inherited fleets cannot qualify.
Once the task is satisfied, skip remaining recommended-plant instructions. Keep the regional
recommendation for players who still need guidance. Ordered paths count; dangling lines do not.
Use the same completion check for scheduling the later payoff chapter after outlook use.

- [x] Reproduce the kind restriction with failing presenter regressions.
- [x] Generalize the connection predicate and skip already-satisfied supply instructions.
- [x] Cover all plant kinds, starter exclusion, battery exclusion and incomplete/ordered paths.
- [x] Exercise the actual Save spotlight and later-chapter scheduling with a new wind farm.
- [x] Run focused tutorial/localization tests, clippy and formatting checks.

Validation: 129 focused tutorial and localization tests passed, including all plant kinds,
geothermal after successful exploration, incomplete and ordered paths, the real Save spotlight
after connecting wind, later payoff scheduling and tutorial-on/off simulation equality.
Workspace clippy with `-D warnings`, `cargo fmt --all --check` and `git diff --check` passed.
The full test suite was not run.
