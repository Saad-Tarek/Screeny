use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tracing::warn;

use screeny_core::Engine;

/// Forwards engine broadcast events to the frontend as a single `core-event`
/// Tauri event (payload is the serialized CoreEvent enum).
pub fn forward_core_events(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut rx = engine.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(e) = app.emit("core-event", &event) {
                        warn!(error = %e, "failed to emit core event");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(skipped, "frontend event forwarder lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
