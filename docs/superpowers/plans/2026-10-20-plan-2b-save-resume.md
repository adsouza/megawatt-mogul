# Megawatt Mogul Plan 2b: Save and resume

**Date:** 2026-10-20  
**Status:** Implemented 2026-10-21; verification is recorded below.
**Depends on:** the implemented Plan 1/2 simulation and game shell, including the current HUD fixes.  
**Precedes:** Plan 3 (3D diorama, overlays, camera, picking and dev tools).

**Goal:** A player can save any accepted game boundary, close the app, load the save and continue
with identical simulation state and historical read models. Manual slots and a separate month-close
autosave are both included, as selected by the user. The binding contract is spec Amendment A6,
[§17](../specs/2026-10-18-megawatt-mogul-design.md#17-amendment-a6-saveresume-before-plan-3-2026-10-20).

Read `AGENTS.md`, `ARCHITECTURE.md`, `TESTING.md`, spec §§3, 5.7, 6.2–6.3, 7.1, 8.4 and 17, and
Plan 1's save/load follow-up before implementing. Preserve unrelated worktree changes. This plan
adds no simulation rule and keeps `SIM_RULES_VERSION == 6` and `LOG_FORMAT_VERSION == 1`.

## Current code and the gaps this plan closes

| Current source | Verified behaviour | Required change |
|---|---|---|
| `crates/application/src/game.rs` | `Game::start_new` loads once and initialises the sink before constructing a session. `execute` applies projections after each successful command. | Retain initial replay inputs and successful command history; export a checkpoint and restore a complete `Game`. |
| `crates/application/src/game.rs::replay` | Returns `GameSession` only; discards replay events. | Share replay validation/core stepping while rebuilding projections per command. Preserve the existing public replay entry point. |
| `crates/application/src/projections/mod.rs` | `apply_all` reads the session after the command, not just its event stream. | Restore in original command batches; folding against the final session is incorrect. |
| `crates/application/src/pipeline/command_log_layer.rs` | A failed disk append is reported but the command has already succeeded. | Keep the save journal complete independently of this sink. |
| `crates/infrastructure/src/command_log/file_log.rs` | Raw logs are append-only RON lines with a separate region archive; individual appends flush but do not fsync. | Keep diagnostic logs; add an independent, atomic, self-contained save format/store. |
| `crates/infrastructure/src/command_log/dto.rs` | Owns serde mirrors and validating command/header conversion. | Reuse those conversions inside the save codec without giving serde to inner layers. |
| `crates/domain/src/session/mod.rs::state_hash` | Hashes Debug output; harmless source changes may change it. | Pin the save's replay build/platform as well as its rules version; check the final hash. |
| `crates/game/src/sim/{mod,driver}.rs` | At most one tick batch is in flight; `OnEnter(Operating)` unconditionally sets 1×. | Coordinate capture/install barriers; ensure a resumed Operating game stays paused through entry hooks. |
| `crates/game/src/hud/{menu,prompt,action}.rs` | No save/load actions; Menu confirms leaving; dispatch is the action writer. | Add save/load dialogs and save-before-menu handling through the same pattern. |

## Implementation decisions

- Command checkpoints contain the full prefix from game creation; no aggregate DTO snapshots,
  parent-save references, event sourcing, raw-log import or save migrations in this release.
- `Game` owns an in-memory journal, separate from the best-effort disk log. Save works even after
  a diagnostic append has failed. New-game log initialisation remains required as today.
- One `.mgsave` file contains versions/compatibility, checksum, original header, original region
  bytes, commands, boundary verification and display metadata. Slot names never become paths.
- A manual slot is overwritten only after confirmation of its specific revision. A new game or
  resume gets a fresh recording run and automatic slot; a resumed source is not appended to.
- Save dialogs freeze play. Save success and load leave Operating paused; cancelling the dialog
  restores the prior speed only if the same active Operating phase still applies.
- Load is offered on the main menu. No in-game replacement, Continue shortcut, delete/rename UI,
  cloud sync or saving transient HUD state is required.
- Existing OS close behaviour stays immediate and unconfirmed. It does not save on exit. The
  in-game Menu offers Save and return, Leave without saving, and Stay.
- Encoding, I/O and replay run off the render thread; capture and candidate installation use
  serialised main-thread barriers. Jobs carry operation tokens; session-directed messages carry an
  entry generation. Stale work cannot affect a later session.

## Interfaces and ownership

Names below define the intended responsibilities; use the repository's existing export conventions.
All application values are serde-free and have typed errors rather than UI copy.

- **Application (`checkpoint.rs`, `save.rs`, `ports.rs`, `error.rs`, `game.rs`):**
  - `GameCheckpoint`: original `LogHeader`, byte-exact region source, ordered `Vec<Command>`,
    accepted-command count, expected state hash, and phase/date/minute/status summary.
  - `Game::checkpoint(&self) -> GameCheckpoint`: pure capture of a complete accepted boundary.
  - `Game::restore(...) -> Result<Game, RestoreError>`: consumes verified checkpoint/loaded region
    and a fresh sink; rebuilds session, projections, journal and command pipeline before return.
  - `SaveId`, `RecordingRunId`, `SaveRevision`, `ReplayCompatibility`, `SaveMetadata`,
    `SaveBundle`, `SaveListing`, `SaveTarget`, `SaveReceipt`, and typed store/restore failures.
  - `SaveTarget` distinguishes exclusive creation from overwrite with an expected revision.
  - `SaveStore: Send + Sync`: list with per-entry failures; read by opaque ID; write with an
    expected revision and return a confirmed receipt. Inject allocation/time at the adapter
    boundary, so tests choose stable IDs/times and application code never reads a wall clock.
- **Infrastructure (`save/{mod,codec,dto,file_store}.rs`, `build.rs`):**
  serde DTOs, version envelope, SHA-256, single-read region mapping, file revisions, atomic writes,
  per-slot locks and conservative build/platform compatibility. `FileSaveStore` receives its root,
  clock and ID source explicitly. Worker-friendly synchronous port methods are sufficient.
- **Game (`sim/persistence.rs`, `hud/saves.rs`, existing bridge/HUD modules):**
  `SaveDir`, store resource, request/result messages, pause and operation state, retained pending
  command queue, session generations, candidate installation, presenters and localised UI.
- **Domain:** no new fields, dependency, serde, save command or persistence-aware code.

## Task 1: Retained journal and complete application restoration

**Modify:** `crates/application/src/{game,ports,error,lib}.rs`; add `checkpoint.rs`/`save.rs` as
needed. Add `crates/application/tests/save_resume.rs`; reuse `tests/support`. Extend projection
query/equality test support narrowly if needed, without exposing aggregate internals.

- [ ] Write failing tests first and demonstrate RED with
  `cargo nextest run -p application save_resume`.
  - Capture initial Planning, modified Planning, mid-Operating, Review and game-over states.
  - Reject a command and assert neither journal length nor state/projections changed.
  - Fail a sink append **after** successful play began, issue more commands, capture and restore.
    The checkpoint includes commands before, at and after the disk failure exactly once.
  - Preserve every successful command's original batch, including `AdvanceTicks { n: 0 }`.
  - Rebuild all projections, including reports, history, regulatory state, lessons, milestones,
    warnings and the timeline. Include multiple months, December and construction commissioning.
  - Reject an invalid historical command with its index, wrong count/hash/summary, region ID or
    fingerprint, unsupported rules/log version and cancellation. No partially restored game escapes.
- [ ] Retain the exact `LoadedRegion` source/header at new-game creation. Append to the journal
  once after a successful handler return, independent of `log_health`; do not journal UI actions.
  Update log-failure doc comments to distinguish an incomplete recording from a complete save journal.
- [ ] Implement capture and a shared replay fold: initialise projections from the initial session,
  then apply each command's events immediately against its resulting session. Replay uses the core
  handler and no diagnostic sink, so it neither double-records history nor emits UI notifications.
- [ ] Validate the result before initialising the fresh branch sink. Initialise/archive its original
  inputs and copy the historical command prefix into it once, without replaying again. If any of
  those writes fail, return an error; do not return an unrecordable candidate. Attach the normal
  pipeline and full retained journal after successful seeding.
- [ ] Keep `application::replay` compatible for existing raw-log callers. Share validation/stepping
  code with restoration rather than introducing a second interpretation of commands.
- [ ] Add property tests over valid command prefixes with interspersed refused inputs: restore,
  continue with the same commands, compare state hashes, public views and every projection.
  Compare projection contents field by field/query by query; don't use Debug strings as the oracle.
- [ ] Round-trip a checkpoint made after restore. The journal is the original prefix plus the new
  commands, not a replayed duplicate or a delta dependent on a parent checkpoint.
- [ ] Run `cargo nextest run -p application` and domain dependency-rule tests. Keep outcome/rules
  snapshots unchanged; investigate differences instead of bumping rules to accommodate persistence.

## Task 2: Versioned self-contained save codec and replay compatibility

**Add:** `crates/infrastructure/src/save/{mod,codec,dto}.rs`,
`crates/infrastructure/build.rs`, `crates/infrastructure/tests/save_codec.rs`. Modify infrastructure
exports/manifests and application save value types as needed.

- [ ] Write failing codec/compatibility tests; run them before implementing.
- [ ] Define `SAVE_FORMAT_VERSION = 1`. Use a small stable RON envelope containing a version,
  declared payload byte length, SHA-256 checksum and payload string. The payload owns the bundle's
  DTOs, including exact region bytes. Hash the exact UTF-8 payload bytes, not re-serialised values.
  Read/validate the envelope version, length and checksum before decoding the version-specific
  payload/commands. Use existing header/command DTO conversion and reject non-finite/invalid values.
- [ ] Limit total file input/output to 64 MiB and commands to 1,000,000, with checked arithmetic
  and an explicit nesting limit. A limit failure is typed and cannot publish a truncated history.
- [ ] Generate `ReplayCompatibility` from spec §17.3's source/build inputs. Use sorted relative
  filenames and length-delimited bytes so concatenation cannot alias; exclude absolute paths,
  timestamps, rendering sources, generated output and assets. Register all source directories and
  build inputs for Cargo rebuilds, including additions/removals and relevant environment flags.
  Add `sha2` as a build dependency if the generator needs it; no new domain dependency.
- [ ] Keep fingerprint input collection separate from hashing, with fixture-based tests: backend
  source, compiler, profile, target, feature, flag and lockfile changes alter identity; checkout
  location/time and game-only source edits do not. A lockfile change may conservatively invalidate
  saves even when it only changes rendering dependencies. Record this limitation in release copy.
- [ ] Check the compatibility identity and existing log/rules versions before replay. Hash embedded
  region bytes before mapping; construct `LoadedRegion` from that same read via `load_region_bytes`.
  Refuse mismatches without consulting current shipped assets.
- [ ] Test truncated payloads, checksum/length/count/metadata discrepancies, unknown versions,
  incompatible build/target, wrong region fingerprint, invalid command DTOs, and over-limit input.
  Property-test codec round trips and rejected corruption with deterministic injected metadata.
  Tests show unsupported versions fail before malformed version-specific command DTO decoding.
- [ ] Retune/delete installed region files and remove original log/archive in a TempDir fixture;
  the standalone bundle still restores correctly on a compatible engine.
- [ ] Run `cargo nextest run -p infrastructure save_codec` and existing command-log/region tests.

## Task 3: Atomic file store, revisions and publication faults

**Add:** `crates/infrastructure/src/save/file_store.rs`,
`crates/infrastructure/tests/save_store.rs`; modify `crates/infrastructure/Cargo.toml` to promote
workspace `tempfile` to a runtime dependency. Add `/saves/` to the root `.gitignore`.

- [ ] Write failing storage tests with a separate `TempDir` per case. Use two store instances for
  contention/conflicts, not process-global environment changes or sleeps.
- [ ] Allocate validated opaque IDs independently of display names; accept Unicode names within
  the spec's bounds, reject controls/empty/oversized names, and never treat names as filenames.
  A missing save directory lists as empty; an unreadable directory is a retryable list failure.
- [ ] List only final `.mgsave` files in stable time/ID order. A bad file becomes an unavailable
  entry with a typed reason, rather than failing the entire list. When saved metadata is unavailable,
  use its file ID and filesystem modification time for ordering, without inventing a game date.
  Verify filename ID against bundle
  ID. Ignore temporary and lock files. Never follow a path supplied inside a bundle to find a region.
- [ ] Under an OS-backed per-slot exclusive lock, compare the expected revision/absence, write a
  unique same-directory `NamedTempFile`, flush and sync the full file, then publish atomically with
  `persist`. Sync the directory/new directory entries using the supported platform's durable-write
  primitive. Never delete the destination before replacement. Release locks on success/error/panic;
  process death must release the OS lock without requiring stale-file deletion.
- [ ] Keep actual filesystem operations behind a small injectable publication seam to force
  failures at create, partial write, flush, file sync, replace and directory sync. Combine those
  probes with real-file round trips; a mocked successful call is not proof of preserved bytes.
- [ ] Assert every pre-publication failure leaves old bytes/readability unchanged and new-slot
  failure leaves no final save. Assert two writes expecting the same revision cannot both replace
  it. Busy/changed slots are reported as conflicts, not retried as unconditional overwrites.
- [ ] Return `DurabilityUnconfirmed { slot, revision }` on post-publication sync failure. The
  published file may already be the new complete save. Retry verifies that revision under the lock
  and completes durability, or returns conflict. Never mark it saved or assert the old bytes remain.
- [ ] Add stale-temp/restart recovery tests and save-directory failure tests independent of log
  failures. Verify a cancelled pre-publication operation never publishes; once publication starts,
  let it finish and return its receipt/error. Cancellation never means rollback of a committed save.
- [ ] Run `cargo nextest run -p infrastructure`, clippy for the changed crates and format checks.

## Task 4: Bridge, barriers, autosaves and candidate installation

**Add:** `crates/game/src/sim/persistence.rs`, `crates/game/tests/game/save_resume.rs` (registered
in `tests/game.rs`). Modify `sim/{mod,driver,new_game}.rs`, `paths.rs`, `state.rs`, `main.rs`,
`lib.rs` and `tests/game/support` as needed. Main stays argument/window setup only.

- [ ] Write headless failing tests before implementing requests, entry barriers or worker plumbing.
  Inject an executor/store that tests can complete, cancel and fail explicitly; no timing sleeps.
- [ ] Add `--save-dir`, `SaveDir` and a store resource. Default to `./saves`, separate from logs.
  Inject compatibility/time/IDs into tests. Pass SaveDir in the headless setup under each test's
  TempDir. Directory accessibility is not a BootChecklist requirement; boot/new games work when
  saves are unavailable. Ensure all CLI usage/help and tests include the new flag.
- [ ] Add typed persistence request/result messages and one operation coordinator. Number jobs.
  Add a SaveSelect menu state (or the existing menu overlay pattern) for the browser, with loading
  progress held in the coordinator rather than overwriting Session. Tag session-directed
  commands/pending work with a session generation. Update HUD dispatch and
  the fixed driver to use the current generation; discard/refuse stale-generation requests.
- [ ] On opening a manual save/leave dialog, stop new ticks/input immediately. Drain already
  admitted commands through `SimSet::Apply`, clear `TicksInFlight` by applying its command, and
  capture at the following checkpoint stage. Keep later messages in an owned pending queue while
  frozen; do not lose them to Bevy reader expiry, apply them twice, or carry them into another game.
- [ ] Detect `MonthClosed` once inside the successful-command loop, after that command's
  `Projections::apply_all`. Freeze its immutable Review checkpoint before any later command.
  This includes December and terminal game over. Hold later queued commands until the barrier
  releases. Replay history and refused commands never trigger autosave.
  If a queued final tick closes the month while a manual save/leave dialog is opening, complete
  autosave or acknowledge its failure first, then service that same-session manual intent. Test
  that the autosave receipt cannot accidentally satisfy Save-and-menu.
- [ ] Create one automatic slot per recording run, update it monthly with the expected revision,
  and keep manual saves independent. Every successful load forks a new run/log/automatic slot.
  Retain a failed autosave's immutable capture/revision for Retry; a newer autosave supersedes it.
  Test that retrying an old failure cannot overwrite a newer successful checkpoint.
- [ ] Perform codec/store/replay work on a worker. Progress and cancellation never mutate `Game`.
  Cancellation is checked between replay commands and before publication. Once publication starts,
  allow completion; ignore stale results for UI/session transitions and never roll back a save.
- [ ] On load success, install only the fully reconstructed candidate with its newly seeded
  diagnostic log. Clear old pending commands, failures, historical events/toasts and transient HUD
  state. Use a resume-entry barrier until `InGame`, the mirrored phase and all phase-entry hooks
  settle; force Paused before releasing it. A later normal BeginOperating still starts at 1×.
- [ ] Test all unsettled entry frames with FixedUpdate at 8×: no tick is queued/applied after load
  until an explicit speed action. Test candidate failures (including fresh-log creation/prefix
  copy failure) and cancelled/superseded completions: no Session/phase/UI installation occurs.
- [ ] Track Save-and-menu intent by operation token. Only its matching confirmed receipt can leave;
  failure, Cancel and durability-unconfirmed keep the game. On Retry, use the correct known revision.
  Manual success stays paused; Cancel restores prior speed only for the same active Operating phase.
- [ ] Run `cargo nextest run -p game save_resume`, then existing simulation, phase and end-to-end
  tests. Fix new state/message constructors without weakening their original assertions.

## Task 5: Save/load presenters, dialogs and localisation

**Add:** `crates/game/src/hud/saves.rs` and `assets/locales/en-US/saves.ftl`. Modify
`hud/{menu,top_bar,prompt,game_over,action,mod}.rs`, `l10n/{keys,mod}.rs` and typed error localisation;
extend `tests/game/save_resume.rs` and existing HUD/l10n/contrast tests.

- [ ] Write pure presenter and real-pass failing tests for all actions and failure states.
- [ ] Add main-menu Load game with empty/error/list views. Show metadata, manual/autosave kind and
  per-slot problems; make good rows selectable even beside a corrupt/incompatible row. While loading,
  show progress/Cancel. Failure stays in the list with a reason; never silently load another slot.
- [ ] Add Save in every phase and game over. The save dialog offers a new named manual slot or an
  existing manual slot, with a separate overwrite confirmation tied to its revision. Show the actual
  captured boundary; explain that unapplied drafts are excluded. Prevent double-click duplicate jobs.
- [ ] Replace Menu's two-action leave prompt with Save and return / Leave without saving / Stay.
  Wire the game-over menu path through the same choice. A save failure offers Retry/Cancel and
  preserves the session; Leave without saving is an explicit separate action, never a failure fallback.
- [ ] Show confirmed last-saved boundary and distinct persistent autosave-failure status. Retry and
  Continue without saving are explicit; dismissing an error barrier does not itself FinishReview.
  A diagnostic log failure does not disable Save. Avoid copy
  implying that an incomplete raw log is the only source for saves.
- [ ] Route every click through `UiAction`/`dispatch`; glue-test every new action and use `ActionSink`
  getters where the dispatcher writes a resource. No duplicate mutable/read `Res` conflicts (B0002).
  Take one frame-start overlay decision so prompts, game over and persistence dialogs do not overlap
  or process a second action from a resource written earlier in the same egui pass.
- [ ] Keep keyboard navigation and 4.5:1 contrast for normal, disabled, error and progress text.
  Real-pass checks at 1024×640 cover the extra top-bar control, long names, many slots, overwrite,
  busy/progress/error views and game-over Save. All new literal keys enter `ALL_UI_KEYS` and typed
  failures receive exhaustive localisation coverage.
- [ ] Run `cargo nextest run -p game`, clippy and formatting. Keep the existing region picker,
  incident-divider, prompt, layout, chart and panel-resize tests green.

## Task 5b: Automatically retained application preferences (2026-10-21)

- [ ] Persist window size/position, UI zoom, last chosen running day speed, sidebar widths,
  bottom-panel heights, Incidents divider width and movable tool-window positions in a separate
  versioned preferences file under the injected save directory. These are application preferences,
  independent of manual slots and month-close autosaves.
- [ ] Restore on launch, clamp geometry to connected monitors and current layout limits, and keep
  malformed/missing preferences from blocking boot. Preserve the selected speed through pauses,
  Review and menu transitions. Loaded games remain paused until explicitly unpaused.
- [ ] Observe real drag/zoom/speed state, write automatically after changes, and flush on normal
  OS/menu exit. Use atomic publication and do not rewrite unchanged preferences every frame.
- [ ] Add isolated restart tests and real egui resize/zoom checks; verify normal exit persistence.

## Task 6: Cross-layer acceptance, playtest and documentation

**Extend:** application/infrastructure save tests and the single game integration binary.
**Update:** `README.md`, `ARCHITECTURE.md`, `TESTING.md`, CLI usage and
`docs/superpowers/plans/2026-10-19-plan-2-playtest.md` to describe the implemented feature.

- [ ] For both shipped regions, save mid-Operating, destroy the app, load in a fresh app, explicitly
  unpause and finish the month. Compare all views/projections and the next month's continuation
  with an uninterrupted run; assert no duplicated ledger entries, incidents, starts or reports.
- [ ] Save Planning changes and a Review after a real December close. Preserve regulator strikes,
  held capital and the displayed report/lessons. Also load a bankrupt/licence-revoked final Review
  and assert ticks remain stopped and See the month works.
- [ ] Save again after resuming, remove the parent save/log and shipped region in isolated fixtures,
  then load the second save. Assert a standalone full history, a fresh branch recording with one
  prefix, and automatic slots independent of the source and manual slots.
- [ ] Inject a mid-run disk-log failure, save to a working directory and load it in a fresh app.
  Inject a separate save-store failure and demonstrate Retry/Cancel without leaving or losing play.
  Cover revision conflicts, post-publish uncertainty and a stale Save-and-menu result.
- [ ] Manually play: create a named save, close via the OS control, relaunch with the same save dir,
  load paused, continue, observe a monthly autosave, load it, and inspect final-month reports. Check
  saving at 8× with a queued tick and cancelling a save dialog. Verify source/manual slots persist.
- [ ] Manually exercise an unwritable/full save destination; a failed Save-and-menu stays in game,
  autosave failure is visible, and Continue without saving works. Restore permissions afterwards.
- [ ] Document paths, applied-state-only saves, monthly capture point, pausing, OS-close behaviour,
  compatibility/refusal policy, size limits, storage errors and diagnostic-log independence. Update
  the architecture's old "save/load later" statement; mark superseded plan rulings as historical
  rather than rewriting their account of the earlier implementation.
- [ ] Run the completion gate:

  ```sh
  cargo build --workspace
  cargo nextest run --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all --check
  git diff --check
  cargo run -p game -- --exit-after-frames 120
  ```

  Launch smoke is a native/manual check, not a nextest case. The separate interactive playtest
  proves save/load behaviour beyond simply drawing 120 menu frames. Review the full final diff,
  including binding spec and all reachable restore/write paths, before calling Plan 2b complete.

## Implementation and verification (2026-10-21)

The runtime work in Tasks 1–5b is implemented. The original checkboxes above are retained as the
review checklist; this record describes the concrete completion evidence rather than claiming
that every individual manual variation was exercised.

- Application checkpoints retain all accepted command batches independently of disk-log health.
  Restore verifies the boundary, rebuilds all projections and seeds a fresh branch log once.
- File saves have a versioned/checksummed envelope, embedded region, conservative build identity,
  bounded input, OS slot locks, revisions, atomic replacement and explicit durability uncertainty.
  Fault and contention tests inspect actual disk bytes. A second resume survives removal of its
  parent save, logs and installed region fixture.
- The bridge owns capture barriers, queued commands, session generations, cancellable worker jobs,
  exact month-close autosaves and a paused load-entry gate. Regression tests cover pending
  Save-and-menu/Leave choices, stale completions, fresh overwrite confirmation and later autosaves
  after an acknowledged uncertain publication. Both shipped regions are restored in fresh apps
  and compared with uninterrupted play through month close and the following month's commands.
- All save/load dialogs, game-over Save, error/retry/overwrite choices and literal localisation
  keys are implemented. Real egui passes check layout and contrast at minimum and larger sizes.
- Separate client preferences retain native window geometry, the full egui zoom range, last
  running speed, panel/divider sizes and movable tool positions. Restart/real-drag/exit tests
  guard the behaviour, including main-menu frames before starting a game.
- Native macOS QA used isolated paths under `/tmp/megawatt-save-qa.ayaQZV`: Planning save, OS close,
  relaunch/load, Operating save restored paused at 01:11, a completed-month automatic Review,
  separate overwrite confirmation, and a permission-denied save whose old bytes were unchanged.
  Retry succeeded after restoring permissions. Current-build startup also retained 1280×800 at
  (100,100), 125% zoom and 8× through the main menu. Native launch smoke exits successfully.
- Final combined gate: all 1,121 workspace tests, workspace build, Clippy with warnings denied,
  rustfmt and whitespace checks pass.
- Automated coverage includes December rate review, construction, terminal Review, codec
  corruption properties, geometry reachability and complete projection equality. The extended
  manual matrix (every phase, final outcome, monitor arrangement and destination failure) remains
  a release playtest checklist; those manual combinations are not all claimed here.

README, architecture and testing docs describe the implemented behaviour. Simulation rules remain
version 6, raw logs remain version 1, and save files start at version 1. Plan 3 has commenced with
CPU terrain geometry only; its 3D rendering integration remains separate.

## Review focus

1. **Incomplete history accepted as a save:** especially after disk-log failure or a second resume.
2. **Correct session, wrong projections:** replay must fold each batch against that command's state.
3. **Operating entry races:** the 1× entry hook or a queued old tick advances a supposedly paused load.
4. **False durability claims:** file replacement is not directory synchronisation; post-publish
   failure differs from pre-publish failure. A successful mocked write is not disk evidence.
5. **Branch/slot interference:** repeated load, duplicate clicks or another app overwrites a source
   or wins a stale overwrite without a revision/lock check.
6. **Unjustified compatibility:** rules version alone cannot certify a different build or platform;
   malformed inputs must fail before replay or session installation.
7. **Autosave at the wrong boundary:** FinishReview, replayed MonthClosed or a late worker changes
   what the slot claims to preserve. Game-over Review must remain reviewable after restore.
