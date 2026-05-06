use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::{AppError, AppResult};

use super::paths::{
    autosave_timestamp, get_auto_save_root, get_current_profile_file, list_profile_dirs,
    utc_timestamp, ACTIVE_MARKER_FILE,
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
            AppError::new(
                "FS_REMOVE_FAILED",
                format!("Failed to remove directory {}: {error}", path.display()),
            )
        })
    } else {
        fs::remove_file(path).map_err(|error| {
            AppError::new(
                "FS_REMOVE_FAILED",
                format!("Failed to remove file {}: {error}", path.display()),
            )
        })
    }
}

/// Path used for the staging file inside an atomic write / copy. Sits next to
/// the target so the final `fs::rename` stays on the same filesystem (a
/// requirement for POSIX rename atomicity).
fn atomic_temp_path(target: &Path) -> AppResult<PathBuf> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            AppError::new(
                "FS_INVALID_PATH",
                format!(
                    "Refusing to write to a path without a file name: {}",
                    target.display()
                ),
            )
        })?;
    let pid = std::process::id();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    Ok(target.with_file_name(format!(".{file_name}.tmp.{pid}.{nonce}")))
}

fn publish_staged_file(
    temp: &Path,
    target: &Path,
    error_code: &'static str,
    operation: &'static str,
) -> AppResult<()> {
    match fs::rename(temp, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            #[cfg(windows)]
            {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
                ) {
                    // Windows std::fs::rename does not replace an existing
                    // destination. Keep Unix's atomic replace behavior, but
                    // fall back to remove-then-rename on Windows so switching
                    // can still overwrite root auth/config files.
                    if let Err(remove_error) = fs::remove_file(target) {
                        return Err(AppError::new(
                            error_code,
                            format!(
                                "Failed to replace existing {} target {} after staging {}: {remove_error}",
                                operation,
                                target.display(),
                                temp.display()
                            ),
                        ));
                    }
                    return fs::rename(temp, target).map_err(|retry_error| {
                        AppError::new(
                            error_code,
                            format!(
                                "Failed to publish {} {} -> {} after replacing existing target: {retry_error}",
                                operation,
                                temp.display(),
                                target.display()
                            ),
                        )
                    });
                }
            }

            Err(AppError::new(
                error_code,
                format!(
                    "Failed to publish {} {} -> {}: {error}",
                    operation,
                    temp.display(),
                    target.display()
                ),
            ))
        }
    }
}

/// Write `contents` to `target` atomically: stage to a sibling temp file then
/// rename into place. Concurrent readers (e.g. the Codex VSCode extension
/// watching `~/.codex/auth.json`) only ever see the previous full contents or
/// the new full contents, never a half-written file.
pub fn atomic_write_bytes(target: &Path, contents: impl AsRef<[u8]>) -> AppResult<()> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::new(
                    "FS_CREATE_FAILED",
                    format!(
                        "Failed to create parent directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
    }

    let temp = atomic_temp_path(target)?;
    let cleanup = || {
        let _ = fs::remove_file(&temp);
    };

    if let Err(error) = fs::write(&temp, contents.as_ref()) {
        cleanup();
        return Err(AppError::new(
            "FS_WRITE_FAILED",
            format!("Failed to stage write to {}: {error}", temp.display()),
        ));
    }

    publish_staged_file(&temp, target, "FS_WRITE_FAILED", "atomic write").map_err(|error| {
        cleanup();
        error
    })
}

/// Atomic file-to-file copy that preserves the source mode (so e.g.
/// `auth.json`'s 0600 permissions survive). Uses `fs::copy` for the staging
/// step — `fs::copy` on POSIX preserves the mode of the source — followed by
/// `fs::rename` to publish.
pub fn atomic_copy_file(src: &Path, dst: &Path) -> AppResult<()> {
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                AppError::new(
                    "FS_CREATE_FAILED",
                    format!(
                        "Failed to create parent directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
    }

    let temp = atomic_temp_path(dst)?;
    let cleanup = || {
        let _ = fs::remove_file(&temp);
    };

    if let Err(error) = fs::copy(src, &temp) {
        cleanup();
        return Err(AppError::new(
            "FS_COPY_FAILED",
            format!(
                "Failed to stage copy {} -> {}: {error}",
                src.display(),
                temp.display()
            ),
        ));
    }

    publish_staged_file(&temp, dst, "FS_COPY_FAILED", "atomic copy").map_err(|error| {
        cleanup();
        error
    })
}

pub fn copy_entry(src: &Path, dst: &Path) -> AppResult<()> {
    if src.is_dir() {
        replace_tree(src, dst)
    } else {
        atomic_copy_file(src, dst)
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
            format!(
                "Failed to create directory {}: {error}",
                target_dir.display()
            ),
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
                format!(
                    "Failed to read directory entry {}: {error}",
                    source_dir.display()
                ),
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

pub fn backup_root_state_to_profile(
    profile: &str,
    codex_home: &Path,
    backup_root: &Path,
) -> AppResult<()> {
    let profile_dir = backup_root.join(profile);
    if !profile_dir.is_dir() {
        return Ok(());
    }

    let mut managed_names = BTreeSet::from(["auth.json".to_string()]);
    for entry in fs::read_dir(&profile_dir).map_err(|error| {
        AppError::new(
            "FS_READ_FAILED",
            format!(
                "Failed to read directory {}: {error}",
                profile_dir.display()
            ),
        )
    })? {
        let entry = entry.map_err(|error| {
            AppError::new(
                "FS_READ_FAILED",
                format!(
                    "Failed to read directory entry {}: {error}",
                    profile_dir.display()
                ),
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
            format!(
                "Failed to create autosave directory {}: {error}",
                snapshot_dir.display()
            ),
        )
    })?;
    copy_entry(&auth_file, &snapshot_dir.join("auth.json"))
}

pub fn set_active_marker(profile: &str, backup_root: &Path) -> AppResult<()> {
    for profile_dir in list_profile_dirs(backup_root) {
        remove_path(&profile_dir.join(ACTIVE_MARKER_FILE))?;
    }

    let marker = backup_root.join(profile).join(ACTIVE_MARKER_FILE);
    atomic_write_bytes(&marker, format!("activated_at={}\n", utc_timestamp()))?;

    let current_profile_file = get_current_profile_file(backup_root.parent());
    atomic_write_bytes(&current_profile_file, format!("{profile}\n"))
}

#[cfg(test)]
mod tests {
    use super::{atomic_copy_file, atomic_write_bytes};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-fs-ops-{name}-{unique}"))
    }

    #[test]
    fn atomic_write_bytes_creates_parent_and_replaces_existing() {
        let root = temp_dir("atomic-write");
        let target = root.join("nested").join("config.toml");
        atomic_write_bytes(&target, b"first\n").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "first\n");

        atomic_write_bytes(&target, b"second\n").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "second\n");

        // No staging file should be left behind on success.
        let parent = target.parent().unwrap();
        let leftovers = fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".config.toml.tmp"))
            .count();
        assert_eq!(leftovers, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn atomic_copy_file_round_trip_preserves_contents() {
        let root = temp_dir("atomic-copy");
        let src = root.join("auth.json");
        let dst = root.join("dest").join("auth.json");
        fs::create_dir_all(root.join("dest")).unwrap();
        fs::write(&src, b"{\"auth_mode\":\"chatgpt\"}").unwrap();

        atomic_copy_file(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"{\"auth_mode\":\"chatgpt\"}");

        // No staging file left behind.
        let leftovers = fs::read_dir(dst.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".auth.json.tmp"))
            .count();
        assert_eq!(leftovers, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn atomic_copy_file_replaces_existing_destination() {
        let root = temp_dir("atomic-copy-replace");
        let src = root.join("auth.json");
        let dst = root.join("dest").join("auth.json");
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::write(&src, b"new-auth").unwrap();
        fs::write(&dst, b"old-auth").unwrap();

        atomic_copy_file(&src, &dst).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), b"new-auth");

        let _ = fs::remove_dir_all(&root);
    }
}
