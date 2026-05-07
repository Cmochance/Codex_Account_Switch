use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::platform::hooks::PlatformHooks;

use super::cli_shim::{get_install_state_file, managed_shim_path, real_codex_resolver_path};

const APP_NAME: &str = "Codex";
const AUTH_REFRESH_PROMPT: &str = "Reply with the single word OK.";
static MACOS_PLATFORM_HOOKS: MacosPlatformHooks = MacosPlatformHooks;
static MACOS_APP_PATH_CACHE: OnceLock<Option<String>> = OnceLock::new();

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstallState {
    pub real_codex_path: Option<String>,
    #[serde(default)]
    pub path_added_by_installer: bool,
}

pub struct MacosPlatformHooks;

pub fn platform_hooks() -> &'static dyn PlatformHooks {
    &MACOS_PLATFORM_HOOKS
}

pub fn load_install_state(codex_home: Option<&Path>) -> InstallState {
    let Some(codex_home) = codex_home else {
        return InstallState::default();
    };
    let path = get_install_state_file(codex_home);
    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return InstallState::default(),
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

pub(super) fn save_install_state(codex_home: Option<&Path>, state: &InstallState) {
    let Some(codex_home) = codex_home else {
        return;
    };
    let path = get_install_state_file(codex_home);
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(serialized) = serde_json::to_string_pretty(state) else {
        return;
    };
    let _ = fs::write(path, format!("{serialized}\n"));
}

fn push_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() && !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

fn codex_home_from_managed_shim(managed_shim_path: Option<&Path>) -> Option<PathBuf> {
    let shim_path = managed_shim_path?;
    let bin_dir = shim_path.parent()?;
    bin_dir.parent().map(Path::to_path_buf)
}

fn codex_cli_from_app_bundle(app_path: &Path) -> PathBuf {
    app_path.join("Contents").join("Resources").join("codex")
}

fn discover_real_codex_cli_from_shell(managed_shim_path: Option<&Path>) -> Option<PathBuf> {
    let codex_home = codex_home_from_managed_shim(managed_shim_path)?;
    let resolver_path = real_codex_resolver_path(&codex_home);
    if !resolver_path.is_file() {
        return None;
    }

    let managed_shim_text = managed_shim_path
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let output = Command::new(&resolver_path)
        .arg(managed_shim_text)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let resolved = stdout
        .lines()
        .map(str::trim)
        .find(|value| !value.is_empty())?;
    let candidate = PathBuf::from(resolved);
    if managed_shim_path.is_some_and(|managed| managed == candidate.as_path()) {
        return None;
    }
    candidate.is_file().then_some(candidate)
}

pub(super) fn discover_real_codex_cli_path(managed_shim_path: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(shell_path) = discover_real_codex_cli_from_shell(managed_shim_path) {
        push_candidate(&mut candidates, shell_path);
    }

    if let Some(path) = env::var_os("PATH") {
        for entry in env::split_paths(&path) {
            let candidate = entry.join("codex");
            if managed_shim_path.is_some_and(|managed| managed == candidate.as_path()) {
                continue;
            }
            push_candidate(&mut candidates, candidate);
        }
    }

    for app_path in codex_app_candidates() {
        let candidate = codex_cli_from_app_bundle(&app_path);
        if managed_shim_path.is_some_and(|managed| managed == candidate.as_path()) {
            continue;
        }
        push_candidate(&mut candidates, candidate);
    }

    candidates.into_iter().next()
}

fn resolve_real_codex_cli(codex_home: Option<&Path>) -> Option<PathBuf> {
    let managed_shim_path = codex_home.map(managed_shim_path);
    let mut state = load_install_state(codex_home);

    if let Some(raw_path) = state.real_codex_path.as_ref().map(PathBuf::from) {
        if raw_path.is_file()
            && managed_shim_path
                .as_deref()
                .is_none_or(|managed_path| managed_path != raw_path.as_path())
        {
            return Some(raw_path);
        }
    }

    let discovered_path = discover_real_codex_cli_path(managed_shim_path.as_deref());
    if let Some(path) = discovered_path.as_ref() {
        let resolved_text = path.to_string_lossy().into_owned();
        if state.real_codex_path.as_deref() != Some(resolved_text.as_str()) {
            state.real_codex_path = Some(resolved_text);
            save_install_state(codex_home, &state);
        }
    }
    discovered_path
}

fn codex_app_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/Codex.app")];
    if let Some(home) = env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("Applications").join("Codex.app"));
    }
    candidates
}

fn resolve_codex_app_path() -> Option<String> {
    MACOS_APP_PATH_CACHE
        .get_or_init(|| {
            codex_app_candidates()
                .into_iter()
                .find(|path| path.is_dir())
                .map(|path| path.to_string_lossy().into_owned())
        })
        .clone()
}

pub fn is_codex_app_running() -> bool {
    Command::new("pgrep")
        .args(["-x", APP_NAME])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn activate_running_app() -> AppResult<()> {
    let script = format!("tell application \"{APP_NAME}\" to activate");
    let status = Command::new("osascript")
        .args(["-e", &script])
        .status()
        .map_err(|error| {
            AppError::new(
                "APP_OPEN_FAILED",
                format!("Failed to activate Codex via AppleScript: {error}"),
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::new(
            "APP_OPEN_FAILED",
            "AppleScript activation for Codex failed.",
        ))
    }
}

pub fn open_or_activate_codex_app(_codex_home: Option<&Path>) -> AppResult<String> {
    if is_codex_app_running() {
        if activate_running_app().is_ok() {
            return Ok(APP_NAME.to_string());
        }
    }

    let mut command = Command::new("open");
    if let Some(app_path) = resolve_codex_app_path() {
        command.args(["-a", &app_path]);
        command.spawn().map_err(|error| {
            AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
        })?;
        return Ok(app_path);
    }

    command.args(["-a", APP_NAME]);
    command.spawn().map_err(|error| {
        AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
    })?;
    Ok(APP_NAME.to_string())
}

pub fn forward_to_real_codex(args: &[String], codex_home: Option<&Path>) -> AppResult<i32> {
    let Some(real_codex_path) = resolve_real_codex_cli(codex_home) else {
        return Err(AppError::new(
            "REAL_CODEX_NOT_FOUND",
            "Real Codex CLI path not found. Make sure `codex` is installed and in PATH.",
        ));
    };

    let status = Command::new(real_codex_path)
        .args(args)
        .status()
        .map_err(|error| {
            AppError::new(
                "REAL_CODEX_LAUNCH_FAILED",
                format!("Failed to launch real Codex CLI: {error}"),
            )
        })?;

    Ok(status.code().unwrap_or(1))
}

fn build_auth_refresh_command(real_codex_path: &Path, runtime_codex_home: &Path) -> Command {
    let mut command = Command::new(real_codex_path);
    command.args([
        "exec",
        "--skip-git-repo-check",
        "--color",
        "never",
        AUTH_REFRESH_PROMPT,
    ]);
    command.current_dir(runtime_codex_home);
    command.env("CODEX_HOME", runtime_codex_home);
    command
}

/// Build the `codex login` command. Resolution of the real codex
/// binary is anchored on `cli_codex_home` (the live `~/.codex`) so the
/// managed-shim filter works correctly even when `runtime_codex_home`
/// is a sandboxed sibling that doesn't have its own install state.
/// `runtime_codex_home` is what the spawned process sees as
/// `CODEX_HOME` and is where it will write `auth.json`.
fn build_login_command(cli_codex_home: &Path, runtime_codex_home: &Path) -> Command {
    let mut command = if let Some(real_codex_path) = resolve_real_codex_cli(Some(cli_codex_home)) {
        let mut command = Command::new(real_codex_path);
        command.arg("login");
        command
    } else {
        let mut command = Command::new("codex");
        command.arg("login");
        command
    };

    command.current_dir(runtime_codex_home);
    command.env("CODEX_HOME", runtime_codex_home);
    command
}

fn classify_auth_refresh_failure(message: &str) -> Option<AppError> {
    let normalized = message.to_ascii_lowercase();
    let requires_relogin = normalized.contains("token_invalidated")
        || normalized.contains("refresh_token_reused")
        || normalized.contains("authentication token has been invalidated")
        || normalized.contains("refresh token has already been used")
        || normalized.contains("please try signing in again")
        || normalized.contains("please log out and sign in again");

    if requires_relogin {
        return Some(AppError::new(
            "AUTH_REFRESH_RELOGIN_REQUIRED",
            "This account session has expired. Please log in again.",
        ));
    }

    None
}

pub fn run_codex_auth_refresh(cli_codex_home: &Path, runtime_codex_home: &Path) -> AppResult<()> {
    let Some(real_codex_path) = resolve_real_codex_cli(Some(cli_codex_home)) else {
        return Err(AppError::new(
            "REAL_CODEX_NOT_FOUND",
            "Real Codex CLI path not found. Make sure `codex` is installed and in PATH.",
        ));
    };

    let output = build_auth_refresh_command(&real_codex_path, runtime_codex_home)
        .output()
        .map_err(|error| {
            AppError::new(
                "AUTH_REFRESH_COMMAND_FAILED",
                format!("Failed to start `codex exec` for auth refresh: {error}"),
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "`codex exec` exited without a success status while refreshing auth.".to_string()
    };

    if let Some(error) = classify_auth_refresh_failure(&message) {
        return Err(error);
    }

    Err(AppError::new("AUTH_REFRESH_FAILED", message))
}

pub fn run_codex_login(cli_codex_home: &Path, runtime_codex_home: &Path) -> AppResult<()> {
    let output = build_login_command(cli_codex_home, runtime_codex_home)
        .output()
        .map_err(|error| {
        AppError::new(
            "LOGIN_COMMAND_FAILED",
            format!("Failed to start `codex login`: {error}"),
        )
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "`codex login` exited without a success status.".to_string()
    };

    Err(AppError::new("LOGIN_FAILED", message))
}

pub fn quit_codex_app_if_running() -> AppResult<bool> {
    if !is_codex_app_running() {
        return Ok(false);
    }

    let _ = Command::new("pkill")
        .args(["-TERM", "-x", APP_NAME])
        .status();
    for _ in 0..20 {
        if !is_codex_app_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(200));
    }

    let _ = Command::new("pkill")
        .args(["-KILL", "-x", APP_NAME])
        .status();
    for _ in 0..10 {
        if !is_codex_app_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(200));
    }

    Err(AppError::new(
        "APP_EXIT_FAILED",
        "Codex did not exit cleanly. Close it manually and retry.",
    ))
}

pub fn reopen_codex_app_if_needed(app_was_running: bool, codex_home: Option<&Path>) -> Vec<String> {
    if !app_was_running {
        return Vec::new();
    }

    if let Err(error) = open_or_activate_codex_app(codex_home) {
        return vec![format!(
            "Warning: failed to relaunch Codex: {}",
            error.message
        )];
    }

    Vec::new()
}

impl PlatformHooks for MacosPlatformHooks {
    fn open_or_activate_codex_app(&self, codex_home: Option<&Path>) -> AppResult<String> {
        open_or_activate_codex_app(codex_home)
    }

    fn quit_codex_app_if_running(&self) -> AppResult<bool> {
        quit_codex_app_if_running()
    }

    fn reopen_codex_app_if_needed(
        &self,
        app_was_running: bool,
        codex_home: Option<&Path>,
    ) -> Vec<String> {
        reopen_codex_app_if_needed(app_was_running, codex_home)
    }

    fn run_codex_login(
        &self,
        cli_codex_home: &Path,
        runtime_codex_home: &Path,
    ) -> AppResult<()> {
        run_codex_login(cli_codex_home, runtime_codex_home)
    }

    fn run_codex_auth_refresh(
        &self,
        cli_codex_home: &Path,
        runtime_codex_home: &Path,
    ) -> AppResult<()> {
        run_codex_auth_refresh(cli_codex_home, runtime_codex_home)
    }

    fn sync_on_window_close(&self) -> AppResult<()> {
        crate::macos::bootstrap::sync_root_state_to_current_profile(None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_auth_refresh_command, codex_app_candidates, codex_cli_from_app_bundle,
        discover_real_codex_cli_path, AUTH_REFRESH_PROMPT,
    };
    use crate::macos::cli_shim::real_codex_resolver_path;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-macos-process-{name}-{unique}"))
    }

    #[test]
    fn discover_real_codex_cli_path_skips_managed_shim() {
        let codex_home = temp_codex_home("discover-real-cli");
        let managed_bin = codex_home.join("bin");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(managed_bin.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(npm_dir.join("codex"), "#!/bin/sh\n").unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            std::env::join_paths([managed_bin.clone(), npm_dir.clone()]).unwrap(),
        );

        let resolved = discover_real_codex_cli_path(Some(&managed_bin.join("codex")));

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert_eq!(resolved, Some(npm_dir.join("codex")));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn discover_real_codex_cli_path_prefers_macos_shell_resolver() {
        let codex_home = temp_codex_home("discover-real-cli-shell");
        let managed_bin = codex_home.join("bin");
        let runtime_dir = codex_home.join("account_backup").join("macos");
        let shell_dir = codex_home.join("shell-bin");
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::create_dir_all(&shell_dir).unwrap();
        fs::write(managed_bin.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(shell_dir.join("codex"), "#!/bin/sh\n").unwrap();

        let resolver_path = real_codex_resolver_path(&codex_home);
        fs::write(
            &resolver_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{}'\n",
                shell_dir.join("codex").display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&resolver_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&resolver_path, permissions).unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", std::env::join_paths([managed_bin.clone()]).unwrap());

        let resolved = discover_real_codex_cli_path(Some(&managed_bin.join("codex")));

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert_eq!(resolved, Some(shell_dir.join("codex")));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn discover_real_codex_cli_path_falls_back_to_app_bundle_cli() {
        let codex_home = temp_codex_home("discover-real-cli-app-bundle");
        let managed_bin = codex_home.join("bin");
        let home_dir = codex_home.join("home");
        let app_path = home_dir.join("Applications").join("Codex.app");
        let app_cli_path = codex_cli_from_app_bundle(&app_path);
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(app_cli_path.parent().unwrap()).unwrap();
        fs::write(managed_bin.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(&app_cli_path, "#!/bin/sh\n").unwrap();

        let original_path = std::env::var_os("PATH");
        let original_home = std::env::var_os("HOME");
        std::env::set_var("PATH", std::env::join_paths([managed_bin.clone()]).unwrap());
        std::env::set_var("HOME", &home_dir);

        let resolved = discover_real_codex_cli_path(Some(&managed_bin.join("codex")));
        let expected = codex_app_candidates()
            .into_iter()
            .map(|path| codex_cli_from_app_bundle(&path))
            .find(|path| path.is_file());

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        assert_eq!(resolved, expected);
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn build_auth_refresh_command_targets_runtime_codex_home() {
        let real_codex_path = PathBuf::from("/opt/homebrew/bin/codex");
        let runtime_codex_home = PathBuf::from("/tmp/codex-home");

        let command = build_auth_refresh_command(&real_codex_path, &runtime_codex_home);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let envs = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            command.get_program().to_string_lossy(),
            real_codex_path.to_string_lossy()
        );
        assert_eq!(
            args,
            vec![
                "exec".to_string(),
                "--skip-git-repo-check".to_string(),
                "--color".to_string(),
                "never".to_string(),
                AUTH_REFRESH_PROMPT.to_string(),
            ]
        );
        assert_eq!(
            command.get_current_dir(),
            Some(runtime_codex_home.as_path())
        );
        assert!(envs.iter().any(|(key, value)| {
            key == "CODEX_HOME"
                && value.as_deref() == Some(runtime_codex_home.to_string_lossy().as_ref())
        }));
    }
}
