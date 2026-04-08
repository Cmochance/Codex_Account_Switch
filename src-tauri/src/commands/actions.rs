use crate::errors::CommandError;
use crate::models::{
    ActionResponse, AddProfilePayload, ProfilePayload, RenameProfilePayload,
    UpdateProfileBaseUrlPayload,
};
use crate::windows;

#[tauri::command]
pub fn open_codex() -> Result<ActionResponse, CommandError> {
    let path = windows::actions::open_codex_app()?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened Codex.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn login_current_profile() -> Result<ActionResponse, CommandError> {
    let path = windows::actions::login_current_profile()?;
    Ok(ActionResponse {
        ok: true,
        message: "Logged in current profile.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub async fn refresh_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let profile = payload.profile;
    let path =
        tauri::async_runtime::spawn_blocking(move || windows::actions::refresh_profile(&profile))
            .await
            .map_err(|error| {
                CommandError::new(
                    "REFRESH_FAILED",
                    format!("Refresh task failed: {error}"),
                )
            })??;
    Ok(ActionResponse {
        ok: true,
        message: "Refreshed profile auth.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn rename_profile(payload: RenameProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = windows::actions::rename_profile(&payload.profile, &payload.new_folder_name)?;
    Ok(ActionResponse {
        ok: true,
        message: "Renamed profile folder.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn update_profile_base_url(
    payload: UpdateProfileBaseUrlPayload,
) -> Result<ActionResponse, CommandError> {
    let path = windows::actions::update_profile_base_url(
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
    let path = windows::actions::open_profile_folder(&app, &payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened profile folder.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn add_profile(payload: AddProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = windows::actions::add_profile(
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
    let path = windows::actions::open_contact(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened contact URL.".to_string(),
        path: Some(path),
    })
}
