# Automatic commitment diagnostics

Record why each automatic commitment run trims or retains thermal plants, including feasible
all-day shutdowns with equal estimated fuel/startup cost and minimum-generation surplus.

The domain returns a deterministic, pure report from the existing bounded search: reference
and final scores, demand cushion, tolerances, per-plant candidate counts, conservative capacity
screening, search-limit exclusions, evaluated schedules, comparison scores and coverage checks.
Margins distinguish the preserved all-day floor from the full cushioned demand. A report does
not evaluate extra candidates, alter commitments, consume randomness or change save schemas.

The application formats developer diagnostics with month, region, currency, seed, journal
position and changed-plant count. Equal-score explanations state that shutdown brings no
estimated saving, fixed maintenance applies either way, and retaining capacity preserves
reserve. Scores are whole-fleet representative-day estimates, not realized costs.

The client queues diagnostics alongside the planner commands, including no-op runs. The
simulation bridge checks session ownership and records the report through the command
pipeline. Native recordings create `<session>.log.diagnostics.log` lazily in `--log-dir`,
beside the replay log; snapshots remain in `--save-dir`. Each report is flushed. Diagnostics
also go through info-level tracing. Replay logs retain their existing format and contents.
Diagnostic write failures warn without changing gameplay, commands or replay-log health.

Verify equal-cost retention, cheaper trimming, coverage/screening and bounded-search reports,
purity/determinism properties, no-op logging, disk persistence/replay independence, write failure,
and the UI-to-recording path with stale-session isolation.
