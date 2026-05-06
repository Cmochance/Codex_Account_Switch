use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::CommandError;
use crate::models::{CurrentQuotaResponse, ProfilesSnapshotResponse};

#[cfg(target_os = "macos")]
use crate::macos as platform_runtime;

#[cfg(not(target_os = "macos"))]
use crate::windows as platform_runtime;

/// Minimum age of the cached quota before the silent background tick will
/// pay for an HTTP refresh. Tuned to be longer than the UI's local 15s
/// JSONL poll (so we don't double-refresh) but short enough that the 5h
/// window stays meaningfully in sync (the window itself updates roughly
/// every minute on the OpenAI side).
const SILENT_REFRESH_MIN_AGE_MS: u64 = 5 * 60 * 1000;

#[tauri::command]
pub fn get_profiles_snapshot() -> Result<ProfilesSnapshotResponse, CommandError> {
    platform_runtime::profiles_index::load_profiles_snapshot(None).map_err(Into::into)
}

#[tauri::command]
pub fn get_current_live_quota() -> Result<CurrentQuotaResponse, CommandError> {
    platform_runtime::profiles_index::load_current_live_quota(None).map_err(Into::into)
}

/// Silent background refresh of the active OAuth profile's quota via the
/// ChatGPT-API path. Skipped (returns the existing snapshot) when:
///   * No active profile is selected.
///   * The active profile is API-key (not OAuth).
///   * The cached quota was updated less than `SILENT_REFRESH_MIN_AGE_MS`
///     ago, so we don't HTTP-spam during fast-tab churn.
///   * The HTTP path itself fails (network, 401 we couldn't recover from,
///     parse error). Failure is treated as "no update", never as an error
///     surfaced to the user — the legacy local-JSONL polling continues to
///     drive the visible quota.
///
/// Returns `CurrentQuotaResponse` so the front-end can apply the snapshot
/// without round-tripping through `get_current_live_quota`.
#[tauri::command]
pub async fn refresh_active_profile_quota_silent() -> Result<CurrentQuotaResponse, CommandError> {
    tauri::async_runtime::spawn_blocking(refresh_active_profile_quota_silent_inner)
        .await
        .map_err(|error| {
            CommandError::new(
                "QUOTA_AUTO_REFRESH_TASK_FAILED",
                format!("Quota auto-refresh task failed: {error}"),
            )
        })?
}

fn refresh_active_profile_quota_silent_inner() -> Result<CurrentQuotaResponse, CommandError> {
    let codex_home = crate::shared::paths::get_codex_home();
    let backup_root = crate::shared::paths::get_backup_root(Some(&codex_home));
    let index = crate::shared::profiles_index::load_profiles_index(Some(&codex_home))
        .map_err(CommandError::from)?;
    let Some(profile_name) = index.current_profile.clone() else {
        return platform_runtime::profiles_index::load_current_live_quota(None)
            .map_err(Into::into);
    };
    let Some(entry) = index
        .profiles
        .iter()
        .find(|profile| profile.folder_name == profile_name)
    else {
        return platform_runtime::profiles_index::load_current_live_quota(None)
            .map_err(Into::into);
    };

    let profile_dir = backup_root.join(&entry.folder_name);
    if !crate::shared::chatgpt_api::profile_supports_api_refresh(&profile_dir) {
        return platform_runtime::profiles_index::load_current_live_quota(None)
            .map_err(Into::into);
    }

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .unwrap_or(0);
    let stored_age_ms = entry
        .stored_quota_updated_at_ms
        .map(|stored| now_ms.saturating_sub(stored))
        .unwrap_or(u64::MAX);
    if stored_age_ms < SILENT_REFRESH_MIN_AGE_MS {
        return platform_runtime::profiles_index::load_current_live_quota(None)
            .map_err(Into::into);
    }

    if let Ok(snapshot) =
        crate::shared::chatgpt_api::refresh_profile_via_api(&profile_name, &codex_home)
    {
        if let Some(quota) = snapshot.quota {
            let _ = crate::shared::metadata::sync_profile_metadata_from_auth_and_quota(
                &profile_name,
                quota,
                Some(now_ms),
                Some(&codex_home),
            );
            let _ =
                crate::shared::profiles_index::load_profiles_index(Some(&codex_home));
        }
    }

    platform_runtime::profiles_index::load_current_live_quota(None).map_err(Into::into)
}
