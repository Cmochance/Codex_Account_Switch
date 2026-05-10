//! Cross-platform "login on a specific account card" workflow.
//!
//! Without this, the only way to (re)log into account `X` was:
//!   1. Switch to `X` (mutates `~/.codex/auth.json`).
//!   2. Run codex login (writes back to `~/.codex/auth.json`).
//!   3. Switch back if `X` wasn't the desired active profile.
//!
//! This module skips the round-trip by spawning `codex login` against a
//! sandboxed CODEX_HOME, capturing the freshly written `auth.json`, and
//! atomically copying it into the target profile's backup folder. The
//! user's live `~/.codex/auth.json` is only touched if the target profile
//! happens to be the currently active one.
//!
//! The workflow is platform-agnostic; all platform-specific behavior
//! flows through `PlatformHooks::run_codex_login`. Tests inject a fake
//! hook that simulates the OAuth handshake by writing an arbitrary
//! `auth.json` into the sandboxed CODEX_HOME.

use std::path::Path;

use crate::errors::{AppError, AppResult};
use crate::platform::hooks::PlatformHooks;

use super::metadata::sync_profile_metadata_from_auth;
use super::paths::{get_backup_root, get_codex_home, validate_profile_name};
use super::process_lock::{acquire_process_lock, ProcessLockGuard};
use super::profiles::resolve_current_profile;
use super::profiles_index::load_profiles_index;
use super::runtime_isolation::{
    clear_runtime_auth_state, prune_runtime_extra_features, seed_runtime_shared_assets,
    RUNTIME_AUTH_FILENAME,
};

fn acquire_login_lock(codex_home: &Path) -> AppResult<ProcessLockGuard> {
    acquire_process_lock(
        Some(codex_home),
        "LOGIN_BUSY",
        "A profile login or switch is already in progress.",
    )
}

/// Build the sandboxed CODEX_HOME for codex login. Idempotent; safe to
/// re-run when a previous attempt left the runtime dir behind.
fn prepare_login_runtime_home(codex_home: &Path, runtime_home: &Path) -> AppResult<()> {
    seed_runtime_shared_assets(codex_home, runtime_home)?;
    prune_runtime_extra_features(runtime_home)?;
    clear_runtime_auth_state(runtime_home)?;
    Ok(())
}

/// Atomic-ish file replace. Stages to a sibling `.tmp` file and renames
/// over the destination. Two Windows-specific wrinkles drive the
/// fallback dance below:
///
/// 1. `std::fs::rename` against an existing file fails when the
///    destination is opened by another process *without*
///    `FILE_SHARE_DELETE` (typical for AV scanners and any handle Codex
///    itself holds on `~/.codex/auth.json`). On the active-profile
///    branch we'd otherwise leave the live auth stale even though the
///    backup folder already got the fresh copy.
/// 2. Even when rename fails, a `truncate-then-write` of the destination
///    succeeds because Windows opens the existing handle with
///    `FILE_SHARE_READ | FILE_SHARE_WRITE` from another process's point
///    of view — they can read while we replace contents.
///
/// So: try the cheap atomic rename first (POSIX + happy-path Windows);
/// if it fails, fall back to writing the temp's bytes into the
/// destination via `std::fs::write` and dropping the temp. The fallback
/// is functionally atomic for the small auth.json payload — write
/// happens in a single syscall, readers see either old or new contents
/// (never a half-written buffer).
fn atomic_publish_file(src: &Path, dst: &Path) -> AppResult<()> {
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AppError::new(
                    "FS_CREATE_FAILED",
                    format!(
                        "Failed to create parent directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
    }
    let temp = dst.with_extension(format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::copy(src, &temp).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        AppError::new(
            "LOGIN_AUTH_PERSIST_FAILED",
            format!(
                "Failed to stage write of {} -> {}: {error}",
                src.display(),
                temp.display()
            ),
        )
    })?;

    if std::fs::rename(&temp, dst).is_ok() {
        return Ok(());
    }

    // Rename failed. Fall back to truncate-then-write of the destination
    // using the bytes we already staged into `temp`.
    let buffered = std::fs::read(&temp).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        AppError::new(
            "LOGIN_AUTH_PERSIST_FAILED",
            format!(
                "Failed to read staged auth {} during fallback publish: {error}",
                temp.display()
            ),
        )
    })?;
    let result = std::fs::write(dst, &buffered).map_err(|error| {
        AppError::new(
            "LOGIN_AUTH_PERSIST_FAILED",
            format!(
                "Failed to publish auth.json to {} via fallback truncate-write: {error}",
                dst.display()
            ),
        )
    });
    let _ = std::fs::remove_file(&temp);
    result
}

/// Drive the per-card login workflow. `runtime_home` must be a path the
/// caller controls (typically `account_backup/<platform>/login_runtime/`).
/// Returns the absolute path of the profile folder whose `auth.json` was
/// just refreshed, mirroring `login_current_profile`'s contract.
pub fn login_profile_with_home<H: PlatformHooks + ?Sized>(
    hooks: &H,
    profile_name: &str,
    codex_home: Option<&Path>,
    runtime_home: &Path,
) -> AppResult<String> {
    let codex_home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    if !backup_root.is_dir() {
        return Err(AppError::new(
            "BACKUP_ROOT_MISSING",
            format!("Backup folder not found: {}", backup_root.display()),
        ));
    }

    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = backup_root.join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let _guard = acquire_login_lock(&codex_home)?;

    prepare_login_runtime_home(&codex_home, runtime_home).map_err(|error| {
        AppError::new(
            "LOGIN_RUNTIME_PREPARE_FAILED",
            format!(
                "Failed to prepare sandboxed CODEX_HOME at {}: {}",
                runtime_home.display(),
                error.message
            ),
        )
    })?;

    // Resolve the real codex binary against the live `~/.codex` (so the
    // user's managed-shim filter and install-state cache work), but
    // point the spawned process's CODEX_HOME at our sandboxed runtime
    // dir. Without this split, a Windows codex shim that does
    // `%CODEX_HOME%\\account_backup\\windows\\codex_switch_cli` would
    // fail to find the script under the empty runtime home.
    hooks.run_codex_login(&codex_home, runtime_home)?;

    let runtime_auth = runtime_home.join(RUNTIME_AUTH_FILENAME);
    if !runtime_auth.is_file() {
        return Err(AppError::new(
            "LOGIN_AUTH_MISSING",
            format!(
                "codex login finished but no auth.json was written to the sandboxed CODEX_HOME at {}",
                runtime_home.display()
            ),
        ));
    }

    let profile_auth = profile_dir.join(RUNTIME_AUTH_FILENAME);
    atomic_publish_file(&runtime_auth, &profile_auth)?;

    // If the profile being logged into is currently active, the live
    // `~/.codex/auth.json` is now stale relative to what we just wrote
    // into the backup folder. Refresh it so any Codex CLI instance the
    // user has open picks up the new token on its next API call.
    if resolve_current_profile(&backup_root).as_deref() == Some(profile_name.as_str()) {
        let live_auth = codex_home.join(RUNTIME_AUTH_FILENAME);
        atomic_publish_file(&runtime_auth, &live_auth)?;
    }

    // Login flow has no fresher plan signal than the new id_token codex
    // just wrote, so let `sync_profile_metadata_from_auth` re-derive
    // plan / label from disk. No API override needed.
    sync_profile_metadata_from_auth(&profile_name, None, Some(&codex_home))?;
    load_profiles_index(Some(&codex_home))?;

    // Best-effort: keep the runtime dir around so the next login on this
    // machine re-uses the seeded models_cache / version pin and skips
    // codex's first-run wizard. Failures here are silent — they only
    // affect the next login's startup latency, not correctness.

    // Best-effort copy of the freshly logged auth.json to do any further
    // bookkeeping (e.g. quota seed). Callers can handle quota refresh as
    // a separate concern; we keep this function focused on auth writes.

    Ok(profile_dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("codex-switch-login-runtime-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// Fake hook that mimics `codex login` by writing whatever
    /// `auth_payload` the test prepared into the runtime CODEX_HOME's
    /// `auth.json`. Records call count so we can assert the lock prevents
    /// double-invocation.
    struct FakeLoginHooks {
        auth_payload: String,
        call_count: Mutex<usize>,
    }

    impl FakeLoginHooks {
        fn new(auth_payload: &str) -> Self {
            Self {
                auth_payload: auth_payload.to_string(),
                call_count: Mutex::new(0),
            }
        }

        fn calls(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    impl PlatformHooks for FakeLoginHooks {
        fn open_or_activate_codex_app(&self, _codex_home: Option<&Path>) -> AppResult<String> {
            unreachable!("not used in login_runtime tests")
        }
        fn quit_codex_app_if_running(&self) -> AppResult<bool> {
            unreachable!("not used in login_runtime tests")
        }
        fn reopen_codex_app_if_needed(
            &self,
            _app_was_running: bool,
            _codex_home: Option<&Path>,
        ) -> Vec<String> {
            unreachable!("not used in login_runtime tests")
        }
        fn run_codex_login(
            &self,
            _cli_codex_home: &Path,
            runtime_codex_home: &Path,
        ) -> AppResult<()> {
            *self.call_count.lock().unwrap() += 1;
            fs::write(runtime_codex_home.join("auth.json"), &self.auth_payload).unwrap();
            Ok(())
        }
        fn fetch_account_via_app_server(
            &self,
            _cli_codex_home: &Path,
            _runtime_codex_home: &Path,
        ) -> AppResult<crate::shared::codex_app_server::AppServerSnapshot> {
            unreachable!("not used in login_runtime tests")
        }
        fn sync_on_window_close(&self) -> AppResult<()> {
            unreachable!("not used in login_runtime tests")
        }
    }

    /// Auth payload that satisfies `sync_profile_metadata_from_auth`'s
    /// expectations enough that it doesn't bail on parse. We don't care
    /// about the metadata itself in these tests; we just need the call
    /// to succeed end-to-end.
    const FAKE_AUTH_JSON: &str = r#"{"OPENAI_API_KEY":null,"auth_mode":"chatgpt","tokens":{"id_token":"","access_token":"","refresh_token":"replace-me","account_id":"acct_login_test"},"last_refresh":"2026-05-08T00:00:00Z"}"#;

    fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let codex_home = temp_root(name);
        let backup_root = codex_home.join("account_backup");
        let profile_dir = backup_root.join("a");
        fs::create_dir_all(&profile_dir).unwrap();
        // Profile starts with a stale auth.json so we can detect the
        // overwrite vs. the fresh one written by the fake login hook.
        fs::write(profile_dir.join("auth.json"), "STALE\n").unwrap();
        let runtime_home = backup_root.join("login_runtime");
        (codex_home, profile_dir, runtime_home)
    }

    #[test]
    fn login_profile_writes_fresh_auth_to_target_profile_folder() {
        let (codex_home, profile_dir, runtime_home) =
            setup("writes-fresh-auth");
        let hooks = FakeLoginHooks::new(FAKE_AUTH_JSON);

        let returned = login_profile_with_home(&hooks, "a", Some(&codex_home), &runtime_home)
            .unwrap();

        assert_eq!(hooks.calls(), 1);
        assert_eq!(returned, profile_dir.to_string_lossy());
        assert_eq!(
            fs::read_to_string(profile_dir.join("auth.json")).unwrap(),
            FAKE_AUTH_JSON,
            "profile auth.json should contain the freshly written payload"
        );
    }

    #[test]
    fn login_profile_also_writes_live_codex_home_when_target_is_active() {
        let (codex_home, _profile_dir, runtime_home) = setup("writes-live");
        // Mark profile "a" as currently active.
        fs::write(
            codex_home.join("account_backup").join(".current_profile"),
            "a\n",
        )
        .unwrap();
        // Live ~/.codex/auth.json starts stale.
        fs::write(codex_home.join("auth.json"), "LIVE_STALE\n").unwrap();
        let hooks = FakeLoginHooks::new(FAKE_AUTH_JSON);

        login_profile_with_home(&hooks, "a", Some(&codex_home), &runtime_home).unwrap();

        assert_eq!(
            fs::read_to_string(codex_home.join("auth.json")).unwrap(),
            FAKE_AUTH_JSON,
            "active-profile login should also refresh ~/.codex/auth.json"
        );
    }

    #[test]
    fn login_profile_leaves_live_codex_home_untouched_when_target_is_inactive() {
        let (codex_home, _profile_dir, runtime_home) = setup("inactive-untouched");
        let backup_root = codex_home.join("account_backup");
        // Set up a second profile and mark it active.
        let profile_b_dir = backup_root.join("b");
        fs::create_dir_all(&profile_b_dir).unwrap();
        fs::write(profile_b_dir.join("auth.json"), "PROFILE_B_STALE\n").unwrap();
        fs::write(backup_root.join(".current_profile"), "b\n").unwrap();
        // Live root reflects "b"'s state.
        fs::write(codex_home.join("auth.json"), "LIVE_FROM_B\n").unwrap();
        let hooks = FakeLoginHooks::new(FAKE_AUTH_JSON);

        // Login into "a" (inactive).
        login_profile_with_home(&hooks, "a", Some(&codex_home), &runtime_home).unwrap();

        assert_eq!(
            fs::read_to_string(codex_home.join("auth.json")).unwrap(),
            "LIVE_FROM_B\n",
            "logging into an inactive profile must not touch ~/.codex/auth.json"
        );
        // And the inactive profile's backup got the fresh auth.
        assert_eq!(
            fs::read_to_string(backup_root.join("a/auth.json")).unwrap(),
            FAKE_AUTH_JSON
        );
    }

    #[test]
    fn login_profile_rejects_concurrent_attempts_via_switch_lock() {
        let (codex_home, _profile_dir, runtime_home) = setup("concurrent");
        // Pre-acquire the shared lock to simulate an in-flight switch.
        let lock_path = super::super::paths::get_switch_lock_path(Some(&codex_home));
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let _staged = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .unwrap();

        let hooks = FakeLoginHooks::new(FAKE_AUTH_JSON);
        let result =
            login_profile_with_home(&hooks, "a", Some(&codex_home), &runtime_home);

        let error = result.unwrap_err();
        assert_eq!(error.error_code, "LOGIN_BUSY");
        assert_eq!(hooks.calls(), 0, "login must not be invoked under contention");
    }

    #[test]
    fn login_profile_surfaces_missing_auth_after_login_finishes() {
        // Hook that "succeeds" without writing auth.json — simulates a
        // codex CLI bug or aborted OAuth flow that exits 0 without
        // producing the expected file.
        struct EmptyLogin;
        impl PlatformHooks for EmptyLogin {
            fn open_or_activate_codex_app(&self, _: Option<&Path>) -> AppResult<String> {
                unreachable!()
            }
            fn quit_codex_app_if_running(&self) -> AppResult<bool> {
                unreachable!()
            }
            fn reopen_codex_app_if_needed(&self, _: bool, _: Option<&Path>) -> Vec<String> {
                unreachable!()
            }
            fn run_codex_login(&self, _: &Path, _: &Path) -> AppResult<()> {
                Ok(())
            }
            fn fetch_account_via_app_server(
                &self,
                _: &Path,
                _: &Path,
            ) -> AppResult<crate::shared::codex_app_server::AppServerSnapshot> {
                unreachable!()
            }
            fn sync_on_window_close(&self) -> AppResult<()> {
                unreachable!()
            }
        }

        let (codex_home, _profile_dir, runtime_home) = setup("missing-auth");
        let result =
            login_profile_with_home(&EmptyLogin, "a", Some(&codex_home), &runtime_home);
        assert_eq!(result.unwrap_err().error_code, "LOGIN_AUTH_MISSING");
    }
}
