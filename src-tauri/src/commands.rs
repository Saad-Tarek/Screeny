use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use screeny_core::{CaptureRow, Config, Engine, RunState};

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
