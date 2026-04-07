use std::env;
use std::fs;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

use super::paths::{get_codex_home, get_install_state_file, APP_PROCESS_NAME};

const WINDOWS_INVOKABLE_SUFFIXES: [&str; 4] = ["cmd", "exe", "bat", "com"];
const WINDOWS_STORE_APP_ID: &str = "OpenAI.Codex_2p2nqsd0c76g0!App";
const WINDOWS_STORE_SHELL_PREFIX: &str = r"shell:AppsFolder\";
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
static WINDOWS_APP_TARGET_CACHE: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppLaunchTarget {
    WindowsStore(String),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstallState {
    pub real_codex_path: Option<String>,
    #[serde(default)]
    pub path_added_by_installer: bool,
}

pub fn load_install_state(codex_home: Option<&Path>) -> InstallState {
    let path = get_install_state_file(codex_home);
    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return InstallState::default(),
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

pub(super) fn save_install_state(codex_home: Option<&Path>, state: &InstallState) {
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

fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

fn paths_match(left: &Path, right: &Path) -> bool {
    normalize_windows_path(left) == normalize_windows_path(right)
}

pub(super) fn resolve_windows_invokable_path(path: &Path) -> Option<PathBuf> {
    let extension = path.extension().and_then(|value| value.to_str());
    if let Some(extension) = extension {
        return WINDOWS_INVOKABLE_SUFFIXES
            .iter()
            .any(|suffix| extension.eq_ignore_ascii_case(suffix))
            .then(|| path.is_file().then(|| path.to_path_buf()))
            .flatten();
    }

    for suffix in WINDOWS_INVOKABLE_SUFFIXES {
        let candidate = path.with_extension(suffix);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn push_real_codex_candidate(
    candidates: &mut Vec<PathBuf>,
    path: PathBuf,
    managed_shim_path: Option<&Path>,
) {
    let Some(resolved_path) = resolve_windows_invokable_path(&path) else {
        return;
    };
    if managed_shim_path.is_some_and(|managed_shim| paths_match(&resolved_path, managed_shim)) {
        return;
    }
    push_candidate(candidates, resolved_path);
}

fn push_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

fn managed_codex_shim_path(codex_home: Option<&Path>) -> PathBuf {
    codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("bin")
        .join("codex.cmd")
}

pub(super) fn hide_console_window(command: &mut Command) -> &mut Command {
    #[cfg(target_os = "windows")]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

pub(super) fn discover_real_codex_cli_path(managed_shim_path: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        let mut command = Command::new("where");
        command.arg("codex");
        if let Ok(output) = hide_console_window(&mut command).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout
                    .lines()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    push_real_codex_candidate(
                        &mut candidates,
                        PathBuf::from(line),
                        managed_shim_path,
                    );
                }
            }
        }
    }

    if let Some(path) = env::var_os("PATH") {
        for entry in env::split_paths(&path) {
            let candidate = if cfg!(target_os = "windows") {
                entry.join("codex")
            } else {
                entry.join("codex")
            };
            push_real_codex_candidate(&mut candidates, candidate, managed_shim_path);
        }
    }

    candidates.into_iter().next()
}

fn windows_store_shell_target(app_id: &str) -> String {
    format!("{WINDOWS_STORE_SHELL_PREFIX}{app_id}")
}

fn is_valid_windows_store_app_id(app_id: &str) -> bool {
    let trimmed = app_id.trim();
    trimmed.starts_with("OpenAI.Codex_") && trimmed.ends_with("!App")
}

fn detect_windows_store_app_target() -> Option<String> {
    if !cfg!(target_os = "windows") {
        return None;
    }

    let script = format!(
        "$package = Get-AppxPackage -Name 'OpenAI.Codex' -ErrorAction SilentlyContinue; \
         if ($package) {{ \
           $appId = Get-StartApps | Where-Object {{ $_.AppID -like 'OpenAI.Codex*' }} | Select-Object -First 1 -ExpandProperty AppID; \
           if ($appId) {{ $appId }} else {{ '{WINDOWS_STORE_APP_ID}' }} \
         }}"
    );
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-Command", &script]);
    let output = hide_console_window(&mut command).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let app_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_valid_windows_store_app_id(&app_id).then(|| windows_store_shell_target(&app_id))
}

fn resolve_windows_store_shell_target() -> String {
    WINDOWS_APP_TARGET_CACHE
        .get_or_init(|| {
            detect_windows_store_app_target()
                .unwrap_or_else(|| windows_store_shell_target(WINDOWS_STORE_APP_ID))
        })
        .clone()
}

fn resolve_windows_app_target() -> AppLaunchTarget {
    AppLaunchTarget::WindowsStore(resolve_windows_store_shell_target())
}

pub fn is_codex_app_running() -> bool {
    let mut command = Command::new("tasklist");
    command.args([
        "/FI",
        &format!("IMAGENAME eq {APP_PROCESS_NAME}"),
        "/FO",
        "CSV",
        "/NH",
    ]);

    let output = match hide_console_window(&mut command).output() {
        Ok(value) => value,
        Err(_) => return false,
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    stdout.contains(&APP_PROCESS_NAME.to_ascii_lowercase())
}

pub fn open_or_activate_codex_app(_codex_home: Option<&Path>) -> AppResult<String> {
    let target = resolve_windows_app_target();

    match target {
        AppLaunchTarget::WindowsStore(shell_target) => {
            let mut command = Command::new("explorer.exe");
            command.arg(&shell_target);
            hide_console_window(&mut command)
                .spawn()
                .map_err(|error| {
                    AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
                })?;

            Ok(shell_target)
        }
    }
}

fn resolve_real_codex_cli(codex_home: Option<&Path>) -> Option<PathBuf> {
    let managed_shim_path = managed_codex_shim_path(codex_home);
    let mut state = load_install_state(codex_home);

    if let Some(raw_path) = state.real_codex_path.as_ref().map(PathBuf::from) {
        if let Some(resolved_path) = resolve_windows_invokable_path(&raw_path) {
            if !paths_match(&resolved_path, &managed_shim_path) {
                let resolved_text = resolved_path.to_string_lossy().into_owned();
                if state.real_codex_path.as_deref() != Some(resolved_text.as_str()) {
                    state.real_codex_path = Some(resolved_text);
                    save_install_state(codex_home, &state);
                }
                return Some(resolved_path);
            }
        }
    }

    let discovered_path = discover_real_codex_cli_path(Some(&managed_shim_path));
    if let Some(path) = discovered_path.as_ref() {
        let resolved_text = path.to_string_lossy().into_owned();
        if state.real_codex_path.as_deref() != Some(resolved_text.as_str()) {
            state.real_codex_path = Some(resolved_text);
            save_install_state(codex_home, &state);
        }
    }
    discovered_path
}

pub fn forward_to_real_codex(args: &[String], codex_home: Option<&Path>) -> AppResult<i32> {
    let Some(real_codex_path) = resolve_real_codex_cli(codex_home) else {
        return Err(AppError::new(
            "REAL_CODEX_NOT_FOUND",
            "Real Codex CLI path not found. Run `codex_switch_cli.exe install` first.",
        ));
    };

    let mut command = Command::new(real_codex_path);
    command.args(args);
    let status = hide_console_window(&mut command).status().map_err(|error| {
        AppError::new(
            "REAL_CODEX_LAUNCH_FAILED",
            format!("Failed to launch real Codex CLI: {error}"),
        )
    })?;

    Ok(status.code().unwrap_or(1))
}

fn build_login_command(codex_home: &Path) -> Command {
    let mut command = if let Some(real_codex_path) = resolve_real_codex_cli(Some(codex_home)) {
        let mut command = Command::new(real_codex_path);
        command.arg("login");
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "codex", "login"]);
        command
    } else {
        let mut command = Command::new("codex");
        command.arg("login");
        command
    };

    hide_console_window(&mut command);
    command.current_dir(codex_home);
    command.env("CODEX_HOME", codex_home);
    command
}

pub fn run_codex_login(codex_home: &Path) -> AppResult<()> {
    let output = build_login_command(codex_home).output().map_err(|error| {
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

    let mut taskkill = Command::new("taskkill");
    taskkill.args(["/IM", APP_PROCESS_NAME]);
    let _ = hide_console_window(&mut taskkill).output();
    for _ in 0..20 {
        if !is_codex_app_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(200));
    }

    let mut force_taskkill = Command::new("taskkill");
    force_taskkill.args(["/F", "/IM", APP_PROCESS_NAME]);
    let _ = hide_console_window(&mut force_taskkill).output();
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

pub fn reopen_codex_app_if_needed(app_was_running: bool, _codex_home: Option<&Path>) -> Vec<String> {
    if !app_was_running {
        return Vec::new();
    }

    let target = resolve_windows_app_target();

    let result = match target {
        AppLaunchTarget::WindowsStore(shell_target) => {
            let mut command = Command::new("explorer.exe");
            command.arg(shell_target);
            hide_console_window(&mut command).spawn()
        }
    };

    if let Err(error) = result {
        return vec![format!("Warning: failed to relaunch Codex: {error}")];
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{
        discover_real_codex_cli_path, load_install_state, resolve_windows_app_target,
        resolve_real_codex_cli, windows_store_shell_target, AppLaunchTarget, InstallState,
        WINDOWS_STORE_APP_ID,
    };
    use crate::windows::env_guard;
    use serde_json::to_string_pretty;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-process-{name}-{unique}"))
    }

    #[test]
    fn discover_real_codex_cli_path_prefers_cmd_and_skips_managed_shim() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("discover-real-cli");
        let managed_bin = codex_home.join("bin");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(managed_bin.join("codex.cmd"), "@echo off\r\n").unwrap();
        fs::write(npm_dir.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            std::env::join_paths([managed_bin.clone(), npm_dir.clone()]).unwrap(),
        );

        let resolved = discover_real_codex_cli_path(Some(&managed_bin.join("codex.cmd")));

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert_eq!(resolved, Some(npm_dir.join("codex.cmd")));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn resolve_real_codex_cli_repairs_legacy_extensionless_state() {
        let codex_home = temp_codex_home("repair-legacy-state");
        let runtime_dir = codex_home.join("account_backup").join("windows");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(npm_dir.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();
        let install_state = InstallState {
            real_codex_path: Some(npm_dir.join("codex").to_string_lossy().into_owned()),
            path_added_by_installer: false,
        };
        fs::write(
            runtime_dir.join("install_state.json"),
            format!("{}\n", to_string_pretty(&install_state).unwrap()),
        )
        .unwrap();

        let resolved = resolve_real_codex_cli(Some(&codex_home));
        let persisted_state = load_install_state(Some(&codex_home));

        assert_eq!(resolved, Some(npm_dir.join("codex.cmd")));
        assert_eq!(
            persisted_state.real_codex_path,
            Some(npm_dir.join("codex.cmd").to_string_lossy().into_owned())
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn resolve_windows_app_target_returns_windows_store_target() {
        let codex_home = temp_codex_home("windows-store-app-target");

        let target = resolve_windows_app_target();

        assert_eq!(
            target,
            AppLaunchTarget::WindowsStore(windows_store_shell_target(
                WINDOWS_STORE_APP_ID
            ))
        );
        let _ = fs::remove_dir_all(&codex_home);
    }
}
