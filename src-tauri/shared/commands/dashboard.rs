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
        let plan_type_from_api = snapshot.plan_type.clone();
        // The API call succeeded — clear any stale quota even when
        // `snapshot.quota` is None (downgraded-to-free path) so we
        // don't render previous-tier numbers next to a freshly
        // updated plan label. Plan update runs unconditionally so the
        // dashboard catches downgrades the same way.
        let _ = crate::shared::metadata::sync_profile_quota(
            &profile_name,
            snapshot.quota.unwrap_or_default(),
            Some(now_ms),
            Some(&codex_home),
        );
        let _ = crate::shared::metadata::sync_profile_metadata_from_auth(
            &profile_name,
            plan_type_from_api,
            Some(&codex_home),
        );
        let _ = crate::shared::profiles_index::load_profiles_index(Some(&codex_home));
    }

    platform_runtime::profiles_index::load_current_live_quota(None).map_err(Into::into)
}

/// Daily / startup background pass: serially refresh every OAuth profile
/// with `force_token_rotation = true` so the cached id_token claims
/// (plan tier, subscription expiry) move forward even for inactive
/// profiles that the 5-min ticker never visits. Runs serially to avoid
/// hammering OpenAI's auth endpoint with N parallel refresh requests.
///
/// Failures per-profile are silent — this is best-effort, never blocks
/// the user, and the dashboard's other refresh paths remain the
/// authoritative source if any individual refresh fails.
///
/// The frontend invokes this twice: once on app startup, then on every
/// local-day rollover (detected by a setInterval comparing the cached
/// last-run date with `new Date().toDateString()`).
#[tauri::command]
pub async fn refresh_all_oauth_profile_plans_silent() -> Result<u32, CommandError> {
    tauri::async_runtime::spawn_blocking(refresh_all_oauth_profile_plans_silent_inner)
        .await
        .map_err(|error| {
            CommandError::new(
                "PLAN_BULK_REFRESH_TASK_FAILED",
                format!("Bulk plan refresh task failed: {error}"),
            )
        })?
}

fn refresh_all_oauth_profile_plans_silent_inner() -> Result<u32, CommandError> {
    let codex_home = crate::shared::paths::get_codex_home();
    let backup_root = crate::shared::paths::get_backup_root(Some(&codex_home));
    let index = crate::shared::profiles_index::load_profiles_index(Some(&codex_home))
        .map_err(CommandError::from)?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .unwrap_or(0);

    let mut refreshed: u32 = 0;
    for entry in &index.profiles {
        let profile_dir = backup_root.join(&entry.folder_name);
        if !crate::shared::chatgpt_api::profile_supports_api_refresh(&profile_dir) {
            continue;
        }

        // Skip profiles whose plan info was confirmed within the
        // last 6 hours. Without this gate, every app launch and
        // every day rollover paid for N × (OAuth POST + usage GET)
        // back-to-back regardless of whether anything could have
        // changed; on a 5-account workspace that's 10–25 s of
        // background work the user could see "trickling" into the
        // cards. With it, repeat launches within a working day are
        // free.
        if let Some(last_check) = entry.last_plan_check_ms {
            if now_ms.saturating_sub(last_check)
                < crate::shared::chatgpt_api::PLAN_FRESHNESS_TTL_MS
            {
                continue;
            }
        }

        let snapshot = match crate::shared::chatgpt_api::refresh_profile_via_api_with_options(
            &entry.folder_name,
            &codex_home,
            crate::shared::chatgpt_api::RefreshOptions {
                force_token_rotation: true,
            },
        ) {
            Ok(value) => value,
            Err(error) => {
                // Best-effort: a single account failing must not abort
                // the batch (one expired refresh_token shouldn't block
                // the other four profiles from getting fresh plan
                // info). Log it so a systematic failure across every
                // profile is visible in stderr / Tauri logs instead of
                // silently leaving the dashboard stale.
                eprintln!(
                    "bulk plan refresh skipped {} ({}): {}",
                    entry.folder_name, error.error_code, error.message
                );
                continue;
            }
        };

        let plan_type_from_api = snapshot.plan_type.clone();
        // Update both quota and plan unconditionally on a successful
        // API response. `unwrap_or_default()` clears stale paid-window
        // data for downgraded-to-free profiles (the API returns no
        // rate_limit for them); without this clear we'd show old
        // 5h/weekly numbers next to a freshly-updated Free label.
        if let Err(error) = crate::shared::metadata::sync_profile_quota(
            &entry.folder_name,
            snapshot.quota.unwrap_or_default(),
            Some(now_ms),
            Some(&codex_home),
        ) {
            eprintln!(
                "bulk plan refresh: failed to persist quota for {}: {}",
                entry.folder_name, error.message
            );
        }
        if let Err(error) = crate::shared::metadata::sync_profile_metadata_from_auth(
            &entry.folder_name,
            plan_type_from_api,
            Some(&codex_home),
        ) {
            eprintln!(
                "bulk plan refresh: failed to persist metadata for {}: {}",
                entry.folder_name, error.message
            );
        }
        refreshed += 1;
    }

    if refreshed > 0 {
        if let Err(error) = crate::shared::profiles_index::load_profiles_index(Some(&codex_home)) {
            eprintln!(
                "bulk plan refresh: post-loop index reload failed: {}",
                error.message
            );
        }
    }
    Ok(refreshed)
}
