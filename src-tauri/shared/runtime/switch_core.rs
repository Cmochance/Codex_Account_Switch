use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::models::SwitchResponse;
use crate::platform::hooks::PlatformHooks;

use super::fs_ops::{
    autosave_auth, backup_root_state_to_profile, overlay_directory_contents, set_active_marker,
};
use super::paths::{get_backup_root, get_codex_home, validate_profile_name};
use super::process_lock::{acquire_process_lock, ProcessLockGuard};
use super::profiles::resolve_current_profile;
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
    let current_profile = resolve_current_profile(&backup_root);
    if let Some(current_profile) = current_profile.as_deref() {
        backup_root_state_to_profile(current_profile, &codex_home, &backup_root)?;
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
}
