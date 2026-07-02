mod commands;
mod events;
mod tray;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};
use tracing::info;

use screeny_core::{capture, Config, Engine, Store};

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,screeny=debug,screeny_core=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch: surface the existing window instead.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            info!(dir = %data_dir.display(), "app data directory");

            let config_path = data_dir.join("config.json");
            let config = Config::load_or_default(&config_path)?;
            let store = Arc::new(Store::open(&data_dir.join("screeny.db"))?);

            // Engine::start spawns tokio tasks, and Tauri's setup hook runs
            // outside the runtime context — enter it explicitly.
            let engine = tauri::async_runtime::block_on(async {
                Engine::start(
                    config,
                    config_path,
                    data_dir,
                    store,
                    Arc::new(capture::capture_primary),
                )
            });

            app.manage(engine.clone());
            tray::setup(app, engine.clone())?;
            events::forward_core_events(app.handle().clone(), engine);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray: hide the window, keep capturing in the background.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_config,
            commands::get_status,
            commands::set_run_state,
            commands::capture_now,
            commands::list_captures,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
