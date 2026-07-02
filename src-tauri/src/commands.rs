use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use screeny_core::{CaptureRow, Config, EmailSink, Engine, RunState, Sink, SMTP_PASSWORD};

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
