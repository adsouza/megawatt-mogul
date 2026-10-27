# Frame the selected grid zone on the map

Selecting a zone in the top bar, commitment picker or forecast inspection should frame its
physical AC component. Bounds include local cities, plants and AC lines, without following an
HVDC connection into another zone. Preserve the viewing angle and account for the actual map
viewport and legend. All zones restores the regional overview.

Queue presentation-only focus requests in the existing action dispatcher and consume them once
the map surface is available. Manual camera motion after framing must persist. Reuse the plant
inspection request mechanism so the most recent selection wins; blocked/hidden maps defer the
request and session reset discards it. No simulation or save-format changes are needed.

- [x] Reproduce missing camera movement through the real zone picker.
- [x] Fit padded local bounds, preserving camera angle and avoiding the legend.
- [x] Handle all zone-selection entry points and restore the overview for All zones.
- [x] Verify framing properties, small-window interactions, manual motion, focus precedence,
      existing plant inspection, snapshot immutability and lint.
- [x] Native launch smoke.

The real picker regression initially failed because the camera target stayed at the regional
origin. The final focused run passed 134 game checks, including camera projection properties,
every SGCC starter zone at 1024×640 with 100%/125% UI zoom, top-bar and forecast actions,
deferred framing during dialogs, stale IDs, single-bus inactive zones and HVDC isolation.
Repeated frames preserve manual camera motion and reuse terrain/assets without simulation
commands. Workspace clippy with warnings denied and formatting/diff checks passed.
A native 120-frame launch exited successfully with an isolated temporary save directory.
