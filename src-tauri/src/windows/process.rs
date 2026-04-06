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

pub fn candidate_app_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local_app_data)
                .join("Programs")
                .join(APP_NAME)
                .join(APP_PROCESS_NAME),
        );
    }

    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join(APP_NAME).join(APP_PROCESS_NAME));
    }

    candidates
}

pub fn detect_codex_app_path() -> Option<PathBuf> {
    candidate_app_paths()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

pub fn resolve_codex_app_path(codex_home: Option<&Path>) -> Option<PathBuf> {
    let state = load_install_state(codex_home);
    state
        .app_path
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(detect_codex_app_path)
}

pub fn is_codex_app_running() -> bool {
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
