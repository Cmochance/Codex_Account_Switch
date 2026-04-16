#[path = "../shared/runtime/cli.rs"]
mod cli;
#[path = "../shared/commands/mod.rs"]
mod commands;
#[path = "../shared/runtime/errors.rs"]
mod errors;
#[path = "../mac/runtime/mod.rs"]
mod macos;
#[path = "../shared/runtime/models.rs"]
mod models;
#[path = "../shared/platform/mod.rs"]
mod platform;
#[path = "../shared/runtime/mod.rs"]
mod shared;
#[path = "../win/runtime/windowing.rs"]
mod windowing;
#[path = "../win/runtime/mod.rs"]
mod windows;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            platform::setup_runtime()?;
            Ok(platform::install_windowing(app)?)
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
            commands::actions::open_releases,
            commands::actions::open_xiaohongshu,
            commands::switch::switch_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub fn run_cli(args: &[String]) -> i32 {
    cli::run(args, None)
}
