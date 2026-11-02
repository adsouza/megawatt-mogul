# Operational crystal ball

Replace capacity-based reassurance with monthly representative-day supply checks. Reuse the
existing water-aware hydro, feasible battery charging/discharge, seasonal renewables and HVDC
planning queries. Assess shortages locally before aggregation. Keep capacity statistics secondary.

The current Planning/Operating month uses the daily forecast's settings and weather. Future months
use mean seasonal weather, commissioned connected orders and all-day automatic dispatch; Review
starts next month. These are expectations, not guarantees against noise, failures or delays.

Cache immutable monthly results, including minute curves, by session revision and generation.
Reuse baseline months across city choices. Execute on a native background worker, with cancellation
between work units; browser jobs yield to the event loop between monthly work units. Discard stale
results, bound the cache, and show a dismissible loading state instead of old green results.

Show worst forecast balance, shortage duration and unserved energy, with red for any meaningful
local shortage. Always plot baseline peak demand, including when no city is selected; selecting
a city adds its peak-demand overlay. Show total and accredited capacity beside demand in the main
graph, with operational balance on a separate scale below it. Use cyan for baseline demand/balance
and reserve yellow for the selected-city overlays. Explain beside the hourly graph why it differs
from accredited capacity minus peak demand, and list accredited headroom separately in the table.
Omit existing capacity, scheduled additions and total capacity from the table. Retain the
all-available-capacity planning assumption.
Month buttons open daily demand/net supply, generation, charging/discharge and
stored deliverable energy. Escape returns from the daily view, then closes the outlook; popups
take precedence. Update tutorial language and localization.

Validate pure/deterministic queries, current-month daily agreement, empty storage, constrained
hydro, construction timing, local shortages, cache reuse/invalidation/cancellation and real UI
clicks/keyboard. Run focused checks, formatting, clippy and a windowed launch smoke only.

Validation: 10 focused domain tests and 41 game/localization tests passed, including current-month
daily agreement in all eight shipped regions, resource constraints, local shortages, construction,
cache ownership and reuse, real month-button clicks, loading cancellation, popup precedence and
maximum zoom. Workspace clippy, formatting, wasm compilation and the 120-frame native launch smoke
passed. Replaying the April 2028 India recording read-only reproduced the 111.235 MW shortage at
23:00 and confirmed exact daily-forecast agreement without changing session state. No full local
test suite was run.
