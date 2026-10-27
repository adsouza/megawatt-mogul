# Browser builds and GitHub Pages

The browser client shares the native Rust simulation and egui UI. Saves and preferences live
in this browser/profile's `megawatt-mogul` IndexedDB database. Manual saves, monthly autosaves
and Resume preserve the complete verified game and history. Clearing site data removes saves;
the header reports when automatic cleanup protection has not been granted.

## Build and run

Use Rust 1.98.1 with `wasm32-unknown-unknown`, Trunk 0.22.1 and its pinned wasm-bindgen
0.2.129 / Binaryen 130 tools. On systems with a dynamically linked `rust-lld`, also install
the matching `llvm-tools-preview` component. Do not mix a Homebrew compiler with another
rustup toolchain's WASM standard library.

```sh
rustup target add wasm32-unknown-unknown --toolchain 1.98.1
cd web
trunk build --release
cd ..
python3 tools/web/serve.py
```

Open `http://127.0.0.1:8765/megawatt-mogul/`. The build hook packages canonical region
bundles and content-addressed locale/audio files. `MM_BUILD_SHA` must match at packaging
and Rust compilation (default `development` for local builds). Third-party notices and
an exact-commit source link accompany CI builds. Generated assets and distributions
are ignored by Git. Trunk supports `--public-url /` when serving at the root instead.

## Checks and deployment

`python3 tools/web/test_contracts.py` checks shell localization. The infrastructure
`web_assets` test compares every bundled region with the native catalog. Playwright tests
under `web/tests/` open the real canvas and probe browser persistence across reload.
Build the native round-trip tool with `cargo build -p infrastructure --example save_backup`.
Run `npm ci`, `npx playwright install chromium`, then `npm test` there while the local
server runs. Each test gets its own browser context; no personal saves are accessed.

The CI `browser` job builds the production artifact and tests the project subpath.
Main builds derive the base path from `configure-pages` metadata; PR builds use the repository
subpath. The test server and Playwright use that same path. `deploy-pages` depends on `check`,
`dev-tools` and `browser`. It publishes only the current
head of `main`, serializes deployments, and uses the `github-pages` environment. Repository
Pages must use GitHub Actions as its publishing source. A manual CI dispatch on `main`
rebuilds the current revision. To roll back source, revert the offending changes on `main`
and let the same checks/deployment run; an old workflow cannot bypass the head guard.

WASM/JS and runtime assets are content-addressed. A stale page or mismatched asset inventory
shows Reload; browser data is never cleared by deployment. This release has no service worker
and does not promise offline startup. Desktop keyboard/mouse and a 1280×640 content area
are the initial interface target; the published browser support matrix awaits runtime evidence.


## Browser storage

`cards` contains only lightweight library metadata. `slots` contains manifests and their
revisions; `charts` contains immutable monthly buffers. Create/replace/rename/delete update
all three stores in one transaction and acknowledge only its completion. Revision hashes and
an in-transaction source-byte comparison protect against another tab's changes. Unknown newer
record schemas are preserved. Preferences use a separate store and the same byte validator as
native files. No per-command disk log or unload-time save is promised.

Codecs run in disposable workers using an independent WASM instance. Boot retains the compiled
module and module sources as object URLs, so later saves need no files from a replacement
deployment. Worker messages carry explicit bounded metadata and binary checkpoint buffers;
the live Bevy game remains on the main thread. Loading still reconstructs the verified game
on the main thread after the worker validates and normalizes the checkpoint. Dialogs stay open
until completion, and obsolete generation results cannot install a game.


## Portable backups

Open the saved-game library and choose **Download backup** for a committed save. **Import
backup…** validates a selected `.mgsave.zip` in a disposable worker, then adds a new named
save; it never replaces a matching local slot or changes the running session. Canceling the
file picker leaves the library unchanged. Keep downloaded copies outside browser site data.

The same transport works with the native infrastructure tool:

```sh
cargo run -p infrastructure --example save_backup -- export saves/<id>.mgsave backup.mgsave.zip
cargo run -p infrastructure --example save_backup -- import backup.mgsave.zip saves
cargo run -p infrastructure --example save_backup -- validate backup.mgsave.zip
```

Export accepts directory saves or supported historical flat saves and refuses to replace an
existing output file. Import creates a fresh manual slot in the native directory store. Validation
prints checkpoint, session and projection hashes plus the accepted command count. Historical
command-only saves still require their recorded rules and verification; incompatible ones are
refused. Source saves and released fixtures are never rewritten by export or validation.

Transport version 1 wraps the existing format-4 manifest and its referenced files. It does not
change simulation rules or portable-state schemas. ZIP metadata is bounded before parsing:
at most 4,097 files, 64 MiB per entry, 128 MiB total expanded transport bytes and 130 MiB input.
The existing archive codec separately bounds inner decompression and RON depth. Duplicates,
paths, symlinks, extra/missing parts, unsupported versions/methods and checksum failures are
rejected before publication. Encryption and multipart/ZIP64 archives are unsupported.

Chromium 156 has automated runtime evidence, including reload, monthly autosave, worker failure,
backup transfer and project-subpath assets. Firefox and Safari remain unverified; the initial
tested browser target is desktop Chromium with WebGL2. Hardware rendering is needed for useful
frame rates; CI uses software rendering for functional acceptance.
