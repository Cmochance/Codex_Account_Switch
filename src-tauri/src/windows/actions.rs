use std::fs;
use std::process::Command;

use tauri_plugin_opener::OpenerExt;

use crate::errors::{AppError, AppResult};
use crate::models::ProfileMetadata;

use super::metadata::save_profile_metadata;
use super::paths::{get_backup_root, validate_profile_name, CONTACT_URL};
use super::process::resolve_codex_app_path;

const AUTH_TEMPLATE: &str = include_str!("../../../examples/account_backup/demo/auth.json.example");

pub fn open_codex_app() -> AppResult<String> {
    let Some(resolved_path) = resolve_codex_app_path(None) else {
        return Err(AppError::new(
            "APP_NOT_FOUND",
            "Codex desktop app path could not be resolved.",
        ));
    };

    Command::new(&resolved_path).spawn().map_err(|error| {
        AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
    })?;

    Ok(resolved_path.to_string_lossy().into_owned())
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
        .open_path(&profile_dir, None::<&str>)
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
            format!("Failed to create profile directory {}: {error}", profile_dir.display()),
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
