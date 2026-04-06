mod commands;
mod errors;
mod models;
mod windows;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::dashboard::get_dashboard,
            commands::actions::open_codex,
            commands::actions::open_profile_folder,
            commands::actions::add_profile,
            commands::actions::open_contact,
            commands::switch::switch_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
