use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::errors::{AppError, AppResult};

use super::fs_ops::remove_path;
use super::paths::{
    get_backup_root, get_codex_home, get_install_state_file, get_runtime_dir,
    utc_timestamp, ACTIVE_MARKER_FILE, CURRENT_PROFILE_FILENAME, DEFAULT_PROFILES,
};
use super::process::{
    discover_real_codex_cli_path, hide_console_window, load_install_state,
    resolve_windows_invokable_path, save_install_state, InstallState,
};
use super::profiles::resolve_current_profile;

const AUTH_TEMPLATE: &str = include_str!("../../../examples/account_backup/demo/auth.json.example");
const CLI_RUNTIME_FILENAME: &str = "codex_switch_cli.exe";

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
}

fn runtime_cli_path(codex_home: &Path) -> PathBuf {
    get_runtime_dir(Some(codex_home)).join(CLI_RUNTIME_FILENAME)
}

fn managed_shim_path(codex_home: &Path) -> PathBuf {
    codex_home.join("bin").join("codex.cmd")
}

fn normalize_windows_path_entry(entry: impl AsRef<str>) -> String {
    let mut normalized = entry.as_ref().trim().replace('/', "\\").to_ascii_lowercase();
    while normalized.ends_with('\\') {
        normalized.pop();
    }
    normalized
}

fn split_windows_path(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

fn ensure_dir_on_path_entries(entries: &[String], target: &Path) -> (Vec<String>, bool) {
    let target_text = target.to_string_lossy().into_owned();
    let target_key = normalize_windows_path_entry(&target_text);
    let filtered = entries
        .iter()
        .filter(|entry| normalize_windows_path_entry(entry) != target_key)
        .cloned()
        .collect::<Vec<_>>();

    let mut next_entries = Vec::with_capacity(filtered.len() + 1);
    next_entries.push(target_text);
    next_entries.extend(filtered);

    let changed = next_entries != entries;
    (next_entries, changed)
}

fn remove_dir_from_path_entries(entries: &[String], target: &Path) -> (Vec<String>, bool) {
    let target_key = normalize_windows_path_entry(target.to_string_lossy());
    let next_entries = entries
        .iter()
        .filter(|entry| normalize_windows_path_entry(entry) != target_key)
        .cloned()
        .collect::<Vec<_>>();

    let changed = next_entries.len() != entries.len();
    (next_entries, changed)
}

fn read_user_path_value() -> AppResult<String> {
    if !cfg!(target_os = "windows") {
        return Ok(String::new());
    }

    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-Command",
        "[Environment]::GetEnvironmentVariable('Path', 'User')",
    ]);

    let output = hide_console_window(&mut command).output().map_err(|error| {
        AppError::new(
            "PATH_READ_FAILED",
            format!("Failed to read user PATH: {error}"),
        )
    })?;

    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn write_user_path_value(value: &str) -> AppResult<()> {
    if !cfg!(target_os = "windows") {
        let _ = value;
        return Ok(());
    }

    let mut command = Command::new("powershell");
    command.args([
        "-NoProfile",
        "-Command",
        "[Environment]::SetEnvironmentVariable('Path', $env:CODEX_SWITCH_USER_PATH, 'User')",
    ]);
    command.env("CODEX_SWITCH_USER_PATH", value);

    let output = hide_console_window(&mut command).output().map_err(|error| {
        AppError::new(
            "PATH_WRITE_FAILED",
            format!("Failed to update user PATH: {error}"),
        )
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(AppError::new(
        "PATH_WRITE_FAILED",
        if stderr.is_empty() {
            "Failed to update user PATH.".to_string()
        } else {
            stderr
        },
    ))
}

fn ensure_dir_on_user_path(path: &Path) -> AppResult<bool> {
    let entries = split_windows_path(&read_user_path_value()?);
    let (next_entries, changed) = ensure_dir_on_path_entries(&entries, path);
    if changed {
        write_user_path_value(&next_entries.join(";"))?;
    }
    Ok(changed)
}

fn remove_dir_from_user_path(path: &Path) -> AppResult<bool> {
    let entries = split_windows_path(&read_user_path_value()?);
    let (next_entries, changed) = remove_dir_from_path_entries(&entries, path);
    if changed {
        write_user_path_value(&next_entries.join(";"))?;
    }
    Ok(changed)
}

fn resolve_real_codex_path(codex_home: &Path) -> AppResult<PathBuf> {
    let managed_shim_path = managed_shim_path(codex_home);
    let state = load_install_state(Some(codex_home));
    if let Some(existing) = state.real_codex_path.as_deref() {
        let path = PathBuf::from(existing);
        if path.is_file() && path != managed_shim_path {
            return Ok(path);
        }
    }

    discover_real_codex_cli_path(Some(&managed_shim_path)).ok_or_else(|| {
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
                format!("Failed to write placeholder auth {}: {error}", auth_file.display()),
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

fn write_codex_shim(shim_path: &Path) -> AppResult<()> {
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
        "@echo off\r\nsetlocal\r\nif not defined CODEX_HOME set \"CODEX_HOME=%USERPROFILE%\\\\.codex\"\r\n\"%CODEX_HOME%\\\\account_backup\\\\windows\\\\{CLI_RUNTIME_FILENAME}\" shim %*\r\nexit /b %ERRORLEVEL%\r\n"
    );
    fs::write(shim_path, shim_contents).map_err(|error| {
        AppError::new(
            "SHIM_WRITE_FAILED",
            format!("Failed to write command shim {}: {error}", shim_path.display()),
        )
    })
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

    Ok(())
}

pub fn refresh_install_state(codex_home: &Path) -> AppResult<()> {
    let managed_shim_path = managed_shim_path(codex_home);
    let previous_state = load_install_state(Some(codex_home));
    let real_codex_path = previous_state
        .real_codex_path
        .as_deref()
        .and_then(|path| resolve_windows_invokable_path(Path::new(path)))
        .filter(|path| path != &managed_shim_path)
        .or_else(|| discover_real_codex_cli_path(Some(&managed_shim_path)))
        .map(|path| path.to_string_lossy().into_owned());

    let state = InstallState {
        real_codex_path,
        path_added_by_installer: previous_state.path_added_by_installer,
    };
    save_install_state(Some(codex_home), &state);
    Ok(())
}

fn install_from_with_path_hook(
    source_cli_path: &Path,
    codex_home: Option<&Path>,
    ensure_dir_on_user_path_hook: impl Fn(&Path) -> AppResult<bool>,
) -> AppResult<InstallSummary> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let runtime_dir = get_runtime_dir(Some(&codex_home));
    let managed_bin_dir = codex_home.join("bin");
    let managed_shim_path = managed_shim_path(&codex_home);
    let runtime_cli_path = runtime_cli_path(&codex_home);
    let real_codex_path = resolve_real_codex_path(&codex_home)?;

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
    copy_runtime_cli(source_cli_path, &runtime_cli_path)?;
    write_codex_shim(&managed_shim_path)?;

    let added_to_path = ensure_dir_on_user_path_hook(&managed_bin_dir)?;
    let previous_state = load_install_state(Some(&codex_home));
    let state = InstallState {
        real_codex_path: Some(real_codex_path.to_string_lossy().into_owned()),
        path_added_by_installer: added_to_path || previous_state.path_added_by_installer,
    };
    save_install_state(Some(&codex_home), &state);

    Ok(InstallSummary {
        seeded_auth,
        placeholder_auth_files,
        initialized_default_profile,
        runtime_cli_path,
        managed_shim_path,
        path_added_by_installer: added_to_path || previous_state.path_added_by_installer,
        path_changed: added_to_path,
        real_codex_path,
    })
}

pub fn install_from(source_cli_path: &Path, codex_home: Option<&Path>) -> AppResult<InstallSummary> {
    install_from_with_path_hook(source_cli_path, codex_home, ensure_dir_on_user_path)
}

#[allow(dead_code)]
pub fn install(codex_home: Option<&Path>) -> AppResult<InstallSummary> {
    let source_cli_path = std::env::var_os("CODEX_SWITCH_RELEASE_EXE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| {
            AppError::new(
                "CLI_PATH_UNAVAILABLE",
                "Failed to resolve current CLI path.",
            )
        })?;
    if source_cli_path.is_file() {
        return install_from(&source_cli_path, codex_home);
    }

    Err(AppError::new(
        "CLI_PATH_UNAVAILABLE",
        "Failed to resolve current CLI path.",
    ))
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

fn is_directory_empty(path: &Path) -> bool {
    path.is_dir() && fs::read_dir(path).map(|mut entries| entries.next().is_none()).unwrap_or(false)
}

fn uninstall_with_path_hook(
    remove_script: bool,
    codex_home: Option<&Path>,
    remove_dir_from_user_path_hook: impl Fn(&Path) -> AppResult<bool>,
) -> AppResult<UninstallSummary> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let runtime_dir = get_runtime_dir(Some(&codex_home));
    let install_state_path = get_install_state_file(Some(&codex_home));
    let install_state = load_install_state(Some(&codex_home));
    let bin_dir = codex_home.join("bin");
    let managed_shim_path = managed_shim_path(&codex_home);
    let runtime_cli_path = runtime_cli_path(&codex_home);

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

    if remove_script {
        let legacy_python_cli = runtime_dir.join("codex_switch.py");
        if legacy_python_cli.exists() {
            remove_path(&legacy_python_cli)?;
        }

        let legacy_python_common = runtime_dir.join("common.py");
        if legacy_python_common.exists() {
            remove_path(&legacy_python_common)?;
        }

        let legacy_python_cache = runtime_dir.join("__pycache__");
        if legacy_python_cache.exists() {
            remove_path(&legacy_python_cache)?;
        }
    }

    let removed_path_entry = if install_state.path_added_by_installer && is_directory_empty(&bin_dir) {
        remove_dir_from_user_path_hook(&bin_dir)?
    } else {
        false
    };

    if is_directory_empty(&bin_dir) {
        let _ = fs::remove_dir(&bin_dir);
    }
    if is_directory_empty(&runtime_dir) {
        let _ = fs::remove_dir(&runtime_dir);
    }

    Ok(UninstallSummary {
        removed_shim,
        removed_install_state,
        removed_runtime_cli,
        removed_path_entry,
    })
}

pub fn uninstall(remove_script: bool, codex_home: Option<&Path>) -> AppResult<UninstallSummary> {
    uninstall_with_path_hook(remove_script, codex_home, remove_dir_from_user_path)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        ensure_dir_on_path_entries, install_from_with_path_hook, remove_dir_from_path_entries,
        uninstall_with_path_hook,
        CLI_RUNTIME_FILENAME,
    };
    use crate::windows::{env_guard, process::{load_install_state, InstallState}};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-install-{name}-{unique}"))
    }

    #[test]
    fn ensure_dir_on_path_entries_moves_target_to_front() {
        let target = PathBuf::from(r"C:\Users\demo\.codex\bin");
        let entries = vec![
            r"C:\Program Files\Codex".to_string(),
            r"c:\users\demo\.codex\bin".to_string(),
        ];

        let (next_entries, changed) = ensure_dir_on_path_entries(&entries, &target);

        assert!(changed);
        assert_eq!(next_entries[0], r"C:\Users\demo\.codex\bin");
        assert_eq!(next_entries.len(), 2);
    }

    #[test]
    fn remove_dir_from_path_entries_removes_target_case_insensitively() {
        let target = PathBuf::from(r"C:\Users\demo\.codex\bin");
        let entries = vec![
            r"C:\Program Files\Codex".to_string(),
            r"c:\users\demo\.codex\bin".to_string(),
        ];

        let (next_entries, changed) = remove_dir_from_path_entries(&entries, &target);

        assert!(changed);
        assert_eq!(next_entries, vec![r"C:\Program Files\Codex".to_string()]);
    }

    #[test]
    fn install_from_copies_cli_and_writes_state() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("install");
        let source_cli = codex_home.join("source").join(CLI_RUNTIME_FILENAME);
        fs::create_dir_all(source_cli.parent().unwrap()).unwrap();
        fs::write(&source_cli, "cli").unwrap();
        fs::write(codex_home.join("auth.json"), "seed-auth").unwrap();

        let original_path = std::env::var_os("PATH");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();
        std::env::set_var("PATH", &npm_dir);

        let result = install_from_with_path_hook(&source_cli, Some(&codex_home), |_| Ok(true)).unwrap();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(result.runtime_cli_path.is_file());
        assert!(result.managed_shim_path.is_file());
        assert_eq!(
            fs::read_to_string(codex_home.join("account_backup").join(".current_profile")).unwrap(),
            "a\n"
        );
        assert_eq!(
            load_install_state(Some(&codex_home)),
            InstallState {
                real_codex_path: Some(npm_dir.join("codex.cmd").to_string_lossy().into_owned()),
                path_added_by_installer: true,
            }
        );
        assert!(result.path_added_by_installer);
        assert!(result.path_changed);
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn uninstall_remove_script_removes_runtime_cli() {
        let codex_home = temp_codex_home("uninstall");
        let runtime_dir = codex_home.join("account_backup").join("windows");
        let bin_dir = codex_home.join("bin");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(runtime_dir.join("install_state.json"), "{\n  \"path_added_by_installer\": false\n}\n").unwrap();
        fs::write(runtime_dir.join(CLI_RUNTIME_FILENAME), "cli").unwrap();
        fs::write(bin_dir.join("codex.cmd"), "shim").unwrap();

        let summary = uninstall_with_path_hook(true, Some(&codex_home), |_| Ok(false)).unwrap();

        assert!(summary.removed_shim);
        assert!(summary.removed_install_state);
        assert!(summary.removed_runtime_cli);
        assert!(!runtime_dir.exists());
        let _ = fs::remove_dir_all(&codex_home);
    }
}
