# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Screeny is a cross-platform (Windows/macOS; Linux deferred) Tauri 2 desktop
app: a privacy-first automatic screen archiver. It captures the screen on an
interval, stores a searchable local archive, optionally analyzes captures with
a local vision LLM (Ollama/LM Studio or any OpenAI-compatible endpoint), and
optionally delivers captures via email (any SMTP) or Telegram. The original
macOS-only Python script lives in `legacy/` as a behavior reference only.

## Commands

```bash
pnpm install                      # frontend deps (pnpm 9, Node 22)
pnpm tauri dev                    # run the app (vite + cargo)
pnpm build                        # build frontend into ./build
pnpm check                        # svelte-check (0 errors AND 0 warnings expected)
cargo test -p screeny-core --lib  # core unit tests (fast, headless-safe)
cargo test -p screeny-core --lib config::   # run one module's tests
cargo clippy --workspace          # lints (keep at zero warnings)
cargo fmt --all                   # format before committing
```

On Windows, `cargo` may need `$env:Path += ";$env:USERPROFILE\.cargo\bin"`.
CI (`.github/workflows/ci.yml`) runs fmt/clippy/tests + frontend build on
Windows/macOS/Ubuntu.

## Architecture

Cargo workspace with two crates plus a SvelteKit frontend:

- **`crates/screeny-core`** — the engine, deliberately Tauri-free so it's unit
  testable. Key modules:
  - `pipeline.rs` — the heart. `Engine::start(EngineOptions)` spawns four
    tokio loops: capture (drift-corrected `tokio::time::interval`, pause via
    watch channel), analysis (single consumer, bounded `mpsc(8)` — when full,
    analysis is *skipped* but delivery/storage still happen), delivery
    (per-sink batching + 2-attempt retry, failures recorded and broadcast,
    never block capture), and hourly retention pruning. Everything injectable:
    `CaptureFn`, `SinkFactory`, `AnalyzerFactory` — tests use fakes.
  - `store/` — rusqlite (WAL) with FTS5. `captures`, `analyses`, `deliveries`
    tables; `search_captures` quotes each term to keep FTS syntax injection-safe;
    list/search queries join the AI description and a per-sink
    `delivery_summary` ("email:sent,telegram:failed").
  - `llm/` — `LlmBackend` trait; Ollama native API (also streaming `/api/pull`
    for wizard progress) and OpenAI-compatible backend (LM Studio/OpenAI/
    custom). `prompts.rs` has a lenient parser because small models wrap JSON
    in prose/fences. Images are downscaled to 1280px before inference.
  - `sinks/` — `Sink` trait (`deliver(&[DeliveryItem])`, per-sink
    `batch_size`). Email via lettre (SSL/STARTTLS), Telegram via Bot API
    (photo w/ caption, >9.5MB → document, analysis-only → text message).
    `ContentMode` (image/analysis/both) controls what goes out.
  - `secrets.rs` — `SecretStore` trait: `KeyringStore` (OS keychain, service
    "screeny") in production, `MemoryStore` in tests. Config files never hold
    credentials.
- **`src-tauri`** — thin shell: tray (pause/resume/capture-now), IPC commands
  (`commands.rs`), event forwarding + failure notifications with
  enter-failing-state dedup (`events.rs`). Engine startup must be wrapped in
  `tauri::async_runtime::block_on` (setup hook runs outside the tokio context).
- **`src`** — Svelte 5 (runes) + SvelteKit static adapter in SPA mode.
  `lib/api.ts` mirrors the Rust types — keep both sides in sync when changing
  config/events. Dashboard listens to the single `core-event` Tauri event.
  First run redirects to `/onboarding` until `config.onboarding_complete`.

## Conventions and gotchas

- Config: versioned JSON in the app data dir, atomic write (temp+rename),
  `#[serde(default)]` everywhere so old configs load; always pass through
  `Config::sanitized()`. When adding fields, update: config.rs (+ its tests'
  struct literals), api.ts, settings UI.
- Events: one enum `CoreEvent` serialized `{type, data}` snake_case; frontend
  discriminates on `type`. Add new variants to api.ts's union.
- Tauri command args are camelCase in JS, snake_case in Rust (auto-converted).
- The asset protocol is scoped to `$APPDATA/captures/**` — thumbnails use
  `convertFileSrc`. New tauri features need both the Cargo feature flag and
  (for frontend-invoked plugins) a capability entry.
- Tests: `wiremock` mocks LLM/Telegram HTTP; sink/analyzer fakes test pipeline
  behavior (batching, retry, give-up, skip-on-full, failure isolation). Keep
  new core logic covered the same way.
- `pnpm check` must stay at 0 warnings (a11y rules included). Rust clippy at
  zero warnings (CI enforces `-D warnings`).
- Milestones M1–M4 are committed; remaining roadmap: WhatsApp sink (validate
  Meta Cloud API template constraints first), Linux pass (X11 → Wayland
  experimental), release packaging/signing (`release.yml`, tauri-action).
- macOS specifics are untested so far (built on Windows): Screen Recording
  TCC grants attach to the bundle signature; dev builds can lose the grant.
  Blank-capture detection exists in `capture.rs` (`looks_blank`).
