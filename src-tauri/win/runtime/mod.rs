#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]

pub mod bootstrap;
pub mod install;
pub mod process;
pub mod profile_actions;
pub mod profiles_index;
pub mod refresh_runtime;
pub mod switch;

pub use crate::shared::{
    config, fs_ops, login_runtime, metadata, paths, profiles, runtime_isolation, session_files,
    session_usage,
};

pub mod actions {
    pub use super::profile_actions::{
        add_profile, clear_profile_account, delete_profile, login_current_profile, login_profile,
        open_codex_app, open_contact, open_profile_folder, open_releases, open_url,
        open_xiaohongshu, rename_profile, update_profile_base_url,
    };
    pub use super::refresh_runtime::refresh_profile;
}

/// Exposed so the shared `codex_cli_path` Tauri command site can grab
/// the Windows resolver without depending on the platform module
/// directly.
pub fn codex_cli_resolver() -> &'static dyn crate::shared::codex_cli_path::CodexPathResolver {
    &process::WINDOWS_CODEX_PATH_RESOLVER
}

#[cfg(test)]
pub(crate) fn env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub(crate) fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    env_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
