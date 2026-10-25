# Globe region picker

**Status:** Implemented 2026-10-25; presentation only, rules 17 unchanged.

**Scope:** Replace the new-game flat overview with a rotating globe, independently of Plan 7's
region research/data work. No simulation rule, save schema or region tuning change.

- [x] Preserve cached region outlines, exclusions, colours, home markers, seed validation and
  keyboard-accessible region cards.
- [x] Cache context/region meshes as unit-sphere faces; project with 22° north-pole tilt toward
  the viewer. Clip the hidden hemisphere and invert only the front disc for picking.
- [x] Rotate once per 24 real seconds. Horizontal drag takes direct control in either direction;
  automatic rotation stays paused while held and resumes two seconds after release.
- [x] Reverse the original automatic direction (visible surface now moves left to right), keeping
  its 24-second period and manual drag behavior. Use two card columns in a sufficiently wide
  right-hand panel, retaining a single column at smaller sizes and row-major keyboard navigation.
- [x] Paint the active hover title after every region fill and home marker, with a regression
  proving that the Western fill cannot obscure an Eastern title positioned across it.
- [x] Let card bodies select/highlight a region without starting the game. Animate its geographic
  bounds' longitude midpoint to the front meridian along the shortest arc in 0.6 seconds, hold
  two seconds, then resume slow rotation. Keep the fixed tilt, let manual dragging cancel the
  animation, and keep title buttons as the keyboard-accessible start actions.
- [x] Test pure geometry/motion and actual egui drag/click behavior, including selection after
  rotation, keyboard navigation, small windows, disabled entries and contrast.
- [x] Review the native globe, exercise drag/idle behavior, and run the applicable validation.

Implementation uses egui meshes and presentation-only transient state. It adds no camera, Bevy
resource borrowing, dependency or simulation command. The coarse context coastline retains its
documented omission of Antarctic detail; selectable regions still use their authored geography.


At the globe validation checkpoint, focused motion/projection/widget checks passed, followed by **1,670 workspace tests,
0 skipped**. Clippy with all targets and `-D warnings`, formatting and diff checks passed. A
separate native preview at 3072×1760 verified the globe, fixed pole tilt, automatic rotation,
both manual drag directions and continued picker state after release. Controlled-time widget
checks pin the two-second idle delay and selection after a quarter turn. Projection and timing
properties check frame partition independence, visible-point inversion and hemisphere clipping.
The high-resolution cached mesh keeps coastline edges smooth on large windows; view trigonometry
is calculated once per frame. Existing region cards, exclusions and contrast checks still pass.

Native preview saves/logs are isolated in a temporary directory. No Plan 7 research/data files or
existing player saves were edited. The preview remains open for inspection.

The native 120-frame smoke exited 0. Concurrent Plan 7 edits subsequently appeared in the
infrastructure raster/loader tests; the final workspace format check reported formatting there.
Those in-progress files were preserved. Globe Rust files passed their own format check; the globe
changes were held uncommitted at that checkpoint to keep them separate from parallel Plan 7 work.

Follow-up UI revision: reversed the automatic direction while preserving the 24-second period;
added responsive card columns with row-major keyboard traversal; moved home markers and the active
hover title above every geographic fill. The hover-layer regression failed before the overlay
change and passes afterwards. All 22 focused globe/picker tests pass, including the updated
one/two-column eight-card keyboard workflow; the game build and game/all-target clippy pass.
The isolated native preview was refreshed and its two-column layout checked at 3072×1760.

Card selection follow-up: all 26 focused globe/picker tests pass, including hidden-region body
selection, geographic midpoint alignment, disabled-card rejection, two-second post-arrival hold,
manual interruption, shortest arcs in both directions across the longitude seam, and frame-partition
properties. The game build, game/all-target clippy with warnings denied, owned-file formatting and
diff checks pass. The isolated native preview executable was refreshed and the picker renders.
Native click verification was blocked by the computer-use surface reporting that the user changed
the app; the controlled-time real-widget tests exercise the complete interaction without relying
on that native click. No simulation state or save format changed.

User acceptance: the refreshed native preview looks good and works well (2026-10-25). The user
authorized a commit containing only the globe changes and their tests/documentation.

Commit checkpoint: all **1,680 workspace tests passed, 0 skipped**, in 173.488 seconds.
Workspace formatting, workspace/all-target clippy with warnings denied and diff checks pass.
The commit is scoped to the globe implementation, widget/motion tests and accompanying docs.
