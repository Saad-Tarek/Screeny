use std::collections::HashSet;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tracing::warn;

use screeny_core::{CoreEvent, Engine};

/// Forwards engine broadcast events to the frontend as a single `core-event`
/// Tauri event, and raises desktop notifications on failures. Notifications
/// fire only when a source *enters* a failing state (not every interval) and
/// re-arm once it recovers.
pub fn forward_core_events(app: AppHandle, engine: Arc<Engine>) {
    tauri::async_runtime::spawn(async move {
        let mut rx = engine.subscribe();
        let mut failing: HashSet<String> = HashSet::new();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    notify_on_state_change(&app, &event, &mut failing);
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

fn notify_on_state_change(app: &AppHandle, event: &CoreEvent, failing: &mut HashSet<String>) {
    match event {
        CoreEvent::CaptureFailed { message } => {
            if failing.insert("capture".into()) {
                notify(app, "Screeny can't capture the screen", message);
            }
        }
        CoreEvent::CaptureTaken(_) => {
            failing.remove("capture");
        }
        CoreEvent::DeliveryFailed { sink, message, .. } => {
            if failing.insert(format!("sink:{}", sink.as_str())) {
                notify(
                    app,
                    &format!("Screeny {} delivery failed", sink.as_str()),
                    &format!("{message} — captures are kept locally."),
                );
            }
        }
        CoreEvent::DeliverySucceeded { sink, .. } => {
            failing.remove(&format!("sink:{}", sink.as_str()));
        }
        _ => {}
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let result = app.notification().builder().title(title).body(body).show();
    if let Err(e) = result {
        warn!(error = %e, "failed to show notification");
    }
}
