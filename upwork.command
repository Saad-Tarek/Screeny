#!/bin/bash
# Double-click this file in Finder to start the AI Session Archiver.
# A Terminal window opens and runs the script. To STOP it: press Ctrl+C
# in that window, or just close the window.

# Move into this file's own folder so it finds session_archiver.py and .env
cd "$(dirname "$0")" || exit 1

echo "=============================================="
echo "  AI Session Archiver"
echo "  Capturing your screen and emailing it."
echo ""
echo "  To STOP: press Ctrl+C, or close this window."
echo "=============================================="
echo ""

# 'exec' replaces this shell with Python, so closing the window kills it cleanly.
exec python3 session_archiver.py
