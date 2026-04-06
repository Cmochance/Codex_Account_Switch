mod commands;
mod errors;
mod models;
mod windowing;
mod windows;

pub fn run() {
    tauri::Builder::default()
        .manage(windowing::WindowSizingState::new())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            windows::bootstrap::ensure_backup_initialized(None)?;
            Ok(windowing::install(app)?)
        })
        .invoke_handler(tauri::generate_handler![
            commands::dashboard::get_dashboard,
            commands::actions::open_codex,
            commands::actions::login_current_profile,
            commands::actions::open_profile_folder,
            commands::actions::add_profile,
            commands::actions::open_contact,
            commands::switch::switch_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
