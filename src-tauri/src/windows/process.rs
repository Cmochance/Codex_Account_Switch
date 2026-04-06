use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::errors::{AppError, AppResult};

use super::paths::{get_install_state_file, APP_NAME, APP_PROCESS_NAME};

#[derive(Debug, Default, Deserialize)]
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

pub fn detect_codex_app_path() -> Option<PathBuf> {
    candidate_app_paths()
        .into_iter()
        .find(|candidate| is_valid_app_path(candidate))
}

pub fn resolve_codex_app_path(codex_home: Option<&Path>) -> Option<PathBuf> {
    let state = load_install_state(codex_home);
    state
        .app_path
        .map(PathBuf::from)
        .filter(|path| is_valid_app_path(path))
        .or_else(detect_codex_app_path)
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
    let Some(resolved_path) = resolve_codex_app_path(codex_home) else {
        return Err(AppError::new(
            "APP_NOT_FOUND",
            "Codex desktop app path could not be resolved.",
        ));
    };

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

        return Ok(resolved_path.to_string_lossy().into_owned());
    }

    Command::new(&resolved_path).spawn().map_err(|error| {
        AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
    })?;

    Ok(resolved_path.to_string_lossy().into_owned())
}

fn resolve_real_codex_cli(codex_home: Option<&Path>) -> Option<PathBuf> {
    let state = load_install_state(codex_home);
    state
        .real_codex_path
        .map(PathBuf::from)
        .filter(|path| path.is_file())
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

    let Some(path) = resolve_codex_app_path(codex_home) else {
        return vec![format!(
            "Warning: could not relaunch {APP_NAME}. Start it manually if needed."
        )];
    };

    if let Err(error) = Command::new(path).spawn() {
        return vec![format!("Warning: failed to relaunch {APP_NAME}: {error}")];
    }

    Vec::new()
}
