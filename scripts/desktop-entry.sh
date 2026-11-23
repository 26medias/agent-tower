#!/bin/sh
# Install a desktop entry for Agent Tower for this user, so that the panel and the launcher show
# the app icon. Run it one time: `pnpm run desktop-entry`. Remove the entry with the argument
# `remove`.
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
file="$dir/agent-tower.desktop"
if [ "${1:-}" = "remove" ]; then
    rm -f "$file"
    echo "Removed $file"
    exit 0
fi
mkdir -p "$dir"
cat > "$file" <<ENTRY
[Desktop Entry]
Type=Application
Name=Agent Tower
Comment=Run a team of agent CLI sessions
Exec=$repo/target/debug/agent-tower
Icon=$repo/src-tauri/icons/icon.png
Terminal=false
Categories=Development;
StartupWMClass=agent-tower
ENTRY
update-desktop-database "$dir" 2>/dev/null || true
echo "Installed $file"
