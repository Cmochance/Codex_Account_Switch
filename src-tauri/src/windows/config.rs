use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};

use super::metadata::load_profile_metadata;
use super::paths::{get_backup_root, get_codex_home, get_root_config_path, validate_profile_name};
use super::profiles::resolve_current_profile;

fn load_profile_auth_mode(profile_dir: &Path) -> Option<String> {
    let auth_path = profile_dir.join("auth.json");
    let raw = fs::read_to_string(auth_path).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    parsed
        .get("auth_mode")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn is_openai_base_url_assignment(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }

    trimmed
        .strip_prefix("openai_base_url")
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

fn render_openai_base_url_assignment(base_url: &str) -> String {
    format!(
        "openai_base_url = {}",
        serde_json::to_string(base_url).unwrap_or_else(|_| format!("\"{base_url}\""))
    )
}

fn sync_root_openai_base_url_value(
    desired_base_url: Option<&str>,
    codex_home: Option<&Path>,
) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let config_path = get_root_config_path(Some(&codex_home));
    let current = fs::read_to_string(&config_path).unwrap_or_default();
    let mut lines = current
        .lines()
        .filter(|line| !is_openai_base_url_assignment(line))
        .map(str::to_string)
        .collect::<Vec<_>>();

    if let Some(base_url) = desired_base_url {
        let insert_at = lines
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .unwrap_or(lines.len());
        lines.insert(insert_at, render_openai_base_url_assignment(base_url));
        if insert_at + 1 < lines.len() && !lines[insert_at + 1].trim().is_empty() {
            lines.insert(insert_at + 1, String::new());
        }
    }

    let next = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };

    if next == current {
        return Ok(());
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create config directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    fs::write(&config_path, next).map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!("Failed to write config {}: {error}", config_path.display()),
        )
    })
}

pub fn profile_uses_api_key_auth(profile_name: &str, codex_home: Option<&Path>) -> AppResult<bool> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = get_backup_root(Some(&codex_home)).join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    Ok(load_profile_auth_mode(&profile_dir).as_deref() == Some("apikey"))
}

pub fn sync_root_openai_base_url_for_profile(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    let desired_base_url = if profile_uses_api_key_auth(&profile_name, Some(&codex_home))? {
        load_profile_metadata(&profile_name, Some(&codex_home))
            .openai_base_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    } else {
        None
    };

    sync_root_openai_base_url_value(desired_base_url.as_deref(), Some(&codex_home))
}

pub fn sync_root_openai_base_url_for_current_profile(codex_home: Option<&Path>) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let Some(current_profile) = resolve_current_profile(&backup_root) else {
        return Ok(());
    };

    sync_root_openai_base_url_for_profile(&current_profile, Some(&codex_home))
}

#[cfg(test)]
mod tests {
    use super::{
        profile_uses_api_key_auth, sync_root_openai_base_url_for_current_profile,
        sync_root_openai_base_url_for_profile,
    };
    use crate::windows::paths::get_current_profile_file;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-config-{name}-{unique}"))
    }

    fn write_profile(codex_home: &PathBuf, profile_name: &str, auth_mode: &str, base_url: Option<&str>) {
        let profile_dir = codex_home.join("account_backup").join(profile_name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"auth_mode":"{auth_mode}"}}"#),
        )
        .unwrap();
        let metadata = match base_url {
            Some(base_url) => format!(
                r#"{{"folder_name":"{profile_name}","openai_base_url":"{base_url}"}}"#
            ),
            None => format!(r#"{{"folder_name":"{profile_name}"}}"#),
        };
        fs::write(profile_dir.join("profile.json"), metadata).unwrap();
    }

    #[test]
    fn profile_uses_api_key_auth_recognizes_apikey_profiles() {
        let codex_home = temp_codex_home("apikey-mode");
        write_profile(&codex_home, "api", "apikey", None);

        assert!(profile_uses_api_key_auth("api", Some(&codex_home)).unwrap());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_openai_base_url_for_profile_sets_value_for_api_key_profiles() {
        let codex_home = temp_codex_home("inject-base-url");
        write_profile(
            &codex_home,
            "api",
            "apikey",
            Some("https://example.com/v1"),
        );
        fs::write(
            codex_home.join("config.toml"),
            "model = \"gpt-5.4\"\n\n[notice]\nhide_rate_limit_model_nudge = true\n",
        )
        .unwrap();

        sync_root_openai_base_url_for_profile("api", Some(&codex_home)).unwrap();

        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.4\""));
        assert!(config.contains("openai_base_url = \"https://example.com/v1\""));
        assert!(config.contains("[notice]"));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_openai_base_url_for_profile_removes_value_for_non_api_profiles() {
        let codex_home = temp_codex_home("remove-base-url");
        write_profile(
            &codex_home,
            "chat",
            "chatgpt",
            Some("https://example.com/v1"),
        );
        fs::write(
            codex_home.join("config.toml"),
            "model = \"gpt-5.4\"\nopenai_base_url = \"https://example.com/v1\"\n\n[notice]\nhide_rate_limit_model_nudge = true\n",
        )
        .unwrap();

        sync_root_openai_base_url_for_profile("chat", Some(&codex_home)).unwrap();

        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.4\""));
        assert!(!config.contains("openai_base_url"));
        assert!(config.contains("[notice]"));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_openai_base_url_for_current_profile_uses_active_marker() {
        let codex_home = temp_codex_home("current-profile-base-url");
        write_profile(
            &codex_home,
            "api",
            "apikey",
            Some("https://example.com/v1"),
        );
        fs::write(get_current_profile_file(Some(&codex_home)), "api\n").unwrap();

        sync_root_openai_base_url_for_current_profile(Some(&codex_home)).unwrap();

        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("openai_base_url = \"https://example.com/v1\""));
        let _ = fs::remove_dir_all(&codex_home);
    }
}
