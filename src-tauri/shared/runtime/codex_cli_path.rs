//! Shared `InstallState` schema + `CodexPathResolver` trait + the
//! Tauri-command helpers (`get_codex_cli_status` / `set_codex_cli_path`
//! / `clear_codex_cli_path` / `redetect_codex_cli_path` /
//! `build_codex_cli_status`).
//!
//! Before this module each platform (`mac/runtime/process.rs` +
//! `mac/runtime/profile_actions.rs` and the Windows mirrors) carried
//! its own byte-identical copy of `InstallState`, the
//! `RealCodexPathSource` enum, and the four wrappers. That violated
//! the project rule (`feedback_share_dont_duplicate`) that cross-
//! platform logic must live in `shared/`. The platform-specific bits
//! that remain — discovery walks, Windows extension resolution,
//! managed-shim filtering — are kept per-platform and reached through
//! the `CodexPathResolver` trait so this shared layer is OS-agnostic.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::errors::AppResult;
use crate::models::{CodexCliCandidate, CodexCliRedetectResult, CodexCliStatus};

/// Persistent install metadata. Both mac and Windows used to declare
/// this struct independently; consolidating here keeps the on-disk
/// schema and the `user_codex_path` extension single-sourced.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstallState {
    pub real_codex_path: Option<String>,
    #[serde(default)]
    pub path_added_by_installer: bool,
    /// User-provided override for the real codex CLI path. Takes
    /// priority over auto-discovery when valid; falls back silently to
    /// auto-discovery when the file disappears so users aren't
    /// permanently wedged.
    #[serde(default)]
    pub user_codex_path: Option<String>,
}

/// Where a resolved codex CLI path came from. Frontend i18n maps this
/// to a label so users can tell whether they're looking at their
/// manual override or the auto-discovered path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealCodexPathSource {
    UserOverride,
    InstallState,
    Discovery,
}

impl RealCodexPathSource {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::UserOverride => "user_override",
            Self::InstallState => "install_state",
            Self::Discovery => "discovery",
        }
    }
}

/// Platform-specific resolver. Mac and Windows each implement this on
/// top of their existing per-platform discovery code (PATH walks,
/// `where codex`, Codex.app bundle probing, Windows extension
/// resolution, managed-shim filtering, `install_state.json` IO). The
/// shared helpers below treat the resolver as a black box.
pub trait CodexPathResolver {
    /// Resolve the real codex CLI path with provenance, or `None` if
    /// nothing is found.
    fn resolve_with_source(&self, codex_home: &Path)
        -> Option<(PathBuf, RealCodexPathSource)>;

    /// Validate + persist a user-provided override. Returns the
    /// canonicalised path that was actually saved (Windows resolves
    /// extensions, so the saved path may differ from the input).
    fn set_user_path(&self, codex_home: &Path, raw_input: &str) -> AppResult<PathBuf>;

    /// Drop any user override and let auto-discovery take over again.
    fn clear_user_path(&self, codex_home: &Path);

    /// Common install locations that exist on disk right now. Frontend
    /// renders these as click-to-fill chips in the dialog.
    fn suggested_paths(&self, codex_home: &Path) -> Vec<PathBuf>;

    /// Force a fresh scan that ignores the cached/override path and
    /// returns every candidate verified runnable via `codex --version`
    /// (deduped, best-first, with the version string captured). Backs the
    /// Settings "auto-detect" button: `resolve_with_source` trusts a
    /// previously-saved path, so when that path is wrong — or there are
    /// several installs — the user needs this to rescan from scratch.
    fn redetect_runnable_paths(&self, codex_home: &Path) -> Vec<CodexCliCandidate>;
}

/// Build the snapshot the front-end consumes. Used by both
/// `get_codex_cli_status` and as a return value after set/clear so the
/// dialog and the Settings row can refresh in lock-step.
pub fn build_codex_cli_status(
    resolver: &dyn CodexPathResolver,
    codex_home: &Path,
) -> CodexCliStatus {
    let (resolved_path, source) = match resolver.resolve_with_source(codex_home) {
        Some((path, source)) => (
            Some(path.to_string_lossy().into_owned()),
            source.as_label().to_string(),
        ),
        None => (None, "none".to_string()),
    };
    let suggested_paths = resolver
        .suggested_paths(codex_home)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    CodexCliStatus {
        resolved_path,
        source,
        suggested_paths,
    }
}

pub fn get_codex_cli_status(
    resolver: &dyn CodexPathResolver,
    codex_home: &Path,
) -> CodexCliStatus {
    build_codex_cli_status(resolver, codex_home)
}

pub fn set_codex_cli_path(
    resolver: &dyn CodexPathResolver,
    codex_home: &Path,
    raw_input: &str,
) -> AppResult<CodexCliStatus> {
    resolver.set_user_path(codex_home, raw_input)?;
    Ok(build_codex_cli_status(resolver, codex_home))
}

pub fn clear_codex_cli_path(
    resolver: &dyn CodexPathResolver,
    codex_home: &Path,
) -> CodexCliStatus {
    resolver.clear_user_path(codex_home);
    build_codex_cli_status(resolver, codex_home)
}

/// Force a fresh detection scan and report which candidates are
/// runnable, alongside a refreshed status snapshot. Backs the Settings
/// "auto-detect" button — the front-end auto-applies a lone candidate
/// and lets the user pick when several survive the probe.
pub fn redetect_codex_cli_path(
    resolver: &dyn CodexPathResolver,
    codex_home: &Path,
) -> CodexCliRedetectResult {
    CodexCliRedetectResult {
        candidates: resolver.redetect_runnable_paths(codex_home),
        status: build_codex_cli_status(resolver, codex_home),
    }
}

/// Run a configured `codex --version` `command`, bounded by `timeout`,
/// and return its trimmed first stdout line on a successful exit. This is
/// the one-shot "is this a real, runnable codex, and which version" probe
/// behind auto-detect: `Some(version)` means it ran and exited 0 (the
/// string may be empty if it printed nothing parseable); `None` means it
/// couldn't spawn, exited non-zero, errored, or overran the timeout (the
/// child is then killed so a hung / input-waiting binary can't wedge the
/// scan). Shared so mac + Windows reuse the poll/kill/capture logic; each
/// platform only builds the `Command` (console hiding, extension res).
pub fn probe_version_with_timeout(command: Command, timeout: Duration) -> Option<String> {
    run_capturing_stdout_with_timeout(command, timeout).map(|stdout| {
        stdout
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("")
            .to_owned()
    })
}

/// After the probed child exits, how long to wait for its stdout to finish
/// draining before giving up on capturing it. A child that left a
/// background process holding the pipe never yields EOF, so this bounds
/// that case instead of blocking forever.
const STDOUT_DRAIN_GRACE: Duration = Duration::from_secs(1);

/// Run `command` bounded by `timeout`, draining stdout on a reader thread
/// so a chatty child can't backpressure its own pipe and deadlock. Returns
/// the full captured stdout on a successful (exit-0) run; `None` if it
/// couldn't spawn, exited non-zero, errored, or overran the timeout (the
/// child is then killed). Shared by the `codex --version` probe (caller
/// takes the first line = version) and the login-shell resolver (caller
/// takes the last line = path), so the poll / kill / drain logic — and the
/// hard timeout that keeps an arbitrary user login shell from wedging the
/// scan — lives in exactly one place.
pub fn run_capturing_stdout_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Option<String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("run_capturing_stdout_with_timeout: spawn failed: {error}");
            return None;
        }
    };
    // Drain stdout on a thread that posts to a channel — then wait with a
    // bounded recv_timeout, NOT an unbounded join. Two hazards this guards:
    // (1) a child that writes more than the pipe buffer (~64 KB) before
    // exiting would block on write() while we poll try_wait; (2) a child
    // that exits but leaves a background process holding the stdout pipe
    // never yields EOF, so read_to_string — and an unbounded join() after
    // it — would block forever and defeat the timeout. We abandon the
    // (detached) reader after STDOUT_DRAIN_GRACE in that case.
    let (tx, rx) = std::sync::mpsc::channel();
    let has_reader = child
        .stdout
        .take()
        .map(|mut stdout| {
            thread::spawn(move || {
                let mut buf = String::new();
                let _ = stdout.read_to_string(&mut buf);
                let _ = tx.send(buf);
            });
        })
        .is_some();
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let stdout = if has_reader {
                    rx.recv_timeout(STDOUT_DRAIN_GRACE).unwrap_or_default()
                } else {
                    String::new()
                };
                return Some(stdout);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                // A real try_wait failure (EINTR, ECHILD, a Windows handle
                // error) is indistinguishable from a clean timeout to the
                // caller — both return None — so leave a diagnostic trail.
                eprintln!("run_capturing_stdout_with_timeout: try_wait failed: {error}");
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::AppError;
    use std::cell::RefCell;
    use std::path::PathBuf;

    /// Hand-rolled `CodexPathResolver` that records calls and returns
    /// scripted answers. Lets the shared wrappers be tested without
    /// touching the per-platform helpers (which already have their own
    /// tests in `mac::process::tests` / `win::process::tests`).
    struct FakeResolver {
        // What `resolve_with_source` returns; mutated by set/clear so
        // post-mutation `build_codex_cli_status` reflects the change.
        state: RefCell<Option<(PathBuf, RealCodexPathSource)>>,
        // What `set_user_path` returns. None → return Ok(path); Some →
        // return Err(that AppError) to test ? propagation.
        set_error: RefCell<Option<AppError>>,
        suggestions: Vec<PathBuf>,
        // What `redetect_runnable_paths` returns — the "verified
        // runnable" candidates, independent of `suggestions`. RefCell so
        // a test can vary it (empty / multiple) per case.
        runnable: RefCell<Vec<CodexCliCandidate>>,
        clear_calls: RefCell<u32>,
    }

    impl FakeResolver {
        fn new() -> Self {
            Self {
                state: RefCell::new(None),
                set_error: RefCell::new(None),
                suggestions: vec![PathBuf::from("/fake/suggested/codex")],
                runnable: RefCell::new(vec![CodexCliCandidate {
                    path: "/fake/runnable/codex".to_string(),
                    version: Some("codex-cli 1.2.3".to_string()),
                }]),
                clear_calls: RefCell::new(0),
            }
        }
    }

    impl CodexPathResolver for FakeResolver {
        fn resolve_with_source(
            &self,
            _codex_home: &Path,
        ) -> Option<(PathBuf, RealCodexPathSource)> {
            self.state.borrow().clone()
        }

        fn set_user_path(&self, _codex_home: &Path, raw_input: &str) -> AppResult<PathBuf> {
            if let Some(error) = self.set_error.borrow_mut().take() {
                return Err(error);
            }
            let path = PathBuf::from(raw_input);
            *self.state.borrow_mut() =
                Some((path.clone(), RealCodexPathSource::UserOverride));
            Ok(path)
        }

        fn clear_user_path(&self, _codex_home: &Path) {
            *self.clear_calls.borrow_mut() += 1;
            *self.state.borrow_mut() = None;
        }

        fn suggested_paths(&self, _codex_home: &Path) -> Vec<PathBuf> {
            self.suggestions.clone()
        }

        fn redetect_runnable_paths(&self, _codex_home: &Path) -> Vec<CodexCliCandidate> {
            self.runnable.borrow().clone()
        }
    }

    #[test]
    fn set_returns_post_mutation_status_with_user_override_label() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        let target = "/fake/codex/cli";

        let status =
            set_codex_cli_path(&resolver, &codex_home, target).expect("set ok");

        // Wrapper must report the *new* state, not the pre-set state.
        assert_eq!(status.resolved_path.as_deref(), Some(target));
        assert_eq!(status.source, "user_override");
        // Suggested paths still surface from the resolver.
        assert_eq!(
            status.suggested_paths,
            vec!["/fake/suggested/codex".to_string()]
        );
    }

    #[test]
    fn set_propagates_resolver_error_via_question_mark() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        *resolver.set_error.borrow_mut() = Some(AppError::new(
            "CODEX_CLI_PATH_INVALID",
            "synthetic failure",
        ));

        let err = set_codex_cli_path(&resolver, &codex_home, "/whatever")
            .expect_err("expected propagated error");
        assert_eq!(err.error_code, "CODEX_CLI_PATH_INVALID");
        assert_eq!(err.message, "synthetic failure");
    }

    #[test]
    fn clear_returns_post_mutation_status_with_none_source() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        // Seed an existing override so we can verify clear actually
        // empties it.
        set_codex_cli_path(&resolver, &codex_home, "/fake/seed/codex").unwrap();

        let status = clear_codex_cli_path(&resolver, &codex_home);

        assert_eq!(status.resolved_path, None);
        assert_eq!(status.source, "none");
        assert_eq!(*resolver.clear_calls.borrow(), 1);
    }

    #[test]
    fn get_reflects_current_resolver_state() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        // Pretend auto-discovery already found a path.
        *resolver.state.borrow_mut() = Some((
            PathBuf::from("/fake/discovered/codex"),
            RealCodexPathSource::Discovery,
        ));

        let status = get_codex_cli_status(&resolver, &codex_home);
        assert_eq!(
            status.resolved_path.as_deref(),
            Some("/fake/discovered/codex")
        );
        assert_eq!(status.source, "discovery");
    }

    #[test]
    fn redetect_returns_runnable_candidates_plus_refreshed_status() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        // Seed auto-discovery so the bundled status snapshot is non-empty.
        *resolver.state.borrow_mut() = Some((
            PathBuf::from("/fake/discovered/codex"),
            RealCodexPathSource::Discovery,
        ));

        let result = redetect_codex_cli_path(&resolver, &codex_home);

        // Candidates come straight from the resolver's runnable probe,
        // not from the suggestion list — version included.
        assert_eq!(
            result.candidates,
            vec![CodexCliCandidate {
                path: "/fake/runnable/codex".to_string(),
                version: Some("codex-cli 1.2.3".to_string()),
            }]
        );
        // The bundled status is rebuilt live, so the Settings row can
        // refresh from the same call.
        assert_eq!(
            result.status.resolved_path.as_deref(),
            Some("/fake/discovered/codex")
        );
        assert_eq!(result.status.source, "discovery");
    }

    #[test]
    fn redetect_with_no_runnable_candidates_returns_empty_plus_status() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        // Nothing survives the runnable probe...
        *resolver.runnable.borrow_mut() = vec![];
        // ...but auto-discovery still resolves a (stale) path, so the
        // bundled status must stay populated for the Settings row.
        *resolver.state.borrow_mut() = Some((
            PathBuf::from("/fake/stale/codex"),
            RealCodexPathSource::Discovery,
        ));

        let result = redetect_codex_cli_path(&resolver, &codex_home);

        assert!(result.candidates.is_empty());
        assert_eq!(
            result.status.resolved_path.as_deref(),
            Some("/fake/stale/codex")
        );
    }

    #[test]
    fn redetect_preserves_multiple_candidates_in_order() {
        let resolver = FakeResolver::new();
        let codex_home = PathBuf::from("/fake/home");
        *resolver.runnable.borrow_mut() = vec![
            CodexCliCandidate {
                path: "/fake/first/codex".to_string(),
                version: Some("codex-cli 0.133.0".to_string()),
            },
            CodexCliCandidate {
                path: "/fake/second/codex".to_string(),
                version: None,
            },
        ];

        let result = redetect_codex_cli_path(&resolver, &codex_home);

        // Order is the contract the front-end relies on (it prefills [0]).
        let paths: Vec<&str> = result.candidates.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, vec!["/fake/first/codex", "/fake/second/codex"]);
        // Version rides along per candidate (one known, one unparsed).
        assert_eq!(
            result.candidates[0].version.as_deref(),
            Some("codex-cli 0.133.0")
        );
        assert_eq!(result.candidates[1].version, None);
    }

    #[cfg(unix)]
    #[test]
    fn probe_version_with_timeout_captures_version_on_success() {
        // Absolute path + shell builtins so a sibling test that mutates
        // PATH (the mac/win `discover_*` tests do) can't make these spawns
        // fail with NotFound. `echo` / `exit` are builtins — no PATH lookup.
        let mut ok = Command::new("/bin/sh");
        ok.args(["-c", "echo codex-cli 9.9.9"]);
        assert_eq!(
            probe_version_with_timeout(ok, Duration::from_secs(5)).as_deref(),
            Some("codex-cli 9.9.9")
        );

        // A clean non-zero exit → None (never a captured string): redetect
        // relies on this to reject "ran but failed" candidates.
        let mut bad = Command::new("/bin/sh");
        bad.args(["-c", "exit 3"]);
        assert_eq!(probe_version_with_timeout(bad, Duration::from_secs(5)), None);
    }

    #[cfg(unix)]
    #[test]
    fn probe_version_with_timeout_kills_and_returns_none_on_overrun() {
        let start = Instant::now();
        // Absolute path (present on macOS + Ubuntu) so a PATH-mutating
        // sibling test can't break the spawn.
        let mut sleeper = Command::new("/bin/sleep");
        sleeper.arg("30");
        // 200ms budget against a 30s sleep: it must be killed and reported
        // as None, and we must not actually block anywhere near 30s.
        assert_eq!(
            probe_version_with_timeout(sleeper, Duration::from_millis(200)),
            None
        );
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn run_capturing_does_not_deadlock_on_large_stdout() {
        // Emit well over the ~64 KB pipe buffer before exiting. Without the
        // reader thread the child would block on write() and we'd hit the
        // timeout; with it, this drains and returns well under the budget.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "yes 0123456789abcdef | head -n 20000; echo done"]);
        let start = Instant::now();
        let out = run_capturing_stdout_with_timeout(command, Duration::from_secs(5));
        assert!(start.elapsed() < Duration::from_secs(4));
        assert!(out.expect("exit 0 with captured stdout").contains("done"));
    }
}
