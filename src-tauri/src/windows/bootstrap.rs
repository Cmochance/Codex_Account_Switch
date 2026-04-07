use std::path::{Path, PathBuf};

use crate::errors::AppResult;

use super::fs_ops::backup_root_state_to_profile;
use super::paths::{get_backup_root, get_codex_home};
use super::profiles::resolve_current_profile;

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
    super::profiles_index::load_profiles_index(Some(&codex_home))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::ensure_backup_initialized;
    use crate::windows::env_guard;
    use crate::windows::process::load_install_state;
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
        let _guard = env_guard();
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
        let _guard = env_guard();
        let codex_home = temp_codex_home("bootstrap-skip");
        fs::create_dir_all(codex_home.join("account_backup")).unwrap();

        let initialized = ensure_backup_initialized(Some(&codex_home)).unwrap();

        assert!(!initialized);
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn bootstrap_records_windows_cmd_path_in_install_state() {
        let _guard = env_guard();
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
        let resolved_path = install_state.real_codex_path.unwrap();
        assert!(resolved_path.ends_with("\\codex.cmd"));
        let _ = fs::remove_dir_all(&codex_home);
    }
}
