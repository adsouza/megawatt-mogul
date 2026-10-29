# Automatic HVDC control

Implement default automatic control for new and inherited HVDC links, retaining an independent
manual hourly schedule and the existing rest-of-day override.

- Add loss-aware deterministic network dispatch: reserve local customer load, share remaining
  generation across available paths to cover shortages, then replace expensive flexible output
  where delivered marginal cost is lower. Stable island, plant and link IDs break ties. This is
  a merit-order heuristic, not an optimal-power-flow solver.
- Use one pure hourly transfer planner for local forecasts, shortage warnings, activation and
  connection assessments, and operating hydro/storage/outage planning. Reserve the worst local
  requirement within each hour; use existing water- and inventory-aware supply forecasts.
- Dispatch live transfers and eligible AGC generation together, within generator and link ramps.
  Respect fixed manual transfers, temporary overrides, synchronous terminals, blocking and the
  existing latched under-frequency runback. Account for unavoidable ramp-down tails before
  allocating more power. Storage stays local and is never offered as nameplate export capacity.
- Expose Automatic/Manual controls and the effective profile in the HVDC Dispatch tab. Retain
  manual text/Enter validation and offer Resume automatic after an operating override.
- Advance rules 30 to 31 and portable state schema 6 to 7. Earlier saves migrate links to Manual,
  preserving their schedules, overrides and source-schema bytes; archived region data is unchanged.
- Validate independent energy/rating/determinism properties, physical full-day operation, mode
  changes, atomic rejection, migration, snapshot continuation, journal replay and real HUD widgets.

## Implementation and validation

Implemented the shared planner and minute dispatch in `grid/power_system/transfer_control.rs`,
mode/state handling in the domain and journal codecs, and controls in the existing HVDC panel.
Automatic transfers and generator outputs share the synthetic midnight boundary. Live dispatch
can release curtailed renewables and respects the existing battery taper using physical dispatch
and projected inventory after transfers, preventing exported power from being counted as charge
headroom. Existing manual source-schema bytes are unchanged.

134 focused checks passed, including eight seeds × twelve SGCC months with no unserved load,
no over-frequency trips, acceptable island frequency performance, conservation and exact journal
replay. Checks also cover snapshot/migration/transport, manual override widgets and keyboard
submission, shared capacity, resource limits and localization. Workspace clippy with warnings
denied, formatting and diff checks passed. The full local test suite was not run.

The final repeat passed all 134 selected checks. The isolated native launch opened the window,
rendered 120 frames and exited 0.
