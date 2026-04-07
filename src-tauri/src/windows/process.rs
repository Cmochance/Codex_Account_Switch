use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

use super::paths::{get_codex_home, get_install_state_file, APP_NAME, APP_PROCESS_NAME};

const WINDOWS_INVOKABLE_SUFFIXES: [&str; 4] = ["cmd", "exe", "bat", "com"];
const WINDOWS_STORE_APP_ID: &str = "OpenAI.Codex_2p2nqsd0c76g0!App";
const WINDOWS_STORE_SHELL_PREFIX: &str = r"shell:AppsFolder\";

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppLaunchTarget {
    Filesystem(PathBuf),
    WindowsStore(String),
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct InstallState {
    pub app_path: Option<String>,
    pub real_codex_path: Option<String>,
}

pub fn load_install_state(codex_home: Option<&Path>) -> InstallState {
    let path = get_install_state_file(codex_home);
    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return InstallState::default(),
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_install_state(codex_home: Option<&Path>, state: &InstallState) {
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

fn push_real_codex_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf, managed_shim_path: Option<&Path>) {
    let Some(resolved_path) = resolve_windows_invokable_path(&path) else {
        return;
    };
    if managed_shim_path.is_some_and(|managed_shim| paths_match(&resolved_path, managed_shim)) {
        return;
    }
    push_candidate(candidates, resolved_path);
}

fn managed_codex_shim_path(codex_home: Option<&Path>) -> PathBuf {
    codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("bin")
        .join("codex.cmd")
}

pub(super) fn discover_real_codex_cli_path(managed_shim_path: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        if let Ok(output) = Command::new("where").arg("codex").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().map(str::trim).filter(|value| !value.is_empty()) {
                    push_real_codex_candidate(&mut candidates, PathBuf::from(line), managed_shim_path);
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

fn push_candidate(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|existing| existing == &path) {
        candidates.push(path);
    }
}

fn push_codex_children(base: &Path, candidates: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(base) {
        Ok(value) => value,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if name.contains("codex") {
            push_candidate(candidates, path.join(APP_PROCESS_NAME));
            continue;
        }

        if name.contains("openai") {
            let nested = match fs::read_dir(&path) {
                Ok(value) => value,
                Err(_) => continue,
            };

            for child in nested.flatten() {
                let child_path = child.path();
                if !child_path.is_dir() {
                    continue;
                }

                let child_name = child_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();

                if child_name.contains("codex") {
                    push_candidate(candidates, child_path.join(APP_PROCESS_NAME));
                }
            }
        }
    }
}

fn registry_app_path_candidates() -> Vec<PathBuf> {
    if !cfg!(target_os = "windows") {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for key in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\App Paths\Codex.exe",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\Codex.exe",
    ] {
        let output = match Command::new("reg").args(["query", key, "/ve"]).output() {
            Ok(value) => value,
            Err(_) => continue,
        };

        if !output.status.success() {
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            for marker in ["REG_EXPAND_SZ", "REG_SZ"] {
                if let Some((_, value)) = line.split_once(marker) {
                    let candidate = value.trim();
                    if !candidate.is_empty() {
                        push_candidate(&mut candidates, PathBuf::from(candidate));
                    }
                }
            }
        }
    }

    candidates
}

fn windows_store_shell_target(app_id: &str) -> String {
    format!("{WINDOWS_STORE_SHELL_PREFIX}{app_id}")
}

fn is_valid_windows_store_app_id(app_id: &str) -> bool {
    let trimmed = app_id.trim();
    trimmed.starts_with("OpenAI.Codex_") && trimmed.ends_with("!App")
}

fn is_valid_windows_store_shell_target(target: &str) -> bool {
    target
        .strip_prefix(WINDOWS_STORE_SHELL_PREFIX)
        .is_some_and(is_valid_windows_store_app_id)
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
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let app_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_valid_windows_store_app_id(&app_id).then(|| windows_store_shell_target(&app_id))
}

pub fn candidate_app_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            let local_app_data = PathBuf::from(local_app_data);
            let programs_dir = local_app_data.join("Programs");

            push_candidate(
                &mut candidates,
                programs_dir.join(APP_NAME).join(APP_PROCESS_NAME),
            );
            push_candidate(
                &mut candidates,
                programs_dir.join("OpenAI").join(APP_NAME).join(APP_PROCESS_NAME),
            );
            push_candidate(
                &mut candidates,
                local_app_data.join(APP_NAME).join(APP_PROCESS_NAME),
            );
            push_candidate(
                &mut candidates,
                local_app_data.join("OpenAI").join(APP_NAME).join(APP_PROCESS_NAME),
            );

            push_codex_children(&programs_dir, &mut candidates);
        }

        if let Some(program_files) = env::var_os("ProgramFiles") {
            let program_files = PathBuf::from(program_files);
            push_candidate(
                &mut candidates,
                program_files.join(APP_NAME).join(APP_PROCESS_NAME),
            );
            push_candidate(
                &mut candidates,
                program_files.join("OpenAI").join(APP_NAME).join(APP_PROCESS_NAME),
            );
            push_codex_children(&program_files, &mut candidates);
        }

        for candidate in registry_app_path_candidates() {
            push_candidate(&mut candidates, candidate);
        }
    } else if cfg!(target_os = "macos") {
        push_candidate(
            &mut candidates,
            PathBuf::from("/Applications").join(format!("{APP_NAME}.app")),
        );

        if let Some(home) = env::var_os("HOME") {
            push_candidate(
                &mut candidates,
                PathBuf::from(home)
                    .join("Applications")
                    .join(format!("{APP_NAME}.app")),
            );
        }
    }

    candidates
}

fn is_valid_app_path(path: &Path) -> bool {
    if cfg!(target_os = "macos") {
        path.is_dir()
    } else {
        path.is_file()
    }
}

fn parse_app_launch_target(raw: &str) -> Option<AppLaunchTarget> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if cfg!(target_os = "windows") && is_valid_windows_store_shell_target(raw) {
        return Some(AppLaunchTarget::WindowsStore(raw.to_string()));
    }

    let path = PathBuf::from(raw);
    is_valid_app_path(&path).then_some(AppLaunchTarget::Filesystem(path))
}

pub fn detect_codex_app_target() -> Option<String> {
    if let Some(target) = detect_windows_store_app_target() {
        return Some(target);
    }
    if cfg!(target_os = "windows") {
        return Some(windows_store_shell_target(WINDOWS_STORE_APP_ID));
    }

    candidate_app_paths()
        .into_iter()
        .find(|candidate| is_valid_app_path(candidate))
        .map(|candidate| candidate.to_string_lossy().into_owned())
}

fn resolve_codex_app_target(codex_home: Option<&Path>) -> Option<AppLaunchTarget> {
    let mut state = load_install_state(codex_home);

    if let Some(target) = state.app_path.as_deref().and_then(parse_app_launch_target) {
        return Some(target);
    }

    let detected = detect_codex_app_target()?;
    if state.app_path.as_deref() != Some(detected.as_str()) {
        state.app_path = Some(detected.clone());
        save_install_state(codex_home, &state);
    }

    parse_app_launch_target(&detected)
}

pub fn is_codex_app_running() -> bool {
    if cfg!(target_os = "macos") {
        let script = format!("application \"{APP_NAME}\" is running");
        let output = match Command::new("osascript").args(["-e", &script]).output() {
            Ok(value) => value,
            Err(_) => return false,
        };

        return output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true";
    }

    let output = match Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {APP_PROCESS_NAME}"), "/FO", "CSV", "/NH"])
        .output()
    {
        Ok(value) => value,
        Err(_) => return false,
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    stdout.contains(&APP_PROCESS_NAME.to_ascii_lowercase())
}

pub fn open_or_activate_codex_app(codex_home: Option<&Path>) -> AppResult<String> {
    let Some(target) = resolve_codex_app_target(codex_home) else {
        return Err(AppError::new(
            "APP_NOT_FOUND",
            "Codex desktop app launch target could not be resolved.",
        ));
    };

    match target {
        AppLaunchTarget::Filesystem(resolved_path) => {
            if cfg!(target_os = "macos") {
                let script = format!("tell application \"{APP_NAME}\" to activate");
                let status = Command::new("osascript")
                    .args(["-e", &script])
                    .status()
                    .map_err(|error| AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}")))?;

                if !status.success() {
                    return Err(AppError::new(
                        "APP_OPEN_FAILED",
                        "Failed to activate Codex.",
                    ));
                }
            } else {
                Command::new(&resolved_path).spawn().map_err(|error| {
                    AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
                })?;
            }

            Ok(resolved_path.to_string_lossy().into_owned())
        }
        AppLaunchTarget::WindowsStore(shell_target) => {
            Command::new("explorer.exe")
                .arg(&shell_target)
                .spawn()
                .map_err(|error| AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}")))?;

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

    command.current_dir(codex_home);
    command.env("CODEX_HOME", codex_home);
    command
}

pub fn run_codex_login(codex_home: &Path) -> AppResult<()> {
    let output = build_login_command(codex_home)
        .output()
        .map_err(|error| AppError::new("LOGIN_COMMAND_FAILED", format!("Failed to start `codex login`: {error}")))?;

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

    let _ = Command::new("taskkill").args(["/IM", APP_PROCESS_NAME]).output();
    for _ in 0..20 {
        if !is_codex_app_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(200));
    }

    let _ = Command::new("taskkill")
        .args(["/F", "/IM", APP_PROCESS_NAME])
        .output();
    for _ in 0..10 {
        if !is_codex_app_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(200));
    }

    Err(AppError::new(
        "APP_EXIT_FAILED",
        format!("{APP_NAME} did not exit cleanly. Close it manually and retry."),
    ))
}

pub fn reopen_codex_app_if_needed(app_was_running: bool, codex_home: Option<&Path>) -> Vec<String> {
    if !app_was_running {
        return Vec::new();
    }

    let Some(target) = resolve_codex_app_target(codex_home) else {
        return vec![format!(
            "Warning: could not relaunch {APP_NAME}. Start it manually if needed."
        )];
    };

    let result = match target {
        AppLaunchTarget::Filesystem(path) => Command::new(path).spawn(),
        AppLaunchTarget::WindowsStore(shell_target) => Command::new("explorer.exe").arg(shell_target).spawn(),
    };

    if let Err(error) = result {
        return vec![format!("Warning: failed to relaunch {APP_NAME}: {error}")];
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{
        discover_real_codex_cli_path, load_install_state, resolve_codex_app_target, resolve_real_codex_cli,
        windows_store_shell_target, AppLaunchTarget, WINDOWS_STORE_APP_ID,
    };
    use crate::windows::env_lock;
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
        let _guard = env_lock().lock().unwrap();
        let codex_home = temp_codex_home("discover-real-cli");
        let managed_bin = codex_home.join("bin");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(managed_bin.join("codex.cmd"), "@echo off\r\n").unwrap();
        fs::write(npm_dir.join("codex"), "#!/bin/sh\n").unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", std::env::join_paths([managed_bin.clone(), npm_dir.clone()]).unwrap());

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
        fs::write(
            runtime_dir.join("install_state.json"),
            format!(
                "{{\"real_codex_path\": \"{}\"}}\n",
                npm_dir.join("codex").to_string_lossy()
            ),
        )
        .unwrap();

        let resolved = resolve_real_codex_cli(Some(&codex_home));
        let state = load_install_state(Some(&codex_home));

        assert_eq!(resolved, Some(npm_dir.join("codex.cmd")));
        assert_eq!(state.real_codex_path, Some(npm_dir.join("codex.cmd").to_string_lossy().into_owned()));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn resolve_codex_app_target_accepts_windows_store_shell_target_from_state() {
        let codex_home = temp_codex_home("windows-store-app-target");
        let runtime_dir = codex_home.join("account_backup").join("windows");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::write(
            runtime_dir.join("install_state.json"),
            format!(
                "{{\"app_path\": \"{}\"}}\n",
                windows_store_shell_target(WINDOWS_STORE_APP_ID)
            ),
        )
        .unwrap();

        let target = resolve_codex_app_target(Some(&codex_home));

        assert_eq!(
            target,
            Some(AppLaunchTarget::WindowsStore(
                windows_store_shell_target(WINDOWS_STORE_APP_ID)
            ))
        );
        let _ = fs::remove_dir_all(&codex_home);
    }
}
