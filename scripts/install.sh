#!/bin/sh
# Build a release of Agent Tower and install it for this user. No root access is needed.
#
#   program:   ~/.local/opt/agent-tower/agent-tower   (with tower-hook next to it)
#   command:   ~/.local/bin/agent-tower
#   app menu:  ~/.local/share/applications/agent-tower.desktop
#   icon:      ~/.local/share/icons/hicolor/<size>/apps/agent-tower.png
#
# Run it again to update. `scripts/install.sh remove` removes the installed files. The config and
# the floors stay: they are in ~/.config and in each project folder.
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
opt="$HOME/.local/opt/agent-tower"
bin="$HOME/.local/bin"
entry="$data/applications/agent-tower.desktop"

if [ "${1:-}" = "remove" ]; then
    rm -rf "$opt"
    rm -f "$bin/agent-tower" "$entry"
    for size in 32 128 256 512; do
        rm -f "$data/icons/hicolor/${size}x${size}/apps/agent-tower.png"
    done
    update-desktop-database "$data/applications" 2>/dev/null || true
    echo "Removed Agent Tower. The config and the floors stay."
    exit 0
fi

cd "$repo"
pnpm --dir ui build
cargo build --release --workspace --features tauri/custom-protocol

# Copy to a new name and rename: a copy of the app that runs now keeps its old file.
mkdir -p "$opt" "$bin" "$data/applications"
for program in agent-tower tower-hook; do
    cp "target/release/$program" "$opt/$program.new"
    chmod 755 "$opt/$program.new"
    mv -f "$opt/$program.new" "$opt/$program"
done
ln -sfn "$opt/agent-tower" "$bin/agent-tower"

icon() {
    dir="$data/icons/hicolor/$1x$1/apps"
    mkdir -p "$dir"
    cp "$2" "$dir/agent-tower.png"
}
icon 32 src-tauri/icons/32x32.png
icon 128 src-tauri/icons/128x128.png
icon 256 "src-tauri/icons/128x128@2x.png"
icon 512 src-tauri/icons/icon.png
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true

cat > "$entry" <<ENTRY
[Desktop Entry]
Type=Application
Name=Agent Tower
Comment=Run a team of agent CLI sessions
Exec=$opt/agent-tower
Icon=agent-tower
Terminal=false
Categories=Development;
StartupWMClass=agent-tower
ENTRY
update-desktop-database "$data/applications" 2>/dev/null || true
echo "Installed $opt/agent-tower"
echo "Start it from the app menu (Agent Tower), or with: agent-tower"
