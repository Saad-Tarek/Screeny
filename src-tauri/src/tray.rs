use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, Manager, Runtime};

use screeny_core::{Engine, RunState};

fn toggle_label(state: RunState) -> &'static str {
    match state {
        RunState::Running => "Pause capturing",
        RunState::Paused => "Resume capturing",
    }
}

pub fn setup<R: Runtime>(app: &App<R>, engine: Arc<Engine>) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        toggle_label(engine.state()),
        true,
        None::<&str>,
    )?;
    let capture_now = MenuItem::with_id(app, "capture_now", "Capture now", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Screeny", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Screeny", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&open, &capture_now, &toggle, &separator, &quit])?;

    let toggle_handle = toggle.clone();
    TrayIconBuilder::with_id("main")
        .icon(
            app.default_window_icon()
                .expect("bundle is missing a window icon")
                .clone(),
        )
        .tooltip("Screeny — screen archiver")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => {
                let next = engine.toggle();
                let _ = toggle_handle.set_text(toggle_label(next));
            }
            "capture_now" => {
                let engine = engine.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = engine.capture_now().await;
                });
            }
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
