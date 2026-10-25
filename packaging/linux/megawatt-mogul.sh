#!/bin/sh
# Installed by release.py with /usr (Debian) or /app (Flatpak) as the prefix.
# Flatpak supplies an app-private XDG_DATA_HOME. Keep logs there as well so they persist.
data_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/megawatt-mogul"
exec "@PREFIX@/lib/megawatt-mogul/game" \
    --assets "@PREFIX@/share/megawatt-mogul/assets" \
    --save-dir "$data_dir/saves" \
    --log-dir "$data_dir/logs" \
    "$@"
