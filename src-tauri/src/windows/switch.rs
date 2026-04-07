use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::models::SwitchResponse;

use super::fs_ops::{
    autosave_auth, backup_root_state_to_profile, overlay_directory_contents, set_active_marker,
};
use super::paths::{get_backup_root, get_switch_lock_path, validate_profile_name};
use super::process;
use super::profiles::resolve_current_profile;

struct SwitchGuard {
    lock_path: PathBuf,
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

fn acquire_switch_lock(codex_home: Option<&Path>) -> AppResult<SwitchGuard> {
    let lock_path = get_switch_lock_path(codex_home);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create lock directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| {
            AppError::new(
                "SWITCH_IN_PROGRESS",
                "A profile switch is already in progress.",
            )
        })?;

    Ok(SwitchGuard { lock_path })
}

pub fn switch_profile(profile_name: &str) -> AppResult<SwitchResponse> {
    let codex_home = super::paths::get_codex_home();
    let backup_root = get_backup_root(Some(&codex_home));
    if !backup_root.is_dir() {
        return Err(AppError::new(
            "BACKUP_ROOT_MISSING",
            format!("Backup folder not found: {}", backup_root.display()),
        ));
    }

    let profile_name = validate_profile_name(profile_name)?;
    let _guard = acquire_switch_lock(Some(&codex_home))?;
    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }
    if !profile_dir.join("auth.json").is_file() {
        return Err(AppError::new(
            "PROFILE_AUTH_MISSING",
            format!(
                "Missing auth file: {}",
                profile_dir.join("auth.json").display()
            ),
        ));
    }

    let app_was_running = process::quit_codex_app_if_running()?;
    let current_profile = resolve_current_profile(&backup_root);
    if let Some(current_profile) = current_profile.as_deref() {
        backup_root_state_to_profile(current_profile, &codex_home, &backup_root)?;
    }

    autosave_auth(&codex_home)?;
    overlay_directory_contents(&profile_dir, &codex_home)?;
    set_active_marker(&profile_name, &backup_root)?;
    super::profiles_index::load_profiles_index(Some(&codex_home))?;
    let warnings = process::reopen_codex_app_if_needed(app_was_running, Some(&codex_home));

    Ok(SwitchResponse {
        ok: true,
        profile: profile_name.clone(),
        message: format!("Switched to profile: {profile_name}"),
        warnings,
    })
}
