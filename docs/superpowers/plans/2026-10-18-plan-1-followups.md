# Plan 1 (simulation core): rulings, open questions and follow-ups

Plan 1 was executed task by task, with one implementer subagent and one reviewer per task, followed by a
whole-branch review and a single fix wave. This file keeps what the execution decided and what it
left open, so that Plan 2 can start from it.

## Rulings made during execution
Each ruling ends with what it costs if it turns out to be wrong.

- **Branch:** implementation is on `feat/simulation-core`, stacked on `docs/m1-design-spec`, rather
  than in a worktree. If wrong: rebase or move the branch.
- **R1:** the fixture's solar daylight hours, January to June, mirrored for July to December. If
  wrong: fixture snapshots churn.
- **R2:** values within 1e-9 of shed 0 or 1 and of an empty hydro budget snap to the boundary. If
  wrong: negligible.
- **R3:** unserved energy is shed load only, i.e. served = demand × (1 − shed), and the residual is
  a signed imbalance. If wrong: strike frequency (tuning).
- **R4:** the start fleet and start lines enter the rate base as legacy assets at full capital
  cost. They are never held or written down. If wrong: the year-1 tariff level.
- **R5:** a test-construction ruling, superseded by the revised Task 19 tests.
- **R6:** `SimRng` has a manual `Debug` and `PartialEq` covering seed and word position, so
  `state_hash` covers RNG state. If wrong: none.
- **Commit trailers:** each subagent commit names the model that wrote it. If wrong: cosmetic.
- **R7:** `initialise_pattern_day` belongs to Task 7, not Task 6. If wrong: an API shuffle.
- **R8:** tripped solar reconnects once blackout restoration first re-energises the grid, and at
  each day initialisation. If wrong: solar timing during restoration.
- **R9:** Task 7's boundary test uses an in-test tick loop; Task 12 re-asserts it through
  `PowerSystem`. If wrong: one duplicate test.
- **R10:** a boundary start is charged only when a plant returns from a whole month offline on an
  all-day pattern. Recurring starts are counted when they fire. If wrong: start-cost accounting.
- **R11:** right-endpoint energy per tick. If wrong: one minute of minimum output per start.
- **R12:** during blackout, AGC targets demand × min(1, 1 − shed + step), pre-positioning supply so
  restoration can't stall. If wrong: restoration pacing.
- **R13:** solar curtailment is physical, through a cap on solar availability; AGC is forced to
  minimum only when curtailment alone can't cover the surplus. If wrong: curtailment tuning.
- **R14:** the conservation proptest gained non-tautological assertions, with solar in the fleet.
  If wrong: none.
- **R15:** demand noise resets each day. If wrong: none (deterministic either way).
- **R16:** generation = served + imbalance; `curtailed` is withheld solar availability; the summary
  is the post-protection state. If wrong: doc wording only.
- **R17:** the held-capital interfaces: `AssetProfile`, `add_legacy`, `ReserveReviewInputs`, the
  capital events and the slice-allocation algorithm. If wrong: interface rework.
- **R18:** written-off capacity still counts toward later excess (follows the plan text). If wrong:
  harsher penalties for building after a write-off.
- **R19:** a line joining two new plants follows the plant with the lowest id. If wrong: a small
  line-capital difference.
- **R20:** several plants may share a cell. If wrong: add an occupancy check.
- **R21:** each day starts with a merit-order dispatch of auto-follow plants; without it every game
  blacked out at midnight. If wrong: midnight dispatch tuning.
- **R22:** projections read the session after the whole command, so a multi-tick `AdvanceTicks`
  judges ramp warnings by the end state. If wrong: warnings stale by one batch.

## Open design questions (need your decision)
- **Q1 (overreach):** connecting a city far larger than capacity causes an instant blackout at
  tick 0. Every plant trips and has to be restarted by hand. There is no graded rolling shedding.
  Consider pre-emptive rotating outages at the day boundary, sized to the forecast deficit.
- **Q2 (over-commitment), implemented (Plan 1a):** resolved by spec Amendment A3 (§14:
  over-frequency trips, a minimum-generation warning and frequency-performance strikes). The
  original problem: with no solar to curtail, surplus generation left frequency stuck high, with no
  over-frequency trip and no cost but fuel burned on unpaid energy.
- **Q3 (hydro under AGC), implemented (Plan 1a):** resolved by spec Amendment A1 (§12, hydro
  peak-shaving schedule). The original problem: hydro dispatched at $0/MWh let AGC empty its daily
  water budget early and then drop it to zero in one tick, often before the evening peak. The West's
  flows are now a realistic hydrograph rather than tuned to cope.
- **Held capital:** `HeldSlice::consecutive_excess_reviews` never exceeds 1 in practice. Either drop
  the field or make the write-off threshold configurable.
- **First-year rate review, implemented (Plan 1a):** resolved by spec Amendment A2 (§13, prorating
  the return by the months in the review period). The original problem: a game starting in any month
  other than January applied an annual return to a partial year of energy.

## Plan 1a rulings

Plan 1a (amendments A1–A3) landed under `SIM_RULES_VERSION` 2. The builder's calls made while
implementing it are recorded in the spec as *(Builder's call: …)* notes: §12.3–§12.4 and §12.7 (the
spill test, the effect-hour ceiling, the look-ahead ramp envelope, unplanning on AGC off or a trip),
§14.2 (the surplus, headroom and falling-output trip gates, the redispatch after a trip, trips
during restoration), §14.4 (`frequency_failed`), §14.5 (CCGT minimum stable level 30%, the Eastern
fleet's coal retirements) and §14.6 (the surplus clears as far as safe trips allow; armed protection
never sheds more than unarmed).

Final-review rulings (fixed within v2, before Plan 1a shipped):
- **Hydro recovery after an emergency lift (§12.4):** when the lift ends, scheduled hydro above its
  ceiling ramps down no faster than the other AGC plants' one-tick headroom, net of the shortfall,
  until it is at or below every ceiling it meets before the next re-plan. Without it, the cap's
  return re-shed load repeatedly and blacked out a slow fleet. Cost: water spent beyond the plan
  after emergencies (allowed by §12.4).
- **Hydro schedule per phase (§12.6):** the view shows today's plan while Operating, the finished
  day with every hour frozen during Review, and nothing from `FinishReview` until `BeginOperating`.
  There is no Planning preview yet; Plan 2 can request one.

Accepted residuals: a trip can still precede a protection-assisted shed when hydro drops at an
hour-boundary re-plan or falls past the next boundary, when a commitment stop starts after the trip,
or when the player cuts a manual target after it.

Parked for Plan 2: a slow fleet that never reaches an emergency can still shed on a foreseen hydro
fall. The envelope ramps hydro down at hydro's own rate, and thermal may be unable to follow. Not
seen in shipped data. If it appears, limit the envelope's descent to the fleet's headroom, as
recovery already does.

Watch (final review, deferred): no snapshotted fleet exercises A1, so consider a West-like
scenario in Plan 2; `check_invariants` could also cover "hydro target ≤ ceiling" (outside a lift or
recovery) and "remaining allocation ≤ remaining budget".

Tuning watch items:
- **East acceptance margin:** the hands-off year passes, but 6 of 8 seeds draw a frequency notice and
  the April/October committed minimum sits about 50 MW above the 02:30 net load. One more peaker
  would push seed 3 to two strikes; firm capacity is 1.105 × the connected peak against the 1.10
  test floor.
- **Overbuild scenarios:** before the frequency strikes landed, 7 of the post-commissioning months
  ended the day over-frequency (+0.63 to +2.86 Hz), a surplus no unit could safely trip.
  - With the strikes, the committed script (`overbuild_overcommitment_loses_licence`) ends in
    licence revocation at the October 2029 close.
  - The used-and-useful review is now covered by `overbuild_idle_capacity_disallowance`, which
    builds the same CCGTs but leaves them uncommitted. It holds capital in December 2029 and
    disallows it in December 2030 (ruling: split the scenario).

## Carry into Plan 2 (Bevy client)
- **Batching:** advance in small batches (`speed` ticks per `FixedUpdate`) so projections stay
  timely (R22).
- **Save/load:** `replay` returns only a `GameSession`. Any future load or resume must rebuild the
  projections by feeding the replayed events through `Projections::apply_all`.
- **Command-log failures:** these are only traced. `FileCommandLog` then silently drops later
  commands. Surface log health to the player.
- **Region picker:** `RonRegionCatalog::list` fails as a whole if any one file is bad, and it fully
  rasterises every region just to list it. Return per-entry results for the picker.
- **Spec §6.3:** asks for price in the timeline and revenue per plant. Both were dropped by the plan,
  so implement them or amend the spec.
- **Blackouts:** a blackout also trips disconnected plants, which contradicts the module doc. Low
  impact.
- **`Megawatts::new`:** it only `debug_assert`s finiteness, so the command-log DTO path can build a
  NaN in release. The domain now rejects non-finite dispatch targets anyway; validating at the DTO
  boundary would close the gap entirely.
- **Cross-platform determinism:** not guaranteed. libm is used in Box–Muller, solar `sin`, line
  `hypot` and the raster's `cos`/`exp`. The documented guarantee is the same build on the same
  platform.
- **`load_replay` doc:** it claims the fingerprint is checked before the region is mapped. In fact
  the region is mapped first; the "verified before any command" guarantee still holds.
- **Remaining minor findings** (test polish, small refactors, CI `--locked` and permissions) are
  tracked only in git history and the per-task review notes. None affects correctness.
