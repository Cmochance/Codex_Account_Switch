use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::errors::{AppError, AppResult};

use super::fs_ops::{copy_entry, remove_path};
use super::metadata::{sync_profile_metadata_from_auth, sync_profile_quota};
use super::paths::{
    get_backup_root, get_codex_home, get_refresh_runtime_dir, validate_profile_name,
};
use super::process::run_codex_auth_refresh;
use super::session_files::{collect_jsonl_files, file_modified_ms};
use super::session_usage::load_latest_local_quota_snapshot_since;

const REFRESH_RUNTIME_SHARED_FILES: [&str; 4] = [
    "models_cache.json",
    "version.json",
    ".codex-global-state.json",
    "cap_sid",
];
const REFRESH_RUNTIME_SHARED_DIRS: [&str; 3] = ["plugins", "cache", "sqlite"];
const REFRESH_RUNTIME_REMOVED_FILES: [&str; 1] = ["AGENTS.md"];
const REFRESH_RUNTIME_REMOVED_DIRS: [&str; 4] = ["rules", "skills", "vendor_imports", "memories"];
const REFRESH_RUNTIME_PROFILE_FILES: [&str; 2] = ["auth.json", "profile.json"];

fn generated_refresh_session_files(
    runtime_home: &Path,
    min_source_mtime_ms: Option<u64>,
) -> Vec<PathBuf> {
    let sessions_root = runtime_home.join("sessions");
    if !sessions_root.is_dir() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_jsonl_files(&sessions_root, &mut files);
    files
        .into_iter()
        .filter(|path| {
            !min_source_mtime_ms
                .is_some_and(|min_mtime| file_modified_ms(path).unwrap_or(0) < min_mtime)
        })
        .collect()
}

fn cleanup_generated_refresh_sessions(session_files: &[PathBuf]) {
    for path in session_files {
        let _ = remove_path(path);
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

fn seed_refresh_runtime_shared_assets(codex_home: &Path, runtime_home: &Path) -> AppResult<()> {
    fs::create_dir_all(runtime_home).map_err(|error| {
        AppError::new(
            "REFRESH_RUNTIME_CREATE_FAILED",
            format!(
                "Failed to create refresh runtime home {}: {error}",
                runtime_home.display()
            ),
        )
    })?;

    for entry_name in REFRESH_RUNTIME_SHARED_FILES {
        let src = codex_home.join(entry_name);
        let dst = runtime_home.join(entry_name);
        if src.exists() {
            copy_entry(&src, &dst)?;
        } else {
            remove_path(&dst)?;
        }
    }

    for entry_name in REFRESH_RUNTIME_SHARED_DIRS {
        let src = codex_home.join(entry_name);
        let dst = runtime_home.join(entry_name);
        if src.exists() && !dst.exists() {
            copy_entry(&src, &dst)?;
        }
    }

    Ok(())
}

fn prune_refresh_runtime_extra_features(runtime_home: &Path) -> AppResult<()> {
    for entry_name in REFRESH_RUNTIME_REMOVED_FILES {
        remove_path(&runtime_home.join(entry_name))?;
    }

    for entry_name in REFRESH_RUNTIME_REMOVED_DIRS {
        remove_path(&runtime_home.join(entry_name))?;
    }

    Ok(())
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
    seed_refresh_runtime_shared_assets(codex_home, &runtime_home)?;
    prune_refresh_runtime_extra_features(&runtime_home)?;
    overlay_profile_refresh_files(profile_dir, &runtime_home)?;
    Ok(runtime_home)
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

    let runtime_codex_home = prepare_refresh_runtime_home(&codex_home, &profile_dir)?;
    let refresh_started_at_ms = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok());
    run_codex_auth_refresh(&codex_home, &runtime_codex_home)?;
    let generated_session_files =
        generated_refresh_session_files(&runtime_codex_home, refresh_started_at_ms);
    let refreshed_quota =
        load_latest_local_quota_snapshot_since(Some(&runtime_codex_home), refresh_started_at_ms);
    cleanup_generated_refresh_sessions(&generated_session_files);

    let refreshed_auth_path = runtime_codex_home.join("auth.json");
    if !refreshed_auth_path.is_file() {
        return Err(AppError::new(
            "AUTH_REFRESH_MISSING",
            "Codex refresh completed but no auth.json was found in the refresh runtime home.",
        ));
    }

    copy_entry(&refreshed_auth_path, &auth_path)?;
    sync_profile_metadata_from_auth(&profile_name, Some(&codex_home))?;
    if let Some(snapshot) = refreshed_quota {
        sync_profile_quota(
            &profile_name,
            snapshot.quota,
            snapshot.source_mtime_ms,
            Some(&codex_home),
        )?;
    }
    super::profiles_index::load_profiles_index(Some(&codex_home))?;

    Ok(profile_dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        cleanup_generated_refresh_sessions, generated_refresh_session_files,
        prepare_refresh_runtime_home,
    };
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
        fs::write(runtime_home.join("memories").join("old.txt"), "stale-memory").unwrap();
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

    #[test]
    fn cleanup_generated_refresh_sessions_only_removes_new_sessions() {
        let codex_home = temp_codex_home("cleanup-refresh-sessions");
        let runtime_home = get_refresh_runtime_dir(Some(&codex_home));
        let sessions_dir = runtime_home.join("sessions").join("2026").join("04").join("08");
        fs::create_dir_all(&sessions_dir).unwrap();

        let old_session = sessions_dir.join("rollout-old.jsonl");
        let new_session = sessions_dir.join("rollout-new.jsonl");
        fs::write(&old_session, "{\"type\":\"event_msg\"}\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let threshold = fs::metadata(&old_session)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 1;
        fs::write(&new_session, "{\"type\":\"event_msg\"}\n").unwrap();

        let generated = generated_refresh_session_files(&runtime_home, Some(threshold));
        cleanup_generated_refresh_sessions(&generated);

        assert!(old_session.is_file());
        assert!(!new_session.exists());
        let _ = fs::remove_dir_all(&codex_home);
    }
}
