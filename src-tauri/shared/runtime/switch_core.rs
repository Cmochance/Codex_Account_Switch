use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::models::SwitchResponse;
use crate::platform::hooks::PlatformHooks;

use super::fs_ops::{
    autosave_auth, backup_root_state_to_profile, clear_active_markers, overlay_directory_contents,
    set_active_marker,
};
use super::paths::{get_backup_root, get_codex_home, validate_profile_name};
use super::process_lock::{acquire_process_lock, ProcessLockGuard};
use super::profiles::{
    detect_unmanaged_live_account, resolve_backup_target, resolve_current_profile,
};
use super::profiles_index::load_profiles_index;

fn acquire_switch_lock(codex_home: Option<&Path>) -> AppResult<ProcessLockGuard> {
    acquire_process_lock(
        codex_home,
        "SWITCH_IN_PROGRESS",
        "A profile switch is already in progress.",
    )
}

pub fn switch_profile_with_home<H: PlatformHooks + ?Sized>(
    hooks: &H,
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<SwitchResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    if !backup_root.is_dir() {
        return Err(AppError::new(
            "BACKUP_ROOT_MISSING",
            format!("Backup folder not found: {}", backup_root.display()),
        ));
    }

    let profile_name = validate_profile_name(profile_name)?;
    let _guard = acquire_switch_lock(Some(&codex_home))?;
    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }
    if !profile_dir.join("auth.json").is_file() {
        return Err(AppError::new(
            "PROFILE_AUTH_MISSING",
            format!(
                "Missing auth file: {}",
                profile_dir.join("auth.json").display()
            ),
        ));
    }

    let app_was_running = hooks.quit_codex_app_if_running()?;
    // Back up the live root state to the profile it *actually* belongs to,
    // verified by account identity — never the `.current_profile` marker
    // blindly. If the live account drifted (manual `codex login`, external
    // re-auth) the marker can be stale, and a blind copy would overwrite an
    // unrelated profile's credentials. `None` = nothing safe to save; skip it.
    if let Some(backup_target) = resolve_backup_target(&backup_root, &codex_home) {
        backup_root_state_to_profile(&backup_target, &codex_home, &backup_root)?;
    }

    autosave_auth(&codex_home)?;
    overlay_directory_contents(&profile_dir, &codex_home)?;
    hooks.sync_root_openai_base_url_for_profile(&profile_name, Some(&codex_home))?;
    set_active_marker(&profile_name, &backup_root)?;
    load_profiles_index(Some(&codex_home))?;
    let warnings = hooks.reopen_codex_app_if_needed(app_was_running, Some(&codex_home));

    Ok(SwitchResponse {
        ok: true,
        profile: profile_name.clone(),
        message: format!("Switched to profile: {profile_name}"),
        warnings,
    })
}

/// Launch-time reconciliation: save the live `~/.codex` state back into the
/// profile that actually owns it (identity-verified), healing a stale
/// `.current_profile` marker when the live account has drifted to a different
/// managed profile.
///
/// Shared by the macOS and Windows bootstrap so the two platforms can't
/// diverge (previously this body was mirror-copied into each platform's
/// `bootstrap.rs`). Returns the profile the state was synced into, or `None`
/// when the live account matches no managed profile — in which case the
/// write-back is skipped so a drifted account can't contaminate a slot on
/// launch, and the stale marker is intentionally left untouched (no managed
/// profile to point it at).
pub fn sync_root_state_to_current_profile_with_home(
    codex_home: Option<&Path>,
) -> AppResult<Option<String>> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));

    let Some(target) = resolve_backup_target(&backup_root, &codex_home) else {
        // No identity-verified target. If the live account is a real account
        // that no profile owns (drift to an unmanaged account), clear the stale
        // marker so the dashboard stops showing a wrong "current" card and
        // surfaces the unmanaged-account prompt instead. Otherwise (no /
        // unparseable auth) leave the markers untouched.
        if detect_unmanaged_live_account(&backup_root, &codex_home).is_some() {
            clear_active_markers(&backup_root)?;
        }
        load_profiles_index(Some(&codex_home))?;
        return Ok(None);
    };

    // If the live account drifted to a different managed profile than the
    // marker claims, heal the marker so the UI and the next switch agree with
    // what is really in ~/.codex.
    if resolve_current_profile(&backup_root).as_deref() != Some(target.as_str()) {
        set_active_marker(&target, &backup_root)?;
    }

    backup_root_state_to_profile(&target, &codex_home, &backup_root)?;
    load_profiles_index(Some(&codex_home))?;
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::errors::AppResult;
    use crate::platform::hooks::PlatformHooks;

    use super::switch_profile_with_home;
    use crate::shared::paths::{get_current_profile_file, get_profiles_index_path};

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
            unreachable!("not used in switch_core tests")
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
            unreachable!("not used in switch_core tests")
        }

        fn fetch_account_via_app_server(
            &self,
            _cli_codex_home: &Path,
            _runtime_codex_home: &Path,
        ) -> AppResult<crate::shared::codex_app_server::AppServerSnapshot> {
            unreachable!("not used in switch_core tests")
        }

        fn sync_on_window_close(&self) -> AppResult<()> {
            unreachable!("not used in switch_core tests")
        }
    }

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-shared-switch-core-{name}-{unique}"))
    }

    #[test]
    fn switch_profile_preserves_windows_behavior_through_hooks() {
        let codex_home = temp_codex_home("switch-success");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");

        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::write(codex_home.join("auth.json"), "root-auth-before-switch\n").unwrap();
        fs::write(profile_a_dir.join("auth.json"), "profile-a-auth\n").unwrap();
        fs::write(profile_b_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();

        let hooks = FakeHooks::new(true);
        let response = switch_profile_with_home(&hooks, "b", Some(&codex_home)).unwrap();

        assert!(response.ok);
        assert_eq!(response.profile, "b");
        assert_eq!(
            fs::read_to_string(codex_home.join("auth.json")).unwrap(),
            "profile-b-auth\n"
        );
        assert_eq!(
            fs::read_to_string(profile_a_dir.join("auth.json")).unwrap(),
            "root-auth-before-switch\n"
        );
        assert_eq!(
            fs::read_to_string(get_current_profile_file(Some(&codex_home))).unwrap(),
            "b\n"
        );
        assert!(profile_b_dir.join(".active_profile").is_file());
        assert!(get_profiles_index_path(Some(&codex_home)).is_file());
        assert_eq!(*hooks.reopen_calls.lock().unwrap(), vec![true]);

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn switch_profile_rejects_missing_profile_auth_before_running_hooks() {
        let codex_home = temp_codex_home("missing-auth");
        let backup_root = codex_home.join("account_backup");
        fs::create_dir_all(backup_root.join("b")).unwrap();

        let hooks = FakeHooks::new(false);
        let error = switch_profile_with_home(&hooks, "b", Some(&codex_home)).unwrap_err();

        assert_eq!(error.error_code, "PROFILE_AUTH_MISSING");
        assert!(hooks.reopen_calls.lock().unwrap().is_empty());
        let _ = fs::remove_dir_all(&codex_home);
    }

    fn auth_with_account(account_id: &str) -> String {
        format!(
            "{{\"tokens\":{{\"account_id\":{}}}}}",
            serde_json::Value::String(account_id.to_string())
        )
    }

    /// Regression for the "串号" / account cross-contamination bug. When the
    /// live `~/.codex/auth.json` has drifted to an account that the
    /// `.current_profile` marker does not actually name (e.g. a manual
    /// `codex login` outside the app), switching must NOT blind-copy that live
    /// account into the stale marker's profile slot. Before the identity guard
    /// this test failed: profile "a" got overwritten with account Z.
    #[test]
    fn switch_does_not_contaminate_stale_marker_profile_with_drifted_account() {
        let codex_home = temp_codex_home("drift-no-contaminate");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");
        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();

        // Card "a" holds account X, card "b" holds account B.
        fs::write(profile_a_dir.join("auth.json"), auth_with_account("acct_X")).unwrap();
        fs::write(profile_b_dir.join("auth.json"), auth_with_account("acct_B")).unwrap();
        // Marker still says "a", but the live root drifted to a stranger Z.
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_Z")).unwrap();

        let hooks = FakeHooks::new(false);
        let response = switch_profile_with_home(&hooks, "b", Some(&codex_home)).unwrap();

        assert!(response.ok);
        // The guard must have refused to write Z into card "a".
        assert_eq!(
            fs::read_to_string(profile_a_dir.join("auth.json")).unwrap(),
            auth_with_account("acct_X"),
            "stale-marker profile must keep its own account, not the drifted live one"
        );
        // Switch still completed: root now reflects card "b".
        assert_eq!(
            fs::read_to_string(codex_home.join("auth.json")).unwrap(),
            auth_with_account("acct_B")
        );

        let _ = fs::remove_dir_all(&codex_home);
    }

    // Launch-time bootstrap: live account drifted from the marker ("a") to a
    // different *managed* profile ("b"). Sync must route the write-back to "b",
    // heal the marker to "b", and leave "a" untouched.
    #[test]
    fn bootstrap_sync_heals_marker_and_routes_backup_on_drift_to_managed_profile() {
        let codex_home = temp_codex_home("bootstrap-heal");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");
        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::write(profile_a_dir.join("auth.json"), auth_with_account("acct_X")).unwrap();
        // "b" genuinely owns account B (stored token is older than the live one).
        let b_stale = "{\"tokens\":{\"account_id\":\"acct_B\"},\"last_refresh\":\"old\"}";
        fs::write(profile_b_dir.join("auth.json"), b_stale).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        // Live root drifted to account B (same account, freshly refreshed) — e.g.
        // the user re-logged b's account outside the app while the marker said "a".
        let b_fresh = "{\"tokens\":{\"account_id\":\"acct_B\"},\"last_refresh\":\"new\"}";
        fs::write(codex_home.join("auth.json"), b_fresh).unwrap();

        let synced = super::sync_root_state_to_current_profile_with_home(Some(&codex_home)).unwrap();

        assert_eq!(synced.as_deref(), Some("b"));
        // Marker healed to the real owner.
        assert_eq!(
            fs::read_to_string(get_current_profile_file(Some(&codex_home))).unwrap(),
            "b\n"
        );
        // Live (refreshed) state saved into "b", not the stale marker "a".
        assert_eq!(
            fs::read_to_string(profile_b_dir.join("auth.json")).unwrap(),
            b_fresh
        );
        assert_eq!(
            fs::read_to_string(profile_a_dir.join("auth.json")).unwrap(),
            auth_with_account("acct_X"),
            "the stale-marker profile must be left untouched"
        );

        let _ = fs::remove_dir_all(&codex_home);
    }

    // Launch-time bootstrap: live account belongs to no managed profile. Sync
    // must skip the write-back (no contamination) AND clear the stale marker so
    // the dashboard stops showing a wrong "current" card.
    #[test]
    fn bootstrap_sync_clears_marker_and_preserves_slots_when_live_account_unmanaged() {
        let codex_home = temp_codex_home("bootstrap-clear-unmanaged");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::write(profile_a_dir.join("auth.json"), auth_with_account("acct_X")).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        fs::write(profile_a_dir.join(".active_profile"), "x\n").unwrap();
        // Live root drifted to a brand-new, unmanaged account Z.
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_Z")).unwrap();

        let synced = super::sync_root_state_to_current_profile_with_home(Some(&codex_home)).unwrap();

        assert_eq!(synced, None);
        // No contamination: "a" keeps its own account.
        assert_eq!(
            fs::read_to_string(profile_a_dir.join("auth.json")).unwrap(),
            auth_with_account("acct_X")
        );
        // Stale marker cleared (no managed profile owns the live account).
        assert!(
            !get_current_profile_file(Some(&codex_home)).exists(),
            ".current_profile must be cleared on unmanaged drift"
        );
        assert!(
            !profile_a_dir.join(".active_profile").exists(),
            ".active_profile markers must be cleared on unmanaged drift"
        );

        let _ = fs::remove_dir_all(&codex_home);
    }

    // Launch-time bootstrap happy path: marker already names the live account.
    // Sync refreshes that slot and leaves the marker unchanged.
    #[test]
    fn bootstrap_sync_refreshes_marked_slot_when_identity_matches() {
        let codex_home = temp_codex_home("bootstrap-happy");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::write(profile_a_dir.join("auth.json"), "A_STALE\n").unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        // Live root is account X — the same account "a" represents (token refreshed).
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_X")).unwrap();
        // Seed "a" with X's identity so resolve_backup_target's happy path matches.
        fs::write(profile_a_dir.join("auth.json"), auth_with_account("acct_X")).unwrap();
        // Now make the live copy differ only in a trailing field to prove it is copied.
        let refreshed = "{\"tokens\":{\"account_id\":\"acct_X\"},\"last_refresh\":\"now\"}";
        fs::write(codex_home.join("auth.json"), refreshed).unwrap();

        let synced = super::sync_root_state_to_current_profile_with_home(Some(&codex_home)).unwrap();

        assert_eq!(synced.as_deref(), Some("a"));
        assert_eq!(
            fs::read_to_string(get_current_profile_file(Some(&codex_home))).unwrap(),
            "a\n"
        );
        assert_eq!(
            fs::read_to_string(profile_a_dir.join("auth.json")).unwrap(),
            refreshed,
            "matched slot should receive the refreshed live auth"
        );

        let _ = fs::remove_dir_all(&codex_home);
    }
}
