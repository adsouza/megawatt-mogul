# Plan 9 — browser client and automatic GitHub Pages deployment

Status: proposed, implementation not started. Requested on 26 October 2026; revised the same
day after review, splitting the work into Plans 9a–9d. This document plans the work; it does not
enable Pages, change repository settings, or claim that the current client builds for WebAssembly.

## Outcome and scope

Run the existing Rust/Bevy game in a desktop browser at the expected project URL
`https://adsouza.github.io/megawatt-mogul/`, subject to the repository's actual Pages
configuration. Keep the simulation, egui presenters, regions, controls and portable
state shared with the native client. GitHub Actions builds and tests pull requests;
successful changes on `main` deploy automatically to GitHub Pages.

Saves stay on the player's device in IndexedDB. Manual saves, monthly autosaves,
Resume, save-library browsing, rename, overwrite and delete retain their existing
behavior. Include download/upload backup controls so browser storage is not the
only copy available to the player. Accounts, cloud synchronization, mobile/touch
redesign and installable/offline PWA support are outside this release.

No simulation-rule or portable-state-schema bump is intended. Browser database
versions and a backup container version are separate from those versions.

## How the work is split

Plan 9a is a viability gate: its measurements choose the rendering backend, decide whether a
codec worker is needed and set size/speed budgets. Later parts are specified against those
outcomes and may be revised when 9a closes. Each part ends in something reviewable and shippable.

| Part | Ships | Depends on |
| --- | --- | --- |
| 9a | Browser runtime proof, then a playable **preview without saves** deployed to Pages by CI | — |
| 9b | Storage-independent archive/preference codecs and a platform persistence executor (native-only behavior change: none) | — (can run alongside 9a) |
| 9c | IndexedDB saves and preferences in the browser; the preview label is removed | 9a, 9b |
| 9d | `.mgsave.zip` backup download/upload and a native import/export tool | 9b (codec, native tool); 9c (browser UI) |

Deploying in 9a, before saves exist, surfaces the `/megawatt-mogul/` subpath, Pages caching,
download size and CI duration at the start rather than in the last checkbox. The preview says
plainly, in localized text, that progress is not kept across reloads. README links to the
browser build only once 9c ships.

## Current implementation and changes required

| Current source | Relevant behavior | Planned change | Part |
| --- | --- | --- | --- |
| `crates/game/src/main.rs`, `paths.rs` | Desktop arguments, filesystem paths, crash reports and window setup | Target-specific startup; browser canvas and bootstrap | 9a |
| `crates/game/src/state.rs` | Boot checklist requires `AssetRoot` and `LogDir` (path resources) | Browser boot requirements: fetched asset set, no log directory | 9a |
| `crates/game/src/crash.rs` | Panic hook writes a report file stamped with `SystemTime::now()` | Native only; browser panic reporting to the console and the boot-failure shell | 9a |
| `crates/game/Cargo.toml` | Bevy 0.20, bevy_egui 0.43.1, native window backends and multithreading | Target-specific Bevy feature tables and a reproducible WASM build | 9a |
| `crates/game/src/sim/mod.rs` | `Catalog` contains `RonRegionCatalog` | A catalog wrapper supporting native files and fetched region bundles | 9a |
| `crates/infrastructure/src/region/catalog.rs`, `bundle.rs` | Region RON and referenced sidecars assembled from disk | Reuse canonical bundle encoding and validation for browser assets | 9a |
| `crates/game/src/l10n/mod.rs`, `audio.rs` | Translations and sound bytes read through `std::fs` | Load packaged browser assets through fetch/Bevy asset APIs | 9a |
| `crates/game/src/sim/new_game.rs` | Seed from `SystemTime::now()`; synchronous file command log created before start | Browser seed/clock source; in-memory command log | 9a |
| `crates/game/src/window.rs`, `window/activity.rs` | Monitor sizing and native redraw policy | Canvas resize, browser visibility and event-loop handling | 9a |
| `.github/workflows/ci.yml`, `release.yml` | Native checks and Linux release packaging | Browser build/checks and a gated Pages deploy job | 9a |
| `crates/infrastructure/src/save/monthly.rs` | Format-4 manifest and binary chart encoding mixed with filesystem reads | Extract byte-oriented archive operations from filesystem publication | 9b |
| `crates/game/src/sim/persistence.rs` | Native thread jobs wrapped in `catch_unwind`, channels, preload and recording activation | Shared state machine with platform-specific job execution | 9b |
| `crates/game/src/preferences.rs`, infrastructure preferences adapter | File reads and a native writer thread | Byte codec shared by native files and IndexedDB | 9b, 9c |
| `crates/application/src/save.rs` | Synchronous `SaveStore: Send + Sync` port, including `now()` | Preserve native contract; add asynchronous browser orchestration above it | 9c |

Existing saves use directory format 4: `snapshot.ron.z` plus immutable, compressed
monthly chart files. The inner flat save encoder writes format 3 and retains older
readers. A browser adapter must reuse these bytes and checks, rather than flatten
all history into a new JSON or localStorage representation. The current flat codec
bounds a save at `MAX_SAVE_BYTES`; preserve the existing component, expansion and
aggregate bounds when extracting the archive API.

### Portability facts that shape the design

- **Restore does not replay portable saves.** `Game::restore` (`crates/application/src/game.rs`)
  verifies a stored snapshot against its state hash and journal hash; only historical
  command-only saves replay. Floating-point differences between native libm and WASM's libm
  therefore affect command-only saves and continuation, not loading a snapshot save.
  `SaveBundle::restore` reports `OtherVersionDiverged` when a foreign build's save fails
  verification; that remains the honest outcome for command-only saves that diverge on WASM.
- **Saves carry their own region.** The flat codec embeds the region source
  (`crates/infrastructure/src/save/codec.rs`) and Resume restores against it, so browser
  region packaging cannot break existing saves. Packaged bundles should still equal the native
  catalog's canonical bytes, so the same region produces the same new-game save on both targets.
- **`catch_unwind` is inert in the browser.** Stable `wasm32-unknown-unknown` aborts on panic.
  The native persistence jobs (`persistence.rs`, both job spawns) convert a panicking restore
  into an error; in the browser the same panic ends the whole page. This matters most for
  uploaded backups (9d), which put untrusted bytes through decode and restore.
- **The wall clock panics.** `SystemTime::now()` panics on `wasm32-unknown-unknown`. Current
  callers: the new-game seed (`new_game.rs`), the crash report (`crash.rs`, inside the panic
  hook, which would turn any panic into a double panic), and the native file store's
  `SaveStore::now()` (`file_store.rs`). The browser store needs its own `now()` from JS `Date`.
  No `Instant::now()` calls exist in our crates; keep it that way or use Bevy's platform time.
- **Pages caches and replaces atomically.** Observed on 26 October 2026: GitHub Pages serves
  every file with `cache-control: max-age=600`, and serves `application/wasm` gzip-compressed
  (no Brotli). Each deployment replaces the whole site. A tab or cache holding the previous
  `index.html` for up to ten minutes can request content-hashed files that no longer exist.

## Plan 9a — browser runtime proof and preview deployment

### Build configuration

- Build `game` for `wasm32-unknown-unknown` with pinned Trunk and a checked-in
  `web/index.html`/Trunk configuration. Keep app/plugin construction in the game library;
  entry points only select target setup and launch it.
- Split Bevy features into target tables: `[target.'cfg(target_arch = "wasm32")'.dependencies.bevy]`
  adds `web` (Bevy 0.20 requires it: `bevy_app/web`, `bevy_platform/web`, `bevy_reflect/web`)
  and exactly one of `webgl2`/`webgpu`; `x11`/`wayland` move to a Linux table and
  `multi_threaded` to a non-WASM table. The renderer choice is a compile-time feature: one
  browser binary per backend. Shipping both WebGL2 and WebGPU means two builds plus a
  selection step in the shell, which this plan does not intend.
- Start with WebGL2 for reach. Prove the existing PBR, egui and world rendering on it before
  committing to the browser support matrix. Disable unsupported optional effects with a
  documented visual fallback. If WebGL2 cannot render the required scene, evaluate WebGPU in
  this proof; do not silently ship a WebGPU-only requirement or duplicate the renderer.
- Check bevy_egui 0.43.1's web features: whether its web clipboard needs
  `--cfg=web_sys_unstable_apis`, and whether `open_url`/`picking` behave on the canvas. Put any
  required flag in checked-in configuration, never an ad-hoc environment variable.
- Add a WASM release profile optimized for simulation speed and download size, retaining
  overflow checks. Pin `wasm-opt` (binaryen) through Trunk's tool configuration and match
  `wasm-bindgen` tooling to the locked crate version. Do not depend on globally installed,
  unpinned tools.
- `tempfile` is a normal dependency of `infrastructure`. Run
  `cargo check -p infrastructure --target wasm32-unknown-unknown` first; if it does not compile,
  move file-store code behind `cfg(not(target_arch = "wasm32"))` rather than stubbing it.
- The baseline browser build must work without WASM threads, SharedArrayBuffer or
  cross-origin-isolation headers (Pages cannot set them). Dedicated Web Workers remain
  available; they do not imply shared WASM memory.

### Platform edges

- Add a browser clock/seed source at the client edge for the new-game seed. No wall clock
  enters the domain except the player-confirmed seed.
- Compile `crash.rs` for native only. In the browser, report panics to the console and to the
  shell's boot-failure state; check whether Bevy's `LogPlugin` already installs a console panic
  hook before adding one.
- Give the boot checklist browser requirements (fetched asset set) in place of `AssetRoot`
  and `LogDir`, keeping the one checklist mechanism.
- Use the existing in-memory command-log sink. Native synchronous command logging and crash
  files retain their current contract.
- Pause browser play on page hiding and return paused, so tab suspension cannot generate a
  burst of catch-up simulation ticks.
- Use a viewport-sized canvas, resize/high-DPI support and explicit focus handling. Retain the
  supported 1280×640 game content size; show a clear small-viewport hint rather than promise a
  mobile layout. Do not run native monitor-position restoration.
- Without a save store, the save UI and Resume present storage as unavailable in this preview
  (localized), and starting a game remains possible. No persistence job is spawned; on WASM,
  `thread::Builder::spawn` returns an error, and that path must not be the mechanism.

### Assets and boot

- Generate canonical region bundles from the shipped RON and sidecars with a small native
  infrastructure packaging tool run by the Trunk build, plus a deterministic asset manifest
  covering every locale file and daily audio cue. Do not commit generated bundles or maintain a
  second hand-edited list of regions or sidecars. Test that each packaged bundle equals the
  native catalog's canonical bytes for all eight regions.
- **Fetch every runtime asset at boot** from the manifest: shipped assets total about 2.6 MB
  before compression, so lazy fetching saves little and would let a session started on one
  deployment fetch another deployment's data. Keep region *decoding and mapping* lazy where
  possible; the picker should not construct every playable simulation.
- Stamp the manifest with the build's source SHA. A manifest/asset mismatch or a 404 for a
  hashed file shows a localized "a new version was published — reload" state, not a generic
  failure. The shell detects a failed WASM/JS load (a stale cached `index.html` for up to ten
  minutes after a deploy) and offers a cache-busting reload before Fluent or WASM is available.
- Load audio via browser-supported asset loading and unlock playback on a user gesture. Muted
  autoplay or a failed optional sound must not prevent startup.
- Resolve every runtime URL relative to the configured deployment base. Test both `/` and
  `/megawatt-mogul/`; never hard-code `/assets/` or a source checkout path.
- Show localized loading, unsupported-graphics and boot-failure states. Shell fallback
  strings that render before Fluent loads live in one checked-in table with a coverage test
  against the English locale, so they cannot drift from the `.ftl` text.

### Preview deployment

Add browser checks and a deploy job to the existing CI workflow and document one-time
repository setup:

- PRs build the optimized browser artifact and run browser tests without deployment
  permissions or secrets. Pushes to `main` run the same checks and deploy only after success.
  `workflow_dispatch` permits deliberate rebuilds; only `main` can publish.
- Add the deploy as a job in `.github/workflows/ci.yml` with `needs:` on both existing native
  jobs (`check` and `dev-tools`) and the browser job, and `if:` restricted to pushes to `main`
  (or dispatch on `main`). Every required job must succeed before publishing; a failed,
  cancelled or skipped dependency cannot permit deployment. Checks and the published artifact
  then refer to the same commit, without a privileged `workflow_run` trigger.
- Give the deploy job its own `concurrency: { group: pages, cancel-in-progress: false }`.
  Concurrency does not order runs by commit: an older run whose checks finish later can still
  deploy after a newer one. Before deploying, the job confirms its SHA is still the head of
  `main` and skips otherwise.
- Use the pinned Rust toolchain from `.github/actions/rust-setup`, the `wasm32-unknown-unknown`
  target, locked dependencies, pinned Trunk and a separate WASM cache. Keep native Linux
  package builds separate.
- Derive the public base from Pages metadata for deployments; use the known repository path
  for unprivileged PR checks. Build and test that exact subpath artifact, including worker
  URLs if introduced.
- Use `configure-pages`, `upload-pages-artifact` and `deploy-pages`, looking up and pinning
  their current releases during implementation. Scope `pages: write` and `id-token: write` to
  the deploy job and use the `github-pages` environment.
- Enable Pages with GitHub Actions as its source once the preview is ready, with the user's
  authorization; Pages is currently not enabled for the repository. Inspect settings,
  permissions, custom domain and environment rules first.
- Stamp the deployed artifact with source SHA/version. Use content-hashed WASM/JS filenames.
  Do not add a service worker in this release.
- Include a "Source" link to the exact deployed commit (the AGPL source offer) and a generated,
  pinned third-party licence list for the WASM dependency graph. Never package personal saves,
  logs, archived recovery tools or local filesystem details.
- Document how to redeploy a prior known-good commit.

### Browser test layers

- **`wasm-bindgen-test`** in headless Chromium for non-Bevy browser code (asset manifest,
  clock, and in 9c the IndexedDB adapter), with the runner matched to the locked
  `wasm-bindgen`. These test targets are not Bevy-linked and do not add to the game's
  one-integration-binary rule; record the layer in TESTING.md.
- **Playwright** for end-to-end checks of the deployed-shaped artifact, pinned by a lockfile
  under `web/tests/`, with a fresh browser context and database per test. Keep JS glue minimal;
  no local TypeScript typecheck.
- CI Chromium on Linux renders WebGL through a software rasterizer. Allow generous timeouts,
  avoid pixel-exact screenshot comparisons, and key tests off a readiness signal from the game
  rather than fixed delays. Collect console errors and failure screenshots.
- Safari can only be automated on macOS runners; treat Safari as a recorded manual check
  unless the cost is accepted later.

### 9a acceptance gates

- [ ] Target-specific build configuration, shell, asset packaging and browser boot.
- [ ] Audit target dependencies, entropy/time sources and native-only APIs against the call
      sites listed above; `tempfile` compiles or is gated.
- [ ] Render the real title globe, localized menu and smallest supported game HUD at the
      project subpath. Start all eight regions and run a normal simulation day.
- [ ] Choose the IndexedDB binding (`indexed_db_futures`, `rexie` or raw `web-sys`) and prove
      write → reload → read with a disposable database. Prove that awaiting a non-IndexedDB
      future inside a transaction surfaces as a typed error, not a silent early commit.
- [ ] Measure gzip transfer size, boot, region selection, first planning frame, maximum-speed
      play, and save capture/encode and decode/restore of the existing native codecs compiled to
      WASM, including an authored long-history fixture. Record machine, browser and timings.
- [ ] Choose the rendering backend and set measured budgets (transfer size, boot, frame time).
      Decide whether a codec worker is required for performance; 9d requires one for untrusted
      import regardless unless decode and restore are shown not to panic.
- [ ] Page hiding pauses play; returning does not burst ticks.
- [ ] Deploy job waits for `check`, `dev-tools` and the browser job; failure, cancellation or
      skipping of any required job prevents publication. Verify concurrency and the
      head-of-`main` guard; reload behavior across two consecutive deployments (stale
      `index.html`, removed hashed files) shows the reload state.
- [ ] Enable Pages (authorized), verify the first deployment's URL and exact source SHA, and
      repeat the browser checks on the deployed URL.

## Plan 9b — storage-independent codecs and execution

Native-facing refactors that the browser needs; native behavior and released fixtures do not
change.

- Refactor the monthly archive codec into pure `encode`, metadata inspection and
  decode/verify functions accepting manifest and part bytes (or a bounded part reader). Keep
  filesystem traversal/symlink checks in the file adapter and byte, reference, checksum and
  size checks in the shared codec. Preserve exact archive bytes and immutable fixtures.
- Extract the preference codec likewise: validation and versioning in bytes, file I/O in the
  adapter.
- Introduce a platform execution boundary in the game persistence adapter with explicit
  operations for list, read/restore, source revision check, write, rename and delete. Both
  paths deliver the same typed completion values to the existing state machine. Native
  execution keeps its threads, `catch_unwind` containment and fault-injection tests.
- Keep generation/token fencing, ordered autosave captures, cancellation, source revision
  checks and session installation in the shared state machine. Make recording activation
  platform-specific: browser Resume must not call `create_log`, write sidecars or require a
  synthetic filesystem path.

### 9b acceptance gates

- [ ] Byte-oriented monthly archive and preference codecs with no filesystem calls.
- [ ] Existing native directory and flat-save fixtures retain exact behavior; released fixture
      bytes stay immutable.
- [ ] The persistence executor lands as a pure native refactor behind the existing tests
      before any browser executor exists.
- [ ] Generated-operation/property coverage for archive split/join and store
      create/replace/rename/delete sequences, revision conflicts and rejected-operation
      atomicity, comparing complete checkpoints and metadata, not only visible cards.

## Plan 9c — browser saves and preferences

### Asynchronous persistence without blocking the browser

Do not implement synchronous `SaveStore` methods by blocking on IndexedDB, returning success
before commit, or treating a memory cache as durable storage.

- Browser execution runs non-`Send` local futures for IndexedDB, keeping JS handles out of
  Bevy `Resource` types requiring `Send + Sync`. Only ordinary Rust completion data crosses
  back to the frame-polled queue.
- A cancelled or obsolete load cannot install its session. A database operation already
  committed cannot be undone by a late cancellation; reconcile its receipt/revision instead of
  reporting that nothing was saved.
- Async I/O alone does not make compression, RON parsing, region mapping or restoration
  nonblocking. Use 9a's measurements: split work at month/batch boundaries and yield where
  feasible, and add an isolated codec worker with transferred buffers if a single step blocks
  interaction materially. Do not label a `spawn_local` wrapper around one large synchronous
  operation a background worker. Worker results still require current generation/revision
  checks on installation; the main thread owns the live `Game`.

### IndexedDB layout and transaction contract

Use an application-specific database name, `megawatt-mogul`, independent of build hashes.
Version 1 has these stores:

| Store | Key | Value |
| --- | --- | --- |
| `meta` | `schema` | Value-level schema version and the build that last upgraded the database |
| `cards` | Slot ID | Lightweight save-card metadata, manifest digest/revision and value schema tag; no manifest, state, journal or chart bytes |
| `slots` | Slot ID | Format-4 manifest bytes, manifest digest/revision and value schema tag |
| `charts` | `[slot ID, validated chart filename]` | Immutable compressed monthly chart bytes |
| `preferences` | `client` | Validated, versioned client-preference payload |

Namespacing is necessary because Pages project paths under the same host share an origin.
Do not reset the database on deployment, change its name per release, or clear it when
opening an unsupported newer version.

**Versioning and rollback.** Opening a database at a lower version than the stored one fails
with `VersionError`, so a rolled-back build cannot read any saves until the newer build returns.
Reserve database version bumps for new stores or indexes; evolve record formats through the
value schema tags and `meta`, which older code can read and refuse per record. A `VersionError`
shows a localized "saved by a newer version" state and never calls `deleteDatabase`.

Write sequence:

1. Capture the accepted-command boundary and encode/validate the archive outside the
   transaction. A later simulation tick must not change the capture.
2. Open one `readwrite` transaction covering `cards`, `slots` and `charts`. Read the current
   revision from `slots` and enforce `Create`/`Replace` compare-and-swap semantics in that same
   transaction, including operations from other tabs. Card metadata is not authoritative for
   conflict checks.
3. Reuse matching immutable chart records, insert missing records, update the manifest in
   `slots` and its matching metadata/revision in `cards` together, and remove obsolete parts
   for that slot. Await only requests on this transaction inside it: any other await (fetch,
   compression, hashing, channels, timers, waiting for a frame) lets the transaction auto-commit.
4. Acknowledge success only on transaction completion, not on a request's success callback.
   On abort/quota failure, retain the previous complete save and report the error through the
   current save/retry UI. Request strict durability where supported; do not describe browser
   guarantees as native file `fsync` guarantees.

Rename checks the expected revision and updates the name-bearing manifest in `slots` and the
matching metadata/revision in `cards` in one transaction; it preserves saved-at time and
gameplay/chart content. Delete checks the revision and removes the card, slot and its parts
atomically. A retry may accept an already absent slot but must reject a replacement.
A new revision is the digest of the actual committed manifest bytes, retaining the native
convention. The browser store's `now()` comes from JS `Date`.

List only rows from `cards`, without reading `slots` or `charts`. IndexedDB value reads return
the complete stored record, so separating the cards prevents library browsing from retrieving
every compressed manifest and its embedded state/journal. On selected load, read a consistent
manifest/part snapshot, verify every referenced part and restore through the normal verified
path. Recheck the source before installing a preloaded candidate. Metadata is a hint; it cannot
bypass validation.

Handle database open failure, blocked upgrades and `versionchange` explicitly. Close old
connections when asked to upgrade; tell the player when another tab needs to close/reload.
Use revision checks as the correctness mechanism. A BroadcastChannel notification can refresh
library hints, and a Web Lock per playing slot can warn when a second tab opens the same save
(which would otherwise conflict at every monthly autosave); neither is the correctness mechanism.

### Durability, lifecycle and preferences

- Preserve background monthly autosaves, manual saves and the persistent failed-save warning.
  Successful autosaves do not open dialogs or pause the next month.
- Request `navigator.storage.persist()` as part of a player save/storage interaction; handle
  denial without failing an otherwise successful save. Storage estimates are advisory, never
  proof a write will fit.
- Explain in the save UI that saves belong to this browser/profile and can be lost when site
  data is cleared. Indicate when persistent storage was not granted. Private browsing and
  unavailable storage get actionable text; playable unsaved sessions remain possible, without
  claiming their saves survive reload. Do not silently fall back to memory after a failed write.
- Never rely on asynchronous `beforeunload` work for a successful save. A best-effort
  lifecycle checkpoint may be added, but manual/monthly committed boundaries remain the
  promised recovery points; no crash-recovery promise for commands since the last confirmed save.
- Reuse the 9b preference codec and debounce writes asynchronously. Browser settings include
  zoom, panels, chart layout, speed and effects; exclude desktop position and size. Keep them
  separate from simulation state and save revisions.
- Checkpoints retain their complete accepted journal as today. Diagnostic export can capture a
  current checkpoint; persistent per-command crash logs require a separate later design.

### 9c acceptance gates

- [ ] IndexedDB schema (including separate `cards` and `slots` stores and `meta`), transactional
      publication and typed errors.
- [ ] Instrument library listing to prove it reads only `cards`, with no `slots` or `charts`
      reads even for long-history saves. Create, replace, rename and delete keep cards and
      manifests consistent; an aborted transaction changes neither, and their revisions agree
      after reload.
- [ ] Wire manual/automatic saves, library, Resume/preload, retry and settings; remove the
      preview label.
- [ ] Test blocked/unavailable storage, denied persistence, quota/abort, cancellation, stale
      completions, other-tab replacement, upgrade blocking, `VersionError` from a newer
      database, and recreated slots.
- [ ] Failure before transaction commit leaves the previous complete revision, and the UI
      cannot announce success before completion.
- [ ] Browser reload restores Planning, mid-Operating and Review, including batteries, wind,
      SGCC/HVDC, historical island charts, journal and RNG state.
- [ ] A background autosave retains its captured month while play continues; cancelling a
      manual dialog cannot cancel it. Failed-boundary warning tests still pass.
- [ ] Browser settings survive reload.
- [ ] Simulate a newer deployment against an existing saved library: Resume still works, and
      records with unknown newer value schemas remain untouched by older code.

## Plan 9d — portable backups

Ship a versioned `.mgsave.zip` transport containing the existing format-4 manifest and
exactly its referenced monthly files. It is a transport wrapper, not another simulation
serialization. Verify a chosen ZIP dependency on WASM and pin it in the lockfile. Share the
transport codec with native tooling.

- Export a consistent committed revision, including the full region/journal/history, and
  trigger the browser download from the player's action. Never export a partially written
  autosave or fetch unrelated filesystem paths.
- Import from a player-selected file. Bound compressed size, expanded size, entry count and
  parse depth; reject duplicates, extra entries, absolute/traversing names,
  encrypted/unsupported entries, corrupt data and unsupported schemas. Enforce archive bounds
  before allocation/decompression can exhaust memory.
- **Contain untrusted decode.** Because a panic aborts the page in the browser, decode and
  restore an uploaded backup in a dedicated Worker (its own WASM instance, so a panic ends only
  the worker and is reported as an import failure), unless 9a/9c evidence shows the decode and
  restore path cannot panic on arbitrary bytes. Fuzz/property-test the transport and restore
  path with malformed inputs either way.
- Decode and restore in a candidate before publishing. Give an import a new slot identity by
  default; never overwrite a conflicting local slot silently. Any explicit replacement uses the
  existing confirmation and revision check.
- Expose native import/export through a small infrastructure command-line tool using the same
  codec, without changing native directory-save storage. Historical command-only saves retain
  their existing rules/verification fences, and may legitimately report
  `OtherVersionDiverged` across targets.
- Route game UI buttons through `UiAction`/`dispatch` and the existing presenters. Add Fluent
  keys and literal-key coverage. Cover Enter submission once, enabled Cancel/Close on Escape,
  popup priority and default-Cancel destructive prompts.

### 9d acceptance gates

- [ ] Bounded transport codec and native import/export tool (needs only 9b).
- [ ] Contained browser import (worker or evidence), localized download/upload controls,
      validation errors and confirmations.
- [ ] Malformed/oversized archives and every supported historical schema reader.
- [ ] Native → browser → native boundaries and each target's uninterrupted continuation
      oracle. Record any float differences; never relax integrity hashes to conceal them or
      promise bit-identical cross-target futures without evidence.

## Release evidence (after 9c, again after 9d)

- [ ] Exercise the documented supported browser matrix, initially current desktop Chrome,
      Firefox and Safari; narrow the published matrix if measured support requires it. Record
      versions and manual checks that automation cannot cover.
- [ ] Serve the production artifact under `/megawatt-mogul/` and verify assets, downloads,
      workers, refreshes and storage on the deployed URL.
- [ ] Update README, architecture, testing, web deployment documentation and the design spec
      with the implemented browser durability/lifecycle contracts and evidence.

## Validation and workflow limits

For this planning change, review the owning code and external platform contracts and run
`git diff --check`; no code build, test suite or repository settings change is needed.

During implementation, use TDD for behavioral changes: demonstrate the failure first,
implement the smallest change, then refactor. Each gate is a reviewable commit; boxes remain
open until their evidence exists. Use focused nextest selections for codecs, portable saves,
preferences, persistence, localization and affected presenters. Put new game test modules under
`crates/game/tests/game/` and include them from the existing `game.rs`. Use isolated TempDirs
on native and disposable browser contexts/databases on web; never read/write personal saves as
test fixtures.

Before implementation commits, run appropriate focused nextest checks, workspace clippy with
warnings denied, formatting and diff checks. Run the isolated native window smoke after
plugin/system-parameter changes and browser acceptance after browser changes. The entire local
suite still requires express advance user permission (TESTING.md); CI may run the full suite as
currently configured. Inspect hooks before commits/pushes and preserve their required checks
without bypassing them.

## References checked while planning

- [Bevy's WASM instructions](https://github.com/bevyengine/bevy/blob/v0.20.0/examples/README.md#wasm)
  and the installed Bevy 0.20 / bevy_internal 0.20 feature metadata (`web`, `webgl2`→`webgl`,
  `webgpu`). Rendering support is an implementation gate; the project has not yet been compiled
  or launched in a browser.
- Installed bevy_egui 0.43.1 metadata: target-specific clipboard dependencies and `getrandom`
  0.4 with `wasm_js` on WASM.
- [GitHub: custom Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
  for deployment artifacts, environment and permission requirements. Response headers from
  `pages.github.com` and a Pages-hosted `.wasm` file, observed 26 October 2026, for
  `max-age=600` caching and gzip compression.
- [MDN: using IndexedDB](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API/Using_IndexedDB)
  for asynchronous requests, transaction auto-commit, upgrades, `VersionError` and browser
  shutdown limits.
- [MDN: storage quotas and eviction](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria)
  for quota errors and persistent-storage behavior. IndexedDB is preferable to localStorage for
  these potentially large binary saves; persistence is not a backup.
