# Automatic commitment diagnostics implementation

- [x] Add failing regressions for equal-cost CCGT retention, report accounting, no-op logging,
  and native diagnostic persistence.
- [x] Return typed evidence from the existing domain search without changing its decisions.
- [x] Format and route developer diagnostics through the application pipeline and optional
  command-log sink diagnostic port; keep replay records separate.
- [x] Persist flushed native diagnostic sidecars and warn independently on write failures.
- [x] Queue reports through the client simulation bridge with session-generation protection.
- [x] Run focused planner/logging/client regressions, clippy, format checks and a native launch
  smoke for the changed system parameters.

Validation: 31 focused commitment tests and 40 pipeline/recording/egui parameter checks passed;
workspace clippy with warnings denied, formatting and diff checks passed. Native launch drew
120 frames and exited successfully using isolated temporary log/save directories.
