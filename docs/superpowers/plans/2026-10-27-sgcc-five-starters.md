# SGCC: five independently viable starter grids

New SGCC games inherit five small local AC grids: Beijing/Tianjin/Langfang, Fuzhou, Lanzhou,
Changchun and Chengdu. Each has commissioned synchronous generation and active customer service;
none starts with an HVDC link or depends on imports. Beijing remains the company headquarters.

Optional `start.island_homes` identifies one inherited anchor per starting electrical area.
Starter cities and fleet plants attach only to that area's anchor. Validation rejects missing,
duplicate, disconnected and unsupported anchors. The empty default preserves archived definitions
and every other region's original starting ID order and topology.

Service ownership and physical interconnection are distinct. Network reachability starts at the
home bus and persisted service-enrolment nodes. Its root set is a derived cache rebuilt from
`activated_service`; schema-5 captures already contain all needed information. Existing source
bytes, load histories and saved assets remain unchanged. Rules 27 records the starting semantics.

- [x] Author and validate the five compact fleets and local demand weights.
- [x] Check each island through all 12 months on seeds 0–7 with no imports or planned outages.
- [x] Verify isolation from a Beijing shortage, snapshot continuation and journal replay.
- [x] Preserve archived games and update UI regressions for the five inherited grids.
- [x] Run focused checks, clippy, formatting and isolated native smoke.

The four added territories each have a 750 MW base load and three plants (two CCGTs and one
peaker), using the unchanged catalog: 1,200 MW maximum / 340 MW minimum per local grid. The
existing Beijing starter territory/fleet and company cash are retained. All 19 initial plants
and their local AC lines are inherited assets. The forecast picker keeps all five inherited
islands reachable at 1024×640 and 125% zoom, with direct shortcuts for urgent grids.

Validation: 110 selected domain checks, 128 persistence/loader/SGCC checks, 44 application
checks, 108 selected game checks and 24 final compact-fleet checks passed. After boxing the
region-loader enum payloads to satisfy clippy's size check, 94 loader/persistence regressions
passed again. Workspace clippy with warnings denied, formatting, diff checks and a native
120-frame launch with an isolated save directory passed. The final three-plant added grids pass all
12 months on seeds 0–7 with zero imports, exports, planned outages or unserved demand. A copied
personal schema-4 SGCC save retains its original two-island topology and city weights, and its
source-schema state bytes round-trip exactly. The source save was not written.
