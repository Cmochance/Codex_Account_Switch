use std::cmp::Ordering;
use std::process::Command;

use semver::Version;
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::models::UpdateCheckResponse;
use crate::shared::paths::{RELEASES_URL, UPDATE_CHECK_URL};

const UPDATE_USER_AGENT: &str = "Codex-Switch-Updater";

pub fn check_update(update_url: &str) -> AppResult<UpdateCheckResponse> {
    let checked_url = normalize_update_url(update_url)?;
    let body = fetch_update_json(&checked_url)?;
    let payload: Value = serde_json::from_str(&body).map_err(|error| {
        AppError::new(
            "UPDATE_JSON_INVALID",
            format!("Failed to parse update response JSON: {error}"),
        )
    })?;

    let latest_version =
        read_string_field(&payload, &["version", "tag_name"]).ok_or_else(|| {
            AppError::new(
                "UPDATE_VERSION_MISSING",
                "Update response did not include a version.",
            )
        })?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let has_update = compare_versions(&latest_version, &current_version)? == Ordering::Greater;
    let release_url = read_string_field(&payload, &["html_url", "release_url"])
        .or_else(|| Some(RELEASES_URL.to_string()));
    let notes = read_string_field(&payload, &["notes", "body"]);

    Ok(UpdateCheckResponse {
        ok: true,
        current_version,
        latest_version: Some(latest_version),
        has_update,
        release_url,
        notes,
        checked_url,
    })
}

fn normalize_update_url(update_url: &str) -> AppResult<String> {
    let trimmed = update_url.trim();
    let url = if trimmed.is_empty() {
        UPDATE_CHECK_URL
    } else {
        trimmed
    };

    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(AppError::new(
            "UPDATE_URL_INVALID",
            "Update URL must start with http:// or https://.",
        ));
    }

    Ok(url.to_string())
}

fn read_string_field(payload: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        payload
            .get(*field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn compare_versions(latest_version: &str, current_version: &str) -> AppResult<Ordering> {
    let latest = parse_version(latest_version).ok_or_else(|| {
        AppError::new(
            "UPDATE_VERSION_INVALID",
            format!("Latest version is not a valid semantic version: {latest_version}"),
        )
    })?;
    let current = parse_version(current_version).ok_or_else(|| {
        AppError::new(
            "CURRENT_VERSION_INVALID",
            format!("Current version is not a valid semantic version: {current_version}"),
        )
    })?;

    Ok(latest.cmp(&current))
}

fn parse_version(version: &str) -> Option<Version> {
    let candidate = version
        .trim()
        .trim_start_matches(|character| character == 'v' || character == 'V')
        .split_whitespace()
        .next()?;

    if let Ok(version) = Version::parse(candidate) {
        return Some(version);
    }

    let normalized = normalize_short_version(candidate)?;
    Version::parse(&normalized).ok()
}

fn normalize_short_version(version: &str) -> Option<String> {
    let core_end = version
        .find(|character| character == '-' || character == '+')
        .unwrap_or(version.len());
    let core = &version[..core_end];
    if core.split('.').count() != 2 {
        return None;
    }

    Some(format!(
        "{}.0{}",
        core,
        version.get(core_end..).unwrap_or_default()
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn fetch_update_json(url: &str) -> AppResult<String> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--max-time",
            "12",
            "--user-agent",
            UPDATE_USER_AGENT,
            url,
        ])
        .output()
        .map_err(|error| {
            AppError::new(
                "UPDATE_REQUEST_FAILED",
                format!("Failed to start update request: {error}"),
            )
        })?;

    parse_fetch_output(output.status.success(), &output.stdout, &output.stderr)
}

#[cfg(windows)]
fn fetch_update_json(url: &str) -> AppResult<String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let script = format!(
        concat!(
            "$ProgressPreference='SilentlyContinue'; ",
            "[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; ",
            "(Invoke-WebRequest -Uri $env:CODEX_SWITCH_UPDATE_URL -UseBasicParsing ",
            "-Headers @{{ 'User-Agent' = '{}' }} -TimeoutSec 12).Content"
        ),
        UPDATE_USER_AGENT,
    );

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .env("CODEX_SWITCH_UPDATE_URL", url)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            AppError::new(
                "UPDATE_REQUEST_FAILED",
                format!("Failed to start update request: {error}"),
            )
        })?;

    parse_fetch_output(output.status.success(), &output.stdout, &output.stderr)
}

fn parse_fetch_output(success: bool, stdout: &[u8], stderr: &[u8]) -> AppResult<String> {
    if success {
        return String::from_utf8(stdout.to_vec()).map_err(|error| {
            AppError::new(
                "UPDATE_RESPONSE_INVALID",
                format!("Update response was not valid UTF-8: {error}"),
            )
        });
    }

    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let message = if stderr.is_empty() {
        "Update request failed.".to_string()
    } else {
        format!("Update request failed: {stderr}")
    };
    Err(AppError::new("UPDATE_REQUEST_FAILED", message))
}

#[cfg(test)]
mod tests {
    use super::{compare_versions, parse_version};
    use std::cmp::Ordering;

    #[test]
    fn parses_versions_with_v_prefix() {
        assert_eq!(parse_version("v1.5.1").unwrap().to_string(), "1.5.1");
    }

    #[test]
    fn parses_two_part_release_versions() {
        assert_eq!(parse_version("1.5").unwrap().to_string(), "1.5.0");
        assert_eq!(parse_version("v1.5").unwrap().to_string(), "1.5.0");
    }

    #[test]
    fn compares_semantic_versions() {
        assert_eq!(
            compare_versions("v1.6.0", "1.5.9").unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("v1.5.0", "1.5.0").unwrap(),
            Ordering::Equal
        );
        assert_eq!(compare_versions("v1.4.9", "1.5.0").unwrap(), Ordering::Less);
    }
}
