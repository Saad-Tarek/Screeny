#!/bin/bash
# Double-click to INSTALL + START the archiver as a background service.
# After this, it auto-starts every time you log in. You can close everything
# and forget it. Use stop_service.command to turn it off.

LABEL="com.screeny.archiver"
SRC="$(cd "$(dirname "$0")" && pwd)/${LABEL}.plist"
DEST="$HOME/Library/LaunchAgents/${LABEL}.plist"

echo "Installing service from:"
echo "  $SRC"

mkdir -p "$HOME/Library/LaunchAgents"
cp -f "$SRC" "$DEST"

# Remove any previous instance, then load the fresh one.
launchctl bootout "gui/$(id -u)/${LABEL}" 2>/dev/null
launchctl bootstrap "gui/$(id -u)" "$DEST"
launchctl enable "gui/$(id -u)/${LABEL}"
launchctl kickstart -k "gui/$(id -u)/${LABEL}"

echo ""
echo "Service installed and started."
echo "It will now also auto-start at every login."
echo ""
echo "First run only: macOS may ask to grant Screen Recording permission."
echo "If screenshots are black, enable it for python3 in:"
echo "  System Settings > Privacy & Security > Screen Recording"
echo ""
echo "Live log: tail -f \"$(cd "$(dirname "$0")" && pwd)/archiver.log\""
echo ""
echo "You can close this window. Press any key to exit."
read -n 1 -s
