use std::env;
use std::path::{Path, PathBuf};

use chrono::{Local, Utc};

use crate::errors::{AppError, AppResult};

pub const ACTIVE_MARKER_FILE: &str = ".active_profile";
pub const CURRENT_PROFILE_FILENAME: &str = ".current_profile";
pub const DEFAULT_PROFILES: [&str; 4] = ["a", "b", "c", "d"];
pub const INSTALL_STATE_FILENAME: &str = "install_state.json";
pub const PROFILE_METADATA_FILENAME: &str = "profile.json";
pub const SWITCH_LOCK_FILENAME: &str = ".switch.lock";
pub const WINDOWS_RUNTIME_DIRNAME: &str = "windows";
pub const APP_NAME: &str = "Codex";
pub const APP_PROCESS_NAME: &str = "Codex.exe";
pub const CONTACT_URL: &str = "https://github.com/Cmochance/Codex_Account_Switch";
pub const DEFAULT_PAGE_SIZE: u32 = 4;

fn fallback_home_dir() -> PathBuf {
    if let Some(path) = env::var_os("USERPROFILE") {
        return PathBuf::from(path);
    }

    if let Some(path) = env::var_os("HOME") {
        return PathBuf::from(path);
    }

    PathBuf::from(".")
}

pub fn get_codex_home() -> PathBuf {
    if let Some(path) = env::var_os("CODEX_HOME") {
        PathBuf::from(path)
    } else {
        fallback_home_dir().join(".codex")
    }
}

pub fn get_backup_root(codex_home: Option<&Path>) -> PathBuf {
    codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("account_backup")
}

pub fn get_auto_save_root(codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home).join("_autosave")
}

pub fn get_current_profile_file(codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home).join(CURRENT_PROFILE_FILENAME)
}

pub fn get_runtime_dir(codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home).join(WINDOWS_RUNTIME_DIRNAME)
}

pub fn get_install_state_file(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(INSTALL_STATE_FILENAME)
}

pub fn get_profile_metadata_path(profile_name: &str, codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home)
        .join(profile_name)
        .join(PROFILE_METADATA_FILENAME)
}

pub fn get_switch_lock_path(codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home).join(SWITCH_LOCK_FILENAME)
}

pub fn list_profile_dirs(backup_root: &Path) -> Vec<PathBuf> {
    if !backup_root.is_dir() {
        return Vec::new();
    }

    let mut dirs = backup_root
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_profile_dir(path))
        .collect::<Vec<_>>();

    dirs.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    dirs
}

pub fn is_profile_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    !matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("_autosave") | Some(WINDOWS_RUNTIME_DIRNAME)
    )
}

pub fn validate_profile_name(profile_name: &str) -> AppResult<String> {
    let is_valid = !profile_name.is_empty()
        && profile_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');

    if is_valid {
        Ok(profile_name.to_string())
    } else {
        Err(AppError::new(
            "INVALID_PROFILE_NAME",
            format!("Invalid profile name: {profile_name:?}"),
        ))
    }
}

pub fn utc_timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn autosave_timestamp() -> String {
    Local::now().format("%Y%m%d-%H%M%S").to_string()
}
