# Automatically scaled regional milestones

Keep three familiar objectives and derive their targets from each region's immutable starting
scenario. Connect half the remaining cities (rounded up), and serve a peak halfway between the
starter grid's annual peak and the entire region's annual peak. Compute simultaneous demand at
every minute of each month in the starting year, respecting local time zones and climate profiles.
Use readable whole-GW targets when possible, preferring the nearest multiple of 2 or 5 GW for
midpoints of at least 10 GW, and falling back to whole MW for small regions. Clamp
rounding within the attainable interval. The clean-season milestone retains the regional calendar,
including India's shorter winter and longer monsoon.

Compute the targets once at creation and freeze them in the projection. Display, progress,
achievement checks and toasts all use that same set. Never require growth beyond all authored
cities to reach a target. Already-earned milestones retain their original target and date.

Portable schema 10 appends the frozen target set and a MW-scale peak variant; readers retain
schemas 1–9. Upgrade older snapshots only after validating their original bytes, retaining all
progress and previously earned awards. Rules 33 fence command-only replays whose achievements
would differ. No region data or physical simulation rules change.
New targets already satisfied by saved progress are credited at the restored month without
replaying achievement notifications.

Validate regional targets for all eight shipped regions, proportional-size properties, achievement
thresholds and no duplicate awards, old-save migration and round trips, UI/toast consistency,
localization, clippy and formatting. Run focused checks only; no full local suite.

Current shipped targets:

| Region | Monthly peak | Connected cities |
|---|---:|---:|
| Eastern Interconnection | 30 GW | 14 |
| Western Interconnection | 9 GW | 13 |
| Brazil | 8 GW | 21 |
| Continental Europe–Maghreb | 35 GW | 22 |
| India–Nepal–Bhutan | 14 GW | 21 |
| IPS/UPS | 10 GW | 20 |
| SGCC | 80 GW | 18 |
| Southern China | 15 GW | 20 |

`cargo run -p infrastructure --example milestone_inventory` audits each region's starter/full
annual peak and the resulting targets after future region edits.

Validation completed: 104 focused application/infrastructure/game tests passed, including regional
scaling, schema-9 migration, all save-codec gates, normal UI resume and milestone rendering.
Domain schema-gate/property checks, workspace clippy with warnings denied, formatting, diff
whitespace checks and the 120-frame windowed launch smoke passed. No full local suite was run.
