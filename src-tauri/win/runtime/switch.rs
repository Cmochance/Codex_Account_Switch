use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::models::{ProfileMetadata, QuotaSummary, SwitchResponse};
use crate::platform::hooks::PlatformHooks;

use crate::{platform, shared::switch_core};

use super::metadata::{load_profile_metadata, sync_profile_quota};
use super::paths::{
    get_backup_root, get_codex_home, validate_profile_name, PROFILE_METADATA_FILENAME,
};
use super::profiles::resolve_current_profile;
use super::session_usage::load_latest_local_quota_snapshot;

fn current_time_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
}

fn quota_summary_has_data(quota: &QuotaSummary) -> bool {
    quota.five_hour.remaining_percent.is_some()
        || quota.five_hour.refresh_at.is_some()
        || quota.weekly.remaining_percent.is_some()
        || quota.weekly.refresh_at.is_some()
}

fn write_root_profile_metadata(codex_home: &Path, metadata: &ProfileMetadata) -> AppResult<()> {
    let metadata_path = codex_home.join(PROFILE_METADATA_FILENAME);
    let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
        AppError::new(
            "PROFILE_METADATA_INVALID",
            format!("Failed to serialize metadata: {error}"),
        )
    })?;

    fs::write(&metadata_path, format!("{serialized}\n")).map_err(|error| {
        AppError::new(
            "PROFILE_METADATA_WRITE_FAILED",
            format!(
                "Failed to write root profile metadata {}: {error}",
                metadata_path.display()
            ),
        )
    })
}

fn sync_current_profile_quota_before_switch(codex_home: &Path) -> AppResult<()> {
    let backup_root = get_backup_root(Some(codex_home));
    let Some(current_profile) = resolve_current_profile(&backup_root) else {
        return Ok(());
    };

    let mut metadata = load_profile_metadata(&current_profile, Some(codex_home));
    if let Some(snapshot) = load_latest_local_quota_snapshot(Some(codex_home)) {
        let live_is_newer =
            snapshot.source_mtime_ms.unwrap_or(0) > metadata.quota_updated_at_ms.unwrap_or(0);
        if live_is_newer || !quota_summary_has_data(&metadata.quota) {
            metadata = sync_profile_quota(
                &current_profile,
                snapshot.quota,
                snapshot.source_mtime_ms,
                Some(codex_home),
            )?;
        }
    }

    write_root_profile_metadata(codex_home, &metadata)
}

fn prime_target_profile_quota_before_switch(
    profile_name: &str,
    codex_home: &Path,
) -> AppResult<()> {
    let metadata = load_profile_metadata(profile_name, Some(codex_home));
    sync_profile_quota(
        profile_name,
        metadata.quota,
        current_time_ms().or(metadata.quota_updated_at_ms),
        Some(codex_home),
    )?;
    Ok(())
}

fn switch_profile_with_home_and_hooks<H: PlatformHooks + ?Sized>(
    hooks: &H,
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<SwitchResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    sync_current_profile_quota_before_switch(&codex_home)?;
    prime_target_profile_quota_before_switch(&profile_name, &codex_home)?;
    switch_core::switch_profile_with_home(hooks, &profile_name, Some(&codex_home))
}

pub fn switch_profile_with_home(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<SwitchResponse> {
    switch_profile_with_home_and_hooks(platform::current_hooks(), profile_name, codex_home)
}

pub fn switch_profile(profile_name: &str) -> AppResult<SwitchResponse> {
    switch_profile_with_home(profile_name, None)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::errors::AppResult;
    use crate::platform::hooks::PlatformHooks;
    use crate::shared::paths::{get_current_profile_file, get_profiles_index_path};

    use super::switch_profile_with_home_and_hooks;

    struct FakeHooks {
        app_was_running: bool,
        reopen_calls: Mutex<Vec<bool>>,
    }

    impl FakeHooks {
        fn new(app_was_running: bool) -> Self {
            Self {
                app_was_running,
                reopen_calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl PlatformHooks for FakeHooks {
        fn open_or_activate_codex_app(&self, _codex_home: Option<&Path>) -> AppResult<String> {
            unreachable!("not used in switch tests")
        }

        fn quit_codex_app_if_running(&self) -> AppResult<bool> {
            Ok(self.app_was_running)
        }

        fn reopen_codex_app_if_needed(
            &self,
            app_was_running: bool,
            _codex_home: Option<&Path>,
        ) -> Vec<String> {
            self.reopen_calls.lock().unwrap().push(app_was_running);
            Vec::new()
        }

        fn run_codex_login(
            &self,
            _cli_codex_home: &Path,
            _runtime_codex_home: &Path,
        ) -> AppResult<()> {
            unreachable!("not used in switch tests")
        }

        fn fetch_account_via_app_server(
            &self,
            _cli_codex_home: &Path,
            _runtime_codex_home: &Path,
        ) -> AppResult<crate::shared::codex_app_server::AppServerSnapshot> {
            unreachable!("not used in switch tests")
        }

        fn sync_on_window_close(&self) -> AppResult<()> {
            unreachable!("not used in switch tests")
        }
    }

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-win-switch-{name}-{unique}"))
    }

    fn write_quota_session(path: &Path) {
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":11.0,"resets_at":1730000000,"window_minutes":300},"secondary":{"used_percent":12.0,"resets_at":1730600000,"window_minutes":10080}}}}"#;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, format!("{line}\n")).unwrap();
    }

    #[test]
    fn switch_profile_syncs_current_live_quota_into_previous_profile_card() {
        let codex_home = temp_codex_home("live-quota");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");

        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::write(codex_home.join("auth.json"), "root-auth-before-switch\n").unwrap();
        fs::write(codex_home.join("profile.json"), r#"{"folder_name":"a"}"#).unwrap();
        fs::write(profile_a_dir.join("auth.json"), "profile-a-auth\n").unwrap();
        fs::write(
            profile_a_dir.join("profile.json"),
            r#"{"folder_name":"a","quota":{"five_hour":{"remaining_percent":25},"weekly":{"remaining_percent":50}},"quota_updated_at_ms":1}"#,
        )
        .unwrap();
        fs::write(profile_b_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(profile_b_dir.join("profile.json"), r#"{"folder_name":"b"}"#).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        write_quota_session(&codex_home.join("sessions").join("session-001.jsonl"));

        let hooks = FakeHooks::new(false);
        let response = switch_profile_with_home_and_hooks(&hooks, "b", Some(&codex_home)).unwrap();

        assert!(response.ok);
        let stored = fs::read_to_string(profile_a_dir.join("profile.json")).unwrap();
        assert!(stored.contains(r#""remaining_percent": 89"#));
        assert!(stored.contains(r#""remaining_percent": 88"#));
        assert_eq!(
            fs::read_to_string(get_current_profile_file(Some(&codex_home))).unwrap(),
            "b\n"
        );
        assert!(get_profiles_index_path(Some(&codex_home)).is_file());

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn switch_profile_preserves_existing_stored_quota_when_live_snapshot_is_older() {
        let codex_home = temp_codex_home("older-live-quota");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");

        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::write(codex_home.join("auth.json"), "root-auth-before-switch\n").unwrap();
        fs::write(profile_a_dir.join("auth.json"), "profile-a-auth\n").unwrap();
        fs::write(
            profile_a_dir.join("profile.json"),
            r#"{"folder_name":"a","quota":{"five_hour":{"remaining_percent":77},"weekly":{"remaining_percent":66}},"quota_updated_at_ms":9999999999999}"#,
        )
        .unwrap();
        fs::write(profile_b_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(profile_b_dir.join("profile.json"), r#"{"folder_name":"b"}"#).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        write_quota_session(&codex_home.join("sessions").join("session-001.jsonl"));

        let hooks = FakeHooks::new(false);
        let response = switch_profile_with_home_and_hooks(&hooks, "b", Some(&codex_home)).unwrap();

        assert!(response.ok);
        let stored = fs::read_to_string(profile_a_dir.join("profile.json")).unwrap();
        assert!(stored.contains(r#""remaining_percent": 77"#));
        assert!(stored.contains(r#""remaining_percent": 66"#));

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn switch_profile_primes_target_profile_quota_timestamp_before_switch() {
        let codex_home = temp_codex_home("prime-target-quota");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");

        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::write(codex_home.join("auth.json"), "root-auth-before-switch\n").unwrap();
        fs::write(codex_home.join("profile.json"), r#"{"folder_name":"a"}"#).unwrap();
        fs::write(profile_a_dir.join("auth.json"), "profile-a-auth\n").unwrap();
        fs::write(profile_a_dir.join("profile.json"), r#"{"folder_name":"a"}"#).unwrap();
        fs::write(profile_b_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(
            profile_b_dir.join("profile.json"),
            r#"{"folder_name":"b","quota":{"five_hour":{"remaining_percent":41},"weekly":{"remaining_percent":63}}}"#,
        )
        .unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        write_quota_session(&codex_home.join("sessions").join("session-001.jsonl"));
        let live_mtime_ms = fs::metadata(codex_home.join("sessions").join("session-001.jsonl"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let hooks = FakeHooks::new(false);
        let response = switch_profile_with_home_and_hooks(&hooks, "b", Some(&codex_home)).unwrap();

        assert!(response.ok);
        let stored = fs::read_to_string(profile_b_dir.join("profile.json")).unwrap();
        assert!(stored.contains(r#""remaining_percent": 41"#));
        assert!(stored.contains(r#""remaining_percent": 63"#));
        let parsed = serde_json::from_str::<serde_json::Value>(&stored).unwrap();
        let stored_updated_at_ms = parsed
            .get("quota_updated_at_ms")
            .and_then(|value| value.as_u64())
            .unwrap();
        assert!(stored_updated_at_ms >= live_mtime_ms);

        let _ = fs::remove_dir_all(&codex_home);
    }
}
