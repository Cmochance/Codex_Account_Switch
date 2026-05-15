use std::env;
use std::path::{Path, PathBuf};

use chrono::{Local, Utc};

use crate::errors::{AppError, AppResult};

pub const ACTIVE_MARKER_FILE: &str = ".active_profile";
pub const CURRENT_PROFILE_FILENAME: &str = ".current_profile";
pub const DEFAULT_PROFILES: [&str; 4] = ["a", "b", "c", "d"];
pub const INSTALL_STATE_FILENAME: &str = "install_state.json";
pub const QUOTA_CACHE_FILENAME: &str = "quota_cache.json";
pub const PROFILES_INDEX_FILENAME: &str = "profiles.json";
pub const PROFILE_METADATA_FILENAME: &str = "profile.json";
pub const REFRESH_RUNTIME_DIRNAME: &str = "refresh_runtime";
pub const LOGIN_RUNTIME_DIRNAME: &str = "login_runtime";
pub const SWITCH_LOCK_FILENAME: &str = ".switch.lock";
pub const WINDOWS_RUNTIME_DIRNAME: &str = "windows";
pub const MACOS_RUNTIME_DIRNAME: &str = "macos";
pub const CONTACT_URL: &str = "https://github.com/Cmochance/Codex_Account_Switch";
pub const RELEASES_URL: &str = "https://github.com/Cmochance/Codex_Account_Switch/releases";
pub const UPDATE_CHECK_URL: &str =
    "https://api.github.com/repos/Cmochance/Codex_Account_Switch/releases/latest";
pub const XIAOHONGSHU_URL: &str = "https://www.xiaohongshu.com/explore/69df8fca000000002302203c";
pub const DEFAULT_PAGE_SIZE: u32 = 8;

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

pub fn get_root_config_path(codex_home: Option<&Path>) -> PathBuf {
    codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("config.toml")
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

pub fn get_refresh_runtime_dir(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(REFRESH_RUNTIME_DIRNAME)
}

pub fn get_login_runtime_dir(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(LOGIN_RUNTIME_DIRNAME)
}

pub fn get_install_state_file(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(INSTALL_STATE_FILENAME)
}

pub fn get_quota_cache_path(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(QUOTA_CACHE_FILENAME)
}

pub fn get_profile_metadata_path(profile_name: &str, codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home)
        .join(profile_name)
        .join(PROFILE_METADATA_FILENAME)
}

pub fn get_profiles_index_path(codex_home: Option<&Path>) -> PathBuf {
    get_backup_root(codex_home).join(PROFILES_INDEX_FILENAME)
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
        Some("_autosave") | Some(WINDOWS_RUNTIME_DIRNAME) | Some(MACOS_RUNTIME_DIRNAME)
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

#[cfg(test)]
mod tests {
    use super::{is_profile_dir, MACOS_RUNTIME_DIRNAME, WINDOWS_RUNTIME_DIRNAME};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-shared-paths-{name}-{unique}"))
    }

    #[test]
    fn is_profile_dir_excludes_runtime_directories() {
        let root = temp_dir("runtime-filter");
        let profile_dir = root.join("a");
        let windows_runtime_dir = root.join(WINDOWS_RUNTIME_DIRNAME);
        let macos_runtime_dir = root.join(MACOS_RUNTIME_DIRNAME);

        fs::create_dir_all(&profile_dir).unwrap();
        fs::create_dir_all(&windows_runtime_dir).unwrap();
        fs::create_dir_all(&macos_runtime_dir).unwrap();

        assert!(is_profile_dir(&profile_dir));
        assert!(!is_profile_dir(&windows_runtime_dir));
        assert!(!is_profile_dir(&macos_runtime_dir));

        let _ = fs::remove_dir_all(&root);
    }
}
