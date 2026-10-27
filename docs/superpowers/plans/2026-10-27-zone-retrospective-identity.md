# Identical SGCC retrospective curves

The four added starter zones have identical 750 MW base loads, the same climate and clock,
and identical two-CCGT/one-peaker fleets. Demand-error noise is shared across the region, so
identical inputs produce identical minute-level demand, dispatch and forecast curves. This
was reproduced in a new seed-7 SGCC game and confirmed by restoring the latest local SGCC
save read-only: January 2027 has 1,440 identical demand/generation points and 24 identical
forecast points for Lanzhou, Changchun and Chengdu. The save was never written.

No retrospective data was being substituted between zones. A separate scenario shuts down
one local fleet and checks every archived demand/served point and every generation-stack value
against its own zone row. That zone then has zero served load and no generation stack, while
the other zones remain supplied. After the distinct regional fleets were authored, this
scenario moved from Lanzhou to Chengdu so that all of the test zone's generators are controllable.

Retrospective and history views now identify their selected grid zone beside the month picker,
or show All zones for company totals. Real widgets verify all zone headings and overview
navigation at minimum window size with 100%/125% UI zoom. Matching historical values remain
unchanged; no regional demand tuning, random draws, simulation rules or save formats change.

Validation: 106 focused chart, review, planning, localization and player-workflow checks passed.
Workspace clippy with warnings denied, formatting and diff checks passed.
Temporary diagnostic examples were removed after use.
