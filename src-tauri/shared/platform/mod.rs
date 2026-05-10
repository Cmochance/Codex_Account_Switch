pub mod hooks;

use std::path::Path;

use crate::errors::AppResult;
use crate::shared::codex_app_server::AppServerSnapshot;
use hooks::PlatformHooks;
use tauri::App;

#[cfg(target_os = "macos")]
fn platform_hooks_impl() -> &'static dyn PlatformHooks {
    crate::macos::process::platform_hooks()
}

#[cfg(not(target_os = "macos"))]
fn platform_hooks_impl() -> &'static dyn PlatformHooks {
    crate::windows::process::platform_hooks()
}

pub fn current_hooks() -> &'static dyn PlatformHooks {
    platform_hooks_impl()
}

#[cfg(target_os = "macos")]
pub fn setup_runtime() -> AppResult<()> {
    crate::macos::bootstrap::ensure_backup_initialized(None)?;
    crate::macos::bootstrap::ensure_refresh_runtime_config_initialized(None)?;
    crate::macos::bootstrap::sync_root_state_to_current_profile(None)?;
    current_hooks().sync_root_openai_base_url_for_current_profile(None)?;
    crate::shared::profiles_index::load_profiles_index(None)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn setup_runtime() -> AppResult<()> {
    crate::windows::bootstrap::ensure_backup_initialized(None)?;
    crate::windows::bootstrap::ensure_refresh_runtime_config_initialized(None)?;
    crate::windows::bootstrap::sync_root_state_to_current_profile(None)?;
    current_hooks().sync_root_openai_base_url_for_current_profile(None)?;
    crate::shared::profiles_index::load_profiles_index(None)?;
    Ok(())
}

pub fn open_or_activate_codex_app(codex_home: Option<&Path>) -> AppResult<String> {
    current_hooks().open_or_activate_codex_app(codex_home)
}

pub fn run_codex_login(cli_codex_home: &Path, runtime_codex_home: &Path) -> AppResult<()> {
    current_hooks().run_codex_login(cli_codex_home, runtime_codex_home)
}

pub fn fetch_account_via_app_server(
    cli_codex_home: &Path,
    runtime_codex_home: &Path,
) -> AppResult<AppServerSnapshot> {
    current_hooks().fetch_account_via_app_server(cli_codex_home, runtime_codex_home)
}

pub fn sync_on_window_close() -> AppResult<()> {
    current_hooks().sync_on_window_close()
}

#[cfg(target_os = "macos")]
pub fn install_windowing(app: &mut App) -> tauri::Result<()> {
    crate::macos::windowing::install(app)
}

#[cfg(not(target_os = "macos"))]
pub fn install_windowing(app: &mut App) -> tauri::Result<()> {
    crate::windowing::install(app)
}
