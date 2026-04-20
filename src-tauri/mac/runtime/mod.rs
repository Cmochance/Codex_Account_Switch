#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

pub mod bootstrap;
pub mod cli_shim;
pub mod install;
pub mod process;
pub mod profile_actions;
pub mod refresh_runtime;
pub mod switch;
pub mod windowing;

pub use crate::shared::{paths, profiles, profiles_index};

pub mod actions {
    pub use super::profile_actions::{
        add_profile, clear_profile_account, delete_profile, login_current_profile, open_codex_app,
        open_contact, open_profile_folder, open_releases, open_xiaohongshu, rename_profile,
        update_profile_base_url,
    };
    pub use super::refresh_runtime::refresh_profile;
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
