#![cfg_attr(target_os = "macos", allow(dead_code))]

use tauri::{utils::config::Color, App, Manager, WindowEvent};

const WINDOW_BG: Color = Color(244, 241, 236, 255);

pub fn install(app: &mut App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let _ = window.set_background_color(Some(WINDOW_BG));

    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { .. } = event {
            // Last persistence opportunity before the app goes away —
            // a swallowed failure here silently loses the session's
            // quota / auth write-back, so leave a trace.
            if let Err(error) = crate::platform::sync_on_window_close() {
                eprintln!("codex_switch: window-close sync failed: {}", error.message);
            }
        }
    });

    Ok(())
}
