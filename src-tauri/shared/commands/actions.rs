use crate::errors::CommandError;
use crate::models::{
    ActionResponse, AddProfilePayload, CodexCliRedetectResult, CodexCliStatus, OpenUrlPayload,
    ProfilePayload, RenameProfilePayload, SetCodexCliPathPayload, UpdateCheckPayload,
    UpdateCheckResponse, UpdateProfileBaseUrlPayload,
};

#[cfg(target_os = "macos")]
use crate::macos as platform_runtime;

#[cfg(not(target_os = "macos"))]
use crate::windows as platform_runtime;

#[tauri::command]
pub fn open_codex() -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_codex_app()?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened Codex.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn login_current_profile() -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::login_current_profile()?;
    Ok(ActionResponse {
        ok: true,
        message: "Logged in current profile.".to_string(),
        path: Some(path),
    })
}

/// Per-card login: drives `codex login` against a sandboxed CODEX_HOME so
/// the OAuth handshake writes a fresh `auth.json` for `payload.profile`,
/// even when that profile is not the currently active one. Avoids the
/// switch-then-login-then-switch round-trip the dashboard used to require.
///
/// Long-running (blocks until the user finishes the OAuth flow in the
/// browser), so it spawns onto the blocking runtime to keep Tauri's main
/// thread responsive.
#[tauri::command]
pub async fn login_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let profile = payload.profile;
    let path = tauri::async_runtime::spawn_blocking(move || {
        platform_runtime::actions::login_profile(&profile)
    })
    .await
    .map_err(|error| CommandError::new("LOGIN_FAILED", format!("Login task failed: {error}")))??;
    Ok(ActionResponse {
        ok: true,
        message: "Logged in profile.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub async fn refresh_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let profile = payload.profile;
    let path = tauri::async_runtime::spawn_blocking(move || {
        platform_runtime::actions::refresh_profile(&profile)
    })
    .await
    .map_err(|error| {
        CommandError::new("REFRESH_FAILED", format!("Refresh task failed: {error}"))
    })??;
    Ok(ActionResponse {
        ok: true,
        message: "Refreshed profile auth.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn rename_profile(payload: RenameProfilePayload) -> Result<ActionResponse, CommandError> {
    let path =
        platform_runtime::actions::rename_profile(&payload.profile, &payload.new_folder_name)?;
    Ok(ActionResponse {
        ok: true,
        message: "Renamed profile folder.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn delete_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::delete_profile(&payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Deleted profile.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn clear_profile_account(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::clear_profile_account(&payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Cleared profile account.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn update_profile_base_url(
    payload: UpdateProfileBaseUrlPayload,
) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::update_profile_base_url(
        &payload.profile,
        &payload.openai_base_url,
    )?;
    Ok(ActionResponse {
        ok: true,
        message: "Updated profile Base Url.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_profile_folder(
    app: tauri::AppHandle,
    payload: ProfilePayload,
) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_profile_folder(&app, &payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened profile folder.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn add_profile(payload: AddProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::add_profile(
        &payload.folder_name,
        payload.openai_base_url.as_deref(),
    )?;
    Ok(ActionResponse {
        ok: true,
        message: "Created profile template.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_contact(app: tauri::AppHandle) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_contact(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened contact URL.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_releases(app: tauri::AppHandle) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_releases(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened releases URL.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_url(
    app: tauri::AppHandle,
    payload: OpenUrlPayload,
) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_url(&app, &payload.url)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened URL.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub async fn check_update(
    payload: UpdateCheckPayload,
) -> Result<UpdateCheckResponse, CommandError> {
    let update_url = payload.update_url;
    tauri::async_runtime::spawn_blocking(move || crate::shared::update::check_update(&update_url))
        .await
        .map_err(|error| {
            CommandError::new(
                "UPDATE_CHECK_TASK_FAILED",
                format!("Update check task failed: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn get_codex_cli_status() -> Result<CodexCliStatus, CommandError> {
    let codex_home = platform_runtime::paths::get_codex_home();
    Ok(crate::shared::codex_cli_path::get_codex_cli_status(
        platform_runtime::codex_cli_resolver(),
        &codex_home,
    ))
}

#[tauri::command]
pub fn set_codex_cli_path(
    payload: SetCodexCliPathPayload,
) -> Result<CodexCliStatus, CommandError> {
    let codex_home = platform_runtime::paths::get_codex_home();
    Ok(crate::shared::codex_cli_path::set_codex_cli_path(
        platform_runtime::codex_cli_resolver(),
        &codex_home,
        &payload.path,
    )?)
}

#[tauri::command]
pub fn clear_codex_cli_path() -> Result<CodexCliStatus, CommandError> {
    let codex_home = platform_runtime::paths::get_codex_home();
    Ok(crate::shared::codex_cli_path::clear_codex_cli_path(
        platform_runtime::codex_cli_resolver(),
        &codex_home,
    ))
}

/// Force a fresh codex CLI detection scan for the Settings auto-detect
/// button. Runs on the blocking pool because it probes each candidate
/// with `codex --version`, which can take a second or two per path and
/// would otherwise stall the UI thread.
#[tauri::command]
pub async fn redetect_codex_cli_path() -> Result<CodexCliRedetectResult, CommandError> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = platform_runtime::paths::get_codex_home();
        crate::shared::codex_cli_path::redetect_codex_cli_path(
            platform_runtime::codex_cli_resolver(),
            &codex_home,
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "CODEX_CLI_REDETECT_FAILED",
            format!("Redetect task failed: {error}"),
        )
    })
}

#[tauri::command]
pub fn cancel_codex_login() -> Result<bool, CommandError> {
    Ok(crate::shared::login_cancel::cancel_login_in_progress())
}

#[tauri::command]
pub fn open_xiaohongshu(app: tauri::AppHandle) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_xiaohongshu(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened Xiaohongshu URL.".to_string(),
        path: Some(path),
    })
}
