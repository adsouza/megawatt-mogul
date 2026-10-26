# Plan 8 — synchronous islands, HVDC and SGCC

Source: design §28.9 and the economic-zone handoff in §28.7.1. Continues rules 18;
new commands use rules 19 and new portable captures use state schema 4. Old state
schemas remain readable. Generated geography stays in canonical region bundles.

- [x] Validate electrical-area partitions and optional HVDC catalog/economic terms.
- [x] Separate AC island topology from AC/DC energisation; reject cross-area AC.
- [x] Add link construction, cancellation, schedules, overrides and exact route quotes.
- [x] Step island dispatch, storage, protection and frequency independently; ramp,
      block and run back DC using the previous tick's state.
- [x] Report island energy and frequency performance; assess link utilisation at review.
- [x] Persist commands, assets and island state, retaining schemas 1–3 readers.
- [x] Add island selection, link controls and AC/DC map presentation with localization.
- [x] Author the five-area SGCC region and source/provenance notes.
- [x] Run focused regressions, clippy, formatting and launch smoke; document results.

Validation uses focused nextest selections. Running the entire local suite requires
advance user permission. Do not use the user's save directories for launch checks.

## Implementation and evidence

Remaining native acceptance (reconciled 2026-10-26):

- [ ] Record a human SGCC/HVDC usability playtest. The scripted scheduled-link day and launch
  smoke below are complete; they do not claim human interaction acceptance.

Rules 19 keeps earlier single-area regions on the existing controller path. Explicit areas opt
into separate AC components. Weather and demand noise still draw once region-wide each minute.
Links retain the player's endpoint orientation while quotes use canonical terminal pricing.
State schema 4 adds links, island controllers, report/chart rows and the original island forecasts
for archived supply charts. Historical source-schema bytes remain the compatibility oracle.

SGCC uses five areas and 29 cities, with an intentionally small Beijing starter territory.
[Authoring and tuning notes](../../research/2026-10-26-plan-8-sgcc.md) distinguish administrative
proxies, schematic seams and illustrative game economics from sourced claims.

Validation on 26 October 2026:

- Initial focused domain regressions: 25 passed, including generated-network component, signed energy,
  ramp, quote-reversal, atomic-refusal and portable-state properties. The new shared RNG test
  failed before the weather/noise change and passes after it.
- Follow-up regressions pin commissioning-month utilisation proration and city service during
  a remote island outage. The city-card regression failed with the regional average and now
  uses the selected city's own island service ratio.
- Application rules guard and save/resume regressions: 21 passed. The rules-guard scenario
  snapshot changes only its version label from 18 to 19.
- Infrastructure compatibility, command-log, economics and region selections: 115 passed after
  repairing three expectations for the new schema/test fixture. Source-schema golden bytes were
  not regenerated. A live two-island codec test restores nonzero flow and an override, then
  continues identically through month close.
- After adding archived island forecasts, a follow-up save/archive selection passed all 33
  tests. This caught and fixed the monthly chart helper's old default schema. The two-island
  codec test also passes through the actual directory format, both mid-day and after closing
  the month.
- Game picker, forecast, planning, operating and localization selections: 67 passed; the real
  HUD schedule test also passes with a selected island, All and the link window. A subsequent
  focused 25-test run covers the new historical-chart regression, all HVDC HUD tests and Fluent
  coverage. Historical island charts use the archived month's island forecast.
  A follow-up city-card/HVDC/history selection passed all 16 tests after the city service fix.
- Region authoring: 27 Python tests passed, including exact geography and operating-exclusion
  regeneration. SGCC's year-one beginner/hands-off and first-expansion acceptance cover seeds
  0–7; shared overlap checks cover all eight shipped regions.
- Optimised dev profile frame-budget medians over 100 runs: terrain build 2.571 ms / 250 ms,
  pick 0.0048 ms / 1 ms, overlay refresh 0.528 ms / 4 ms, picker paint 1.641 ms / 4 ms.
- Isolated native SGCC acceptance: seed 7, four remote CCGTs near Xining, one Beijing–Xining
  HVDC link, 500 MW schedule for all 24 hours. Construction months and the complete August 2027
  day finished with an active company, two report rows and nonzero received DC energy. The
  temporary driver sent normal player-command messages, rendered 120 frames and exited 0.
  Its logs and saves used a temporary directory, and the temporary example was removed.
- Workspace clippy with `-D warnings`, `cargo fmt --all --check` and `git diff --check` pass.
  The final standard `cargo run -p game -- --exit-after-frames 120` launch also exited 0 with
  isolated temporary save/log directories.

The entire local test suite was not run, following the user's advance-permission requirement.
The native run is scripted acceptance, not a human SGCC playtest. Existing Plan 7 human playtest
limits remain as recorded in the spec.

## Adversarial review fixes

All four reported issues have permanent regressions that failed before their fixes:

- Runback retains its triggering converter terminal when flow reaches zero. Both directions
  stay at zero while that sender remains below threshold, including across portable restore,
  and resume at the link ramp after that sender recovers. A signed ramp or released latch also
  checks the prospective sender before reversing. The terminal is persisted and validated.
- Hydro AGC off/on clears the owning island's cached current/future plan and ceilings, preserving
  frozen history. A subsequent hourly replan can schedule the plant again.
- A review writes off older excess shares while placing newly excess shares on a fresh first
  hold. Already-disallowed shares count toward the original link's unused fraction. Tests cover
  worsening use, steady partial use, and generated utilisation sequences with depreciation,
  exact capital conservation and same-year idempotence.
- Planning, Operating and Review use one scoped history presenter. Real SGCC tests compare both
  islands' archived demand, service, transfers and forecasts, restrict month choices to available
  island history, and verify that All restores region totals.

The final focused runs passed 62 domain, 99 application/infrastructure and 74 game tests, plus
27 Python authoring tests. The isolated native SGCC scenario above was rerun after the fixes and
again completed the scheduled-link day, rendered 120 frames and exited 0. Workspace clippy with
warnings denied and formatting/diff checks passed before the commit. The full local suite
remains unrun under the user's advance-permission rule.
