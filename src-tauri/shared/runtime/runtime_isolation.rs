//! Shared CODEX_HOME isolation primitives.
//!
//! The app spawns the real `codex` binary in two situations that need a
//! sandboxed CODEX_HOME instead of the user's live `~/.codex`:
//!
//!   * **Refresh** (`refresh_runtime`) — runs `codex exec` to provoke a
//!     session-file write so we can sample the latest rate limits without
//!     mutating the user's session history or AGENTS.md / skills config.
//!   * **Login** (`login_runtime`) — runs `codex login` for an arbitrary
//!     profile (not necessarily the active one) so the OAuth flow writes
//!     to a sandboxed `auth.json` we can copy into the target profile
//!     folder.
//!
//! Both flows want the *same* baseline: copy the small bookkeeping files
//! that let codex skip first-run setup (models cache, version pin,
//! global state, cap session id), avoid leaking AGENTS / skills / rules,
//! and never carry over the user's live `auth.json`. This module owns
//! that shared list so we don't drift between mac and win, or between
//! refresh and login.

use std::path::Path;

use crate::errors::{AppError, AppResult};

use super::fs_ops::{copy_entry, remove_path};

/// Files copied verbatim from the user's `~/.codex` into a sandboxed
/// runtime CODEX_HOME so codex CLI doesn't redo first-run setup work.
pub const RUNTIME_SHARED_FILES: [&str; 4] = [
    "models_cache.json",
    "version.json",
    ".codex-global-state.json",
    "cap_sid",
];

/// Directories lazily seeded (only when missing in the runtime home).
/// These are large and codex re-uses them safely across CODEX_HOMEs.
pub const RUNTIME_SHARED_DIRS: [&str; 3] = ["plugins", "cache", "sqlite"];

/// Files explicitly removed from the runtime home before each run so we
/// don't leak AGENTS / skills overrides into a sandboxed CLI invocation.
pub const RUNTIME_REMOVED_FILES: [&str; 1] = ["AGENTS.md"];

/// Directories explicitly removed from the runtime home for the same
/// reason as `RUNTIME_REMOVED_FILES`.
pub const RUNTIME_REMOVED_DIRS: [&str; 4] = ["rules", "skills", "vendor_imports", "memories"];

/// `auth.json` is intentionally NOT in the shared list — refresh overlays
/// the per-profile copy, login expects codex to write a fresh one. Keep
/// this constant for callers that need to know the auth file name.
pub const RUNTIME_AUTH_FILENAME: &str = "auth.json";

/// `profile.json` mirrors `auth.json`: refresh overlays the profile's
/// copy so codex can read existing session metadata; login starts blank.
pub const RUNTIME_PROFILE_METADATA_FILENAME: &str = "profile.json";

/// Create `runtime_home` (and parents) if missing. Codex CLI itself will
/// `mkdir -p` again, but we want any FS error to surface here with a
/// project-specific code instead of buried inside a child process.
fn ensure_runtime_home(runtime_home: &Path) -> AppResult<()> {
    std::fs::create_dir_all(runtime_home).map_err(|error| {
        AppError::new(
            "RUNTIME_HOME_CREATE_FAILED",
            format!(
                "Failed to create runtime CODEX_HOME {}: {error}",
                runtime_home.display()
            ),
        )
    })
}

/// Seed the shared bookkeeping files / dirs from the user's live
/// CODEX_HOME into a sandboxed runtime home. Idempotent: re-running
/// against an already-seeded runtime updates the shared files to
/// whatever is currently in `codex_home` and lazily fills any missing
/// shared dirs.
pub fn seed_runtime_shared_assets(codex_home: &Path, runtime_home: &Path) -> AppResult<()> {
    ensure_runtime_home(runtime_home)?;

    for entry_name in RUNTIME_SHARED_FILES {
        let src = codex_home.join(entry_name);
        let dst = runtime_home.join(entry_name);
        if src.exists() {
            copy_entry(&src, &dst)?;
        } else {
            remove_path(&dst)?;
        }
    }

    for entry_name in RUNTIME_SHARED_DIRS {
        let src = codex_home.join(entry_name);
        let dst = runtime_home.join(entry_name);
        if src.exists() && !dst.exists() {
            copy_entry(&src, &dst)?;
        }
    }

    Ok(())
}

/// Strip out AGENTS / skills / rules / vendor / memories so a sandboxed
/// codex run behaves like a clean install regardless of what the user
/// has under `~/.codex`. Without this, e.g. an AGENTS.md that hijacks
/// behavior could distort a refresh / login flow.
pub fn prune_runtime_extra_features(runtime_home: &Path) -> AppResult<()> {
    for entry_name in RUNTIME_REMOVED_FILES {
        remove_path(&runtime_home.join(entry_name))?;
    }

    for entry_name in RUNTIME_REMOVED_DIRS {
        remove_path(&runtime_home.join(entry_name))?;
    }

    Ok(())
}

/// Clear any leftover auth state from a previous sandboxed run. Used by
/// the login flow before invoking `codex login` so the OAuth handler
/// writes into a guaranteed-empty `auth.json` instead of merging on top
/// of stale tokens from an aborted prior attempt.
pub fn clear_runtime_auth_state(runtime_home: &Path) -> AppResult<()> {
    remove_path(&runtime_home.join(RUNTIME_AUTH_FILENAME))?;
    remove_path(&runtime_home.join(RUNTIME_PROFILE_METADATA_FILENAME))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("codex-switch-runtime-isolation-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn seed_copies_shared_files_and_dirs_lazily() {
        let codex_home = temp_root("seed-copy-src");
        let runtime_home = temp_root("seed-copy-dst");
        write(&codex_home.join("models_cache.json"), "{}");
        write(&codex_home.join("version.json"), "{}");
        write(&codex_home.join("plugins/foo.js"), "// plugin");

        seed_runtime_shared_assets(&codex_home, &runtime_home).unwrap();

        assert!(runtime_home.join("models_cache.json").is_file());
        assert!(runtime_home.join("version.json").is_file());
        assert!(runtime_home.join("plugins/foo.js").is_file());
    }

    #[test]
    fn seed_removes_shared_file_in_runtime_when_absent_in_codex_home() {
        let codex_home = temp_root("seed-remove-src");
        let runtime_home = temp_root("seed-remove-dst");
        write(&runtime_home.join("models_cache.json"), "stale");

        seed_runtime_shared_assets(&codex_home, &runtime_home).unwrap();

        assert!(!runtime_home.join("models_cache.json").exists());
    }

    #[test]
    fn prune_removes_extras_even_when_already_absent() {
        let runtime_home = temp_root("prune");
        write(&runtime_home.join("AGENTS.md"), "# stale");
        write(&runtime_home.join("skills/test.toml"), "[task]");

        prune_runtime_extra_features(&runtime_home).unwrap();

        assert!(!runtime_home.join("AGENTS.md").exists());
        assert!(!runtime_home.join("skills").exists());

        // Idempotent.
        prune_runtime_extra_features(&runtime_home).unwrap();
    }

    #[test]
    fn clear_auth_state_removes_auth_and_profile_only() {
        let runtime_home = temp_root("clear-auth");
        write(&runtime_home.join("auth.json"), "{}");
        write(&runtime_home.join("profile.json"), "{}");
        write(&runtime_home.join("models_cache.json"), "{}");

        clear_runtime_auth_state(&runtime_home).unwrap();

        assert!(!runtime_home.join("auth.json").exists());
        assert!(!runtime_home.join("profile.json").exists());
        // Shared bookkeeping must be left intact.
        assert!(runtime_home.join("models_cache.json").is_file());
    }
}
