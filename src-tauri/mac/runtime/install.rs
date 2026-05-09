use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};
use crate::shared::fs_ops::remove_path;
use crate::shared::paths::{
    get_backup_root, get_codex_home, utc_timestamp, ACTIVE_MARKER_FILE, CURRENT_PROFILE_FILENAME,
    DEFAULT_PROFILES,
};

use super::cli_shim::{
    get_install_state_file, get_runtime_dir, managed_shim_path, real_codex_resolver_path,
    runtime_cli_path, write_codex_shim, write_real_codex_resolver,
};
use super::process::{
    discover_real_codex_cli_path, load_install_state, save_install_state, InstallState,
};
use crate::shared::profiles::resolve_current_profile;

const AUTH_TEMPLATE: &str = include_str!("../../../examples/account_backup/demo/auth.json.example");

#[allow(dead_code)]
pub struct InstallSummary {
    pub seeded_auth: bool,
    pub placeholder_auth_files: Vec<PathBuf>,
    pub initialized_default_profile: bool,
    pub runtime_cli_path: PathBuf,
    pub managed_shim_path: PathBuf,
    pub path_added_by_installer: bool,
    pub path_changed: bool,
    pub real_codex_path: PathBuf,
}

pub struct UninstallSummary {
    pub removed_shim: bool,
    pub removed_install_state: bool,
    pub removed_runtime_cli: bool,
    pub removed_path_entry: bool,
    pub managed_shim_path: PathBuf,
    pub install_state_path: PathBuf,
    pub runtime_cli_path: PathBuf,
}

fn resolve_real_codex_path(codex_home: &Path) -> AppResult<PathBuf> {
    let shim_path = managed_shim_path(codex_home);
    let state = load_install_state(Some(codex_home));
    if let Some(existing) = state.real_codex_path.as_deref() {
        let path = PathBuf::from(existing);
        if path.is_file() && path != shim_path {
            return Ok(path);
        }
    }

    discover_real_codex_cli_path(Some(&shim_path)).ok_or_else(|| {
        AppError::new(
            "REAL_CODEX_NOT_FOUND",
            "Unable to resolve the real Codex CLI. Make sure `codex` is installed first.",
        )
    })
}

fn has_initialized_active_profile(backup_root: &Path) -> bool {
    resolve_current_profile(backup_root).is_some()
}

pub(super) fn initialize_default_active_profile(backup_root: &Path) -> AppResult<()> {
    let current_profile_file = backup_root.join(CURRENT_PROFILE_FILENAME);
    fs::write(&current_profile_file, "a\n").map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!(
                "Failed to write current profile marker {}: {error}",
                current_profile_file.display()
            ),
        )
    })?;

    let marker_path = backup_root.join("a").join(ACTIVE_MARKER_FILE);
    fs::write(&marker_path, format!("activated_at={}\n", utc_timestamp())).map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!(
                "Failed to write active marker {}: {error}",
                marker_path.display()
            ),
        )
    })
}

pub(super) fn ensure_default_profiles(backup_root: &Path) -> AppResult<()> {
    for profile in DEFAULT_PROFILES {
        let profile_dir = backup_root.join(profile);
        fs::create_dir_all(&profile_dir).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create profile directory {}: {error}",
                    profile_dir.display()
                ),
            )
        })?;
    }

    Ok(())
}

pub(super) fn ensure_placeholder_auth_files(backup_root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut created_files = Vec::new();
    for profile in DEFAULT_PROFILES {
        let auth_file = backup_root.join(profile).join("auth.json");
        if auth_file.is_file() {
            continue;
        }

        fs::write(&auth_file, AUTH_TEMPLATE).map_err(|error| {
            AppError::new(
                "AUTH_TEMPLATE_WRITE_FAILED",
                format!(
                    "Failed to write placeholder auth {}: {error}",
                    auth_file.display()
                ),
            )
        })?;
        created_files.push(auth_file);
    }

    Ok(created_files)
}

pub(super) fn seed_default_profile(codex_home: &Path, backup_root: &Path) -> AppResult<bool> {
    let root_auth_file = codex_home.join("auth.json");
    if !root_auth_file.is_file() {
        return Ok(false);
    }

    let default_profile_auth_file = backup_root.join("a").join("auth.json");
    fs::copy(&root_auth_file, &default_profile_auth_file).map_err(|error| {
        AppError::new(
            "FS_COPY_FAILED",
            format!(
                "Failed to seed default profile auth {} -> {}: {error}",
                root_auth_file.display(),
                default_profile_auth_file.display()
            ),
        )
    })?;

    Ok(true)
}

fn copy_runtime_cli(source_cli_path: &Path, target_cli_path: &Path) -> AppResult<()> {
    if let Some(parent) = target_cli_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create runtime directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    if source_cli_path != target_cli_path {
        fs::copy(source_cli_path, target_cli_path).map_err(|error| {
            AppError::new(
                "FS_COPY_FAILED",
                format!(
                    "Failed to copy CLI {} -> {}: {error}",
                    source_cli_path.display(),
                    target_cli_path.display()
                ),
            )
        })?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(target_cli_path)
            .map_err(|error| {
                AppError::new(
                    "FS_COPY_FAILED",
                    format!(
                        "Failed to read runtime CLI permissions {}: {error}",
                        target_cli_path.display()
                    ),
                )
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(target_cli_path, permissions).map_err(|error| {
            AppError::new(
                "FS_COPY_FAILED",
                format!(
                    "Failed to update runtime CLI permissions {}: {error}",
                    target_cli_path.display()
                ),
            )
        })?;
    }

    Ok(())
}

pub fn refresh_install_state(codex_home: &Path) -> AppResult<()> {
    write_real_codex_resolver(codex_home)?;
    let managed_shim_path = managed_shim_path(codex_home);
    let previous_state = load_install_state(Some(codex_home));
    let real_codex_path = previous_state
        .real_codex_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file() && path != &managed_shim_path)
        .or_else(|| discover_real_codex_cli_path(Some(&managed_shim_path)))
        .map(|path| path.to_string_lossy().into_owned());

    let state = InstallState {
        real_codex_path,
        path_added_by_installer: previous_state.path_added_by_installer,
        user_codex_path: previous_state.user_codex_path,
    };
    save_install_state(Some(codex_home), &state);
    Ok(())
}

pub fn install_from(
    source_cli_path: &Path,
    codex_home: Option<&Path>,
) -> AppResult<InstallSummary> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let runtime_dir = get_runtime_dir(&codex_home);
    let managed_shim_path = managed_shim_path(&codex_home);
    let runtime_cli_path = runtime_cli_path(&codex_home);

    fs::create_dir_all(&backup_root).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!(
                "Failed to create backup root {}: {error}",
                backup_root.display()
            ),
        )
    })?;

    ensure_default_profiles(&backup_root)?;
    let placeholder_auth_files = ensure_placeholder_auth_files(&backup_root)?;
    let seeded_auth = seed_default_profile(&codex_home, &backup_root)?;
    let mut initialized_default_profile = false;
    if seeded_auth && !has_initialized_active_profile(&backup_root) {
        initialize_default_active_profile(&backup_root)?;
        initialized_default_profile = true;
    }

    fs::create_dir_all(&runtime_dir).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!(
                "Failed to create runtime directory {}: {error}",
                runtime_dir.display()
            ),
        )
    })?;
    write_real_codex_resolver(&codex_home)?;
    let real_codex_path = resolve_real_codex_path(&codex_home)?;
    copy_runtime_cli(source_cli_path, &runtime_cli_path)?;
    write_codex_shim(&codex_home)?;

    let previous_state = load_install_state(Some(&codex_home));
    let state = InstallState {
        real_codex_path: Some(real_codex_path.to_string_lossy().into_owned()),
        path_added_by_installer: false,
        user_codex_path: previous_state.user_codex_path,
    };
    save_install_state(Some(&codex_home), &state);

    Ok(InstallSummary {
        seeded_auth,
        placeholder_auth_files,
        initialized_default_profile,
        runtime_cli_path,
        managed_shim_path,
        path_added_by_installer: false,
        path_changed: false,
        real_codex_path,
    })
}

pub fn install_current_exe(codex_home: Option<&Path>) -> AppResult<InstallSummary> {
    let source_cli_path = std::env::current_exe().map_err(|error| {
        AppError::new(
            "CLI_PATH_UNAVAILABLE",
            format!("Failed to resolve current CLI path: {error}"),
        )
    })?;

    install_from(&source_cli_path, codex_home)
}

pub fn uninstall(remove_script: bool, codex_home: Option<&Path>) -> AppResult<UninstallSummary> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let runtime_dir = get_runtime_dir(&codex_home);
    let install_state_path = get_install_state_file(&codex_home);
    let managed_shim_path = managed_shim_path(&codex_home);
    let runtime_cli_path = runtime_cli_path(&codex_home);
    let resolver_path = real_codex_resolver_path(&codex_home);

    let removed_shim = if managed_shim_path.exists() {
        remove_path(&managed_shim_path)?;
        true
    } else {
        false
    };

    let removed_install_state = if install_state_path.exists() {
        remove_path(&install_state_path)?;
        true
    } else {
        false
    };

    let removed_runtime_cli = if remove_script && runtime_cli_path.exists() {
        remove_path(&runtime_cli_path)?;
        true
    } else {
        false
    };

    if remove_script && resolver_path.exists() {
        remove_path(&resolver_path)?;
    }

    if remove_script
        && runtime_dir.exists()
        && fs::read_dir(&runtime_dir)
            .ok()
            .is_some_and(|mut entries| entries.next().is_none())
    {
        let _ = fs::remove_dir(&runtime_dir);
    }

    Ok(UninstallSummary {
        removed_shim,
        removed_install_state,
        removed_runtime_cli,
        removed_path_entry: false,
        managed_shim_path,
        install_state_path,
        runtime_cli_path,
    })
}
