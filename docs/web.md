# Browser builds and GitHub Pages

The browser client shares the native Rust simulation and egui UI. Plan 9a is a preview:
progress is not retained across reloads until the IndexedDB adapter in Plan 9c ships.

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
Run `npm ci`, `npx playwright install chromium`, then `npm test` there while the local
server runs. Each test gets its own browser context; no personal saves are accessed.

The CI `browser` job builds the production artifact and tests the project subpath.
`deploy-pages` depends on `check`, `dev-tools` and `browser`. It publishes only the current
head of `main`, serializes deployments, and uses the `github-pages` environment. Repository
Pages must use GitHub Actions as its publishing source. A manual CI dispatch on `main`
rebuilds the current revision. To roll back source, revert the offending changes on `main`
and let the same checks/deployment run; an old workflow cannot bypass the head guard.

WASM/JS and runtime assets are content-addressed. A stale page or mismatched asset inventory
shows Reload; browser data is never cleared by deployment. This release has no service worker
and does not promise offline startup. Desktop keyboard/mouse and a 1280×640 content area
are the initial interface target; the published browser support matrix awaits runtime evidence.
