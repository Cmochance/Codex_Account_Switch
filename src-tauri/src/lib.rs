use tauri::Manager;

#[path = "../shared/runtime/cli.rs"]
pub mod cli;
#[path = "../shared/commands/mod.rs"]
pub mod commands;
#[path = "../shared/runtime/errors.rs"]
pub mod errors;
#[cfg(target_os = "macos")]
#[path = "../mac/runtime/mod.rs"]
pub mod macos;
#[path = "../shared/runtime/models.rs"]
pub mod models;
#[path = "../shared/platform/mod.rs"]
pub mod platform;
#[path = "../shared/runtime/mod.rs"]
pub mod shared;
#[cfg(not(target_os = "macos"))]
#[path = "../win/runtime/windowing.rs"]
pub mod windowing;
#[cfg(not(target_os = "macos"))]
#[path = "../win/runtime/mod.rs"]
pub mod windows;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            platform::setup_runtime()?;
            Ok(platform::install_windowing(app)?)
        })
        .invoke_handler(tauri::generate_handler![
            commands::dashboard::get_profiles_snapshot,
            commands::dashboard::get_current_live_quota,
            commands::dashboard::refresh_active_profile_quota_silent,
            commands::dashboard::refresh_all_oauth_profile_plans_silent,
            commands::actions::open_codex,
            commands::actions::login_current_profile,
            commands::actions::login_profile,
            commands::actions::refresh_profile,
            commands::actions::rename_profile,
            commands::actions::delete_profile,
            commands::actions::clear_profile_account,
            commands::actions::update_profile_base_url,
            commands::actions::open_profile_folder,
            commands::actions::add_profile,
            commands::actions::open_contact,
            commands::actions::open_releases,
            commands::actions::open_url,
            commands::actions::check_update,
            commands::actions::open_xiaohongshu,
            commands::actions::get_codex_cli_status,
            commands::actions::set_codex_cli_path,
            commands::actions::clear_codex_cli_path,
            commands::actions::redetect_codex_cli_path,
            commands::actions::cancel_codex_login,
            commands::switch::switch_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub fn run_cli(args: &[String]) -> i32 {
    cli::run(args, None)
}
