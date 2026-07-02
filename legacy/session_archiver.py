#!/usr/bin/env python3
"""
AI Session Archiver
===================

Periodically screenshots your Mac's main display and emails each capture to
your Gmail inbox, so you always have a visual record of your projects and
prompts — even after Claude compacts and loses context.

- Uses macOS's built-in `screencapture` (no extra tools to install).
- Sends mail via Gmail SMTP using an App Password (pure Python stdlib).
- Captures 6 shots/minute (every 10s) and emails each one individually.

SETUP (one time)
----------------
1. Enable 2-Step Verification on your Google account:
       https://myaccount.google.com/security
2. Create a Gmail App Password (16 chars, e.g. "abcd efgh ijkl mnop"):
       https://myaccount.google.com/apppasswords
3. Export your credentials in the terminal BEFORE running (don't hardcode them):
       export ARCHIVER_GMAIL="saad.elmisery@gmail.com"
       export ARCHIVER_APP_PASSWORD="abcdefghijklmnop"   # no spaces
       # optional: send to a different address
       export ARCHIVER_RECIPIENT="saad.elmisery@gmail.com"
4. Grant Screen Recording permission to your terminal app the first time:
       System Settings > Privacy & Security > Screen Recording > enable Terminal
   (Until granted, screenshots come out black.)

RUN
---
    python3 session_archiver.py

Stop anytime with Ctrl+C.
"""

import os
import sys
import time
import smtplib
import ssl
import subprocess
import tempfile
from datetime import datetime
from email.message import EmailMessage
from pathlib import Path

def _load_dotenv(path: Path) -> None:
    """Load KEY=VALUE lines from a .env file into os.environ (no overwrite).

    Tiny stdlib parser — supports `KEY=value`, `export KEY=value`, comments
    (#), and optional surrounding single/double quotes. Existing env vars win.
    """
    if not path.exists():
        return
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export "):]
        if "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        if key and key not in os.environ:
            os.environ[key] = value


# Load .env sitting next to this script before reading any config below.
_load_dotenv(Path(__file__).resolve().parent / ".env")

# ----------------------------- Config ---------------------------------------
GMAIL_ADDRESS = os.environ.get("ARCHIVER_GMAIL", "saad.elmisery@gmail.com")
# Strip spaces — Gmail shows App Passwords as "abcd efgh ijkl mnop" but the
# real value has none, so we remove any whitespace the user pasted.
GMAIL_APP_PASSWORD = (os.environ.get("ARCHIVER_APP_PASSWORD") or "").replace(" ", "") or None
RECIPIENT = os.environ.get("ARCHIVER_RECIPIENT", GMAIL_ADDRESS)

SHOTS_PER_MINUTE = int(os.environ.get("ARCHIVER_SHOTS_PER_MINUTE", "3"))
SHOTS_PER_MINUTE = max(1, SHOTS_PER_MINUTE)   # at least 1/min
INTERVAL_SECONDS = 60.0 / SHOTS_PER_MINUTE

# How many shots to bundle per email. 1 = send each shot immediately.
# Raise it (e.g. 30) to bundle many shots into one email and stay under
# Gmail's ~500 emails/day limit.
BATCH_SIZE = max(1, int(os.environ.get("ARCHIVER_BATCH_SIZE", "1")))

# Keep a local copy as a backup that survives even if email fails.
KEEP_LOCAL = True
SAVE_DIR = Path.home() / "SessionArchive"

SUBJECT_PREFIX = "Session Archive"
SMTP_HOST = "smtp.gmail.com"
SMTP_PORT = 465                            # SSL
# ----------------------------------------------------------------------------


def capture_screenshot(dest: Path) -> bool:
    """Capture the main display to `dest` (PNG). Returns True on success."""
    try:
        # -x: silent (no shutter sound)   -m: main display only
        subprocess.run(
            ["screencapture", "-x", "-m", str(dest)],
            check=True,
            capture_output=True,
        )
        return dest.exists() and dest.stat().st_size > 0
    except (subprocess.CalledProcessError, FileNotFoundError) as exc:
        print(f"[!] screencapture failed: {exc}", file=sys.stderr)
        return False


def build_message(images: list[Path], when: datetime) -> EmailMessage:
    """Build an email with one or more PNG screenshots attached."""
    msg = EmailMessage()
    stamp = when.strftime("%Y-%m-%d %H:%M:%S")
    count = len(images)
    suffix = f" ({count} shots)" if count > 1 else ""
    msg["Subject"] = f"{SUBJECT_PREFIX} — {stamp}{suffix}"
    msg["From"] = GMAIL_ADDRESS
    msg["To"] = RECIPIENT
    msg.set_content(
        f"Automated screen archive.\n"
        f"Captured: {stamp}\n"
        f"Screenshots attached: {count}\n"
    )
    for img in images:
        try:
            data = img.read_bytes()
        except OSError as exc:
            print(f"[!] could not read {img}: {exc}", file=sys.stderr)
            continue
        msg.add_attachment(
            data, maintype="image", subtype="png", filename=img.name
        )
    return msg


def send_email(server: smtplib.SMTP_SSL, msg: EmailMessage) -> None:
    server.send_message(msg)


def connect() -> smtplib.SMTP_SSL:
    context = ssl.create_default_context()
    server = smtplib.SMTP_SSL(SMTP_HOST, SMTP_PORT, context=context, timeout=30)
    server.login(GMAIL_ADDRESS, GMAIL_APP_PASSWORD)
    return server


def flush_batch(batch: list[Path], when: datetime) -> None:
    """Send the queued screenshots, reconnecting on transient failures."""
    if not batch:
        return
    msg = build_message(batch, when)
    for attempt in (1, 2):
        try:
            server = connect()
            try:
                send_email(server, msg)
            finally:
                server.quit()
            print(f"[+] sent {len(batch)} screenshot(s) at "
                  f"{when.strftime('%H:%M:%S')}")
            return
        except (smtplib.SMTPException, ssl.SSLError, OSError) as exc:
            print(f"[!] send attempt {attempt} failed: {exc}", file=sys.stderr)
            time.sleep(2)
    print("[!] giving up on this batch (will keep capturing).", file=sys.stderr)


def main() -> int:
    if not GMAIL_APP_PASSWORD:
        print(
            "ERROR: set ARCHIVER_APP_PASSWORD to your Gmail App Password.\n"
            "See the setup notes at the top of this file.",
            file=sys.stderr,
        )
        return 1

    if KEEP_LOCAL:
        SAVE_DIR.mkdir(parents=True, exist_ok=True)
    work_dir = SAVE_DIR if KEEP_LOCAL else Path(tempfile.gettempdir())

    print(f"AI Session Archiver running.")
    print(f"  capturing {SHOTS_PER_MINUTE}/min (every {INTERVAL_SECONDS:.0f}s), "
          f"main display")
    print(f"  emailing {GMAIL_ADDRESS} -> {RECIPIENT}, batch size {BATCH_SIZE}")
    print(f"  local copies: {SAVE_DIR if KEEP_LOCAL else 'off'}")
    print("  Ctrl+C to stop.\n")

    batch: list[Path] = []
    batch_started: datetime | None = None

    try:
        while True:
            cycle_start = time.monotonic()
            now = datetime.now()
            shot_path = work_dir / f"screen_{now.strftime('%Y%m%d_%H%M%S')}.png"

            if capture_screenshot(shot_path):
                batch.append(shot_path)
                if batch_started is None:
                    batch_started = now
            else:
                print("[!] skipped a capture (permission? display asleep?)",
                      file=sys.stderr)

            if len(batch) >= BATCH_SIZE:
                flush_batch(batch, batch_started or now)
                if not KEEP_LOCAL:
                    for p in batch:
                        p.unlink(missing_ok=True)
                batch = []
                batch_started = None

            # Sleep the remainder of the interval (accounts for capture/send time).
            elapsed = time.monotonic() - cycle_start
            time.sleep(max(0.0, INTERVAL_SECONDS - elapsed))
    except KeyboardInterrupt:
        print("\n[+] stopping; flushing any pending screenshots...")
        flush_batch(batch, batch_started or datetime.now())
        print("[+] done.")
        return 0


if __name__ == "__main__":
    sys.exit(main())
