# Screeny

**A privacy-first automatic screen archiver with local AI analysis.**

Screeny quietly screenshots your screen on a schedule, keeps a searchable local
archive, and can optionally describe every capture with a *local* vision model
(OCR + summary) and deliver captures to your email inbox or a Telegram chat.

Built with Tauri 2 + Rust + Svelte. Runs on Windows and macOS (Linux support
planned).

## Why

Ever lost track of what you were working on, what an AI session showed you
before the context vanished, or needed a visual record of your day? Screeny is
a flight recorder for your screen — private by default, searchable by the text
that was actually on screen.

## Features

- **Automatic capture** — drift-corrected interval (default every 20 s),
  pause/resume from the system tray, "Capture now" on demand.
- **Local archive** — screenshots in dated folders + SQLite index in your app
  data directory, with configurable retention (days) and JPEG/PNG quality.
- **Local AI analysis (optional)** — every capture is OCR'd and described by a
  vision model via [Ollama](https://ollama.com) or LM Studio running on *your*
  machine. Nothing leaves your computer. Any OpenAI-compatible endpoint is also
  supported if you opt into the cloud.
- **Full-text search** — find any capture by the text that was on screen or by
  the AI's description.
- **Email delivery (optional)** — any SMTP provider (SSL/TLS or STARTTLS),
  batching to stay under provider limits, content modes: screenshots only,
  analysis text only, or both.
- **Telegram delivery (optional)** — official Bot API; screenshots with AI
  captions, or text-only analysis messages. Built-in chat-ID discovery.
- **Secrets in the OS keychain** — SMTP password, bot token, and API keys are
  stored in Windows Credential Manager / macOS Keychain, never in files.
- **Desktop notifications** on capture/delivery failures (deduplicated), tray
  app with close-to-tray, autostart at login, guided first-run wizard.

## Getting started (development)

Prerequisites: [Rust](https://rustup.rs), Node 20+, [pnpm](https://pnpm.io).
On Windows you also need the Visual Studio Build Tools C++ workload.

```bash
pnpm install
pnpm tauri dev      # run the app
cargo test -p screeny-core   # core unit tests
```

The first launch opens an onboarding wizard: test a capture, optionally set up
a local AI model (it can download one through Ollama with progress), and choose
autostart.

## Recommended local models

| Model | Size | Notes |
|---|---|---|
| `moondream` | ~1.7 GB | Smallest/fastest; good descriptions, basic OCR |
| `qwen2.5vl:3b` | ~3.2 GB | Balanced quality and speed |
| `qwen2.5vl:7b` | ~6 GB | Best OCR; needs 16 GB RAM or a GPU |

## Architecture

```
crates/screeny-core   # pure Rust engine (no Tauri): capture scheduler,
                      # SQLite+FTS5 store, LLM backends, delivery sinks
src-tauri             # Tauri 2 shell: tray, IPC commands, notifications
src                   # Svelte 5 frontend: dashboard, settings, onboarding
legacy                # original macOS-only Python script (reference)
```

The pipeline is `capture → analyze → deliver`: a bounded analysis queue skips
AI analysis (never delivery or storage) when local inference is slower than
the capture interval, and a failing delivery channel never blocks capturing —
your local archive is always the source of truth.

## Privacy

- Local-only by default: no network calls unless you enable a channel or a
  cloud LLM endpoint.
- Screenshots may contain sensitive information. Treat delivery channels and
  cloud endpoints accordingly.
- Secrets live in your OS keychain; the config file stores no credentials.

## Roadmap

- WhatsApp delivery (Meta Cloud API)
- Linux support (X11 first, Wayland experimental)
- Signed installers via GitHub Releases
