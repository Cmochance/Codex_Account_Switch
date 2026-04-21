use std::path::{Path, PathBuf};

use crate::errors::AppResult;
use crate::models::{CurrentQuotaResponse, ProfileIndexEntry, ProfilesIndex};

use super::paths::get_codex_home;
use super::session_usage::{
    load_latest_local_quota_snapshot, normalize_quota_summary, LocalQuotaSnapshot,
};

pub use crate::shared::profiles_index::{load_profiles_index, load_profiles_snapshot};

fn select_current_quota(
    entry: &ProfileIndexEntry,
    live_snapshot: Option<&LocalQuotaSnapshot>,
) -> crate::models::QuotaSummary {
    let stored_updated_at_ms = entry.stored_quota_updated_at_ms.unwrap_or(0);

    match live_snapshot {
        Some(snapshot) if snapshot.source_mtime_ms.unwrap_or(0) > stored_updated_at_ms => {
            snapshot.quota.clone()
        }
        _ => entry.stored_quota.clone(),
    }
}

pub fn load_current_live_quota(codex_home: Option<&Path>) -> AppResult<CurrentQuotaResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let index: ProfilesIndex = load_profiles_index(Some(&codex_home))?;
    let Some(current_profile) = index.current_profile.clone() else {
        return Ok(CurrentQuotaResponse {
            profile: None,
            quota: None,
        });
    };
    let Some(entry) = index
        .profiles
        .iter()
        .find(|profile| profile.folder_name == current_profile)
    else {
        return Ok(CurrentQuotaResponse {
            profile: Some(current_profile),
            quota: None,
        });
    };

    let live_snapshot = load_latest_local_quota_snapshot(Some(&codex_home));
    let quota = normalize_quota_summary(
        Some(select_current_quota(entry, live_snapshot.as_ref())),
        entry.plan_name.as_deref(),
        entry.has_account_identity,
    );

    Ok(CurrentQuotaResponse {
        profile: Some(entry.folder_name.clone()),
        quota: Some(quota),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::load_current_live_quota;
    use crate::windows::env_guard;

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-win-profiles-index-{name}-{unique}"))
    }

    fn write_quota_session(path: PathBuf, primary_used_percent: f64, secondary_used_percent: f64) {
        let line = format!(
            r#"{{"type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"limit_id":"codex","primary":{{"used_percent":{primary_used_percent},"resets_at":1730000000,"window_minutes":300}},"secondary":{{"used_percent":{secondary_used_percent},"resets_at":1730600000,"window_minutes":10080}}}}}}}}"#
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, format!("{line}\n")).unwrap();
    }

    #[test]
    fn current_live_quota_prefers_stored_target_quota_when_it_was_primed_after_switch() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("prefer-primed-stored");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            codex_home.join("account_backup").join(".current_profile"),
            "b\n",
        )
        .unwrap();
        fs::write(profile_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"b","account_label":"b@example.com","quota":{"five_hour":{},"weekly":{}},"quota_updated_at_ms":9999999999999}"#,
        )
        .unwrap();
        write_quota_session(
            codex_home.join("sessions").join("session-001.jsonl"),
            11.0,
            12.0,
        );

        let response = load_current_live_quota(Some(&codex_home)).unwrap();

        assert_eq!(response.profile.as_deref(), Some("b"));
        let quota = response.quota.expect("expected quota");
        assert_eq!(quota.five_hour.remaining_percent, None);
        assert_eq!(quota.weekly.remaining_percent, None);

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn current_live_quota_still_uses_live_session_when_it_is_newer_than_stored_quota() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("prefer-live-when-newer");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            codex_home.join("account_backup").join(".current_profile"),
            "b\n",
        )
        .unwrap();
        fs::write(profile_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"b","account_label":"b@example.com","quota":{"five_hour":{"remaining_percent":41},"weekly":{"remaining_percent":63}},"quota_updated_at_ms":1}"#,
        )
        .unwrap();
        write_quota_session(
            codex_home.join("sessions").join("session-001.jsonl"),
            11.0,
            12.0,
        );

        let response = load_current_live_quota(Some(&codex_home)).unwrap();

        assert_eq!(response.profile.as_deref(), Some("b"));
        let quota = response.quota.expect("expected quota");
        assert_eq!(quota.five_hour.remaining_percent, Some(89));
        assert_eq!(quota.weekly.remaining_percent, Some(88));

        let _ = fs::remove_dir_all(&codex_home);
    }
}
