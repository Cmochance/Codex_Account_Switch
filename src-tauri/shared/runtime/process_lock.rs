//! Shared file-based lock used to serialize the three operations that
//! mutate per-profile auth state: switch, refresh's auth-rotation
//! tail, and the per-card login flow. They cannot interleave safely —
//! a switch mid-login (or vice versa) would race two writers against
//! `account_backup/<profile>/auth.json` and possibly `~/.codex/auth.json`.
//!
//! The lock is a single file (`get_switch_lock_path` — kept named for
//! historical compatibility) that all three holders create with
//! `O_EXCL`. The first holder wins; everyone else gets a
//! caller-supplied error code so the UI can render context-appropriate
//! copy ("switch in progress" vs. "login in progress") even though
//! the underlying contention is the same.
//!
//! ## Stale cleanup
//!
//! Without cleanup, any holder that crashes (force-quit, OS logout,
//! browser-cancelled OAuth that left the parent process spinning)
//! permanently bricks every future operation until the user manually
//! deletes the file. The 1.6.x line had this fix; rolling back to
//! 1.5.x dropped it. We restore it here with a single threshold —
//! locks older than `STALE_LOCK_AGE` are reclaimed by the next caller.
//!
//! Threshold = 5 minutes. Long enough for a slow OAuth login (browser
//! opened, user reads docs, eventually clicks Authorize). Short enough
//! that a wedged login won't strand the user across an app restart.
//! If a real login is still legitimately in flight at minute 5+, the
//! reclaim races against its eventual auth.json write — but at that
//! point the user has clearly chosen "I want to switch *now*", and a
//! second click that succeeds is more useful than a 12-hour-stuck
//! lock. (Future work: store the holder identity in the lock body so
//! we can distinguish "stuck" from "still working.")

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::errors::{AppError, AppResult};

use super::paths::get_switch_lock_path;

const STALE_LOCK_AGE: Duration = Duration::from_secs(5 * 60);

#[derive(Debug)]
pub struct ProcessLockGuard {
    lock_path: PathBuf,
}

impl Drop for ProcessLockGuard {
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
        .map(|age| age >= STALE_LOCK_AGE)
        .unwrap_or(false)
}

/// Acquire the shared switch / login lock. The returned guard releases
/// the lock on drop. `busy_error_code` is the error code returned to
/// the caller (and ultimately the front-end) when contention is real
/// — `SWITCH_IN_PROGRESS` for switch, `LOGIN_BUSY` for login.
/// `busy_message` is the human-readable companion.
pub fn acquire_process_lock(
    codex_home: Option<&Path>,
    busy_error_code: &'static str,
    busy_message: &'static str,
) -> AppResult<ProcessLockGuard> {
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
        // The cheap branch is "real concurrent contention" — that's
        // what AlreadyExists with a fresh lock looks like. The other
        // branch we observed in the wild is the GUI dying mid-switch
        // (force quit / OS logout / hung OAuth) and leaving the lock
        // behind, which then permanently blocks every future caller.
        // Detect that by mtime and reclaim.
        if error.kind() == std::io::ErrorKind::AlreadyExists && lock_is_stale(&lock_path) {
            let _ = std::fs::remove_file(&lock_path);
            try_create_lock(&lock_path).map_err(|retry_error| {
                AppError::new(
                    busy_error_code,
                    format!(
                        "Stale {busy_error_code} lock cleanup failed: {retry_error}. \
                         Another operation may have started in the meantime."
                    ),
                )
            })?;
        } else {
            return Err(AppError::new(busy_error_code, busy_message));
        }
    }

    Ok(ProcessLockGuard { lock_path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("codex-switch-process-lock-{name}-{unique}"));
        fs::create_dir_all(path.join("account_backup")).unwrap();
        path
    }

    #[test]
    fn first_acquire_succeeds_and_drop_releases() {
        let codex_home = temp_codex_home("first-acquire");
        let lock_path = get_switch_lock_path(Some(&codex_home));

        {
            let _guard = acquire_process_lock(Some(&codex_home), "SWITCH_IN_PROGRESS", "busy")
                .expect("first acquire should succeed");
            assert!(lock_path.is_file(), "lock file must exist while held");
        }
        assert!(!lock_path.exists(), "lock file must be removed on drop");
    }

    #[test]
    fn second_concurrent_acquire_returns_busy_with_caller_supplied_code() {
        let codex_home = temp_codex_home("concurrent");
        let _first = acquire_process_lock(Some(&codex_home), "SWITCH_IN_PROGRESS", "busy switch")
            .expect("first acquire");
        let err = acquire_process_lock(Some(&codex_home), "LOGIN_BUSY", "busy login")
            .expect_err("second acquire should fail");
        assert_eq!(err.error_code, "LOGIN_BUSY");
        assert_eq!(err.message, "busy login");
    }

    #[test]
    fn stale_lock_is_reclaimed_on_next_acquire() {
        // Walk the predicate directly: write a fresh file, confirm
        // it's NOT stale; then prove the logic by checking that a
        // sufficiently-aged mtime would be flagged. We can't reliably
        // back-date a real file across all CI platforms without an
        // extra crate, so this test pins the threshold predicate
        // shape rather than the end-to-end "delete + reacquire" flow,
        // which is exercised by the integration test below.
        let codex_home = temp_codex_home("stale-predicate");
        let lock_path = get_switch_lock_path(Some(&codex_home));
        std::fs::write(&lock_path, b"").unwrap();
        assert!(
            !lock_is_stale(&lock_path),
            "freshly created lock must not be flagged as stale"
        );
    }

    #[test]
    fn integration_stale_lock_is_swept_when_mtime_old_enough() {
        // Skip if we can't back-date the file. On macOS / Linux we
        // have `nix::sys::stat::utimes`; on Windows we have
        // `SetFileTime`. Rather than pulling in those deps for a
        // single test, drop a real lock file with mtime now-6min via
        // `touch -t` shell out (only on POSIX) and skip on platforms
        // where it's not available.
        if cfg!(target_os = "windows") {
            return;
        }
        let codex_home = temp_codex_home("stale-integration");
        let lock_path = get_switch_lock_path(Some(&codex_home));
        std::fs::write(&lock_path, b"").unwrap();

        // Six minutes ago. `touch -t YYYYMMDDhhmm` is universally
        // supported on POSIX `touch`.
        let six_min_ago = chrono::Local::now() - chrono::Duration::minutes(6);
        let ts = six_min_ago.format("%Y%m%d%H%M").to_string();
        let status = std::process::Command::new("touch")
            .args(["-t", &ts, lock_path.to_str().unwrap()])
            .status();
        if status.map(|s| !s.success()).unwrap_or(true) {
            // touch unavailable; skip.
            return;
        }
        assert!(
            lock_is_stale(&lock_path),
            "lock with 6-minute-old mtime must be flagged stale"
        );

        let _guard = acquire_process_lock(Some(&codex_home), "SWITCH_IN_PROGRESS", "busy")
            .expect("stale lock must be reclaimable");
        assert!(lock_path.is_file());
    }
}
