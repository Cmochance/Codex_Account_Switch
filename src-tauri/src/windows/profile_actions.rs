use std::fs;
use std::path::{Path, PathBuf};

use tauri_plugin_opener::OpenerExt;

use crate::errors::{AppError, AppResult};
use crate::models::ProfileMetadata;

use super::config::{profile_uses_api_key_auth, sync_root_openai_base_url_for_profile};
use super::fs_ops::backup_root_state_to_profile;
use super::metadata::{
    load_profile_metadata, save_profile_metadata, sync_profile_metadata_from_auth,
    sync_profile_openai_base_url,
};
use super::paths::{
    get_backup_root, get_codex_home, validate_profile_name, CONTACT_URL,
};
use super::process::{open_or_activate_codex_app, run_codex_login};
use super::profiles::resolve_current_profile;

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
    sync_profile_metadata_from_auth(&current_profile, Some(&codex_home))?;
    super::profiles_index::load_profiles_index(Some(&codex_home))?;

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
    if normalized_base_url.is_some()
        && !profile_uses_api_key_auth(&profile_name, Some(&codex_home))?
    {
        return Err(AppError::new(
            "PROFILE_BASE_URL_REQUIRES_API_KEY",
            "Custom Base Url is only supported for API KEY logins.",
        ));
    }

    sync_profile_openai_base_url(&profile_name, normalized_base_url, Some(&codex_home))?;
    if resolve_current_profile(&backup_root).as_deref() == Some(profile_name.as_str()) {
        sync_root_openai_base_url_for_profile(&profile_name, Some(&codex_home))?;
    }
    super::profiles_index::load_profiles_index(Some(&codex_home))?;

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
    super::profiles_index::load_profiles_index(Some(&codex_home))?;

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

#[cfg(test)]
mod tests {
    use super::{add_profile, rename_profile_with_home, update_profile_base_url};
    use crate::windows::env_guard;
    use crate::windows::metadata::load_profile_metadata;
    use crate::windows::paths::get_current_profile_file;
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
    fn update_profile_base_url_rejects_non_api_key_profiles() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("base-url-chatgpt");
        let original_codex_home = std::env::var_os("CODEX_HOME");
        let profile_dir = codex_home.join("account_backup").join("chat");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(profile_dir.join("auth.json"), r#"{"auth_mode":"chatgpt"}"#).unwrap();
        fs::write(profile_dir.join("profile.json"), r#"{"folder_name":"chat"}"#).unwrap();
        std::env::set_var("CODEX_HOME", &codex_home);

        let error = update_profile_base_url("chat", "https://example.com/v1").unwrap_err();

        assert_eq!(error.error_code, "PROFILE_BASE_URL_REQUIRES_API_KEY");
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
