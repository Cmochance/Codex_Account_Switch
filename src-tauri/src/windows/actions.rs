use std::fs;
use tauri_plugin_opener::OpenerExt;

use crate::errors::{AppError, AppResult};
use crate::models::ProfileMetadata;

use super::dashboard::resolve_current_profile;
use super::fs_ops::backup_root_state_to_profile;
use super::metadata::save_profile_metadata;
use super::paths::{get_backup_root, get_codex_home, validate_profile_name, CONTACT_URL};
use super::process::{open_or_activate_codex_app, run_codex_login};

const AUTH_TEMPLATE: &str = include_str!("../../../examples/account_backup/demo/auth.json.example");

pub fn open_codex_app() -> AppResult<String> {
    open_or_activate_codex_app(None)
}

pub fn login_current_profile() -> AppResult<String> {
    let codex_home = get_codex_home();
    let backup_root = get_backup_root(Some(&codex_home));
    let current_profile = resolve_current_profile(&backup_root).ok_or_else(|| {
        AppError::new(
            "CURRENT_PROFILE_MISSING",
            "No active profile is selected. Switch to a profile before logging in.",
        )
    })?;

    let profile_dir = backup_root.join(&current_profile);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {current_profile}"),
        ));
    }

    run_codex_login(&codex_home)?;

    if !codex_home.join("auth.json").is_file() {
        return Err(AppError::new(
            "LOGIN_AUTH_MISSING",
            "Login finished but no auth.json was written to CODEX_HOME.",
        ));
    }

    backup_root_state_to_profile(&current_profile, &codex_home, &backup_root)?;
    super::profiles_index::load_profiles_index(Some(&codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

pub fn open_profile_folder(app: &tauri::AppHandle, profile_name: &str) -> AppResult<String> {
    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = get_backup_root(None).join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    app.opener()
        .open_path(profile_dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| {
            AppError::new(
                "PROFILE_FOLDER_OPEN_FAILED",
                format!("Failed to open profile folder: {error}"),
            )
        })?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

pub fn add_profile(folder_name: &str) -> AppResult<String> {
    let folder_name = validate_profile_name(folder_name)?;
    let profile_dir = get_backup_root(None).join(&folder_name);
    if profile_dir.exists() {
        return Err(AppError::new(
            "PROFILE_ALREADY_EXISTS",
            format!("Profile already exists: {folder_name}"),
        ));
    }

    fs::create_dir_all(&profile_dir).map_err(|error| {
        AppError::new(
            "PROFILE_CREATE_FAILED",
            format!(
                "Failed to create profile directory {}: {error}",
                profile_dir.display()
            ),
        )
    })?;
    fs::write(profile_dir.join("auth.json"), AUTH_TEMPLATE).map_err(|error| {
        AppError::new(
            "AUTH_TEMPLATE_WRITE_FAILED",
            format!("Failed to write auth.json: {error}"),
        )
    })?;

    let metadata = ProfileMetadata::with_folder_name(&folder_name);
    save_profile_metadata(&folder_name, &metadata, None)?;
    super::profiles_index::load_profiles_index(None)?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

pub fn open_contact(app: &tauri::AppHandle) -> AppResult<String> {
    app.opener()
        .open_url(CONTACT_URL, None::<&str>)
        .map_err(|error| {
            AppError::new(
                "CONTACT_URL_OPEN_FAILED",
                format!("Failed to open contact URL: {error}"),
            )
        })?;

    Ok(CONTACT_URL.to_string())
}
