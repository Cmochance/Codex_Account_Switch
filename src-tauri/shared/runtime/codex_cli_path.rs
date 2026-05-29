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

use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::errors::AppResult;
use crate::models::{CodexCliRedetectResult, CodexCliStatus};

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
    /// returns only candidates verified runnable via `codex --version`
    /// (deduped, best-first). Backs the Settings "auto-detect" button:
    /// `resolve_with_source` trusts a previously-saved path, so when
    /// that path is wrong the user needs this to rescan from scratch.
    fn redetect_runnable_paths(&self, codex_home: &Path) -> Vec<PathBuf>;
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
    let candidates = resolver
        .redetect_runnable_paths(codex_home)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    CodexCliRedetectResult {
        candidates,
        status: build_codex_cli_status(resolver, codex_home),
    }
}

/// Poll-wait for a child process, killing it if it outlives `timeout`.
/// Shared by the per-platform `codex --version` runnable probes so a
/// hung or input-waiting candidate can't wedge the re-detection scan.
/// The platforms own `Command` construction (console hiding, Windows
/// extension resolution); only this bounded wait is common.
pub fn wait_child_with_timeout(mut child: Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
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
                // caller — both return None — so leave a diagnostic trail
                // instead of silently demoting the candidate.
                eprintln!("wait_child_with_timeout: try_wait failed: {error}");
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
        // runnable" subset, independent of `suggestions`. RefCell so a
        // test can vary it (empty / multiple) per case.
        runnable: RefCell<Vec<PathBuf>>,
        clear_calls: RefCell<u32>,
    }

    impl FakeResolver {
        fn new() -> Self {
            Self {
                state: RefCell::new(None),
                set_error: RefCell::new(None),
                suggestions: vec![PathBuf::from("/fake/suggested/codex")],
                runnable: RefCell::new(vec![PathBuf::from("/fake/runnable/codex")]),
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

        fn redetect_runnable_paths(&self, _codex_home: &Path) -> Vec<PathBuf> {
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
        // not from the suggestion list.
        assert_eq!(result.candidates, vec!["/fake/runnable/codex".to_string()]);
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
            PathBuf::from("/fake/first/codex"),
            PathBuf::from("/fake/second/codex"),
        ];

        let result = redetect_codex_cli_path(&resolver, &codex_home);

        // Order is the contract the front-end relies on (it prefills [0]).
        assert_eq!(
            result.candidates,
            vec![
                "/fake/first/codex".to_string(),
                "/fake/second/codex".to_string(),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn wait_child_with_timeout_returns_status_for_fast_exit() {
        use std::process::Command;

        // Absolute path + shell builtin so a sibling test that mutates
        // PATH (the mac/win `discover_*` tests do) can't make these spawns
        // fail with NotFound. `exit N` is a builtin, so no PATH lookup.
        let zero = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn /bin/sh");
        assert!(wait_child_with_timeout(zero, Duration::from_secs(5))
            .is_some_and(|status| status.success()));

        // A clean non-zero exit must be observed as Some(!success), never
        // collapsed into the timeout/error None — `probe_codex_runnable`
        // relies on telling "ran but failed" from "didn't run".
        let nonzero = Command::new("/bin/sh")
            .args(["-c", "exit 3"])
            .spawn()
            .expect("spawn /bin/sh");
        assert!(wait_child_with_timeout(nonzero, Duration::from_secs(5))
            .is_some_and(|status| !status.success()));
    }

    #[cfg(unix)]
    #[test]
    fn wait_child_with_timeout_kills_and_returns_none_on_overrun() {
        use std::process::Command;

        let start = Instant::now();
        // Absolute path (present on macOS + Ubuntu) so a PATH-mutating
        // sibling test can't break the spawn.
        let sleeper = Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn /bin/sleep");
        // 200ms budget against a 30s sleep: it must be killed and reported
        // as None, and we must not actually block anywhere near 30s.
        assert!(wait_child_with_timeout(sleeper, Duration::from_millis(200)).is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
