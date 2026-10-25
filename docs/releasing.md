# Linux releases

`.github/workflows/release.yml` builds Linux **x86_64** packages when a `v*` tag is pushed.
After validation, tests, packaging and installed-package launch checks succeed, it creates a
**draft** GitHub release with generated notes and three downloads:

- `megawatt-mogul_<Debian version>_amd64.deb`
- `megawatt-mogul_<version>_x86_64.flatpak`
- `SHA256SUMS`

Both packages contain the same optimized, locked-dependency game build, regions, translations,
custom icon and desktop entry. The Flatpak is a downloadable bundle, not a Flathub submission;
it uses the Freedesktop 25.08 runtime from Flathub. It permits graphics devices, Wayland and
fallback X11, without network access or access to the host's home directory.

## Versioning

`[workspace.package].version` in the root `Cargo.toml` is the only application-version setting.
All four crate manifests inherit it with `version.workspace = true`.
`game --version` and the installed `megawatt-mogul --version` report Cargo's version without
opening a window. Package filenames, AppStream release metadata and GitHub release titles
derive from the same workspace value.

The tag must be exactly `v<workspace version>`. A mismatched tag, a crate that stops inheriting
the workspace version, or stale workspace entries in `Cargo.lock` fails before the build.
Versions use SemVer, including prereleases such as `0.2.0-rc.1`. Debian represents that version
as `0.2.0~rc.1`, so it sorts before `0.2.0`; the Flatpak and executable retain `0.2.0-rc.1`.
Prereleases are also marked as prereleases in GitHub. Save-envelope, portable-schema and
simulation-rules versions remain independent compatibility counters, not application versions.

## Cut a release

1. Set the desired version in the root `Cargo.toml`. Refresh workspace versions in `Cargo.lock`
   with `cargo update --workspace` and commit both files along with the intended release changes.
2. Merge the changes and wait for CI and cache warming. Check out `main` with a clean checkout.
3. Run the release script with the version tag. For the initial version:

   ```sh
   ./tools/release.sh v0.1.0
   ```

   It fast-forwards from `origin/main`, validates the shared version, creates an annotated tag
   and pushes only that tag. It refuses dirty checkouts, other branches, unpublished local main
   commits and existing tags. The push runs the normal clippy/test hooks. If the push fails,
   the local tag remains; the script prints the command to retry the push.

4. Wait for **Linux release**, inspect its draft and test the downloaded packages on Linux.
   Publish the draft from GitHub when ready. The workflow never publishes it automatically.

Re-running the workflow uploads replacement assets to an existing draft for that tag. It refuses
to change an already-published release. Pull requests touching release tooling, packaging,
Cargo metadata or the Rust setup action exercise the packaging jobs without creating a release.

## Install and play

The Debian package is built on Ubuntu 24.04 and needs glibc 2.39 or newer (for example, Ubuntu
24.04+ or Debian 13+). Its ELF dependencies are calculated by `dpkg-shlibdeps`; libraries loaded
dynamically by the windowing and graphics backends are declared explicitly. A working Vulkan
graphics driver is required. `mesa-vulkan-drivers` is recommended for Mesa-supported hardware.

```sh
sudo apt install ./megawatt-mogul_0.1.0_amd64.deb
megawatt-mogul
```

For other distributions, install Flatpak and then:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user ./megawatt-mogul_0.1.0_x86_64.flatpak
flatpak run io.github.adsouza.MegawattMogul
```

The bundle points to Flathub for its runtime; the runtime is downloaded separately on first
installation. Both formats add Megawatt Mogul to the desktop application menu.

The launcher reads immutable assets from the package and stores saves/preferences in
`$XDG_DATA_HOME/megawatt-mogul/saves` and command logs in `$XDG_DATA_HOME/megawatt-mogul/logs`.
Outside Flatpak, `XDG_DATA_HOME` defaults to `~/.local/share`; Flatpak supplies its private
`~/.var/app/io.github.adsouza.MegawattMogul/data`. Native and Flatpak installations have separate
save libraries. Existing checkout-local `./saves` are not moved automatically. Explicit CLI
path flags are passed through and override launcher defaults.

## Local validation

The release helper uses Python 3.11+ and its standard library:

```sh
python3 -m unittest discover -s tools/release/tests
python3 tools/release/release.py metadata
actionlint .github/workflows/release.yml
shellcheck packaging/linux/megawatt-mogul.sh
```

On Linux, build with `cargo build --release --locked -p game`, then run the packaging commands
in the workflow. The helper's `deb` command requires `dpkg-dev` and `dpkg-deb` on the native
amd64 build host. Flatpak packages that same Ubuntu-built binary against a newer runtime;
installed-bundle version and graphical launch checks catch missing runtime libraries.
The shared `rust-release-build` action restores a dedicated completed-build cache, compiles the
game and verifies its version, then saves the cache immediately before packaging starts. Failed
or cancelled compilations never save a partial cache; later packaging failures preserve the
completed checkpoint. An exact cache hit still runs Cargo and verifies the executable version.

Keys include the runner platform, actual compiler/Cargo/linker identity, build flags, Cargo
configuration, manifests, lockfile and source revision. Fallback keys reuse compatible artifacts
from earlier revisions and dependency sets; Cargo validates which artifacts can be reused. Only
Cargo's registry/git downloads and `target/release` are cached, excluding test binaries and
Flatpak repositories. A fresh namespace supersedes the old partial release cache.

Relevant pushes to `main` run a build-only `warm-cache` job with the same action and configuration.
That default-branch cache can be restored by subsequent PRs and version tags. Warming does not
package or create a release. Before this workflow merges, its PR cache is available only to that
PR's subsequent runs. GitHub can evict unused or older caches; a cache miss always builds normally.
Flatpak's symbol processing requires `elfutils` in addition to `flatpak-builder`.
CI also validates desktop/AppStream metadata and launches both installed packages from outside
the checkout using Xvfb and Mesa. Debian's launch runs in a clean Ubuntu container so build
dependencies cannot hide missing runtime dependencies. The full Rust suite runs in CI for tags;
local focused checks do not imply permission to run the full local suite.

## Icon

`packaging/linux/icon.png` is the shared 512×512 RGBA package icon. The game window's application
ID, desktop filename, Flatpak ID and icon name all match `io.github.adsouza.MegawattMogul`.
See [icon provenance](../packaging/linux/icon.md) for the generation prompt.

Reference: [Flatpak bundles](https://docs.flatpak.org/en/latest/single-file-bundles.html),
[sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html),
[GitHub release creation](https://cli.github.com/manual/gh_release_create).
