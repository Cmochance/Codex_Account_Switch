use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

use crate::errors::{AppError, AppResult};

use super::dashboard::resolve_current_profile;
use super::fs_ops::backup_root_state_to_profile;
use super::paths::{
    get_backup_root, get_codex_home, get_runtime_dir, ACTIVE_MARKER_FILE, CURRENT_PROFILE_FILENAME,
    DEFAULT_PROFILES,
};
use super::process::discover_real_codex_cli_path;

const AUTH_TEMPLATE: &str = include_str!("../../../examples/account_backup/demo/auth.json.example");

fn resolve_real_codex_cli(codex_home: &Path) -> Option<PathBuf> {
    let managed_shim_path = codex_home.join("bin").join("codex.cmd");
    discover_real_codex_cli_path(Some(&managed_shim_path))
}

fn write_install_state(codex_home: &Path) -> AppResult<()> {
    let runtime_dir = get_runtime_dir(Some(codex_home));
    fs::create_dir_all(&runtime_dir).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!(
                "Failed to create runtime directory {}: {error}",
                runtime_dir.display()
            ),
        )
    })?;

    let state_path = runtime_dir.join("install_state.json");
    let payload = json!({
        "real_codex_path": resolve_real_codex_cli(codex_home)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    });

    let serialized = serde_json::to_string_pretty(&payload).map_err(|error| {
        AppError::new(
            "INSTALL_STATE_INVALID",
            format!("Failed to serialize install state: {error}"),
        )
    })?;

    fs::write(&state_path, format!("{serialized}\n")).map_err(|error| {
        AppError::new(
            "INSTALL_STATE_WRITE_FAILED",
            format!(
                "Failed to write install state {}: {error}",
                state_path.display()
            ),
        )
    })
}

fn initialize_default_active_profile(backup_root: &Path) -> AppResult<()> {
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
    fs::write(
        &marker_path,
        format!("activated_at={}\n", super::paths::utc_timestamp()),
    )
    .map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!(
                "Failed to write active marker {}: {error}",
                marker_path.display()
            ),
        )
    })
}

pub fn sync_root_state_to_current_profile(codex_home: Option<&Path>) -> AppResult<Option<String>> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let Some(current_profile) = resolve_current_profile(&backup_root) else {
        return Ok(None);
    };

    backup_root_state_to_profile(&current_profile, &codex_home, &backup_root)?;
    super::profiles_index::load_profiles_index(Some(&codex_home))?;
    Ok(Some(current_profile))
}

pub fn ensure_backup_initialized(codex_home: Option<&Path>) -> AppResult<bool> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    if backup_root.is_dir() {
        return Ok(false);
    }

    fs::create_dir_all(&backup_root).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!(
                "Failed to create backup root {}: {error}",
                backup_root.display()
            ),
        )
    })?;

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

    for profile in DEFAULT_PROFILES {
        let auth_path = backup_root.join(profile).join("auth.json");
        if auth_path.exists() {
            continue;
        }
        fs::write(&auth_path, AUTH_TEMPLATE).map_err(|error| {
            AppError::new(
                "AUTH_TEMPLATE_WRITE_FAILED",
                format!(
                    "Failed to write placeholder auth {}: {error}",
                    auth_path.display()
                ),
            )
        })?;
    }

    let root_auth_path = codex_home.join("auth.json");
    if root_auth_path.is_file() {
        let target_auth = backup_root.join("a").join("auth.json");
        fs::copy(&root_auth_path, &target_auth).map_err(|error| {
            AppError::new(
                "FS_COPY_FAILED",
                format!(
                    "Failed to seed default profile auth {} -> {}: {error}",
                    root_auth_path.display(),
                    target_auth.display()
                ),
            )
        })?;
        initialize_default_active_profile(&backup_root)?;
    }

    write_install_state(&codex_home)?;
    super::profiles_index::load_profiles_index(Some(&codex_home))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::ensure_backup_initialized;
    use crate::windows::process::load_install_state;
    use crate::windows::env_lock;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-{name}-{unique}"))
    }

    #[test]
    fn initializes_backup_layout_and_active_profile_from_root_auth() {
        let codex_home = temp_codex_home("bootstrap-seed");
        fs::create_dir_all(&codex_home).unwrap();
        fs::write(codex_home.join("auth.json"), "seed-auth\n").unwrap();

        let initialized = ensure_backup_initialized(Some(&codex_home)).unwrap();

        assert!(initialized);
        for profile in ["a", "b", "c", "d"] {
            assert!(codex_home.join("account_backup").join(profile).is_dir());
            assert!(codex_home
                .join("account_backup")
                .join(profile)
                .join("auth.json")
                .is_file());
        }
        assert_eq!(
            fs::read_to_string(codex_home.join("account_backup").join(".current_profile")).unwrap(),
            "a\n"
        );

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn skips_when_backup_root_already_exists() {
        let codex_home = temp_codex_home("bootstrap-skip");
        fs::create_dir_all(codex_home.join("account_backup")).unwrap();

        let initialized = ensure_backup_initialized(Some(&codex_home)).unwrap();

        assert!(!initialized);
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn bootstrap_records_windows_cmd_path_in_install_state() {
        let _guard = env_lock().lock().unwrap();
        let codex_home = temp_codex_home("bootstrap-real-cli");
        let bin_dir = codex_home.join("bin");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(bin_dir.join("codex.cmd"), "@echo off\r\n").unwrap();
        fs::write(npm_dir.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            std::env::join_paths([bin_dir.clone(), npm_dir.clone()]).unwrap(),
        );

        let initialized = ensure_backup_initialized(Some(&codex_home)).unwrap();
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        let install_state = load_install_state(Some(&codex_home));

        assert!(initialized);
        assert_eq!(
            install_state.real_codex_path,
            Some(npm_dir.join("codex.cmd").to_string_lossy().into_owned())
        );
        let _ = fs::remove_dir_all(&codex_home);
    }
}
