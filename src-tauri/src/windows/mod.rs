pub mod actions;
pub mod bootstrap;
pub mod fs_ops;
pub mod metadata;
pub mod paths;
pub mod process;
pub mod profiles;
pub mod profiles_index;
pub mod session_usage;
pub mod switch;

#[cfg(test)]
pub(crate) fn env_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}
