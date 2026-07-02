#!/bin/bash
# Double-click to STOP the archiver and remove it from auto-start at login.
# Your script and .env stay in place; re-run start_service.command anytime.

LABEL="com.screeny.archiver"
DEST="$HOME/Library/LaunchAgents/${LABEL}.plist"

echo "Stopping service..."
launchctl bootout "gui/$(id -u)/${LABEL}" 2>/dev/null
rm -f "$DEST"

echo "Service stopped and removed from login items."
echo "(Re-run start_service.command to turn it back on.)"
echo ""
echo "You can close this window. Press any key to exit."
read -n 1 -s
