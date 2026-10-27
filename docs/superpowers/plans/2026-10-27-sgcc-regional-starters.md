# Regional character for the five SGCC starter zones

Keep the five inherited local grids independent and viable while replacing the four cloned
starter territories. Each zone gets a distinct fleet composition and a distinct normalized
daily demand shape in every represented month. All cities in a zone share its new climate;
China Standard Time remains common to all five.

| Zone / inherited territory | Demand character | Inherited fleet |
|---|---|---|
| North–Central / Beijing–Tianjin–Langfang | Existing mixed industry and evening demand | 2 coal, 3 CCGT, 2 peakers |
| East / Fuzhou | Dense coastal load; high overnight floor; summer cooling | Nuclear, CCGT, peaker, solar |
| Northwest / Lanzhou | Flatter industry; later evening peak | Coal, CCGT, wind, solar |
| Northeast / Changchun | Winter heating and morning demand | Coal, CCGT, peaker, wind |
| Southwest / Chengdu | Evening peak and summer cooling; seasonal water availability | 2 hydro, CCGT, peaker |

The existing national plant catalog, company cash, climate semantics, noise draws, controller,
rules version and save schemas stay unchanged. This is region-data authoring: new games get a
new region fingerprint; archived definitions and existing histories retain their original data.
Numeric load weights and profiles are explicit game tuning, not measured regional load curves
or a claim that the company owns the real generating fleet.

- [x] Reproduce cloned fleets/profiles with failing acceptance tests.
- [x] Author distinct daily/seasonal profiles and valid local generating sites.
- [x] Verify characteristic sources actually generate and retrospective curves differ.
- [x] Verify each zone through all 12 months on seeds 0–7 without imports, planned outages,
      unserved demand or planning shortfall/minimum-generation warnings.
- [x] Check first expansion, save compatibility, HUD regressions, formatting, clippy and launch.

Validation: 130 focused infrastructure checks passed, including the complete local-grid
year on seeds 0–7, SGCC first expansion, archive/codec/monthly-chart regressions, restore and
journal replay. A further 132 focused game checks passed with the actual nuclear, hydro, wind
and solar fleets. The first-expansion and starter reserve checks now calculate each connected
city's own climate/clock demand, rather than reusing the first city's shape for the entire region.

Read-only restoration of an existing SGCC save preserves its exact checkpoint, all 41 saved
plants, original 19-plant starter definition and original 750 MW Fuzhou load. The installed
new-game definition has 23 inherited assets and a 1,300 MW Fuzhou territory. The source archive
was read back unchanged, and the temporary diagnostic example was removed.

All 64 shipped-region first-expansion cases passed after the climate-aware peak helper change.
The complete SGCC local-grid year passed again with stronger checks: zero over-frequency trips
and every zone independently within its regulatory frequency limit. Workspace clippy with
warnings denied, formatting and diff checks passed. A native 120-frame launch exited 0 using
an isolated temporary save directory.
