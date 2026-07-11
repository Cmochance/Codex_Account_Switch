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
use crate::models::CodexCliCandidate;
use crate::platform::hooks::PlatformHooks;
use crate::shared::codex_app_server::{fetch_account_snapshot, AppServerSnapshot};
use crate::shared::codex_cli_path::CodexPathResolver;
pub use crate::shared::codex_cli_path::{InstallState, RealCodexPathSource};
use crate::shared::login_cancel::wait_for_login_or_cancel;

use super::paths::{get_codex_home, get_install_state_file};

/// Post-merge desktop host process name. OpenAI folded Codex into the
/// ChatGPT desktop app on Windows on 2026-07-09 (executable
/// `ChatGPT.exe`, installed under `%LOCALAPPDATA%\Programs\ChatGPT\` or
/// inside the MSIX package's `app\` dir); exact-name checks against
/// `Codex.exe` no longer see the running UI.
const APP_PROCESS_NAME_CHATGPT: &str = "ChatGPT.exe";
/// Historical standalone process name, still shipped by
/// not-yet-updated `Codex.exe` installs.
const APP_PROCESS_NAME_CODEX: &str = "Codex.exe";
/// Lower-cased path marker of the MSIX package family shared by the
/// pre-merge Codex app and the merged ChatGPT host. The Store package
/// "updates as usual" into the merged app and a package family name
/// never changes across updates, so `OpenAI.Codex_2p2nqsd0c76g0` stays
/// (the prefix also covers the `OpenAI.CodexBeta_…` channel).
const WINDOWSAPPS_CODEX_MARKER: &str = r"\windowsapps\openai.codex";
/// Lower-cased path marker of ChatGPT Classic — the pre-merge consumer
/// chat app (`OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0`). Its executable is
/// also named `ChatGPT.exe`, but it is NOT a codex host and must never
/// be counted as running or killed.
const WINDOWSAPPS_CLASSIC_CHATGPT_MARKER: &str = r"\windowsapps\openai.chatgpt-desktop_";
const WINDOWS_INVOKABLE_SUFFIXES: [&str; 4] = ["cmd", "exe", "bat", "com"];
const WINDOWS_APPS_PATH_SEGMENT: &str = r"\microsoft\windowsapps\";
const WINDOWS_STORE_APP_ID: &str = "OpenAI.Codex_2p2nqsd0c76g0!App";
const WINDOWS_STORE_SHELL_PREFIX: &str = r"shell:AppsFolder\";
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
static WINDOWS_APP_TARGET_CACHE: OnceLock<Option<String>> = OnceLock::new();
static WINDOWS_PLATFORM_HOOKS: WindowsPlatformHooks = WindowsPlatformHooks;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppLaunchTarget {
    WindowsStore(String),
    Executable(PathBuf),
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

    // Desktop-host bundled CLI as the last tier — mirrors the macOS
    // app-bundle fallback so a machine whose only codex is the one
    // inside the (merged) desktop app still resolves.
    for candidate in desktop_host_bundled_cli_candidates() {
        push_real_codex_candidate(&mut candidates, candidate, managed_shim_path);
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

/// Detected Store shell target, cached for the process lifetime.
/// `None` when the package is absent or PowerShell is unavailable.
fn detected_windows_store_target() -> Option<String> {
    WINDOWS_APP_TARGET_CACHE
        .get_or_init(detect_windows_store_app_target)
        .clone()
}

/// InstallLocation of the (merged) codex host's MSIX package, cached
/// for the process lifetime. `None` when the Store package is absent
/// (non-Store install) or PowerShell is unavailable.
static WINDOWS_STORE_INSTALL_LOCATION_CACHE: OnceLock<Option<String>> = OnceLock::new();

fn windows_store_install_location() -> Option<String> {
    WINDOWS_STORE_INSTALL_LOCATION_CACHE
        .get_or_init(|| {
            if !cfg!(target_os = "windows") {
                return None;
            }
            let mut command = Command::new("powershell");
            command.args([
                "-NoProfile",
                "-Command",
                "Get-AppxPackage -Name 'OpenAI.Codex' -ErrorAction SilentlyContinue \
                 | Select-Object -First 1 -ExpandProperty InstallLocation",
            ]);
            let output = hide_console_window(&mut command).output().ok()?;
            if !output.status.success() {
                return None;
            }
            let location = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!location.is_empty()).then_some(location)
        })
        .clone()
}

/// Non-Store install locations of the desktop host executable, current
/// host first. Each candidate must embed the codex CLI
/// (`resources\codex.exe`) to qualify — a ChatGPT.exe without it could
/// be an unrelated install and must not be launched as "Codex".
fn non_store_host_executables() -> Vec<PathBuf> {
    let Some(local_app_data) = env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    let programs = PathBuf::from(local_app_data).join("Programs");
    [
        programs.join("ChatGPT").join(APP_PROCESS_NAME_CHATGPT),
        programs.join("Codex").join(APP_PROCESS_NAME_CODEX),
    ]
    .into_iter()
    .filter(|exe| exe.is_file() && host_dir_embeds_codex_cli(exe))
    .collect()
}

/// Codex CLI copies bundled inside the desktop host installs — the
/// Windows analog of the macOS `Contents/Resources/codex` fallback.
/// Order is discovery preference: the MSIX package
/// (`<InstallLocation>\app\resources\codex.exe`), then the non-Store
/// ChatGPT install, then the legacy non-Store Codex install.
fn desktop_host_bundled_cli_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(install_location) = windows_store_install_location() {
        candidates.push(
            PathBuf::from(install_location)
                .join("app")
                .join("resources")
                .join("codex.exe"),
        );
    }
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let programs = PathBuf::from(local_app_data).join("Programs");
        candidates.push(programs.join("ChatGPT").join("resources").join("codex.exe"));
        candidates.push(programs.join("Codex").join("resources").join("codex.exe"));
    }
    candidates
}

fn resolve_windows_app_target() -> AppLaunchTarget {
    // Prefer the Store package when it is actually installed (the
    // merged app keeps the OpenAI.Codex package family). Otherwise fall
    // back to a qualified non-Store executable; the hardcoded shell
    // target stays as the historical last resort.
    if let Some(target) = detected_windows_store_target() {
        return AppLaunchTarget::WindowsStore(target);
    }
    if let Some(executable) = non_store_host_executables().into_iter().next() {
        return AppLaunchTarget::Executable(executable);
    }
    AppLaunchTarget::WindowsStore(windows_store_shell_target(WINDOWS_STORE_APP_ID))
}

/// Process names that may own the Codex desktop UI after the ChatGPT
/// merge. Order only matters for probe latency (current host first).
fn desktop_app_process_names() -> &'static [&'static str] {
    &[APP_PROCESS_NAME_CHATGPT, APP_PROCESS_NAME_CODEX]
}

/// Install identity of a name-matched PID. `Other` is the only verdict
/// that positively rules a process out (ChatGPT Classic). `Unknown`
/// (no executable path, unrecognized location) is treated
/// asymmetrically: it COUNTS for is-running — proceeding with a
/// possibly-live host risks account cross-contamination, so the switch
/// must abort instead — but it is NEVER signalled (we do not kill what
/// we cannot identify). Mirrors the macOS classification in
/// `mac/runtime/process.rs`.
#[derive(Debug, PartialEq)]
enum HostIdentity {
    Ours,
    Other,
    Unknown,
}

/// Classify a host process by its executable path. The bare name match
/// is not enough: ChatGPT Classic ships an executable that is also
/// named `ChatGPT.exe`.
fn classify_host_executable(executable_path: &str) -> HostIdentity {
    let normalized = executable_path.replace('/', "\\").to_ascii_lowercase();
    if normalized.trim().is_empty() {
        return HostIdentity::Unknown;
    }
    if normalized.contains(WINDOWSAPPS_CODEX_MARKER) {
        return HostIdentity::Ours;
    }
    if normalized.contains(WINDOWSAPPS_CLASSIC_CHATGPT_MARKER) {
        return HostIdentity::Other;
    }
    // Non-Store installs (%LOCALAPPDATA%\Programs\ChatGPT, portable
    // copies): the codex host embeds the codex CLI next to its
    // executable (`resources\codex.exe` — the Windows analog of the
    // macOS `Contents/Resources/codex` qualifier). Classic has no such
    // file, but its absence alone cannot rule a host out (probe
    // failures), so it stays Unknown rather than Other.
    if host_dir_embeds_codex_cli(Path::new(executable_path)) {
        return HostIdentity::Ours;
    }
    HostIdentity::Unknown
}

fn host_dir_embeds_codex_cli(executable: &Path) -> bool {
    executable
        .parent()
        .map(|dir| dir.join("resources").join("codex.exe").is_file())
        .unwrap_or(false)
}

/// Parse the `<pid>|<executable path>` lines emitted by the PowerShell
/// CIM query in [`desktop_app_pid_classifications`]. Extracted for unit
/// tests (the query itself needs a live Windows host).
fn parse_pid_classification_lines(stdout: &str) -> Vec<(u32, HostIdentity)> {
    stdout
        .lines()
        .filter_map(|line| {
            let (pid, path) = line.trim().split_once('|')?;
            let pid = pid.trim().parse::<u32>().ok()?;
            Some((pid, classify_host_executable(path)))
        })
        .collect()
}

/// Name-matched desktop host PIDs with their install classification,
/// via one PowerShell CIM query (~150-400ms — only paid after the cheap
/// tasklist pre-check matched a name). Returns an empty list when
/// PowerShell itself is unavailable; callers must treat that as
/// "unattributable", not as "not running".
fn desktop_app_pid_classifications() -> Vec<(u32, HostIdentity)> {
    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"Name='{APP_PROCESS_NAME_CHATGPT}' OR Name='{APP_PROCESS_NAME_CODEX}'\" \
         | ForEach-Object {{ \"$($_.ProcessId)|$($_.ExecutablePath)\" }}"
    );
    let mut command = Command::new("powershell");
    command.args(["-NoProfile", "-Command", &script]);
    let output = match hide_console_window(&mut command).output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    parse_pid_classification_lines(&String::from_utf8_lossy(&output.stdout))
}

/// Cheap pre-check: does any process with a known host name exist at
/// all? Avoids paying the PowerShell classification cost on every
/// 200ms quit-wait poll when nothing is running.
fn any_host_process_name_running() -> bool {
    desktop_app_process_names().iter().any(|name| {
        let mut command = Command::new("tasklist");
        command.args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"]);
        let output = match hide_console_window(&mut command).output() {
            Ok(value) => value,
            Err(_) => return false,
        };
        String::from_utf8_lossy(&output.stdout)
            .to_ascii_lowercase()
            .contains(&name.to_ascii_lowercase())
    })
}

pub fn is_codex_app_running() -> bool {
    if !any_host_process_name_running() {
        return false;
    }
    let classified = desktop_app_pid_classifications();
    if classified.is_empty() {
        // A host name is running but PowerShell could not attribute it:
        // count as running so the switch aborts instead of proceeding
        // over a possibly-live host (quit will refuse to signal it).
        return true;
    }
    classified
        .iter()
        .any(|(_, identity)| *identity != HostIdentity::Other)
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
        AppLaunchTarget::Executable(executable) => {
            let mut command = Command::new(&executable);
            hide_console_window(&mut command).spawn().map_err(|error| {
                AppError::new("APP_OPEN_FAILED", format!("Failed to open Codex: {error}"))
            })?;

            Ok(executable.to_string_lossy().into_owned())
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
pub fn set_user_codex_cli_path(codex_home: Option<&Path>, raw_input: &str) -> AppResult<PathBuf> {
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
    fn resolve_with_source(&self, codex_home: &Path) -> Option<(PathBuf, RealCodexPathSource)> {
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
        push(
            base.join("Programs")
                .join("codex")
                .join("bin")
                .join("codex.cmd"),
        );
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
    for candidate in desktop_host_bundled_cli_candidates() {
        push(candidate);
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

/// Signal every identity-verified host PID (`taskkill /PID`, graceful
/// WM_CLOSE by default, `/F` when `force`) and return how many were
/// signalled. Signalling by PID instead of `/IM <name>` keeps ChatGPT
/// Classic (same executable name, different install) untouched, and
/// Unknown PIDs are never signalled.
fn signal_desktop_app_processes(force: bool) -> usize {
    let mut signalled = 0;
    for (pid, identity) in desktop_app_pid_classifications() {
        if identity != HostIdentity::Ours {
            continue;
        }
        let pid_text = pid.to_string();
        let mut taskkill = Command::new("taskkill");
        if force {
            taskkill.args(["/F", "/PID", &pid_text]);
        } else {
            taskkill.args(["/PID", &pid_text]);
        }
        match hide_console_window(&mut taskkill).output() {
            Ok(output) if output.status.success() => signalled += 1,
            // Non-zero exit: the process exited between enumeration and
            // signalling — the wait loop below re-checks, nothing to do.
            Ok(_) => {}
            Err(error) => {
                eprintln!("codex_switch: failed to spawn taskkill for pid {pid}: {error}");
            }
        }
    }
    signalled
}

pub fn quit_codex_app_if_running() -> AppResult<bool> {
    if !is_codex_app_running() {
        return Ok(false);
    }

    let mut signalled = signal_desktop_app_processes(false);
    let mut exited = false;
    for _ in 0..20 {
        if !is_codex_app_running() {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }

    if !exited {
        signalled += signal_desktop_app_processes(true);
        for _ in 0..10 {
            if !is_codex_app_running() {
                exited = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    if exited {
        return Ok(true);
    }

    // Distinguish "we signalled it and it would not die" from "we never
    // managed to signal anything" — the latter means the failure is on
    // our side (no identifiable PID / taskkill unavailable), not the
    // app's. Mirrors the macOS quit path.
    let message = if signalled == 0 {
        "Codex/ChatGPT still appears to be running, but no matching process could be \
         identified and signalled. Close it manually and retry."
    } else {
        "Codex/ChatGPT did not exit cleanly. Close it manually and retry."
    };
    Err(AppError::new("APP_EXIT_FAILED", message))
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

    fn run_codex_login(&self, cli_codex_home: &Path, runtime_codex_home: &Path) -> AppResult<()> {
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
        assert_eq!(
            probe_codex_version(&codex_home.join("does-not-exist")),
            None
        );

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

    #[test]
    fn classify_host_executable_identifies_msix_packages() {
        use super::{classify_host_executable, HostIdentity};
        // Merged host: the OpenAI.Codex package family survives the
        // in-place Store update (display/executable renamed to ChatGPT).
        assert_eq!(
            classify_host_executable(
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.707.0.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe"
            ),
            HostIdentity::Ours
        );
        // Beta channel shares the OpenAI.Codex prefix.
        assert_eq!(
            classify_host_executable(
                r"C:\Program Files\WindowsApps\OpenAI.CodexBeta_26.513.4821.0_x64__2p2nqsd0c76g0\app\Codex (Beta).exe"
            ),
            HostIdentity::Ours
        );
        // ChatGPT Classic: same executable name, NOT a codex host.
        assert_eq!(
            classify_host_executable(
                r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2025.112.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe"
            ),
            HostIdentity::Other
        );
        assert_eq!(
            super::classify_host_executable(""),
            HostIdentity::Unknown,
            "missing executable path must stay unattributable"
        );
        assert_eq!(
            classify_host_executable(r"C:\Users\me\Desktop\ChatGPT.exe"),
            HostIdentity::Unknown
        );
    }

    #[test]
    fn host_dir_embeds_codex_cli_detects_bundled_cli() {
        use super::host_dir_embeds_codex_cli;
        let root = temp_codex_home("host-embed-cli");
        fs::create_dir_all(root.join("resources")).unwrap();
        let exe = root.join("ChatGPT.exe");
        fs::write(&exe, "stub").unwrap();
        assert!(!host_dir_embeds_codex_cli(&exe));
        fs::write(root.join("resources").join("codex.exe"), "stub").unwrap();
        assert!(host_dir_embeds_codex_cli(&exe));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_pid_classification_lines_parses_and_skips_malformed() {
        use super::{parse_pid_classification_lines, HostIdentity};
        let stdout = "123|C:\\Program Files\\WindowsApps\\OpenAI.Codex_26.707.0.0_x64__2p2nqsd0c76g0\\app\\ChatGPT.exe\r\n\
                      456|C:\\Program Files\\WindowsApps\\OpenAI.ChatGPT-Desktop_1.0_x64__2p2nqsd0c76g0\\app\\ChatGPT.exe\r\n\
                      789|\r\n\
                      not-a-line\r\n";
        let parsed = parse_pid_classification_lines(stdout);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], (123, HostIdentity::Ours));
        assert_eq!(parsed[1], (456, HostIdentity::Other));
        assert_eq!(parsed[2], (789, HostIdentity::Unknown));
    }

    #[test]
    fn desktop_app_process_names_cover_merged_and_legacy_hosts() {
        assert_eq!(
            super::desktop_app_process_names(),
            &["ChatGPT.exe", "Codex.exe"]
        );
    }
}
