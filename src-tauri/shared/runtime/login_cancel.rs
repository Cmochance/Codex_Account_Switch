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
//! ## Design: keep the `Child` handle, not just the PID
//!
//! v1.5.6's first cut stored only the OS PID and re-discovered the
//! process via `kill(pid, …)` / `taskkill /F /PID …`. That was wrong:
//! once `wait_with_output` returned, the slot's PID was stale, and a
//! late cancel click could SIGTERM (or, on Windows with `/F /T`, kill
//! a whole process tree of) an unrelated process that happened to
//! recycle the PID. The window is short but the worst case is bad
//! enough that we now hold the actual `Child` handle.
//!
//! The trade-off is that `Child::wait_with_output` consumes the child,
//! so the spawning site can't both stash the child and wait on it. We
//! resolve that with a try_wait poll loop:
//!
//! 1. Spawn → put `Child` in the slot.
//! 2. Loop: lock the slot, peek with `Child::try_wait`. If `Some(status)`
//!    take the child out *within the same lock* (no race with cancel)
//!    and call `wait_with_output` outside the lock to collect stdio.
//!    If `None`, drop the lock and sleep briefly.
//! 3. Cancel: lock the slot, take the `Child` out, call `Child::kill`,
//!    drop. Subsequent loop iterations on the spawn site see `None`
//!    and return `LOGIN_CANCELLED`.
//!
//! Because both "spawn site notices natural exit" and "cancel kills"
//! atomically `take()` the same `Mutex<Option<Child>>`, only one path
//! ever owns the child at a time — there's no PID-reuse window left.

use std::io::Read;
use std::process::{Child, Output};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use crate::errors::{AppError, AppResult};

static CURRENT_LOGIN: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

/// Polling cadence for the spawn-site loop when peeking at the in-flight
/// child via `try_wait`. 200 ms is short enough that login completion
/// feels instant to the user yet long enough to keep the syscall load
/// negligible — codex login itself is dominated by the OAuth round-trip
/// (seconds), so polling latency is dwarfed by user think time.
pub const POLL_INTERVAL: Duration = Duration::from_millis(200);

fn slot() -> &'static Mutex<Option<Child>> {
    CURRENT_LOGIN.get_or_init(|| Mutex::new(None))
}

fn lock() -> MutexGuard<'static, Option<Child>> {
    slot().lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Kill a child and reap it. Both calls are best-effort: `kill` may
/// fail because the process already exited (ESRCH/InvalidInput) or, on
/// Windows, because it's mid-teardown — either way `wait` is what
/// actually reaps the zombie on Unix and frees the handle on Windows.
/// Without the `wait` we'd leak a zombie per cancelled login (Unix
/// `Child` has no reaping `Drop`).
///
/// **Windows caveat:** if `TerminateProcess` fails (e.g. the child is
/// running elevated and the app isn't), `Child::kill` returns Err and
/// the subsequent `wait` blocks until the child exits naturally —
/// which for a stuck `codex login` could be the OAuth timeout
/// (minutes). In practice the app spawns codex login as a non-elevated
/// child of itself so access-denied shouldn't fire; if it ever does
/// we'd want to bound the wait or move it to a detached thread.
/// Tracking that as a follow-up rather than blocking on it here.
pub(crate) fn drop_killed_child(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Stash the spawned child so a concurrent cancel can find it.
/// `acquire_login_lock` is supposed to keep this slot single-occupant,
/// but if a previous login somehow leaked its child (e.g. via the
/// `try_wait` Err path inside `peek_login_status`), reap it before
/// overwriting so we don't leave a zombie behind.
///
/// **Caller invariant**: stdout/stderr handles must already be `take()`
/// off the child. `wait_for_login_or_cancel` does this so its drainer
/// threads own the pipes; if a future caller forgets, the
/// overwrite-kill path here can't release the prev drainer's pipe FDs
/// (the prev drainer still owns them) — but the prev child's process
/// gets killed, the OS closes the write end, the drainer EOFs and
/// exits. The debug_assert guards against that subtle invariant
/// silently breaking.
pub fn register_login_child(child: Child) {
    debug_assert!(
        child.stdout.is_none() && child.stderr.is_none(),
        "register_login_child requires stdio to be taken first; \
         see wait_for_login_or_cancel"
    );
    if let Some(prev) = lock().replace(child) {
        drop_killed_child(prev);
    }
}

/// Outcome of one peek at the slot from the spawn site.
pub enum LoginPeek {
    /// Cancel already took the child. Spawn site should bail with
    /// LOGIN_CANCELLED.
    Cancelled,
    /// The codex login process exited on its own; the slot has been
    /// emptied and the `Child` is handed back so the caller can
    /// `wait_with_output()` to collect stdio.
    Exited(Child),
    /// Still running. Caller should sleep `POLL_INTERVAL` and try again.
    Running,
}

/// Inspect (and possibly drain) the slot. The take-on-exit branch and
/// the cancel path both go through this same `lock`/`take` pair, so
/// cancel can't race a natural exit — only one of them ever ends up
/// owning the child.
pub fn peek_login_status() -> std::io::Result<LoginPeek> {
    let mut guard = lock();
    let Some(child) = guard.as_mut() else {
        return Ok(LoginPeek::Cancelled);
    };
    match child.try_wait()? {
        Some(_status) => {
            let child = guard.take().expect("checked Some above");
            Ok(LoginPeek::Exited(child))
        }
        None => Ok(LoginPeek::Running),
    }
}

/// Drive a freshly spawned codex login child to completion, surfacing
/// cancellation as `LOGIN_CANCELLED`. The caller passes a `Child` whose
/// stdout/stderr are already piped — we take ownership of those pipe
/// handles and **drain them concurrently** in dedicated threads, so the
/// child can never block on a write to a full pipe buffer (typically
/// 64 KB). Without that drain, a verbose `codex login` would fill its
/// stderr pipe, block on write, and our `try_wait` poll loop would see
/// `Running` forever — login appears stuck. (P1 caught by the
/// chatgpt-codex-connector review on PR #29.)
///
/// Both platform `run_codex_login` implementations call this; keeping
/// the orchestration here means there's exactly one place that knows
/// the contract between "spawn site holds the child" and "cancel takes
/// the child" — they both go through `peek_login_status` and the slot.
pub fn wait_for_login_or_cancel(mut child: Child) -> AppResult<Output> {
    // Take the stdio handles before parking the Child in the slot. The
    // drainer threads own them for the duration of the process; when
    // the child dies (naturally or via cancel kill) the OS closes the
    // pipes, the drainers see EOF, and they hand back their accumulated
    // bytes via thread::join.
    let stdout_drainer = child.stdout.take().map(spawn_pipe_drainer);
    let stderr_drainer = child.stderr.take().map(spawn_pipe_drainer);

    register_login_child(child);

    loop {
        match peek_login_status() {
            Ok(LoginPeek::Exited(mut child)) => {
                // try_wait already cached the status; wait() returns it
                // without re-reaping. Match `wait_with_output`'s
                // contract by calling wait() before joining drainers.
                let status = child.wait().map_err(|error| {
                    AppError::new(
                        "LOGIN_COMMAND_FAILED",
                        format!("`codex login` reap failed: {error}"),
                    )
                })?;
                let stdout = stdout_drainer
                    .map(join_drainer)
                    .unwrap_or_default();
                let stderr = stderr_drainer
                    .map(join_drainer)
                    .unwrap_or_default();
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(LoginPeek::Cancelled) => {
                // Drainer threads will see EOF when the killed child's
                // pipes close and exit on their own — we don't need
                // their accumulated output for the cancelled path.
                return Err(AppError::new(
                    "LOGIN_CANCELLED",
                    "Login was cancelled before the OAuth flow finished.",
                ));
            }
            Ok(LoginPeek::Running) => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => {
                // try_wait failed mid-poll. Drain the slot and reap the
                // child ourselves — leaving it stashed would let the
                // next login's `register_login_child` silently kill+reap
                // it (or, worse, drop it without reaping if that path
                // ever loses the cleanup), and the codex login process
                // could keep running in the background long enough to
                // write `auth.json` and confuse subsequent state.
                if let Some(orphan) = lock().take() {
                    drop_killed_child(orphan);
                }
                return Err(AppError::new(
                    "LOGIN_COMMAND_FAILED",
                    format!("`codex login` wait failed: {error}"),
                ));
            }
        }
    }
}

fn spawn_pipe_drainer<P: Read + Send + 'static>(
    mut pipe: P,
) -> std::thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        // Best-effort: an I/O error mid-read (broken pipe, EINTR storm,
        // OS-level pipe error) just leaves whatever was already read in
        // `buf`. We accept partial stdio rather than escalate to
        // LOGIN_COMMAND_FAILED because:
        //   1) On a successful login (status.success() == true) the
        //      bytes are purely informational — auth.json is already
        //      written by the codex login process before stdio closes.
        //   2) On a failed login, partial bytes are still better than
        //      no diagnostic message; the toast surfaces what we got.
        // This trade-off would be worth revisiting if the project ever
        // gains a structured logger so we could at least emit a
        // breadcrumb for the rare Err path.
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

fn join_drainer(handle: std::thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    // A panicked drainer collapses to empty bytes. Realistic causes are
    // allocator failure (OOM) on the read buffer or a future regression
    // that introduces an unwrap inside the closure — both rare and both
    // distinct from "child exited cleanly with empty output". Same
    // trade-off applies as the read_to_end Err path: we don't have a
    // structured logger to surface the panic, so we lose visibility
    // into it. If this ever fires in the wild, the symptom is "login
    // succeeded but toast has no detail" — annoying, not corrupting.
    handle.join().unwrap_or_default()
}

/// Send a kill signal to the in-flight codex login child, if any, and
/// reap it. Returns `true` when something was actually targeted,
/// `false` when no login was in progress (idempotent — safe to call
/// repeatedly).
pub fn cancel_login_in_progress() -> bool {
    let Some(child) = lock().take() else {
        return false;
    };
    drop_killed_child(child);
    // The spawn-site loop is responsible for surfacing LOGIN_CANCELLED
    // on its next iteration via `peek_login_status` returning Cancelled.
    true
}

// Tests rely on `/bin/sleep` and `/bin/sh` to spawn real, killable
// children. They run on macOS + Linux (which is what our CI exercises)
// and are gated off Windows so a hypothetical `cargo test` there
// doesn't fail to find those binaries.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;
    use std::time::Instant;

    /// Tests in this module touch a process-wide static slot, so they
    /// must serialize even though `cargo test` runs them in parallel by
    /// default.
    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn spawn_sleep(seconds: &str) -> Child {
        // Use the absolute path; some test runners (and the `cargo test`
        // binary itself when run from certain shells) hand the spawned
        // command a stripped PATH that doesn't include `/bin`.
        Command::new("/bin/sleep")
            .arg(seconds)
            .spawn()
            .expect("spawn sleep")
    }

    fn drain_slot() {
        if let Some(child) = lock().take() {
            super::drop_killed_child(child);
        }
    }

    #[test]
    fn cancel_returns_false_when_no_login_in_progress() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();
        assert!(!cancel_login_in_progress());
    }

    #[test]
    fn cancel_kills_registered_child_and_clears_slot() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        let child = spawn_sleep("30");
        let pid = child.id();
        register_login_child(child);

        let started = Instant::now();
        assert!(cancel_login_in_progress());
        // Slot is cleared atomically with the take.
        assert!(lock().is_none());
        // We dropped the child but the process is being killed by the
        // OS in the background. Wait briefly for it to actually exit so
        // a hypothetical `kill -0 pid` would fail. We can't easily
        // verify the kernel state portably without `waitpid`, but the
        // important contract — slot cleared + cancel reported true —
        // is already covered.
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = pid; // silence unused warning when the assert above is the only check
    }

    #[test]
    fn second_cancel_after_first_is_a_noop() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        let child = spawn_sleep("30");
        register_login_child(child);

        assert!(cancel_login_in_progress());
        assert!(!cancel_login_in_progress());
    }

    #[test]
    fn peek_returns_cancelled_after_cancel() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        let child = spawn_sleep("30");
        register_login_child(child);

        cancel_login_in_progress();
        match peek_login_status().expect("peek") {
            LoginPeek::Cancelled => {}
            LoginPeek::Exited(_) => panic!("expected Cancelled, got Exited"),
            LoginPeek::Running => panic!("expected Cancelled, got Running"),
        }
    }

    #[test]
    fn peek_returns_exited_after_natural_exit_and_drains_slot() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        // Sleep 0 exits immediately; on macOS / Linux this is a few ms.
        let child = spawn_sleep("0");
        register_login_child(child);

        // Give the OS a moment to reap the very short sleep.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match peek_login_status().expect("peek") {
                LoginPeek::Exited(mut child) => {
                    // Slot must have been drained atomically.
                    assert!(lock().is_none());
                    // Reap so the test doesn't leak a zombie.
                    let _ = child.wait();
                    return;
                }
                LoginPeek::Running => {
                    if Instant::now() > deadline {
                        panic!("sleep 0 did not exit within 2s");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                LoginPeek::Cancelled => panic!("unexpected Cancelled before any cancel"),
            }
        }
    }

    #[test]
    fn wait_for_login_or_cancel_returns_output_on_natural_exit() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        let child = Command::new("/bin/sh")
            .args(["-c", "echo hello; exit 0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn shell");

        let output = wait_for_login_or_cancel(child).expect("ok");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
        assert!(lock().is_none(), "slot must be empty after natural exit");
    }

    #[test]
    fn register_login_child_replaces_and_reaps_previous_occupant() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        // Stage a long-lived "leftover" child as if a prior login had
        // wedged its handle in the slot (e.g. via the try_wait Err
        // cleanup path racing a new login). Without the replace+reap
        // contract this child would be silently dropped on the next
        // register call and leak a zombie on Unix.
        let leftover = spawn_sleep("30");
        let leftover_pid = leftover.id();
        register_login_child(leftover);

        let fresh = spawn_sleep("30");
        let fresh_pid = fresh.id();
        register_login_child(fresh);

        // Slot now holds the fresh child; the leftover was killed and
        // reaped inside register_login_child.
        let stashed_pid = lock().as_ref().map(|c| c.id());
        assert_eq!(stashed_pid, Some(fresh_pid));
        assert_ne!(stashed_pid, Some(leftover_pid));

        // Drain the fresh one too so the test doesn't leak.
        drain_slot();
    }

    #[test]
    fn wait_for_login_or_cancel_drains_oversized_pipe_output() {
        // Regression for the codex-codex-connector P1 on PR #29: if
        // try_wait polls a piped child without draining stdio, a
        // verbose codex login can fill its 64 KB pipe buffer and block
        // on write forever. Push 256 KiB through stdout and confirm
        // wait_for_login_or_cancel still returns Ok with the full
        // payload.
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        // Absolute paths sidestep PATH manipulation that other parallel
        // tests (e.g. `macos::process` discovery tests) do via
        // `std::env::set_var("PATH", ...)`. Without this, the shell
        // would fail to find yes/head and exit non-zero in parallel
        // runs even though the same test passes single-threaded.
        let target_bytes: usize = 256 * 1024;
        let child = Command::new("/bin/sh")
            .args([
                "-c",
                &format!("/usr/bin/yes | /usr/bin/head -c {target_bytes}"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn shell");

        let output = wait_for_login_or_cancel(child).expect("ok");
        assert!(output.status.success());
        assert_eq!(
            output.stdout.len(),
            target_bytes,
            "drainer should have collected the full {target_bytes}-byte payload"
        );
        assert!(lock().is_none());
    }

    #[test]
    fn wait_for_login_or_cancel_captures_stderr_on_nonzero_exit() {
        // Counterpart to the stdout-overflow regression test: exercise
        // the stderr drainer in isolation, with a non-zero exit code,
        // because production feeds `output.stderr` (not stdout) into
        // the LOGIN_FAILED toast when codex login itself errors out.
        // 256 KiB is well past any pipe buffer the kernels we ship to
        // can hold without explicit fcntl resizing.
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        let target_bytes: usize = 256 * 1024;
        let child = Command::new("/bin/sh")
            .args([
                "-c",
                &format!("/usr/bin/yes | /usr/bin/head -c {target_bytes} 1>&2; exit 1"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn shell");

        let output = wait_for_login_or_cancel(child).expect("ok");
        assert!(!output.status.success());
        assert_eq!(output.stdout.len(), 0);
        assert_eq!(
            output.stderr.len(),
            target_bytes,
            "stderr drainer should have collected the full {target_bytes}-byte payload"
        );
        assert!(lock().is_none());
    }

    #[test]
    fn wait_for_login_or_cancel_returns_login_cancelled_when_killed_by_another_thread() {
        let _guard = test_lock().lock().unwrap_or_else(|p| p.into_inner());
        drain_slot();

        let child = Command::new("/bin/sleep")
            .arg("30")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sleep");

        // Cancel from a sibling thread once the orchestrator has had a
        // chance to register the child. Polling the slot rather than
        // sleeping a fixed amount keeps the test stable on slow CI
        // runners where Command::spawn + register can take longer than
        // a heuristic delay.
        let canceller = std::thread::spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(2);
            while lock().is_none() {
                if Instant::now() > deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            cancel_login_in_progress()
        });

        let result = wait_for_login_or_cancel(child);
        let cancelled = canceller.join().expect("canceller thread");

        assert!(cancelled, "cancel should have found a registered child");
        let err = result.expect_err("expected LOGIN_CANCELLED");
        assert_eq!(err.error_code, "LOGIN_CANCELLED");
        assert!(lock().is_none(), "slot must be empty after cancel");
    }
}
