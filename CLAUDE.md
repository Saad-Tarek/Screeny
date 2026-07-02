# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Screeny is a single-file Python tool (`session_archiver.py`) that periodically
screenshots the Mac's main display and emails each capture to a Gmail inbox — a
visual record of work that survives context loss. It uses only the Python
standard library plus macOS's built-in `screencapture`; there are no third-party
dependencies, no build step, and no test suite.

## Run / operate

```bash
python3 session_archiver.py        # run in foreground; Ctrl+C to stop
```

Double-clickable Finder wrappers (all target macOS):
- `upwork.command` — run in a Terminal window (`exec python3 session_archiver.py`).
- `start_service.command` — install `com.screeny.archiver.plist` as a launchd
  LaunchAgent (auto-start at login, `KeepAlive` restart on crash) and kickstart it.
- `stop_service.command` — bootout and remove the LaunchAgent.

Logs (when run as a service) go to `archiver.log` in the working directory.

## Configuration

All config is environment variables, auto-loaded from a `.env` file next to the
script by the tiny `_load_dotenv` parser (existing env vars always win). Copy
`.env.example` to `.env` and fill in. Requires a Gmail **App Password** (2-Step
Verification must be on); the script strips spaces from the pasted password.

Key vars: `ARCHIVER_GMAIL`, `ARCHIVER_APP_PASSWORD` (required),
`ARCHIVER_RECIPIENT`, `ARCHIVER_SHOTS_PER_MINUTE`, `ARCHIVER_BATCH_SIZE`.
Raise `ARCHIVER_BATCH_SIZE` to bundle shots into fewer emails and stay under
Gmail's ~500 emails/day limit.

## Architecture notes

The capture→email loop in `main()` is drift-corrected: it measures each cycle
with `time.monotonic()` and sleeps only the *remainder* of the interval, so
capture and SMTP time don't accumulate into slippage. Screenshots queue into a
`batch` list and flush to one `EmailMessage` when `BATCH_SIZE` is reached (and on
Ctrl+C). `flush_batch` reconnects and retries SMTP twice on transient failures,
then gives up on that batch rather than crashing the loop. `KEEP_LOCAL` keeps a
copy of every shot in `~/SessionArchive` so captures survive email failure.

## Gotchas

- **Platform mismatch:** the code is macOS-only (`screencapture`, launchd) but
  this repo is checked out on Windows. You cannot actually run it here — reason
  about the code statically or test on a Mac.
- **`com.screeny.archiver.plist` has hardcoded absolute paths** for a specific
  user (`/Users/Noha/Desktop/screeny`, `/opt/homebrew/bin/python3`). Anyone
  installing on another machine must edit the plist paths to match.
- `.env`, `SessionArchive/`, `*.png`, and `*.log` are gitignored — never commit
  secrets or captured screenshots.
