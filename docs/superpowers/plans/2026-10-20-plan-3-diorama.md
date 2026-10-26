# Megawatt Mogul Plan 3: Interactive 3D diorama

**Date:** 2026-10-20
**Status:** Implemented 2026-10-21. Acceptance evidence and native automation limits are recorded below.
**Dependency:** the playable Plan 2 shell. Plan 2b save/resume precedes
this plan, as confirmed by the user on 2026-10-21.

**Goal:** Replace the flat in-game map with a readable, interactive tabletop diorama, preserving
every existing planning and operating action. Implement the world, overlays, camera, picking and
optional development tools specified in [§8](../specs/2026-10-18-megawatt-mogul-design.md#8-presentation-cratesgame).

Read `AGENTS.md`, `ARCHITECTURE.md`, `TESTING.md`, spec §§4, 5.7, 8, 15–17 and the Plan 2
follow-ups before implementation. Keep simulation outcomes, region tuning, rules version and log
format unchanged. Use the pinned Bevy 0.20 and bevy_egui 0.43.1 sources for API details.

## Verified starting point

- `hud/map.rs` owns the flat terrain texture, resource-layer controls, markers, selection and
  construction labels. Its pure colours, localisation and suitability queries are reusable.
- `HudArea` records this frame's remaining logical screen rectangle after resizable panels claim
  space. The map draws last among panels; Review, prompts and other floating windows draw later.
- `Selection` is shared by the inspector, build menu and Operating side panel. Selection and layer
  changes go through `UiAction` and `dispatch`, with reads through `ActionSink` getters.
- `main.rs` currently spawns the first `Camera2d`, which receives the primary egui context.
  bevy_egui derives its screen rectangle from that camera's physical viewport. Constraining that
  same camera to the map would also constrain and move the HUD.
- Plant and line views expose IDs and construction state. City views expose connectivity;
  `CityDetail` supplies base demand and the copper-plate share of served load. No line-flow values
  exist. Terrain is a row-major grid of land, height and resource ratios.
- Bevy currently has rendering/core-pipeline features, but no PBR or post-processing stack. All
  game integration tests share `tests/game.rs`; no additional Bevy-linked test binary is allowed.

## Implementation boundaries

- Add `world/` modules for pure geometry, camera mathematics, render systems and view identity.
  Domain/application remain free of rendering dependencies and presentation state.
- Keep a full-window UI camera with an explicit primary egui context, and a separate world camera
  restricted to the diorama rectangle. Verify actual compositing order and clear behaviour in a
  native window before relying on this arrangement. World rendering never changes HUD coordinates.
- Derive one immutable world model from public queries after simulation commands. Track
  `PlantId`, `CityId` and `LineId` with ordered entity maps. Reconcile additions/removals and update
  changed visuals without rebuilding terrain or spawning duplicate entities every tick.
- Presentation animation may use frame time; it must never feed time, randomness or resource
  edits back into the simulation. Every simulation action remains a `PlayerCommand`.
- Do not add direct aggregate mutation through an inspector. The optional tuning resource may
  adjust visual scales and colours live. Simulation tuning remains region data applied to a new
  session through the existing region-loading path; document this boundary in the dev UI.
- Keep line building through the existing endpoint menu. Drawing lines directly on the map is
  an optional later interaction, not a prerequisite for replacing the map.

## Task 1: Terrain geometry and coordinate contract

**Add:** `crates/game/src/world/{mod,terrain}.rs` and tests in
`crates/game/tests/game/world.rs`, registered in `tests/game.rs`.

- [x] Write failing pure tests for flat and raised land, water, boundaries and both shipped grids.
  Define a single centred X/Z cell-to-world transform used by meshes, markers and picking. Map
  row zero consistently; reject out-of-grid coordinates rather than selecting a saturated cast.
- [x] Generate indexed low-poly terrain geometry from cell heights, with face normals, opaque
  vertex colours and a tabletop base. Define the visual height scale separately from plant tuning.
  Ensure adjacent cells meet, water is visible and face winding is consistent.
- [x] Property-test finite positions/normals, bounded indices, nondegenerate faces, complete cell
  coverage and agreement between surface height and marker/picking coordinates. Handle empty
  synthetic grids without panicking, even though shipped regions are validated.
- [x] Reuse existing resource/suitability colour functions through `world::region_terrain`.
- [x] Cache terrain geometry by session identity and terrain content when the renderer is added; changing a layer updates colours rather than duplicating assets.
  A new game with the same region ID must not retain an old session's geometry.
- [x] Run the focused world tests before and after minimal implementation, then game tests.

## Task 2: Camera, viewport and input mathematics

**Add:** `world/camera.rs`; extend world tests and real egui layout tests.

- [x] Specify orbit, pan, zoom and reset gestures with localised help. Distinguish a click from a
  drag so releasing an orbit/pan does not select or order a build. Keep keyboard text entry and
  existing Space-to-pause behaviour intact.
- [x] Test camera fit for both regions and narrow/wide map areas, bounded distance/tilt/pan, and
  finite transforms through repeated extreme inputs. Keep the camera above the terrain.
- [x] Convert the exact map response rectangle from egui points to a clamped physical viewport
  using egui's actual pixels-per-point. Test fractional scaling, edge rounding, resize, minimised
  windows and zero-sized areas. Never create an invalid zero-sized render viewport.
- [x] Read current-frame HUD geometry after panel layout. Keep viewport, projection and ray
  conversion on the same rectangle and camera pose. Gate camera input on that rectangle and on
  modal/legend/control ownership; scroll over a panel must not zoom the world.
- [x] Move camera setup out of `main.rs` into the library plugin. Keep full-window menu rendering
  and the primary egui context stable across menu/game transitions.

## Task 3: Rendered world and lifecycle

**Add:** `world/{render,model,plugins}.rs`; update game features and `GamePlugins`.

- [x] Enable only required PBR/light/post-process features. Add headless lifecycle tests before
  integration: entering creates one world; leaving destroys its entities/assets; another region
  starts fresh. Keep the pure/core test setup independent of a GPU and window.
- [x] Add warm lighting, soft shadows, a tabletop base and procedural meshes distinguishing all
  six plant kinds. Draw city block clusters scaled by base demand and transmission connections
  following the terrain. Reuse mesh/material assets rather than allocating them each frame.
- [x] Preserve under-construction visibility and months-left labels, plus commissioned, offline,
  disconnected and selected states. Reconcile entities by stable domain ID after commands and
  construction completion. Test no duplicates, stale mappings or asset growth across many ticks.
- [x] Integrate the transparent map interaction surface and world viewport, replacing the flat
  map system only when the 3D view is usable. Retain the resource controls and relevant legend
  semantics; remove obsolete flat rendering code/tests only after equivalent coverage exists.
- [x] Run a native launch smoke immediately after plugin/system-parameter integration to catch
  B0002, missing assets, duplicate egui contexts and render graph/feature failures.

## Task 4: Picking and readable map annotations

**Add:** `world/picking.rs`; extend `UiAction` only where necessary.

- [x] Test rays against actual rendered terrain and object bounds, including raised terrain,
  water, near/far edges and objects sharing a cell. Resolve nearest visible hits deterministically;
  return existing `Selection` values through `dispatch`. Terrain hits select the correct cell.
- [x] Test real egui passes with panel resize, menus, Review, prompts, legends and game-over
  overlays: covered clicks/scrolls/drag gestures must not reach world actions. Account for current
  pass ownership rather than a previous frame's pointer-over-UI flag.
- [x] Project labels through the world camera into the HUD coordinate space. Use opaque backing
  and existing text contrast rules; flip/clamp anchors near viewport edges and skip occluded or
  behind-camera labels. Keep construction information accessible through the existing cards.
- [x] Preserve click-to-inspect and build-menu workflows in both regions. Test a selected city's
  details, a selected plant's controls and a selected cell's site suitability through real actions.

## Task 5: Phase overlays and visual feedback

**Add:** `world/overlay.rs`; add localised controls and coverage keys.

- [x] Test pure overlay models for Planning, Operating, Review, blackout, disconnection,
  construction and partial shedding. Phase presentation follows the domain phase; Bevy's mirror
  must not cause a stale frame to expose the wrong action or data.
- [x] Planning shows terrain/resource/suitability layers. Operating retains the lit terrain and shaded
  buildings beneath plant output pillars and city supply status. A hotkey hides the overlay; keep clear
  localised help and an accessible button. Define restoration on phase changes explicitly.
- [x] Under copper plate, connected cities share the same served proportion. Visualise that
  proportion honestly; do not invent a geographically rotating city outage. Lines may pulse when
  energised, but must not imply direction, congestion or MW flow.
- [x] Add restrained depth of field and bloom where supported. Keep labels/HUD outside the
  blurred world pass and ensure the terrain remains usable at the minimum map size. Provide a
  graceful effect-off path for unsupported render configurations.
- [x] Test overlay toggles, entry defaults, material reuse and updates without simulation changes.

## Task 6: Optional development tools and acceptance

- [x] Add the optional `dev` feature and compatible `bevy-inspector-egui` dependency. Put
  `DevToolsPlugin` behind it; register the visual `TuningConfig` for inspection only in that build. Confine editable data to the
  presentation boundary described above; normal builds have no inspector or extra controls.
- [x] Check both normal and `--features dev` builds, tests and Clippy. Inspector windows must
  capture pointer/keyboard input and use readable text. Verify dependency versions agree with
  the pinned Bevy/egui stack before adding the plugin.
- [x] Exercise both shipped regions through Planning, Operating, Review and game over in a native
  window. Inspect 1024×640 and larger sizes with fractional UI scaling, selection, camera zoom/reset,
  overlay hiding and menu transitions. Cover every resource layer, commissioning and all drag
  gestures with real egui passes and headless lifecycle tests; record native input limits below.
  Do not terminate an existing unsaved player session to conduct the playtest.
- [x] Compare a fixed command script's state hash and reports before/after the presentation work.
  If Plan 2b is available, load each phase and verify world reconciliation and paused entry.
- [x] Update `ARCHITECTURE.md`, `README.md`, `TESTING.md` and superseded Plan 2 follow-up notes.
  Document gestures, dev feature, render fallbacks and the copper-plate presentation limits.
- [x] Complete `cargo build --workspace`, `cargo nextest run --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `git diff --check` and `cargo run -p game -- --exit-after-frames 120`.
  Review the complete final diff and native behaviour before marking this plan complete.


## Acceptance evidence (2026-10-21)

- [ ] Verify native orbit/drag feel on a computer-use surface that delivers pointer motion.
  Reconciled 2026-10-26: the real-egui regressions and native scroll/reset checks recorded below
  pass, but do not close the documented native drag limitation.

- Production uses the 3D world. The existing flat draw adapter remains available to HUD-only
  consumers and its tests; it is not a player-selectable fallback. Session identity controls the
  terrain cache, resource-layer changes update mesh colours, and leaving releases owned assets.
- Native macOS playtests used isolated log/save directories and both shipped regions through
  Planning, Operating, month Review and game over. The game-over month-details/back/menu flow
  works. No existing player session or preferences were used.
- Checked 1280×800 at 100% and 1024×640 at 125% UI zoom. Narrow maps use a compact layer
  selector; the explicitly opened legend scrolls within the map. Projected labels have opaque
  backing, clamp to the viewport and suppress overlaps/occluded labels.
- Native checks covered terrain and Sunshine layers, cell selection and plant ordering, a
  12-month construction label progressing to 11 months after review, Operating output pillars,
  effects-off, overlay hiding, Control room restoration, scroll zoom, reset and menu transitions.
  An Eastern save resumed paused at exactly 12:56. Western construction restored correctly.
- The development binary's F12 inspector opens without a registry panic and exposes visual
  tuning/render entities only. Normal builds have no inspector. Unsupported depth textures
  disable Bevy's depth-of-field pass; the effects toggle also removes both post-processing passes.
- CUA's native drag operation did not deliver pointer motion in this app. Orbit and primary,
  secondary and middle drag capture are covered by real egui input regressions; native drag feel
  remains a manual verification limitation. Native scroll zoom and reset were verified.
- Headless tests cover viewport/DPI rounding and minimisation, camera bounds and fitting,
  terrain/object/pillar picking, labels, all layers, narrow/zoomed real egui layouts, current-pass
  popup/modal/legend ownership, wheel inertia from panels, missing-localisation boot screens,
  session cleanup, stable IDs and bounded asset counts through 12 months and commissioning.
- A fixed 12-month public-command script produces identical session checkpoints and reports in
  clients with and without the presentation plugins. Save/resume tests also reconcile the world.
  Domain/application/infrastructure source, shipped region tuning, rules version and log format
  are unchanged.
- Final checks: workspace build; 1,182 normal tests and 1,183 development tests; normal and
  development Clippy with warnings denied; formatting and whitespace checks; native 120-frame
  launch smoke. See `TESTING.md` for the repeatable commands and coverage boundaries.


### Visual refinement (2026-10-21)

The Operating supply overlay retains the base diorama lighting, terrain colours and building
shadows. Only output beams and the selection marker bypass lighting. City emission is restrained
so it does not wash out face shading, and the surrounding backdrop uses a lighter slate colour.
A regression checks both shipped regions for lit terrain/buildings, unchanged terrain colours and
sunlight, and overlay toggles that preserve simulation checkpoints and asset counts.

Validation for this refinement: the new regression failed before the fix, then the full workspace
suite passed (1,183 tests), as did all 24 world tests with `dev`, normal/development Clippy, formatting
and whitespace checks. Native inspection of the Eastern midday save while advancing at 8× confirms
coloured terrain, distinct building faces/shadows and supply indicators remain visible.
