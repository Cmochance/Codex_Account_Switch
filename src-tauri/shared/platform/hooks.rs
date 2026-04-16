use std::path::Path;

use crate::errors::AppResult;

pub trait PlatformHooks: Send + Sync {
    fn open_or_activate_codex_app(&self, codex_home: Option<&Path>) -> AppResult<String>;
    fn quit_codex_app_if_running(&self) -> AppResult<bool>;
    fn reopen_codex_app_if_needed(
        &self,
        app_was_running: bool,
        codex_home: Option<&Path>,
    ) -> Vec<String>;
    fn run_codex_login(&self, codex_home: &Path) -> AppResult<()>;
    fn run_codex_auth_refresh(
        &self,
        cli_codex_home: &Path,
        runtime_codex_home: &Path,
    ) -> AppResult<()>;
    fn sync_on_window_close(&self) -> AppResult<()>;
}
