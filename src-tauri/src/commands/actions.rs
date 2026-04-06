use crate::errors::CommandError;
use crate::models::{ActionResponse, AddProfilePayload, ProfilePayload};
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
    let path = windows::actions::add_profile(&payload.folder_name)?;
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
