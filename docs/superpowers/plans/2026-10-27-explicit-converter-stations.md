# Explicit converter stations and HVDC construction

Separate local AC expansion from inter-island HVDC projects. Each HVDC project owns two
explicit converter stations with stable identities `(link ID, terminal A/B)`. A station is
co-located with its selected local AC bus (city or plant switchyard); its bus attachment,
construction and capital are included in the existing regional converter terms. The route
connects those stations. It does not join the AC islands or activate customer service.

Stations share their owning project's construction/cancellation lifecycle. They cannot become
orphaned or accidentally synchronise electrical areas. Generation support is assessed across
the whole local AC island, not only at the selected site.

- [x] Add typed station identities and local grid attachments to the DC domain model.
- [x] Preserve source-schema bytes and all existing link IDs, flows, schedules, runback,
      construction, economics, island identities and activation decisions on read.
- [x] Give local AC lines and HVDC projects separate construction sections and selections.
- [x] Show stations and local attachments on the map and in project details.
- [x] Cover legacy wire migration, station lifecycle, UI construction and save continuation.
- [x] Run focused checks, clippy, formatting and isolated native launch smoke.

Portable schema 5 remains sufficient: link ID and endpoint orientation define each station's
identity, and the existing endpoint bytes encode its local AC bus attachment. Readers wrap
those legacy attachments in explicit station objects; no history, IDs, money or source files
are rewritten. Rules 26 distinguishes the new domain representation for diagnostic hashes.

Validation: 36 focused domain, 53 persistence/economics, 44 application and 53 game checks passed.
The focused game checks cover separate tools, station details, activation safeguards, marker
selection and Fluent keys. A copied personal SGCC schema-4 snapshot restored with identical
source-schema bytes: links 96 and 97 became four stations; the former retained its zero schedule
and in-service status, the latter retained its final construction month. Chifeng and Yinchuan
remain inactive. The original save was not written. No golden fixtures were regenerated.

The final game selection includes keeping the HVDC tool active after selecting a city/plant,
station marker selection without a simulation write, and preserving visible DC routes when a
terminal is outside the viewport. Workspace clippy with warnings denied, formatting and diff
checks passed. The isolated native launch drew 120 frames and exited 0. The full local suite
was not run. The unrelated design-spec edits were left untouched.
