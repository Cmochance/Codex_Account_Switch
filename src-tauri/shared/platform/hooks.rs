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
    fn sync_root_openai_base_url_for_profile(
        &self,
        profile_name: &str,
        codex_home: Option<&Path>,
    ) -> AppResult<()> {
        crate::shared::config::sync_root_openai_base_url_for_profile(profile_name, codex_home)
    }
    fn sync_root_openai_base_url_for_current_profile(
        &self,
        codex_home: Option<&Path>,
    ) -> AppResult<()> {
        let codex_home = codex_home
            .map(Path::to_path_buf)
            .unwrap_or_else(crate::shared::paths::get_codex_home);
        let backup_root = crate::shared::paths::get_backup_root(Some(&codex_home));
        let Some(current_profile) = crate::shared::profiles::resolve_current_profile(&backup_root)
        else {
            return Ok(());
        };

        self.sync_root_openai_base_url_for_profile(&current_profile, Some(&codex_home))
    }
    fn sync_on_window_close(&self) -> AppResult<()>;
}
