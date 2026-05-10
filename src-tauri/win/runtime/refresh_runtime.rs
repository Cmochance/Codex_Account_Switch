use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::errors::{AppError, AppResult};
use crate::models::QuotaSummary;
use crate::platform;

use super::fs_ops::{copy_entry, remove_path};
use super::metadata::{
    load_profile_metadata, sync_profile_metadata_from_auth, sync_profile_quota,
};
use super::paths::{
    get_backup_root, get_codex_home, get_refresh_runtime_dir, validate_profile_name,
};
use super::runtime_isolation::{
    prune_runtime_extra_features, seed_runtime_shared_assets, RUNTIME_AUTH_FILENAME,
    RUNTIME_PROFILE_METADATA_FILENAME,
};

const REFRESH_RUNTIME_PROFILE_FILES: [&str; 2] =
    [RUNTIME_AUTH_FILENAME, RUNTIME_PROFILE_METADATA_FILENAME];

fn should_force_oauth_rotation(profile_name: &str, codex_home: &Path) -> bool {
    let metadata = load_profile_metadata(profile_name, Some(codex_home));
    let Some(last_check) = metadata.last_plan_check_ms else {
        return true;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok());
    match now_ms {
        Some(now) => {
            now.saturating_sub(last_check)
                >= crate::shared::chatgpt_api::PLAN_FRESHNESS_TTL_MS
        }
        None => true,
    }
}

fn ensure_refreshable_auth(auth_path: &Path) -> AppResult<()> {
    let raw = fs::read_to_string(auth_path).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_READ_FAILED",
            format!("Failed to read auth.json {}: {error}", auth_path.display()),
        )
    })?;
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_INVALID",
            format!("Failed to parse auth.json {}: {error}", auth_path.display()),
        )
    })?;
    let auth_mode = parsed
        .get("auth_mode")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let refresh_token = parsed
        .pointer("/tokens/refresh_token")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or_default();

    if !refresh_token.is_empty()
        && !refresh_token.eq_ignore_ascii_case("replace-me")
        && (auth_mode.is_empty() || auth_mode.eq_ignore_ascii_case("chatgpt"))
    {
        Ok(())
    } else {
        Err(AppError::new(
            "PROFILE_AUTH_NOT_REFRESHABLE",
            "Profile auth.json does not contain a refreshable ChatGPT session. Use Login first.",
        ))
    }
}

fn overlay_profile_refresh_files(profile_dir: &Path, runtime_home: &Path) -> AppResult<()> {
    for entry_name in REFRESH_RUNTIME_PROFILE_FILES {
        let src = profile_dir.join(entry_name);
        let dst = runtime_home.join(entry_name);
        if src.exists() {
            copy_entry(&src, &dst)?;
        } else {
            remove_path(&dst)?;
        }
    }

    Ok(())
}

fn prepare_refresh_runtime_home(codex_home: &Path, profile_dir: &Path) -> AppResult<PathBuf> {
    let runtime_home = get_refresh_runtime_dir(Some(codex_home));
    seed_runtime_shared_assets(codex_home, &runtime_home)?;
    prune_runtime_extra_features(&runtime_home)?;
    overlay_profile_refresh_files(profile_dir, &runtime_home)?;
    Ok(runtime_home)
}

/// Best-effort attempt to refresh a profile's quota by hitting the ChatGPT
/// backend directly. Returns:
///   * `Ok(Some(path))` — quota was refreshed via HTTP and persisted.
///   * `Ok(None)` — HTTP path was not applicable or returned an empty
///     payload; caller should fall back to the legacy `codex exec` path.
///   * `Err(_)` — only propagated if `sync_profile_quota` or
///     `sync_profile_metadata_from_auth`
///     fails after a successful HTTP fetch (the metadata write itself is
///     considered authoritative).
fn try_refresh_via_chatgpt_api(
    profile_name: &str,
    codex_home: &Path,
    profile_dir: &Path,
) -> AppResult<Option<String>> {
    // Force OAuth token rotation only when the cached plan info has
    // gone stale (older than `STALE_PLAN_THRESHOLD_MS`). See
    // `mac/runtime/refresh_runtime.rs` for the same logic — this
    // module mirrors it pending the planned mac/win merge.
    let force_rotation = should_force_oauth_rotation(profile_name, codex_home);
    let snapshot = match crate::shared::chatgpt_api::refresh_profile_via_api_with_options(
        profile_name,
        codex_home,
        crate::shared::chatgpt_api::RefreshOptions {
            force_token_rotation: force_rotation,
        },
    ) {
        Ok(value) => value,
        Err(error) => {
            // Mirror of mac/refresh_runtime.rs's relogin handling.
            if crate::shared::chatgpt_api::looks_like_relogin_required(
                &error.error_code,
                &error.message,
            ) {
                return Err(AppError::new(
                    "AUTH_REFRESH_RELOGIN_REQUIRED",
                    "This account session has expired. Please log in again.",
                ));
            }
            eprintln!(
                "chatgpt_api fast path failed for {profile_name} ({}): {}; falling back to app-server RPC",
                error.error_code, error.message
            );
            return Ok(None);
        }
    };
    let plan_type_from_api = snapshot.plan_type.clone();
    let now_ms = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok());
    // The API call succeeded — even if it returned no rate_limit data
    // (typical for downgraded-to-free accounts whose previous Plus /
    // Pro window cached on disk). Always clear stale quota in that
    // case so the dashboard doesn't show old paid-window numbers
    // alongside the now-correct Free plan label.
    sync_profile_quota(
        profile_name,
        snapshot.quota.unwrap_or_default(),
        now_ms,
        Some(codex_home),
    )?;
    sync_profile_metadata_from_auth(profile_name, plan_type_from_api, Some(codex_home))?;
    super::profiles_index::load_profiles_index(Some(codex_home))?;
    Ok(Some(profile_dir.to_string_lossy().into_owned()))
}

pub fn refresh_profile(profile_name: &str) -> AppResult<String> {
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

    let auth_path = profile_dir.join("auth.json");
    if !auth_path.is_file() {
        return Err(AppError::new(
            "PROFILE_AUTH_MISSING",
            format!("Missing auth file: {}", auth_path.display()),
        ));
    }
    ensure_refreshable_auth(&auth_path)?;

    // Fast path for ChatGPT/OAuth profiles: read live quota from the same
    // private backend endpoint the Codex CLI uses, instead of paying for
    // an LLM round-trip. Falls through to the app-server RPC path on any
    // failure so existing behavior is preserved.
    if crate::shared::chatgpt_api::profile_supports_api_refresh(&profile_dir) {
        if let Some(profile_path) =
            try_refresh_via_chatgpt_api(&profile_name, &codex_home, &profile_dir)?
        {
            return Ok(profile_path);
        }
    }

    refresh_via_app_server(&profile_name, &codex_home, &profile_dir, &auth_path)
}

/// Fallback path when the direct ChatGPT HTTP call failed. Uses
/// `codex app-server`'s JSON-RPC interface to fetch the same data
/// (account plan + rate limits) without spawning a real LLM session.
/// Replaces the historical `codex exec "Reply with the single word OK."`
/// hack which burned ~30–90 s and real user quota on every fallback.
fn refresh_via_app_server(
    profile_name: &str,
    codex_home: &Path,
    profile_dir: &Path,
    auth_path: &Path,
) -> AppResult<String> {
    let runtime_codex_home = prepare_refresh_runtime_home(codex_home, profile_dir)?;
    let snapshot = platform::fetch_account_via_app_server(codex_home, &runtime_codex_home)?;

    let refreshed_auth_path = runtime_codex_home.join("auth.json");
    if !refreshed_auth_path.is_file() {
        return Err(AppError::new(
            "AUTH_REFRESH_MISSING",
            "codex app-server completed but no auth.json was found in the refresh runtime home.",
        ));
    }
    copy_entry(&refreshed_auth_path, auth_path)?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok());
    sync_profile_quota(
        profile_name,
        snapshot.quota.unwrap_or_else(QuotaSummary::default),
        now_ms,
        Some(codex_home),
    )?;
    sync_profile_metadata_from_auth(profile_name, snapshot.plan_type, Some(codex_home))?;
    super::profiles_index::load_profiles_index(Some(codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::prepare_refresh_runtime_home;
    use crate::windows::paths::get_refresh_runtime_dir;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-refresh-runtime-{name}-{unique}"))
    }

    #[test]
    fn prepare_refresh_runtime_home_preserves_existing_config_and_prunes_extra_features() {
        let codex_home = temp_codex_home("refresh-runtime-home");
        let profile_dir = codex_home.join("account_backup").join("001");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"account_id":"acct_001"}}"#,
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"001","account_label":"001@example.com"}"#,
        )
        .unwrap();
        fs::write(profile_dir.join("notes.txt"), "profile-local-only").unwrap();

        fs::write(codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();
        fs::write(codex_home.join("AGENTS.md"), "extra-runtime-instructions").unwrap();
        fs::write(codex_home.join("models_cache.json"), "{\"ok\":true}\n").unwrap();
        let plugins_dir = codex_home.join("plugins");
        let cache_dir = codex_home.join("cache");
        let skills_dir = codex_home.join("skills");
        fs::create_dir_all(&plugins_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(plugins_dir.join("plugin.txt"), "shared-plugin").unwrap();
        fs::write(cache_dir.join("cache.txt"), "shared-cache").unwrap();
        fs::write(skills_dir.join("skill.txt"), "should-not-copy").unwrap();

        let runtime_home = get_refresh_runtime_dir(Some(&codex_home));
        fs::create_dir_all(runtime_home.join("memories")).unwrap();
        fs::write(runtime_home.join("AGENTS.md"), "stale-agents").unwrap();
        fs::write(runtime_home.join("config.toml"), "model = \"existing\"\n").unwrap();
        fs::write(
            runtime_home.join("memories").join("old.txt"),
            "stale-memory",
        )
        .unwrap();
        let runtime_home = prepare_refresh_runtime_home(&codex_home, &profile_dir).unwrap();

        assert_eq!(runtime_home, get_refresh_runtime_dir(Some(&codex_home)));
        assert_eq!(
            fs::read_to_string(runtime_home.join("config.toml")).unwrap(),
            "model = \"existing\"\n"
        );
        assert_eq!(
            fs::read_to_string(runtime_home.join("models_cache.json")).unwrap(),
            "{\"ok\":true}\n"
        );
        assert_eq!(
            fs::read_to_string(runtime_home.join("plugins").join("plugin.txt")).unwrap(),
            "shared-plugin"
        );
        assert_eq!(
            fs::read_to_string(runtime_home.join("cache").join("cache.txt")).unwrap(),
            "shared-cache"
        );
        assert_eq!(
            fs::read_to_string(runtime_home.join("auth.json")).unwrap(),
            r#"{"tokens":{"account_id":"acct_001"}}"#
        );
        assert_eq!(
            fs::read_to_string(runtime_home.join("profile.json")).unwrap(),
            r#"{"folder_name":"001","account_label":"001@example.com"}"#
        );
        assert!(!runtime_home.join("AGENTS.md").exists());
        assert!(!runtime_home.join("skills").exists());
        assert!(!runtime_home.join("memories").exists());
        assert!(!runtime_home.join("notes.txt").exists());
        let _ = fs::remove_dir_all(&codex_home);
    }

    mod force_oauth_rotation {
        use super::super::should_force_oauth_rotation;
        use crate::models::ProfileMetadata;
        use crate::shared::chatgpt_api::PLAN_FRESHNESS_TTL_MS;
        use crate::shared::metadata::save_profile_metadata;
        use std::fs;
        use std::path::PathBuf;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn temp_codex_home(name: &str) -> PathBuf {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let pid = std::process::id();
            let path = std::env::temp_dir()
                .join(format!("codex-switch-win-force-rotate-{name}-{pid}-{unique}"));
            fs::create_dir_all(path.join("account_backup").join("a")).unwrap();
            path
        }

        fn now_ms() -> u64 {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        }

        fn write_metadata(codex_home: &PathBuf, last_plan_check_ms: Option<u64>) {
            let mut metadata = ProfileMetadata::with_folder_name("a");
            metadata.last_plan_check_ms = last_plan_check_ms;
            save_profile_metadata("a", &metadata, Some(codex_home)).unwrap();
        }

        #[test]
        fn missing_last_plan_check_forces_rotation() {
            let codex_home = temp_codex_home("missing");
            write_metadata(&codex_home, None);
            assert!(should_force_oauth_rotation("a", &codex_home));
            let _ = fs::remove_dir_all(&codex_home);
        }

        #[test]
        fn recent_last_plan_check_skips_rotation() {
            let codex_home = temp_codex_home("recent");
            write_metadata(&codex_home, Some(now_ms().saturating_sub(60 * 60 * 1000)));
            assert!(!should_force_oauth_rotation("a", &codex_home));
            let _ = fs::remove_dir_all(&codex_home);
        }

        #[test]
        fn stale_last_plan_check_forces_rotation() {
            let codex_home = temp_codex_home("stale");
            write_metadata(
                &codex_home,
                Some(now_ms().saturating_sub(PLAN_FRESHNESS_TTL_MS + 1_000)),
            );
            assert!(should_force_oauth_rotation("a", &codex_home));
            let _ = fs::remove_dir_all(&codex_home);
        }

        #[test]
        fn future_dated_last_plan_check_does_not_force_rotation() {
            let codex_home = temp_codex_home("future");
            write_metadata(&codex_home, Some(now_ms() + 24 * 60 * 60 * 1000));
            assert!(!should_force_oauth_rotation("a", &codex_home));
            let _ = fs::remove_dir_all(&codex_home);
        }
    }
}
