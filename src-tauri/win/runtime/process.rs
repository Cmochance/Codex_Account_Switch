use std::env;
use std::fs;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use crate::errors::{AppError, AppResult};
use crate::platform::hooks::PlatformHooks;
use crate::shared::codex_app_server::{fetch_account_snapshot, AppServerSnapshot};
use crate::models::CodexCliCandidate;
use crate::shared::codex_cli_path::CodexPathResolver;
pub use crate::shared::codex_cli_path::{InstallState, RealCodexPathSource};
use crate::shared::login_cancel::wait_for_login_or_cancel;

use super::paths::{get_codex_home, get_install_state_file};

const APP_PROCESS_NAME: &str = "Codex.exe";
const WINDOWS_INVOKABLE_SUFFIXES: [&str; 4] = ["cmd", "exe", "bat", "com"];
const WINDOWS_APPS_PATH_SEGMENT: &str = r"\microsoft\windowsapps\";
const WINDOWS_STORE_APP_ID: &str = "OpenAI.Codex_2p2nqsd0c76g0!App";
const WINDOWS_STORE_SHELL_PREFIX: &str = r"shell:AppsFolder\";
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
static WINDOWS_APP_TARGET_CACHE: OnceLock<String> = OnceLock::new();
static WINDOWS_PLATFORM_HOOKS: WindowsPlatformHooks = WindowsPlatformHooks;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppLaunchTarget {
    WindowsStore(String),
}

pub struct WindowsPlatformHooks;

pub fn platform_hooks() -> &'static dyn PlatformHooks {
    &WINDOWS_PLATFORM_HOOKS
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
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn paths_match(left: &Path, right: &Path) -> bool {
    normalize_windows_path(left) == normalize_windows_path(right)
}

fn is_windows_apps_alias_path(path: &Path) -> bool {
    normalize_windows_path(path).contains(WINDOWS_APPS_PATH_SEGMENT)
}

pub(super) fn is_acceptable_real_codex_cli_path(
    path: &Path,
    managed_shim_path: Option<&Path>,
) -> bool {
    if managed_shim_path.is_some_and(|managed_shim| paths_match(path, managed_shim)) {
        return false;
    }

    !is_windows_apps_alias_path(path)
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
    if !is_acceptable_real_codex_cli_path(&resolved_path, managed_shim_path) {
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
            hide_console_window(&mut command).spawn().map_err(|error| {
                AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
            })?;

            Ok(shell_target)
        }
    }
}

fn persist_real_codex_path(
    codex_home: Option<&Path>,
    state: &mut InstallState,
    path: Option<&Path>,
) {
    let next_path = path.map(|path| path.to_string_lossy().into_owned());
    if state.real_codex_path != next_path {
        state.real_codex_path = next_path;
        save_install_state(codex_home, state);
    }
}

pub(super) fn resolve_real_codex_cli_with_source(
    codex_home: Option<&Path>,
) -> Option<(PathBuf, RealCodexPathSource)> {
    let managed_shim_path = managed_codex_shim_path(codex_home);
    let state = load_install_state(codex_home);

    // User override wins. If it doesn't pass validation any more (file
    // moved, AV pruned the .cmd, etc.) we silently fall through to the
    // normal discovery chain so the user isn't permanently stuck — the
    // override stays persisted so a stable retry will pick it up if the
    // file reappears.
    if let Some(raw_user_path) = state.user_codex_path.as_ref().map(PathBuf::from) {
        if let Some(resolved_path) = resolve_windows_invokable_path(&raw_user_path)
            .filter(|path| is_acceptable_real_codex_cli_path(path, Some(&managed_shim_path)))
        {
            return Some((resolved_path, RealCodexPathSource::UserOverride));
        }
    }

    let mut state = state;
    if let Some(raw_path) = state.real_codex_path.as_ref().map(PathBuf::from) {
        if let Some(resolved_path) = resolve_windows_invokable_path(&raw_path)
            .filter(|path| is_acceptable_real_codex_cli_path(path, Some(&managed_shim_path)))
        {
            persist_real_codex_path(codex_home, &mut state, Some(&resolved_path));
            return Some((resolved_path, RealCodexPathSource::InstallState));
        }
    }

    let discovered_path = discover_real_codex_cli_path(Some(&managed_shim_path));
    if let Some(path) = discovered_path.as_deref() {
        persist_real_codex_path(codex_home, &mut state, Some(path));
    } else if state.real_codex_path.is_some() {
        persist_real_codex_path(codex_home, &mut state, None);
    }
    discovered_path.map(|path| (path, RealCodexPathSource::Discovery))
}

fn resolve_real_codex_cli(codex_home: Option<&Path>) -> Option<PathBuf> {
    resolve_real_codex_cli_with_source(codex_home).map(|(path, _)| path)
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
    let status = hide_console_window(&mut command)
        .status()
        .map_err(|error| {
            AppError::new(
                "REAL_CODEX_LAUNCH_FAILED",
                format!("Failed to launch real Codex CLI: {error}"),
            )
        })?;

    Ok(status.code().unwrap_or(1))
}

fn build_app_server_command(real_codex_path: &Path, runtime_codex_home: &Path) -> Command {
    let mut command = Command::new(real_codex_path);
    // `codex app-server` is the upstream control-plane subcommand; it
    // takes no sandbox/approval flags (those bind only to the TUI). See
    // `openai/codex` `codex-rs/cli/src/main.rs` for the subcommand
    // wiring.
    command.arg("app-server");
    hide_console_window(&mut command);
    command.current_dir(runtime_codex_home);
    command.env("CODEX_HOME", runtime_codex_home);
    command
}

/// Build the `codex login` command using a resolved real-codex path.
/// Anchoring on `cli_codex_home` (the live `~/.codex`) for resolution
/// keeps the managed-shim filter correct even when `runtime_codex_home`
/// is a sandboxed sibling. `runtime_codex_home` is what the spawned
/// process sees as `CODEX_HOME` and is where it will write `auth.json`.
///
/// Callers must resolve the path beforehand and surface
/// `REAL_CODEX_NOT_FOUND` to the user instead of falling back to
/// `cmd /C codex login` — that fallback only ever produced a Chinese
/// "command not found" message in the OEM codepage that
/// `from_utf8_lossy` then mangled into mojibake.
fn build_login_command(real_codex_path: &Path, runtime_codex_home: &Path) -> Command {
    let mut command = Command::new(real_codex_path);
    command.arg("login");
    hide_console_window(&mut command);
    command.current_dir(runtime_codex_home);
    command.env("CODEX_HOME", runtime_codex_home);
    command
}

pub fn fetch_account_via_app_server(
    cli_codex_home: &Path,
    runtime_codex_home: &Path,
) -> AppResult<AppServerSnapshot> {
    let Some(real_codex_path) = resolve_real_codex_cli(Some(cli_codex_home)) else {
        return Err(AppError::new(
            "REAL_CODEX_NOT_FOUND",
            "Real Codex CLI path not found. Run `codex_switch_cli.exe install` first.",
        ));
    };

    let command = build_app_server_command(&real_codex_path, runtime_codex_home);
    fetch_account_snapshot(command)
}

pub fn run_codex_login(cli_codex_home: &Path, runtime_codex_home: &Path) -> AppResult<()> {
    let Some(real_codex_path) = resolve_real_codex_cli(Some(cli_codex_home)) else {
        return Err(AppError::new(
            "REAL_CODEX_NOT_FOUND",
            "Real Codex CLI path not found. Set the codex CLI location in the dashboard before logging in.",
        ));
    };

    // Pipe stdio so wait_with_output() captures stderr/stdout the same
    // way the previous `.output()` call did — we surface those bytes in
    // the LOGIN_FAILED toast when codex login itself errors out.
    let child = build_login_command(&real_codex_path, runtime_codex_home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::new(
                "LOGIN_COMMAND_FAILED",
                format!("Failed to start `codex login`: {error}"),
            )
        })?;
    let output = wait_for_login_or_cancel(child)?;

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

/// Validate a user-provided codex CLI path. Resolves Windows extensions
/// (`.cmd` / `.exe` / etc.) so a user can paste either `C:\...\codex` or
/// `C:\...\codex.cmd`. Rejects the managed shim because pointing the
/// override at our own shim creates an infinite indirection.
pub(super) fn validate_user_codex_cli_path(
    codex_home: Option<&Path>,
    raw_input: &str,
) -> AppResult<PathBuf> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(
            "CODEX_CLI_PATH_EMPTY",
            "Please provide the full path to the codex CLI binary.",
        ));
    }
    let candidate = PathBuf::from(trimmed);
    let resolved = resolve_windows_invokable_path(&candidate).ok_or_else(|| {
        AppError::new(
            "CODEX_CLI_PATH_INVALID",
            format!("No invokable file found at {}.", candidate.display()),
        )
    })?;

    let managed_shim_path = managed_codex_shim_path(codex_home);
    if !is_acceptable_real_codex_cli_path(&resolved, Some(&managed_shim_path)) {
        return Err(AppError::new(
            "CODEX_CLI_PATH_REJECTED",
            "That path is the codex_switch managed shim or a Windows Apps alias; pick the real codex CLI binary instead.",
        ));
    }

    Ok(resolved)
}

/// Persist a user override for the real codex CLI path. Returns the
/// canonicalized path that was saved.
pub fn set_user_codex_cli_path(
    codex_home: Option<&Path>,
    raw_input: &str,
) -> AppResult<PathBuf> {
    let resolved = validate_user_codex_cli_path(codex_home, raw_input)?;
    let mut state = load_install_state(codex_home);
    let next = Some(resolved.to_string_lossy().into_owned());
    if state.user_codex_path != next {
        state.user_codex_path = next;
        save_install_state(codex_home, &state);
    }
    Ok(resolved)
}

/// Clear the user override and let auto-discovery take over again.
pub fn clear_user_codex_cli_path(codex_home: Option<&Path>) {
    let mut state = load_install_state(codex_home);
    if state.user_codex_path.is_some() {
        state.user_codex_path = None;
        save_install_state(codex_home, &state);
    }
}

/// Resolver impl that delegates to the per-platform helpers above. The
/// shared `codex_cli_path` module talks to this via the trait so the
/// Tauri command bridge stays OS-agnostic.
pub struct WindowsCodexPathResolver;

pub static WINDOWS_CODEX_PATH_RESOLVER: WindowsCodexPathResolver = WindowsCodexPathResolver;

impl CodexPathResolver for WindowsCodexPathResolver {
    fn resolve_with_source(
        &self,
        codex_home: &Path,
    ) -> Option<(PathBuf, RealCodexPathSource)> {
        resolve_real_codex_cli_with_source(Some(codex_home))
    }

    fn set_user_path(&self, codex_home: &Path, raw_input: &str) -> AppResult<PathBuf> {
        set_user_codex_cli_path(Some(codex_home), raw_input)
    }

    fn clear_user_path(&self, codex_home: &Path) {
        clear_user_codex_cli_path(Some(codex_home));
    }

    fn suggested_paths(&self, codex_home: &Path) -> Vec<PathBuf> {
        suggested_codex_cli_paths(Some(codex_home))
    }

    fn redetect_runnable_paths(&self, codex_home: &Path) -> Vec<CodexCliCandidate> {
        redetect_runnable_codex_cli_paths(Some(codex_home))
    }
}

/// Return common codex CLI install locations on Windows that actually
/// exist on disk right now. Used to seed clickable hints in the
/// "Codex 路径" dialog so users don't have to hunt manually.
pub fn suggested_codex_cli_paths(codex_home: Option<&Path>) -> Vec<PathBuf> {
    let mut suggestions: Vec<PathBuf> = Vec::new();
    let managed_shim = managed_codex_shim_path(codex_home);
    let mut push = |path: PathBuf| {
        if let Some(resolved) = resolve_windows_invokable_path(&path) {
            if is_acceptable_real_codex_cli_path(&resolved, Some(&managed_shim))
                && !suggestions.iter().any(|existing| existing == &resolved)
            {
                suggestions.push(resolved);
            }
        }
    };

    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(local_app_data);
        push(base.join("Programs").join("codex").join("codex.exe"));
        push(base.join("Programs").join("codex").join("codex.cmd"));
        push(base.join("Programs").join("codex").join("bin").join("codex.cmd"));
    }
    if let Some(app_data) = env::var_os("APPDATA") {
        let npm_base = PathBuf::from(app_data).join("npm");
        push(npm_base.join("codex.cmd"));
        push(npm_base.join("codex"));
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        let base = PathBuf::from(program_files).join("codex");
        push(base.join("codex.exe"));
        push(base.join("codex.cmd"));
        push(base.join("bin").join("codex.cmd"));
    }
    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        let base = PathBuf::from(program_files_x86).join("codex");
        push(base.join("codex.exe"));
        push(base.join("codex.cmd"));
    }

    let mut where_command = Command::new("where");
    where_command.arg("codex");
    if let Ok(output) = hide_console_window(&mut where_command).output() {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push(PathBuf::from(line));
            }
        }
    }

    suggestions
}

/// How long a single `codex --version` probe may run before we kill it
/// and treat the candidate as unusable. A little more generous than
/// macOS: a Windows `.cmd` shim plus npm wrapper has a slower cold
/// start, but a healthy codex still answers well under this.
const RUNNABLE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound on how many candidates the auto-detect scan will probe.
/// Each probe spawns a child (up to `RUNNABLE_PROBE_TIMEOUT`), so without
/// a cap a machine with many `where codex` hits could stall the scan.
/// Realistic machines have 1-3 candidates.
const MAX_PROBE_CANDIDATES: usize = 12;

/// Probe whether `path` is a runnable codex CLI and capture its version.
/// `Some(version)` (possibly empty) means it's a file that ran and exited
/// 0; `None` means not-a-file, couldn't spawn, exited non-zero, or timed
/// out. The failure is logged so a broken install leaves a diagnostic
/// trail instead of looking identical to "not found".
fn probe_codex_version(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    let mut command = Command::new(path);
    command.arg("--version");
    hide_console_window(&mut command);
    let result =
        crate::shared::codex_cli_path::probe_version_with_timeout(command, RUNNABLE_PROBE_TIMEOUT);
    if result.is_none() {
        eprintln!(
            "codex probe: {} is not a runnable codex (spawn / non-zero exit / timeout)",
            path.display()
        );
    }
    result
}

/// Force a fresh scan for the Settings auto-detect button, keeping only
/// candidates that pass the runnable probe. Reuses
/// `suggested_codex_cli_paths`, which already resolves Windows
/// extensions, filters the managed shim / Windows Apps aliases, and
/// folds in `where codex` (every PATH match) — so it is the full
/// candidate set. Ignores the cached/override path so a wrong saved
/// path can be corrected.
pub fn redetect_runnable_codex_cli_paths(codex_home: Option<&Path>) -> Vec<CodexCliCandidate> {
    suggested_codex_cli_paths(codex_home)
        .into_iter()
        .take(MAX_PROBE_CANDIDATES)
        .filter_map(|path| {
            probe_codex_version(&path).map(|version| CodexCliCandidate {
                path: path.to_string_lossy().into_owned(),
                version: (!version.is_empty()).then_some(version),
            })
        })
        .collect()
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

pub fn reopen_codex_app_if_needed(
    app_was_running: bool,
    _codex_home: Option<&Path>,
) -> Vec<String> {
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

impl PlatformHooks for WindowsPlatformHooks {
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

    fn fetch_account_via_app_server(
        &self,
        cli_codex_home: &Path,
        runtime_codex_home: &Path,
    ) -> AppResult<AppServerSnapshot> {
        fetch_account_via_app_server(cli_codex_home, runtime_codex_home)
    }

    fn sync_root_openai_base_url_for_profile(
        &self,
        profile_name: &str,
        codex_home: Option<&Path>,
    ) -> AppResult<()> {
        crate::shared::config::sync_root_openai_base_url_from_profile_metadata(
            profile_name,
            codex_home,
        )
    }

    fn sync_on_window_close(&self) -> AppResult<()> {
        crate::windows::bootstrap::sync_root_state_to_current_profile(None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_app_server_command, discover_real_codex_cli_path, is_acceptable_real_codex_cli_path,
        load_install_state, probe_codex_version, resolve_real_codex_cli,
        resolve_windows_app_target, windows_store_shell_target, AppLaunchTarget, InstallState,
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

    // Runs on the Linux `cargo test --lib` job (the win module compiles on
    // non-macOS): a `#!/bin/sh` candidate is spawnable there, and
    // `hide_console_window` is a no-op off Windows. Pins the three
    // behaviours auto-detect depends on: a non-file is rejected without
    // spawning, a binary that runs but exits non-zero is rejected (broken
    // install), and only a zero-exit binary is accepted.
    #[cfg(unix)]
    #[test]
    fn probe_codex_version_rejects_missing_and_failing_captures_zero_exit() {
        use std::os::unix::fs::PermissionsExt;

        let codex_home = temp_codex_home("probe-runnable");
        fs::create_dir_all(&codex_home).unwrap();

        // (a) non-file path → None, never spawned.
        assert_eq!(probe_codex_version(&codex_home.join("does-not-exist")), None);

        let set_exec = |path: &std::path::Path| {
            let mut perm = fs::metadata(path).unwrap().permissions();
            perm.set_mode(0o755);
            fs::set_permissions(path, perm).unwrap();
        };

        // (b) exists + runs but exits non-zero → None (broken install).
        let bad = codex_home.join("bad-codex");
        fs::write(&bad, "#!/bin/sh\nexit 3\n").unwrap();
        set_exec(&bad);
        assert_eq!(probe_codex_version(&bad), None);

        // (c) exists + exits zero, prints a version → Some(version).
        let good = codex_home.join("good-codex");
        fs::write(&good, "#!/bin/sh\necho codex-cli 0.133.0\n").unwrap();
        set_exec(&good);
        assert_eq!(
            probe_codex_version(&good).as_deref(),
            Some("codex-cli 0.133.0")
        );

        let _ = fs::remove_dir_all(&codex_home);
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
    fn discover_real_codex_cli_path_skips_windows_apps_aliases() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("discover-real-cli-windowsapps");
        let managed_bin = codex_home.join("bin");
        let alias_dir = codex_home
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WindowsApps");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(&alias_dir).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(managed_bin.join("codex.cmd"), "@echo off\r\n").unwrap();
        fs::write(alias_dir.join("codex.exe"), "alias").unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var(
            "PATH",
            std::env::join_paths([alias_dir.clone(), npm_dir.clone()]).unwrap(),
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
            user_codex_path: None,
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
    fn resolve_real_codex_cli_skips_cached_windows_apps_alias_and_repairs_state() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("repair-windowsapps-state");
        let runtime_dir = codex_home.join("account_backup").join("windows");
        let alias_dir = codex_home
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WindowsApps");
        let npm_dir = codex_home.join("npm");
        fs::create_dir_all(&runtime_dir).unwrap();
        fs::create_dir_all(&alias_dir).unwrap();
        fs::create_dir_all(&npm_dir).unwrap();
        fs::write(alias_dir.join("codex.exe"), "alias").unwrap();
        fs::write(npm_dir.join("codex.cmd"), "@echo off\r\n").unwrap();
        let install_state = InstallState {
            real_codex_path: Some(alias_dir.join("codex.exe").to_string_lossy().into_owned()),
            path_added_by_installer: false,
            user_codex_path: None,
        };
        fs::write(
            runtime_dir.join("install_state.json"),
            format!("{}\n", to_string_pretty(&install_state).unwrap()),
        )
        .unwrap();

        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", &npm_dir);

        let resolved = resolve_real_codex_cli(Some(&codex_home));
        let persisted_state = load_install_state(Some(&codex_home));

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert_eq!(resolved, Some(npm_dir.join("codex.cmd")));
        assert_eq!(
            persisted_state.real_codex_path,
            Some(npm_dir.join("codex.cmd").to_string_lossy().into_owned())
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn is_acceptable_real_codex_cli_path_rejects_windows_apps_aliases() {
        let alias_path =
            PathBuf::from(r"C:\Users\demo\AppData\Local\Microsoft\WindowsApps\codex.exe");

        assert!(!is_acceptable_real_codex_cli_path(&alias_path, None));
    }

    #[test]
    fn build_app_server_command_targets_runtime_codex_home() {
        let runtime_codex_home = temp_codex_home("app-server-command");
        let real_codex_path = runtime_codex_home.join("bin").join("codex.exe");
        let command = build_app_server_command(&real_codex_path, &runtime_codex_home);

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

        assert_eq!(command.get_program(), real_codex_path.as_os_str());
        assert_eq!(args, vec!["app-server".to_string()]);
        assert_eq!(
            command.get_current_dir(),
            Some(runtime_codex_home.as_path())
        );
        assert!(envs.iter().any(|(key, value)| {
            key == "CODEX_HOME"
                && value.as_deref() == Some(runtime_codex_home.to_string_lossy().as_ref())
        }));
    }

    #[test]
    fn resolve_windows_app_target_returns_windows_store_target() {
        let codex_home = temp_codex_home("windows-store-app-target");

        let target = resolve_windows_app_target();

        assert_eq!(
            target,
            AppLaunchTarget::WindowsStore(windows_store_shell_target(WINDOWS_STORE_APP_ID))
        );
        let _ = fs::remove_dir_all(&codex_home);
    }
}
