# SGCC inherited HVDC corridors

The five viable local starter grids now inherit commissioned HVDC corridors between every pair
of areas sharing a land-cell boundary: North–Central/East, North–Central/Northwest,
North–Central/Northeast, North–Central/Southwest and Northwest/Southwest. Endpoints are the
already served starter homes. Each uses the existing 1,000 MW regional catalog, ramp and losses.
Schedules start at zero; the operator can schedule exchanges immediately.

- Add optional `start.hvdc_links` city-ID pairs to region data and the typed starting scenario.
- Validate served endpoints, different areas, catalog availability, unordered uniqueness and
  representable route/converter costs, including total inherited capital.
- Create in-service links after local cities/plants/AC lines, preserving ID order when absent.
- Value each as legacy capital, with no construction delay, instalments or upfront cash charge.
- Preserve saved topology through archived definitions; record new-game semantics as rules 30.
- Check adjacency coverage, independent local operation, transfers, archive continuation, replay,
  invalid-data properties and the existing HVDC UI workflows.

Validation: 123 selected simulation, loader, archive and replay checks passed, including the
eight-seed, twelve-month local-grid scenario and adjacency coverage derived from the area masks.
All 17 HVDC HUD tests passed, including real widgets, Enter/Escape, map selection, cancellation
and minimum-size inspection. Workspace clippy with warnings denied, formatting and diff checks
passed. The full workspace test suite was not run.
