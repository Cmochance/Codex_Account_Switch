mod cli;
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
            windows::bootstrap::ensure_refresh_runtime_config_initialized(None)?;
            windows::bootstrap::sync_root_state_to_current_profile(None)?;
            windows::config::sync_root_openai_base_url_for_current_profile(None)?;
            windows::profiles_index::load_profiles_index(None)?;
            Ok(windowing::install(app)?)
        })
        .invoke_handler(tauri::generate_handler![
            commands::dashboard::get_profiles_snapshot,
            commands::dashboard::get_current_live_quota,
            commands::actions::open_codex,
            commands::actions::login_current_profile,
            commands::actions::refresh_profile,
            commands::actions::rename_profile,
            commands::actions::update_profile_base_url,
            commands::actions::open_profile_folder,
            commands::actions::add_profile,
            commands::actions::open_contact,
            commands::switch::switch_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub fn run_cli(args: &[String]) -> i32 {
    cli::run(args, None)
}
