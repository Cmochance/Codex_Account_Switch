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

fn toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            ch if (ch as u32) < 0x20 || ch == '\u{007F}' => {
                out.push_str(&format!("\\u{:04X}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn render_openai_base_url_assignment(base_url: &str) -> String {
    format!("openai_base_url = {}", toml_basic_string(base_url))
}

fn load_normalized_profile_base_url(profile_name: &str, codex_home: &Path) -> Option<String> {
    load_profile_metadata(profile_name, Some(codex_home))
        .openai_base_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn sync_root_openai_base_url_value(
    desired_base_url: Option<&str>,
    codex_home: Option<&Path>,
) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    if super::gateway::is_enabled(Some(&codex_home)) {
        // Forwarding owns the root URL while it is enabled; per-profile
        // derivations would clobber the proxy endpoint. The gateway module
        // writes its own value through `force_root_openai_base_url`, which
        // bypasses this guard via the lower-level helper below.
        return Ok(());
    }
    write_root_openai_base_url_value(desired_base_url, Some(&codex_home))
}

fn write_root_openai_base_url_value(
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
        load_normalized_profile_base_url(&profile_name, &codex_home)
    } else {
        None
    };

    sync_root_openai_base_url_value(desired_base_url.as_deref(), Some(&codex_home))
}

pub fn sync_root_openai_base_url_from_profile_metadata(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = get_backup_root(Some(&codex_home)).join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let desired_base_url = load_normalized_profile_base_url(&profile_name, &codex_home);

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

/// Force the root config.toml `openai_base_url` to the supplied value.
///
/// Pass `None` to remove the assignment. Used by the gateway module so the proxy
/// endpoint stays in place regardless of which profile is active. This is the
/// privileged write path that bypasses the gateway-enabled guard in
/// `sync_root_openai_base_url_value`.
pub fn force_root_openai_base_url(
    base_url: Option<&str>,
    codex_home: Option<&Path>,
) -> AppResult<()> {
    write_root_openai_base_url_value(base_url, codex_home)
}

/// Read the current `openai_base_url` assignment out of the root config.toml.
///
/// Returns `None` when the file is absent or the assignment is missing. Used by
/// the gateway module to capture an externally-managed endpoint before
/// overwriting it, so disable/recover can restore the prior value.
pub fn read_root_openai_base_url(codex_home: Option<&Path>) -> Option<String> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let config_path = get_root_config_path(Some(&codex_home));
    let content = fs::read_to_string(&config_path).ok()?;
    for line in content.lines() {
        if !is_openai_base_url_assignment(line) {
            continue;
        }
        let (_, value_part) = line.split_once('=')?;
        let trimmed = value_part.trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or(trimmed);
        let normalized = unquoted.trim();
        if normalized.is_empty() {
            return None;
        }
        return Some(normalized.to_string());
    }
    None
}
