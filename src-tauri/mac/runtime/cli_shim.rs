use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::shared::paths::{
    get_backup_root, INSTALL_STATE_FILENAME, LOGIN_RUNTIME_DIRNAME, REFRESH_RUNTIME_DIRNAME,
};

pub const MACOS_RUNTIME_DIRNAME: &str = "macos";
pub const CLI_RUNTIME_FILENAME: &str = "codex_switch_cli";
pub const REAL_CODEX_RESOLVER_FILENAME: &str = "resolve-real-codex.sh";
const REAL_CODEX_RESOLVER_TEMPLATE: &str =
    include_str!("../../../macOS-backup/resolve-real-codex.sh");

pub fn get_runtime_dir(codex_home: &Path) -> PathBuf {
    get_backup_root(Some(codex_home)).join(MACOS_RUNTIME_DIRNAME)
}

pub fn get_refresh_runtime_dir(codex_home: &Path) -> PathBuf {
    get_runtime_dir(codex_home).join(REFRESH_RUNTIME_DIRNAME)
}

pub fn get_login_runtime_dir(codex_home: &Path) -> PathBuf {
    get_runtime_dir(codex_home).join(LOGIN_RUNTIME_DIRNAME)
}

pub fn get_install_state_file(codex_home: &Path) -> PathBuf {
    get_runtime_dir(codex_home).join(INSTALL_STATE_FILENAME)
}

pub fn runtime_cli_path(codex_home: &Path) -> PathBuf {
    get_runtime_dir(codex_home).join(CLI_RUNTIME_FILENAME)
}

pub fn real_codex_resolver_path(codex_home: &Path) -> PathBuf {
    get_runtime_dir(codex_home).join(REAL_CODEX_RESOLVER_FILENAME)
}

pub fn managed_shim_path(codex_home: &Path) -> PathBuf {
    codex_home.join("bin").join("codex")
}

pub fn write_real_codex_resolver(codex_home: &Path) -> AppResult<PathBuf> {
    let resolver_path = real_codex_resolver_path(codex_home);
    if let Some(parent) = resolver_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create resolver directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    fs::write(&resolver_path, REAL_CODEX_RESOLVER_TEMPLATE).map_err(|error| {
        AppError::new(
            "RESOLVER_WRITE_FAILED",
            format!(
                "Failed to write Codex resolver {}: {error}",
                resolver_path.display()
            ),
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&resolver_path)
            .map_err(|error| {
                AppError::new(
                    "RESOLVER_WRITE_FAILED",
                    format!(
                        "Failed to read resolver permissions {}: {error}",
                        resolver_path.display()
                    ),
                )
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&resolver_path, permissions).map_err(|error| {
            AppError::new(
                "RESOLVER_WRITE_FAILED",
                format!(
                    "Failed to update resolver permissions {}: {error}",
                    resolver_path.display()
                ),
            )
        })?;
    }

    Ok(resolver_path)
}

pub fn write_codex_shim(codex_home: &Path) -> AppResult<PathBuf> {
    let shim_path = managed_shim_path(codex_home);
    let runtime_cli = runtime_cli_path(codex_home);
    if let Some(parent) = shim_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create shim directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    let shim_contents = format!(
        concat!(
            "#!/usr/bin/env bash\n",
            "set -euo pipefail\n",
            "export CODEX_HOME=\"${{CODEX_HOME:-$HOME/.codex}}\"\n",
            "\"{}\" shim \"$@\"\n"
        ),
        runtime_cli.display()
    );
    fs::write(&shim_path, shim_contents).map_err(|error| {
        AppError::new(
            "SHIM_WRITE_FAILED",
            format!(
                "Failed to write command shim {}: {error}",
                shim_path.display()
            ),
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&shim_path)
            .map_err(|error| {
                AppError::new(
                    "SHIM_WRITE_FAILED",
                    format!(
                        "Failed to read shim permissions {}: {error}",
                        shim_path.display()
                    ),
                )
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shim_path, permissions).map_err(|error| {
            AppError::new(
                "SHIM_WRITE_FAILED",
                format!(
                    "Failed to update shim permissions {}: {error}",
                    shim_path.display()
                ),
            )
        })?;
    }

    Ok(shim_path)
}
