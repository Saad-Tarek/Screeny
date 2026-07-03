use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

use screeny_core::{
    backend_from_config, detect_local_backends, CaptureRow, Config, DetectResult, DiscoveredChat,
    EmailSink, Engine, LlmBackendKind, OllamaBackend, RunState, Sink, TelegramSink, LLM_API_KEY,
    SMTP_PASSWORD, TELEGRAM_BOT_TOKEN,
};

type EngineState<'a> = State<'a, Arc<Engine>>;

#[derive(Serialize)]
pub struct Status {
    pub state: RunState,
    pub interval_seconds: u64,
    pub total_captures: i64,
}

#[tauri::command]
pub fn get_config(engine: EngineState) -> Config {
    engine.config()
}

#[tauri::command]
pub fn set_config(engine: EngineState, config: Config) -> Result<Config, String> {
    engine.set_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_status(engine: EngineState) -> Result<Status, String> {
    Ok(Status {
        state: engine.state(),
        interval_seconds: engine.config().capture.interval_seconds,
        total_captures: engine.store().capture_count().map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
pub fn set_run_state(engine: EngineState, running: bool) -> RunState {
    let state = if running {
        RunState::Running
    } else {
        RunState::Paused
    };
    engine.set_state(state);
    state
}

#[tauri::command]
pub async fn capture_now(engine: EngineState<'_>) -> Result<(), String> {
    engine.capture_now().await.map_err(|e| e.to_string())
}

/// Store (or clear, when empty) the SMTP password in the OS keychain.
#[tauri::command]
pub async fn set_email_password(engine: EngineState<'_>, password: String) -> Result<(), String> {
    let secrets = engine.secrets().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = password.trim();
        if trimmed.is_empty() {
            secrets.delete(SMTP_PASSWORD)
        } else {
            secrets.set(SMTP_PASSWORD, trimmed)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn email_password_set(engine: EngineState<'_>) -> Result<bool, String> {
    let secrets = engine.secrets().clone();
    tauri::async_runtime::spawn_blocking(move || secrets.is_set(SMTP_PASSWORD))
        .await
        .map_err(|e| e.to_string())
}

/// Send a small test email with the *pending* (unsaved) email settings so
/// users can verify before committing them.
#[tauri::command]
pub async fn test_email(engine: EngineState<'_>, config: Config) -> Result<(), String> {
    let sink = EmailSink::new(config.sanitized().channels.email, engine.secrets().clone());
    sink.test().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|e| e.to_string())
    } else {
        launcher.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn set_telegram_token(engine: EngineState<'_>, token: String) -> Result<(), String> {
    let secrets = engine.secrets().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            secrets.delete(TELEGRAM_BOT_TOKEN)
        } else {
            secrets.set(TELEGRAM_BOT_TOKEN, trimmed)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn telegram_token_set(engine: EngineState<'_>) -> Result<bool, String> {
    let secrets = engine.secrets().clone();
    tauri::async_runtime::spawn_blocking(move || secrets.is_set(TELEGRAM_BOT_TOKEN))
        .await
        .map_err(|e| e.to_string())
}

/// Send a test message with the pending (unsaved) Telegram settings.
#[tauri::command]
pub async fn test_telegram(engine: EngineState<'_>, config: Config) -> Result<(), String> {
    let sink = TelegramSink::new(
        config.sanitized().channels.telegram,
        engine.secrets().clone(),
    );
    sink.test().await.map_err(|e| e.to_string())
}

/// List chats the bot has recently seen (user must message the bot first).
#[tauri::command]
pub async fn telegram_discover_chats(
    engine: EngineState<'_>,
) -> Result<Vec<DiscoveredChat>, String> {
    let sink = TelegramSink::new(engine.config().channels.telegram, engine.secrets().clone());
    sink.discover_chats().await.map_err(|e| e.to_string())
}

/// Probe localhost for Ollama / LM Studio and report installed models.
#[tauri::command]
pub async fn detect_backends() -> DetectResult {
    detect_local_backends(None, None).await
}

/// List models on the backend described by the (possibly unsaved) config.
#[tauri::command]
pub async fn list_models(engine: EngineState<'_>, config: Config) -> Result<Vec<String>, String> {
    let llm = config.sanitized().llm;
    let backend = backend_from_config(&llm, engine.secrets().clone());
    backend.list_models().await.map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
struct PullProgressEvent {
    model: String,
    status: String,
    total: Option<u64>,
    completed: Option<u64>,
}

/// Download a model through Ollama, emitting `pull-progress` events so the
/// onboarding wizard can render a progress bar.
#[tauri::command]
pub async fn pull_model(
    app: AppHandle,
    engine: EngineState<'_>,
    model: String,
) -> Result<(), String> {
    let llm = engine.config().llm;
    let base_url = if llm.backend == LlmBackendKind::Ollama {
        llm.base_url
    } else {
        LlmBackendKind::Ollama.default_base_url().to_string()
    };
    let backend = OllamaBackend::new(base_url);
    let model_name = model.clone();
    backend
        .pull_model(&model, move |progress| {
            let _ = app.emit(
                "pull-progress",
                PullProgressEvent {
                    model: model_name.clone(),
                    status: progress.status,
                    total: progress.total,
                    completed: progress.completed,
                },
            );
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_captures(
    engine: EngineState,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<CaptureRow>, String> {
    engine
        .store()
        .search_captures(&query, limit.unwrap_or(100).min(500))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_llm_api_key(engine: EngineState<'_>, key: String) -> Result<(), String> {
    let secrets = engine.secrets().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            secrets.delete(LLM_API_KEY)
        } else {
            secrets.set(LLM_API_KEY, trimmed)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn llm_api_key_set(engine: EngineState<'_>) -> Result<bool, String> {
    let secrets = engine.secrets().clone();
    tauri::async_runtime::spawn_blocking(move || secrets.is_set(LLM_API_KEY))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_captures(
    engine: EngineState,
    limit: Option<u32>,
    before_id: Option<i64>,
) -> Result<Vec<CaptureRow>, String> {
    engine
        .store()
        .list_captures(limit.unwrap_or(60).min(500), before_id)
        .map_err(|e| e.to_string())
}
