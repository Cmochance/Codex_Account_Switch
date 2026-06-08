use std::path::{Path, PathBuf};

use crate::errors::AppResult;
use crate::shared::paths::{get_backup_root, get_codex_home};
use crate::shared::profiles::resolve_current_profile;

use super::cli_shim::get_refresh_runtime_dir;

const REFRESH_RUNTIME_DEFAULT_CONFIG: &str = concat!(
    "model = \"gpt-5.4-mini\"\n",
    "model_provider = \"openai-custom\"\n",
    "\n",
    "[model_providers.\"openai-custom\"]\n",
    "name = \"OpenAI Custom\"\n",
    "base_url = \"https://chatgpt.com/backend-api/codex\"\n",
    "wire_api = \"responses\"\n",
    "requires_openai_auth = true\n",
    "supports_websockets = false\n",
);

pub fn sync_root_state_to_current_profile(codex_home: Option<&Path>) -> AppResult<Option<String>> {
    // Identity-checked write-back lives in the shared layer so macOS and
    // Windows can't drift apart. See
    // `switch_core::sync_root_state_to_current_profile_with_home`.
    crate::shared::switch_core::sync_root_state_to_current_profile_with_home(codex_home)
}

pub fn ensure_backup_initialized(codex_home: Option<&Path>) -> AppResult<bool> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    if backup_root.is_dir() {
        super::install::refresh_install_state(&codex_home)?;
        return Ok(false);
    }

    std::fs::create_dir_all(&backup_root).map_err(|error| {
        crate::errors::AppError::new(
            "FS_CREATE_FAILED",
            format!(
                "Failed to create backup root {}: {error}",
                backup_root.display()
            ),
        )
    })?;
    super::install::ensure_default_profiles(&backup_root)?;
    super::install::ensure_placeholder_auth_files(&backup_root)?;

    let seeded_auth = super::install::seed_default_profile(&codex_home, &backup_root)?;
    if seeded_auth && resolve_current_profile(&backup_root).is_none() {
        super::install::initialize_default_active_profile(&backup_root)?;
    }

    super::install::refresh_install_state(&codex_home)?;
    crate::shared::profiles_index::load_profiles_index(Some(&codex_home))?;
    Ok(true)
}

pub fn ensure_refresh_runtime_config_initialized(codex_home: Option<&Path>) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let runtime_home = get_refresh_runtime_dir(&codex_home);
    std::fs::create_dir_all(&runtime_home).map_err(|error| {
        crate::errors::AppError::new(
            "FS_CREATE_FAILED",
            format!(
                "Failed to create refresh runtime directory {}: {error}",
                runtime_home.display()
            ),
        )
    })?;

    let config_path = runtime_home.join("config.toml");
    std::fs::write(&config_path, REFRESH_RUNTIME_DEFAULT_CONFIG).map_err(|error| {
        crate::errors::AppError::new(
            "FS_WRITE_FAILED",
            format!(
                "Failed to write refresh runtime config {}: {error}",
                config_path.display()
            ),
        )
    })
}
