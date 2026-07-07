mod commands;
mod events;
mod tray;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};
use tracing::info;

use screeny_core::{capture, Config, Engine, EngineOptions, KeyringStore, Store};
use tauri_plugin_autostart::MacosLauncher;

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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
                Engine::start(EngineOptions {
                    config,
                    config_path,
                    data_dir,
                    store,
                    capture_fn: Arc::new(capture::capture_primary),
                    secrets: Arc::new(KeyringStore),
                    sink_factory: None,
                    analyzer_factory: None,
                })
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
            commands::get_analysis,
            commands::set_email_password,
            commands::email_password_set,
            commands::test_email,
            commands::get_autostart,
            commands::set_autostart,
            commands::detect_backends,
            commands::test_llm,
            commands::list_models,
            commands::pull_model,
            commands::search_captures,
            commands::set_llm_api_key,
            commands::llm_api_key_set,
            commands::set_telegram_token,
            commands::telegram_token_set,
            commands::test_telegram,
            commands::telegram_discover_chats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
