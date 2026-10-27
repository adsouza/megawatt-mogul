# Zone-based commitment planner

The forecast already follows the selected island, but commitments previously listed the entire
company fleet by fuel type. Use the same `UiState.island` for both tools, with a local-grid picker
in the commitment planner. Planning opens on the headquarters island and shows only that grid's
plants in fuel merit order and plant ID order. The heading uses the same area/city label as the
forecast and links directly to it. The company-wide All selection opens the forecast overview;
returning to Commitments restores the last valid local island. Stale selections fall back to the
headquarters component, then the first remaining island. Legacy single-grid regions keep their
existing planner. No combined company-wide commitment editor is offered.

Grid selection is presentation state only. Preserve draft schedules and target form values;
show a return-to-draft action when the selected grid hides an unapplied edit. Starting the day
remains disabled until the existing draft is applied or reverted. Preserve the existing
cross-row draft validation and flushing behavior.

- [x] Demonstrate selected-island leakage and interleaved fleets with failing presenter tests.
- [x] Name and filter separate local fleets; localize picker, forecast links and hidden-draft text.
- [x] Verify actual selection, forecast navigation, local commands and draft recovery at minimum
      window size, including 125% UI zoom.
- [x] Verify island-first defaults and overview/return navigation, then rerun focused regressions, formatting and clippy.

Validation: 110 focused game checks passed (commitments, planning, forecast, activation/HVDC,
player workflows, localization and planning performance), plus four dispatch/lifecycle checks.
The island-first workflow initially failed because planning opened with no island selected;
it now verifies the local default, All overview and immediate return to the remembered island.
Stale island IDs fall back safely. Workspace clippy with warnings denied and formatting/diff
checks passed. The minimum-size workflow exposed horizontally clipped Apply/Revert controls;
these now appear below the edited row and remain reachable at both zoom settings.

All changes are in game presentation, localized text and tests; simulation and save formats are
unchanged. A native 120-frame launch smoke exited successfully using a temporary save directory.
Desktop inspection could not attach to the raw game executable, so visual layout claims rely on actual egui widget geometry and
clicks, rather than a native screenshot.

Player-facing terminology uses **grid zone** in explanatory text and **zone** in compact labels,
including Planning zone, Inspect zone and All zones. Forecast warnings, activation guidance,
HVDC equipment and the SGCC region description use the same vocabulary. Internal island IDs,
localization keys and saved-game formats are unchanged.
