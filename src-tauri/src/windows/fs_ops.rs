use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::errors::{AppError, AppResult};

use super::paths::{
    autosave_timestamp, get_auto_save_root, get_current_profile_file, list_profile_dirs, utc_timestamp,
    ACTIVE_MARKER_FILE,
};

fn should_ignore_entry(name: &str) -> bool {
    matches!(name, ".DS_Store" | ACTIVE_MARKER_FILE)
}

pub fn read_text_stripped(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|content| content.trim().to_string())
        .unwrap_or_default()
}

pub fn remove_path(path: &Path) -> AppResult<()> {
    if !path.exists() && !path.is_symlink() {
        return Ok(());
    }

    if path.is_dir() && !path.is_symlink() {
        fs::remove_dir_all(path).map_err(|error| {
            AppError::new("FS_REMOVE_FAILED", format!("Failed to remove directory {}: {error}", path.display()))
        })
    } else {
        fs::remove_file(path).map_err(|error| {
            AppError::new("FS_REMOVE_FAILED", format!("Failed to remove file {}: {error}", path.display()))
        })
    }
}

pub fn copy_entry(src: &Path, dst: &Path) -> AppResult<()> {
    if src.is_dir() {
        replace_tree(src, dst)
    } else {
        remove_path(dst)?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::new(
                    "FS_CREATE_FAILED",
                    format!("Failed to create parent directory {}: {error}", parent.display()),
                )
            })?;
        }

        fs::copy(src, dst).map_err(|error| {
            AppError::new(
                "FS_COPY_FAILED",
                format!("Failed to copy {} -> {}: {error}", src.display(), dst.display()),
            )
        })?;
        Ok(())
    }
}

pub fn replace_tree(src: &Path, dst: &Path) -> AppResult<()> {
    remove_path(dst)?;
    fs::create_dir_all(dst).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!("Failed to create directory {}: {error}", dst.display()),
        )
    })?;

    for entry in fs::read_dir(src).map_err(|error| {
        AppError::new(
            "FS_READ_FAILED",
            format!("Failed to read directory {}: {error}", src.display()),
        )
    })? {
        let entry = entry.map_err(|error| {
            AppError::new(
                "FS_READ_FAILED",
                format!("Failed to read directory entry {}: {error}", src.display()),
            )
        })?;
        let source_path = entry.path();
        let target_path = dst.join(entry.file_name());
        copy_entry(&source_path, &target_path)?;
    }

    Ok(())
}

pub fn overlay_directory_contents(source_dir: &Path, target_dir: &Path) -> AppResult<()> {
    fs::create_dir_all(target_dir).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!("Failed to create directory {}: {error}", target_dir.display()),
        )
    })?;

    for entry in fs::read_dir(source_dir).map_err(|error| {
        AppError::new(
            "FS_READ_FAILED",
            format!("Failed to read directory {}: {error}", source_dir.display()),
        )
    })? {
        let entry = entry.map_err(|error| {
            AppError::new(
                "FS_READ_FAILED",
                format!("Failed to read directory entry {}: {error}", source_dir.display()),
            )
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if should_ignore_entry(name) {
            continue;
        }

        copy_entry(&entry.path(), &target_dir.join(name))?;
    }

    Ok(())
}

pub fn backup_root_state_to_profile(profile: &str, codex_home: &Path, backup_root: &Path) -> AppResult<()> {
    let profile_dir = backup_root.join(profile);
    if !profile_dir.is_dir() {
        return Ok(());
    }

    let mut managed_names = BTreeSet::from(["auth.json".to_string()]);
    for entry in fs::read_dir(&profile_dir).map_err(|error| {
        AppError::new(
            "FS_READ_FAILED",
            format!("Failed to read directory {}: {error}", profile_dir.display()),
        )
    })? {
        let entry = entry.map_err(|error| {
            AppError::new(
                "FS_READ_FAILED",
                format!("Failed to read directory entry {}: {error}", profile_dir.display()),
            )
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if should_ignore_entry(name) {
            continue;
        }
        managed_names.insert(name.to_string());
    }

    for name in managed_names {
        let src = codex_home.join(&name);
        let dst = profile_dir.join(&name);
        if src.is_dir() || src.is_file() {
            copy_entry(&src, &dst)?;
        } else {
            remove_path(&dst)?;
        }
    }

    Ok(())
}

pub fn autosave_auth(codex_home: &Path) -> AppResult<()> {
    let auth_file = codex_home.join("auth.json");
    if !auth_file.is_file() {
        return Ok(());
    }

    let snapshot_dir = get_auto_save_root(Some(codex_home)).join(autosave_timestamp());
    fs::create_dir_all(&snapshot_dir).map_err(|error| {
        AppError::new(
            "FS_CREATE_FAILED",
            format!("Failed to create autosave directory {}: {error}", snapshot_dir.display()),
        )
    })?;
    copy_entry(&auth_file, &snapshot_dir.join("auth.json"))
}

pub fn set_active_marker(profile: &str, backup_root: &Path) -> AppResult<()> {
    for profile_dir in list_profile_dirs(backup_root) {
        remove_path(&profile_dir.join(ACTIVE_MARKER_FILE))?;
    }

    let marker = backup_root.join(profile).join(ACTIVE_MARKER_FILE);
    fs::write(&marker, format!("activated_at={}\n", utc_timestamp())).map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!("Failed to write active marker {}: {error}", marker.display()),
        )
    })?;

    let current_profile_file = get_current_profile_file(backup_root.parent());
    fs::write(&current_profile_file, format!("{profile}\n")).map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!(
                "Failed to write current profile marker {}: {error}",
                current_profile_file.display()
            ),
        )
    })
}
