use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::errors::{AppError, AppResult};
use crate::models::SwitchResponse;
use crate::platform::hooks::PlatformHooks;

use super::fs_ops::{
    autosave_auth, backup_root_state_to_profile, overlay_directory_contents, set_active_marker,
};
use super::paths::{get_backup_root, get_codex_home, get_switch_lock_path, validate_profile_name};
use super::profiles::resolve_current_profile;
use super::profiles_index::load_profiles_index;

/// Locks older than this are treated as stale (a previous switch crashed
/// before its `Drop` could clean up), and the new caller is allowed to
/// reclaim the lock. The longest legitimate switch — overlay + login app
/// reopen on Windows — completes well under this window.
const STALE_SWITCH_LOCK_AGE: Duration = Duration::from_secs(60);

struct SwitchGuard {
    lock_path: PathBuf,
}

impl Drop for SwitchGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

fn try_create_lock(lock_path: &Path) -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)
        .map(|_| ())
}

fn lock_is_stale(lock_path: &Path) -> bool {
    let metadata = match std::fs::metadata(lock_path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let modified = match metadata.modified() {
        Ok(value) => value,
        Err(_) => return false,
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age >= STALE_SWITCH_LOCK_AGE)
        .unwrap_or(false)
}

fn acquire_switch_lock(codex_home: Option<&Path>) -> AppResult<SwitchGuard> {
    let lock_path = get_switch_lock_path(codex_home);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create lock directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    if let Err(error) = try_create_lock(&lock_path) {
        // The most common cause is a real concurrent switch. The other case
        // we see in the wild is the GUI dying mid-switch (force quit, OS
        // logout, etc.) and leaving a lock file behind, which then blocks
        // every future switch. Detect that by the lock file's age and
        // reclaim it instead of telling the user to manually delete a file.
        if error.kind() == std::io::ErrorKind::AlreadyExists && lock_is_stale(&lock_path) {
            let _ = std::fs::remove_file(&lock_path);
            try_create_lock(&lock_path).map_err(|retry_error| {
                AppError::new(
                    "SWITCH_IN_PROGRESS",
                    format!(
                        "Stale switch lock cleanup failed: {retry_error}. \
                         Another switch may have started in the meantime."
                    ),
                )
            })?;
        } else {
            return Err(AppError::new(
                "SWITCH_IN_PROGRESS",
                "A profile switch is already in progress.",
            ));
        }
    }

    Ok(SwitchGuard { lock_path })
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

    // When forwarding is active, the running Codex talks to the local
    // sidecar instead of OpenAI directly, so the auth swap is enough — we
    // must not quit/reopen the app. That bypass IS the value proposition of
    // this feature; a restart here would defeat the whole point. We gate
    // on `is_active` (enabled + sidecar listening) rather than `is_enabled`:
    // if the sidecar died unexpectedly, falling back to the quit/reopen
    // path lets Codex pick up the new auth via a clean restart instead of
    // silently failing because the proxy port is dead.
    let gateway_active = super::gateway::is_active(Some(&codex_home));
    let app_was_running = if gateway_active {
        false
    } else {
        hooks.quit_codex_app_if_running()?
    };
    let current_profile = resolve_current_profile(&backup_root);
    if let Some(current_profile) = current_profile.as_deref() {
        backup_root_state_to_profile(current_profile, &codex_home, &backup_root)?;
    }

    autosave_auth(&codex_home)?;
    overlay_directory_contents(&profile_dir, &codex_home)?;
    hooks.sync_root_openai_base_url_for_profile(&profile_name, Some(&codex_home))?;
    set_active_marker(&profile_name, &backup_root)?;
    load_profiles_index(Some(&codex_home))?;
    // Forwarding (when enabled) needs to see the new profile's auth tokens
    // immediately so the in-flight sidecar doesn't keep using the previous
    // profile's credentials. Best-effort: a sidecar hiccup does not roll back
    // the switch we just performed.
    super::gateway::refresh_auths_best_effort(Some(&codex_home));
    let warnings = if gateway_active {
        Vec::new()
    } else {
        hooks.reopen_codex_app_if_needed(app_was_running, Some(&codex_home))
    };

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
        quit_calls: Mutex<u32>,
        reopen_calls: Mutex<Vec<bool>>,
    }

    impl FakeHooks {
        fn new(app_was_running: bool) -> Self {
            Self {
                app_was_running,
                quit_calls: Mutex::new(0),
                reopen_calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl PlatformHooks for FakeHooks {
        fn open_or_activate_codex_app(&self, _codex_home: Option<&Path>) -> AppResult<String> {
            unreachable!("not used in switch_core tests")
        }

        fn quit_codex_app_if_running(&self) -> AppResult<bool> {
            *self.quit_calls.lock().unwrap() += 1;
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

        fn run_codex_login(&self, _codex_home: &Path) -> AppResult<()> {
            unreachable!("not used in switch_core tests")
        }

        fn run_codex_auth_refresh(
            &self,
            _cli_codex_home: &Path,
            _runtime_codex_home: &Path,
        ) -> AppResult<()> {
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
    fn switch_profile_skips_app_lifecycle_when_gateway_is_active() {
        let codex_home = temp_codex_home("switch-gateway-on");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");
        let gateway_dir = backup_root.join("gateway");

        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::create_dir_all(&gateway_dir).unwrap();
        fs::write(codex_home.join("auth.json"), "root-auth-before-switch\n").unwrap();
        fs::write(profile_a_dir.join("auth.json"), "profile-a-auth\n").unwrap();
        fs::write(profile_b_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();

        // Bind a real TCP listener on an ephemeral port so the gateway's
        // `is_active` check (enabled + TCP probe succeeds) actually
        // returns true. Without a live listener, the gating now correctly
        // treats this as "enabled but down" and falls back to quit/reopen.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        fs::write(
            gateway_dir.join("state.json"),
            format!(
                "{{\"enabled\":true,\"port\":{port},\"session_affinity\":true,\
                 \"strategy\":\"round-robin\",\"external_base_url_backup\":null}}",
            ),
        )
        .unwrap();

        let hooks = FakeHooks::new(true);
        let response = switch_profile_with_home(&hooks, "b", Some(&codex_home)).unwrap();
        drop(listener);

        assert!(response.ok);
        assert_eq!(response.profile, "b");
        // Auth swap still happens.
        assert_eq!(
            fs::read_to_string(codex_home.join("auth.json")).unwrap(),
            "profile-b-auth\n"
        );
        // The platform lifecycle hooks must not run when forwarding is
        // healthy.
        assert_eq!(*hooks.quit_calls.lock().unwrap(), 0);
        assert!(hooks.reopen_calls.lock().unwrap().is_empty());

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn switch_profile_falls_back_to_quit_reopen_when_gateway_is_dead() {
        let codex_home = temp_codex_home("switch-gateway-dead");
        let backup_root = codex_home.join("account_backup");
        let profile_a_dir = backup_root.join("a");
        let profile_b_dir = backup_root.join("b");
        let gateway_dir = backup_root.join("gateway");

        fs::create_dir_all(&profile_a_dir).unwrap();
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::create_dir_all(&gateway_dir).unwrap();
        fs::write(codex_home.join("auth.json"), "root-auth-before-switch\n").unwrap();
        fs::write(profile_a_dir.join("auth.json"), "profile-a-auth\n").unwrap();
        fs::write(profile_b_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();

        // enabled=true but port 1 is essentially never listening — simulates
        // the "sidecar died" scenario. is_active must return false so the
        // quit/reopen path runs and the user is not silently stuck talking
        // to a dead local proxy.
        fs::write(
            gateway_dir.join("state.json"),
            r#"{"enabled":true,"port":1,"session_affinity":true,"strategy":"round-robin","external_base_url_backup":null}"#,
        )
        .unwrap();

        let hooks = FakeHooks::new(true);
        let response = switch_profile_with_home(&hooks, "b", Some(&codex_home)).unwrap();

        assert!(response.ok);
        assert_eq!(*hooks.quit_calls.lock().unwrap(), 1);
        assert_eq!(*hooks.reopen_calls.lock().unwrap(), vec![true]);

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn acquire_switch_lock_reclaims_stale_lock() {
        use super::{acquire_switch_lock, STALE_SWITCH_LOCK_AGE};
        use crate::shared::paths::get_switch_lock_path;
        use std::fs::{File, FileTimes, OpenOptions};
        use std::time::{Duration, SystemTime};

        let codex_home = temp_codex_home("stale-lock");
        let backup_root = codex_home.join("account_backup");
        fs::create_dir_all(&backup_root).unwrap();

        let lock_path = get_switch_lock_path(Some(&codex_home));
        fs::write(&lock_path, b"").unwrap();

        // Backdate the lock's mtime so it is unambiguously stale even on
        // filesystems with second-resolution mtime. We add a generous
        // safety margin on top of STALE_SWITCH_LOCK_AGE so the test does
        // not get flaky on slow CI runners.
        let stale_when = SystemTime::now() - STALE_SWITCH_LOCK_AGE - Duration::from_secs(5);
        let times = FileTimes::new().set_modified(stale_when);
        let handle: File = OpenOptions::new().write(true).open(&lock_path).unwrap();
        handle.set_times(times).unwrap();
        drop(handle);

        // The new caller should reclaim the stale lock instead of failing.
        let _guard = acquire_switch_lock(Some(&codex_home))
            .expect("stale switch lock should be reclaimed automatically");

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
