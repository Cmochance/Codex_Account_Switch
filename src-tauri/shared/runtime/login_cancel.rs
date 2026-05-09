//! Cancellation handle for the in-flight `codex login` process.
//!
//! `codex login` blocks until the user finishes (or abandons) the OAuth
//! flow in their browser. There's no way to detect "browser tab closed"
//! from the spawned process, so without this hook a user who closes the
//! tab without completing OAuth would see a permanent spinner — the
//! login process keeps its local callback server up indefinitely.
//!
//! Login serializes through `acquire_login_lock` so at most one codex
//! login runs at a time — a single global slot is enough; we don't need
//! to key by profile.
//!
//! Platform-specific cancellation goes through `kill` (Unix) or
//! `taskkill` (Windows) rather than a `Child` handle so the spawning
//! site can keep using `wait_with_output()` (which consumes the child).

use std::process::Command;
use std::sync::{Mutex, OnceLock};

static CURRENT_LOGIN_PID: OnceLock<Mutex<Option<u32>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<u32>> {
    CURRENT_LOGIN_PID.get_or_init(|| Mutex::new(None))
}

pub fn register_login_pid(pid: u32) {
    *slot().lock().unwrap_or_else(|poison| poison.into_inner()) = Some(pid);
}

pub fn clear_login_pid() {
    *slot().lock().unwrap_or_else(|poison| poison.into_inner()) = None;
}

/// Sends a terminate signal to the in-flight codex login process, if any.
/// Returns `true` when something was actually targeted, `false` when no
/// login was in progress (idempotent — safe to call repeatedly).
pub fn cancel_login_in_progress() -> bool {
    let pid = {
        let mut guard = slot().lock().unwrap_or_else(|poison| poison.into_inner());
        guard.take()
    };
    let Some(pid) = pid else {
        return false;
    };
    kill_pid(pid);
    true
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    let mut command = Command::new("taskkill");
    command.args(["/F", "/T", "/PID", &pid.to_string()]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command.status();
}

#[cfg(not(any(unix, windows)))]
fn kill_pid(_pid: u32) {}
