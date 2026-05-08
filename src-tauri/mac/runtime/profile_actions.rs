use std::fs;
use std::path::{Path, PathBuf};

use tauri_plugin_opener::OpenerExt;

use crate::errors::{AppError, AppResult};
use crate::models::ProfileMetadata;
use crate::platform;
use crate::shared::config::sync_root_openai_base_url_from_profile_metadata;
use crate::shared::fs_ops::{backup_root_state_to_profile, remove_path};
use crate::shared::metadata::{
    load_profile_metadata, save_profile_metadata, sync_profile_metadata_from_auth,
    sync_profile_openai_base_url,
};
use crate::shared::paths::{
    get_backup_root, get_codex_home, validate_profile_name, ACTIVE_MARKER_FILE, CONTACT_URL,
    RELEASES_URL, XIAOHONGSHU_URL,
};
use crate::shared::login_runtime::login_profile_with_home;
use crate::shared::profiles::resolve_current_profile;
use crate::shared::profiles_index::load_profiles_index;

use super::cli_shim::get_login_runtime_dir;

const AUTH_TEMPLATE: &str = include_str!("../../../examples/account_backup/demo/auth.json.example");

fn normalize_openai_base_url(openai_base_url: &str) -> AppResult<Option<String>> {
    let trimmed = openai_base_url.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(Some(trimmed.to_string()));
    }

    Err(AppError::new(
        "INVALID_BASE_URL",
        "Base Url must start with http:// or https://.",
    ))
}

pub fn open_codex_app() -> AppResult<String> {
    platform::open_or_activate_codex_app(None)
}

pub fn login_profile(profile_name: &str) -> AppResult<String> {
    let codex_home = get_codex_home();
    let runtime_home = get_login_runtime_dir(&codex_home);
    let hooks = platform::current_hooks();
    login_profile_with_home(hooks, profile_name, Some(&codex_home), &runtime_home)
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

    platform::run_codex_login(&codex_home, &codex_home)?;

    if !codex_home.join("auth.json").is_file() {
        return Err(AppError::new(
            "LOGIN_AUTH_MISSING",
            "Login finished but no auth.json was written to CODEX_HOME.",
        ));
    }

    backup_root_state_to_profile(&current_profile, &codex_home, &backup_root)?;
    sync_profile_metadata_from_auth(&current_profile, None, Some(&codex_home))?;
    load_profiles_index(Some(&codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

pub fn update_profile_base_url(profile_name: &str, openai_base_url: &str) -> AppResult<String> {
    let codex_home = get_codex_home();
    let backup_root = get_backup_root(Some(&codex_home));
    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let normalized_base_url = normalize_openai_base_url(openai_base_url)?;
    sync_profile_openai_base_url(&profile_name, normalized_base_url, Some(&codex_home))?;
    if resolve_current_profile(&backup_root).as_deref() == Some(profile_name.as_str()) {
        sync_root_openai_base_url_from_profile_metadata(&profile_name, Some(&codex_home))?;
    }
    load_profiles_index(Some(&codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

fn ensure_profile_not_current(
    backup_root: &Path,
    profile_name: &str,
    error_code: &'static str,
    message: &'static str,
) -> AppResult<()> {
    if resolve_current_profile(backup_root).as_deref() == Some(profile_name) {
        return Err(AppError::new(error_code, message));
    }

    Ok(())
}

pub fn delete_profile(profile_name: &str) -> AppResult<String> {
    let codex_home = get_codex_home();
    let backup_root = get_backup_root(Some(&codex_home));
    let profile_name = validate_profile_name(profile_name)?;
    ensure_profile_not_current(
        &backup_root,
        &profile_name,
        "CURRENT_PROFILE_DELETE_FORBIDDEN",
        "The active profile cannot be deleted while it is in use.",
    )?;

    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    remove_path(&profile_dir)?;
    load_profiles_index(Some(&codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

pub fn clear_profile_account(profile_name: &str) -> AppResult<String> {
    let codex_home = get_codex_home();
    let backup_root = get_backup_root(Some(&codex_home));
    let profile_name = validate_profile_name(profile_name)?;
    ensure_profile_not_current(
        &backup_root,
        &profile_name,
        "CURRENT_PROFILE_CLEAR_FORBIDDEN",
        "The active profile cannot be cleared while it is in use.",
    )?;

    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    remove_path(&profile_dir.join("auth.json"))?;
    remove_path(&profile_dir.join(ACTIVE_MARKER_FILE))?;
    save_profile_metadata(
        &profile_name,
        &ProfileMetadata::with_folder_name(&profile_name),
        Some(&codex_home),
    )?;
    load_profiles_index(Some(&codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

fn rename_profile_with_home(
    profile_name: &str,
    new_folder_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<String> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let profile_name = validate_profile_name(profile_name)?;
    let new_folder_name = validate_profile_name(new_folder_name)?;

    if profile_name == new_folder_name {
        return Err(AppError::new(
            "PROFILE_RENAME_UNCHANGED",
            "The new folder name must be different from the current name.",
        ));
    }

    if resolve_current_profile(&backup_root).as_deref() == Some(profile_name.as_str()) {
        return Err(AppError::new(
            "CURRENT_PROFILE_RENAME_FORBIDDEN",
            "The active profile cannot be renamed while it is in use.",
        ));
    }

    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let renamed_dir = backup_root.join(&new_folder_name);
    if renamed_dir.exists() {
        return Err(AppError::new(
            "PROFILE_ALREADY_EXISTS",
            format!("Profile already exists: {new_folder_name}"),
        ));
    }

    fs::rename(&profile_dir, &renamed_dir).map_err(|error| {
        AppError::new(
            "PROFILE_RENAME_FAILED",
            format!(
                "Failed to rename profile directory {} -> {}: {error}",
                profile_dir.display(),
                renamed_dir.display()
            ),
        )
    })?;

    let mut metadata = load_profile_metadata(&new_folder_name, Some(&codex_home));
    metadata.folder_name = Some(new_folder_name.clone());
    save_profile_metadata(&new_folder_name, &metadata, Some(&codex_home))?;
    load_profiles_index(Some(&codex_home))?;

    Ok(renamed_dir.to_string_lossy().into_owned())
}

pub fn rename_profile(profile_name: &str, new_folder_name: &str) -> AppResult<String> {
    rename_profile_with_home(profile_name, new_folder_name, None)
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

pub fn add_profile(folder_name: &str, openai_base_url: Option<&str>) -> AppResult<String> {
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

    let mut metadata = ProfileMetadata::with_folder_name(&folder_name);
    metadata.openai_base_url = normalize_openai_base_url(openai_base_url.unwrap_or_default())?;
    save_profile_metadata(&folder_name, &metadata, None)?;
    load_profiles_index(None)?;

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

pub fn open_releases(app: &tauri::AppHandle) -> AppResult<String> {
    app.opener()
        .open_url(RELEASES_URL, None::<&str>)
        .map_err(|error| {
            AppError::new(
                "RELEASES_URL_OPEN_FAILED",
                format!("Failed to open releases URL: {error}"),
            )
        })?;

    Ok(RELEASES_URL.to_string())
}

pub fn open_url(app: &tauri::AppHandle, url: &str) -> AppResult<String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err(AppError::new(
            "URL_OPEN_INVALID",
            "URL must start with http:// or https://.",
        ));
    }

    app.opener()
        .open_url(trimmed, None::<&str>)
        .map_err(|error| {
            AppError::new("URL_OPEN_FAILED", format!("Failed to open URL: {error}"))
        })?;

    Ok(trimmed.to_string())
}

pub fn open_xiaohongshu(app: &tauri::AppHandle) -> AppResult<String> {
    app.opener()
        .open_url(XIAOHONGSHU_URL, None::<&str>)
        .map_err(|error| {
            AppError::new(
                "XIAOHONGSHU_URL_OPEN_FAILED",
                format!("Failed to open Xiaohongshu URL: {error}"),
            )
        })?;

    Ok(XIAOHONGSHU_URL.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        add_profile, clear_profile_account, delete_profile, rename_profile_with_home,
        update_profile_base_url,
    };
    use crate::macos::env_guard;
    use crate::shared::metadata::load_profile_metadata;
    use crate::shared::paths::get_current_profile_file;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-profile-actions-{name}-{unique}"))
    }

    fn write_profile(codex_home: &PathBuf, profile_name: &str) {
        let profile_dir = codex_home.join("account_backup").join(profile_name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"account_id":"acct_123"}}"#,
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            format!(
                r#"{{"folder_name":"{profile_name}","account_label":"user@example.com","quota":{{"five_hour":{{"remaining_percent":33}},"weekly":{{"remaining_percent":66}}}}}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn rename_profile_with_home_moves_directory_and_updates_profile_metadata() {
        let codex_home = temp_codex_home("rename-profile-success");
        write_profile(&codex_home, "old_name");

        let renamed_path =
            rename_profile_with_home("old_name", "new_name", Some(&codex_home)).unwrap();

        assert!(!codex_home.join("account_backup").join("old_name").exists());
        assert!(codex_home.join("account_backup").join("new_name").is_dir());
        assert_eq!(
            renamed_path,
            codex_home
                .join("account_backup")
                .join("new_name")
                .to_string_lossy()
                .into_owned()
        );
        let metadata = load_profile_metadata("new_name", Some(&codex_home));
        assert_eq!(metadata.folder_name.as_deref(), Some("new_name"));
        assert_eq!(metadata.account_label.as_deref(), Some("user@example.com"));
        assert_eq!(metadata.quota.five_hour.remaining_percent, Some(33));
        assert_eq!(metadata.quota.weekly.remaining_percent, Some(66));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn rename_profile_with_home_rejects_current_profile() {
        let codex_home = temp_codex_home("rename-profile-current");
        write_profile(&codex_home, "active");
        fs::write(get_current_profile_file(Some(&codex_home)), "active\n").unwrap();

        let error = rename_profile_with_home("active", "renamed", Some(&codex_home)).unwrap_err();

        assert_eq!(error.error_code, "CURRENT_PROFILE_RENAME_FORBIDDEN");
        assert!(codex_home.join("account_backup").join("active").is_dir());
        assert!(!codex_home.join("account_backup").join("renamed").exists());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn delete_profile_removes_non_current_profile() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("delete-profile");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        write_profile(&codex_home, "delete_me");
        std::env::set_var("CODEX_HOME", &codex_home);

        delete_profile("delete_me").unwrap();

        assert!(!codex_home.join("account_backup").join("delete_me").exists());
        let _ = fs::remove_dir_all(&codex_home);
        if let Some(path) = original_codex_home {
            std::env::set_var("CODEX_HOME", path);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }

    #[test]
    fn clear_profile_account_keeps_card_and_removes_account_binding() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("clear-profile-account");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        write_profile(&codex_home, "clear_me");
        std::env::set_var("CODEX_HOME", &codex_home);

        clear_profile_account("clear_me").unwrap();

        let profile_dir = codex_home.join("account_backup").join("clear_me");
        assert!(profile_dir.is_dir());
        assert!(!profile_dir.join("auth.json").exists());
        let metadata = load_profile_metadata("clear_me", Some(&codex_home));
        assert_eq!(metadata.folder_name.as_deref(), Some("clear_me"));
        assert_eq!(metadata.account_label, None);
        assert_eq!(metadata.openai_base_url, None);
        assert_eq!(metadata.quota.five_hour.remaining_percent, None);
        let _ = fs::remove_dir_all(&codex_home);
        if let Some(path) = original_codex_home {
            std::env::set_var("CODEX_HOME", path);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }

    #[test]
    fn delete_and_clear_profile_reject_current_profile() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("delete-profile-current");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        write_profile(&codex_home, "active");
        fs::write(get_current_profile_file(Some(&codex_home)), "active\n").unwrap();
        std::env::set_var("CODEX_HOME", &codex_home);

        let delete_error = delete_profile("active").unwrap_err();
        let clear_error = clear_profile_account("active").unwrap_err();

        assert_eq!(delete_error.error_code, "CURRENT_PROFILE_DELETE_FORBIDDEN");
        assert_eq!(clear_error.error_code, "CURRENT_PROFILE_CLEAR_FORBIDDEN");
        assert!(codex_home
            .join("account_backup")
            .join("active")
            .join("auth.json")
            .is_file());
        let _ = fs::remove_dir_all(&codex_home);
        if let Some(path) = original_codex_home {
            std::env::set_var("CODEX_HOME", path);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }

    #[test]
    fn update_profile_base_url_allows_non_api_key_profiles_and_restores_when_cleared() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("base-url-chatgpt");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        let profile_dir = codex_home.join("account_backup").join("chat");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(profile_dir.join("auth.json"), r#"{"auth_mode":"chatgpt"}"#).unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"chat"}"#,
        )
        .unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "chat\n").unwrap();
        fs::write(codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();
        std::env::set_var("CODEX_HOME", &codex_home);

        update_profile_base_url("chat", "https://example.com/v1").unwrap();

        let metadata = load_profile_metadata("chat", Some(&codex_home));
        assert_eq!(
            metadata.openai_base_url.as_deref(),
            Some("https://example.com/v1")
        );
        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("openai_base_url = \"https://example.com/v1\""));

        update_profile_base_url("chat", "  ").unwrap();

        let metadata = load_profile_metadata("chat", Some(&codex_home));
        assert_eq!(metadata.openai_base_url, None);
        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(!config.contains("openai_base_url"));
        assert!(config.contains("model = \"gpt-5.4\""));
        let _ = fs::remove_dir_all(&codex_home);
        if let Some(path) = original_codex_home {
            std::env::set_var("CODEX_HOME", path);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }

    #[test]
    fn update_profile_base_url_updates_current_root_config_for_active_api_key_profile() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("base-url-current");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        let profile_dir = codex_home.join("account_backup").join("api");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(profile_dir.join("auth.json"), r#"{"auth_mode":"apikey"}"#).unwrap();
        fs::write(profile_dir.join("profile.json"), r#"{"folder_name":"api"}"#).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "api\n").unwrap();
        fs::write(codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();
        std::env::set_var("CODEX_HOME", &codex_home);

        update_profile_base_url("api", "https://example.com/v1").unwrap();

        let metadata = load_profile_metadata("api", Some(&codex_home));
        assert_eq!(
            metadata.openai_base_url.as_deref(),
            Some("https://example.com/v1")
        );
        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("openai_base_url = \"https://example.com/v1\""));
        let _ = fs::remove_dir_all(&codex_home);
        if let Some(path) = original_codex_home {
            std::env::set_var("CODEX_HOME", path);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }

    #[test]
    fn add_profile_persists_optional_base_url() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("add-profile-base-url");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        fs::create_dir_all(codex_home.join("account_backup")).unwrap();
        std::env::set_var("CODEX_HOME", &codex_home);

        add_profile("api_new", Some("https://example.com/v1")).unwrap();

        let metadata = load_profile_metadata("api_new", Some(&codex_home));
        assert_eq!(
            metadata.openai_base_url.as_deref(),
            Some("https://example.com/v1")
        );
        let _ = fs::remove_dir_all(&codex_home);
        if let Some(path) = original_codex_home {
            std::env::set_var("CODEX_HOME", path);
        } else {
            std::env::remove_var("CODEX_HOME");
        }
    }
}
