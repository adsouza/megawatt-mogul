#!/usr/bin/env bash
# Runs only in the disposable Ubuntu container in release.yml, without build dependencies.
set -euo pipefail
: "${VERSION:?Expected workspace version is required}"
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y \
    /packages/*.deb xvfb xauth mesa-vulkan-drivers
test "$(megawatt-mogul --version)" = "Megawatt Mogul $VERSION"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export XDG_DATA_HOME="$work/data"
export XDG_CONFIG_HOME="$work/config"
export XDG_CACHE_HOME="$work/cache"
export XDG_RUNTIME_DIR="$work/runtime"
mkdir -m 700 "$XDG_RUNTIME_DIR"
cd "$work"
timeout 120s xvfb-run -a -s '-screen 0 1280x720x24' \
    megawatt-mogul --exit-after-frames 120

# A crash must leave a report whose backtrace names the game's functions: the package keeps symbols.
if megawatt-mogul --crash 2> crash.err; then
    echo "megawatt-mogul --crash exited successfully" >&2
    exit 1
fi
grep -l 'game::crash::requested_crash' "$XDG_DATA_HOME"/megawatt-mogul/logs/crash-*.txt
