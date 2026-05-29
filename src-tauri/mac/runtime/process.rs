use std::env;
use std::fs;
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

use super::cli_shim::{get_install_state_file, managed_shim_path, real_codex_resolver_path};

const APP_NAME: &str = "Codex";
static MACOS_PLATFORM_HOOKS: MacosPlatformHooks = MacosPlatformHooks;
static MACOS_APP_PATH_CACHE: OnceLock<Option<String>> = OnceLock::new();

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
    let mut command = Command::new(&resolver_path);
    command.arg(managed_shim_text);
    // Bounded like the other shell spawns: even this first-party resolver
    // script sources the login environment, which can stall on a hung rc.
    let stdout = crate::shared::codex_cli_path::run_capturing_stdout_with_timeout(
        command,
        LOGIN_SHELL_PROBE_TIMEOUT,
    )?;
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

fn is_acceptable_real_codex_path(path: &Path, managed_shim_path: Option<&Path>) -> bool {
    if !path.is_file() {
        return false;
    }
    !managed_shim_path.is_some_and(|managed| managed == path)
}

pub(super) fn resolve_real_codex_cli_with_source(
    codex_home: Option<&Path>,
) -> Option<(PathBuf, RealCodexPathSource)> {
    let managed_shim_path = codex_home.map(managed_shim_path);
    let state = load_install_state(codex_home);

    // User override wins. Falls through silently if the file disappeared
    // so the user isn't permanently wedged on a stale path; the override
    // stays persisted for when the file reappears.
    if let Some(raw_user_path) = state.user_codex_path.as_ref().map(PathBuf::from) {
        if is_acceptable_real_codex_path(&raw_user_path, managed_shim_path.as_deref()) {
            return Some((raw_user_path, RealCodexPathSource::UserOverride));
        }
    }

    let mut state = state;
    if let Some(raw_path) = state.real_codex_path.as_ref().map(PathBuf::from) {
        if is_acceptable_real_codex_path(&raw_path, managed_shim_path.as_deref()) {
            return Some((raw_path, RealCodexPathSource::InstallState));
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
    discovered_path.map(|path| (path, RealCodexPathSource::Discovery))
}

fn resolve_real_codex_cli(codex_home: Option<&Path>) -> Option<PathBuf> {
    resolve_real_codex_cli_with_source(codex_home).map(|(path, _)| path)
}

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
    if !candidate.is_file() {
        return Err(AppError::new(
            "CODEX_CLI_PATH_INVALID",
            format!("No file found at {}.", candidate.display()),
        ));
    }
    let managed_shim = codex_home.map(managed_shim_path);
    if managed_shim
        .as_deref()
        .is_some_and(|managed| managed == candidate.as_path())
    {
        return Err(AppError::new(
            "CODEX_CLI_PATH_REJECTED",
            "That path is the codex_switch managed shim; pick the real codex CLI binary instead.",
        ));
    }
    Ok(candidate)
}

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
pub struct MacosCodexPathResolver;

pub static MACOS_CODEX_PATH_RESOLVER: MacosCodexPathResolver = MacosCodexPathResolver;

impl CodexPathResolver for MacosCodexPathResolver {
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

/// Soft cap on how long the PATH walk can spend stat'ing entries
/// before we bail and return what we have. Each `is_file` probe blocks
/// the Tauri command thread; an NFS / SMB entry on PATH can stall for
/// seconds. Fixed locations (Codex.app bundle, Homebrew, /usr/local,
/// npm-global / bun / volta) are checked first because they're
/// guaranteed-fast local stats — by the time we hit the bounded PATH
/// walk we've usually already collected the realistic candidates.
const PATH_PROBE_DEADLINE: Duration = Duration::from_millis(500);

pub fn suggested_codex_cli_paths(codex_home: Option<&Path>) -> Vec<PathBuf> {
    let mut suggestions: Vec<PathBuf> = Vec::new();
    let managed_shim = codex_home.map(managed_shim_path);
    let mut push = |path: PathBuf| {
        if is_acceptable_real_codex_path(&path, managed_shim.as_deref())
            && !suggestions.iter().any(|existing| existing == &path)
        {
            suggestions.push(path);
        }
    };

    for app_path in codex_app_candidates() {
        push(codex_cli_from_app_bundle(&app_path));
    }
    push(PathBuf::from("/opt/homebrew/bin/codex"));
    push(PathBuf::from("/usr/local/bin/codex"));
    push(PathBuf::from("/usr/bin/codex"));
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        push(home.join(".local/bin/codex"));
        push(home.join(".npm-global/bin/codex"));
        push(home.join(".bun/bin/codex"));
        push(home.join(".volta/bin/codex"));
    }

    if let Some(path) = env::var_os("PATH") {
        let deadline = std::time::Instant::now() + PATH_PROBE_DEADLINE;
        for entry in env::split_paths(&path) {
            if std::time::Instant::now() >= deadline {
                break;
            }
            push(entry.join("codex"));
        }
    }

    suggestions
}

/// How long a single `codex --version` probe may run before we kill it
/// and treat the candidate as unusable. Keeps a hung or input-waiting
/// binary from wedging the auto-detect scan; a healthy codex answers
/// well under this.
const RUNNABLE_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Upper bound on how many candidates the auto-detect scan will probe.
/// Each probe spawns a child (up to `RUNNABLE_PROBE_TIMEOUT`), so without
/// a cap a pathological PATH with many `codex` entries could stall the
/// scan. Realistic machines have 1-3 candidates.
const MAX_PROBE_CANDIDATES: usize = 12;

/// Login shells source the user's full profile chain (nvm / asdf / brew
/// shellenv / network-y rc files), which can be slower than a bare
/// `--version`, so give the login-shell resolve a more generous budget —
/// but still hard-bounded so a hung profile can't wedge the whole scan.
const LOGIN_SHELL_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

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

/// Resolve codex through the user's login shell, so installs on the
/// shell's PATH (nvm / asdf / brew / fnm / any rc-managed location) are
/// found even when the app was launched from Finder with the narrow
/// launchd PATH. A non-interactive login shell + `command -v` avoids
/// loading the user's `codex` shell *function* (if any), so we resolve to
/// the real binary; the result is verified to be an absolute file.
fn discover_codex_via_login_shell(managed_shim_path: Option<&Path>) -> Option<PathBuf> {
    let shell = env::var_os("SHELL")?;
    let mut command = Command::new(&shell);
    command.args(["-lc", "command -v codex"]);
    // Bounded via the shared helper: a slow / hung login profile (nvm,
    // asdf, network-y rc files) must NOT wedge the scan the way an
    // unbounded `.output()` would — this runs first and synchronously.
    let stdout = crate::shared::codex_cli_path::run_capturing_stdout_with_timeout(
        command,
        LOGIN_SHELL_PROBE_TIMEOUT,
    )?;
    // `command -v` prints the resolved path last — after any banner a noisy
    // profile may have echoed to stdout — so take the last non-empty line.
    let resolved = stdout
        .lines()
        .map(str::trim)
        .rev()
        .find(|value| !value.is_empty())?;
    let candidate = PathBuf::from(resolved);
    // `command -v` of a function/alias returns a bare name, not a path —
    // require an absolute path to a real file so we never feed that back.
    if !candidate.is_absolute() || !candidate.is_file() {
        return None;
    }
    if managed_shim_path.is_some_and(|managed| managed == candidate.as_path()) {
        return None;
    }
    Some(candidate)
}

/// Force a fresh scan for the Settings auto-detect button: gather every
/// candidate the discovery + suggestion paths know about (login-shell
/// resolution, managed-shim resolver, Codex.app bundle, fixed install
/// locations, PATH), then keep only those that pass the runnable probe,
/// capturing each one's version. Ignores the cached/override path so a
/// wrong saved path can be corrected.
pub fn redetect_runnable_codex_cli_paths(codex_home: Option<&Path>) -> Vec<CodexCliCandidate> {
    let managed_shim = codex_home.map(managed_shim_path);
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Login-shell resolution first — catches nvm / asdf / brew / fnm
    // installs on the user's PATH even under Finder's narrow launchd PATH.
    if let Some(path) = discover_codex_via_login_shell(managed_shim.as_deref()) {
        push_candidate(&mut candidates, path);
    }
    // The managed-shim resolver (present when codex_switch installed its
    // shim) and the suggestion list (Codex.app bundle, fixed locations,
    // bounded PATH walk) fill in the rest.
    if let Some(shell_path) = discover_real_codex_cli_from_shell(managed_shim.as_deref()) {
        push_candidate(&mut candidates, shell_path);
    }
    for path in suggested_codex_cli_paths(codex_home) {
        push_candidate(&mut candidates, path);
    }

    candidates
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

fn build_app_server_command(real_codex_path: &Path, runtime_codex_home: &Path) -> Command {
    let mut command = Command::new(real_codex_path);
    // `codex app-server` is the canonical control-plane subcommand
    // (verified against `openai/codex` `codex-rs/cli/src/main.rs`). It
    // takes no sandbox/approval flags — the `-s` / `-a` flags only bind
    // to the interactive TUI and are silently ignored here, so we omit
    // them rather than carry dead weight on every refresh.
    command.arg("app-server");
    command.current_dir(runtime_codex_home);
    command.env("CODEX_HOME", runtime_codex_home);
    command
}

/// Build the `codex login` command using a resolved real-codex path.
/// Anchoring on `cli_codex_home` (the live `~/.codex`) for resolution
/// keeps the managed-shim filter correct even when `runtime_codex_home`
/// is a sandboxed sibling. Callers must resolve the path beforehand and
/// surface `REAL_CODEX_NOT_FOUND` to the user instead of falling back to
/// a bare `Command::new("codex")` — that fallback turned a missing-CLI
/// install into an opaque "No such file or directory" instead of an
/// actionable hint.
fn build_login_command(real_codex_path: &Path, runtime_codex_home: &Path) -> Command {
    let mut command = Command::new(real_codex_path);
    command.arg("login");
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
            "Real Codex CLI path not found. Make sure `codex` is installed and in PATH.",
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

    fn fetch_account_via_app_server(
        &self,
        cli_codex_home: &Path,
        runtime_codex_home: &Path,
    ) -> AppResult<AppServerSnapshot> {
        fetch_account_via_app_server(cli_codex_home, runtime_codex_home)
    }

    fn sync_on_window_close(&self) -> AppResult<()> {
        crate::macos::bootstrap::sync_root_state_to_current_profile(None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_app_server_command, codex_app_candidates, codex_cli_from_app_bundle,
        discover_real_codex_cli_path, resolve_real_codex_cli_with_source,
        set_user_codex_cli_path, validate_user_codex_cli_path, RealCodexPathSource,
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
        let _guard = crate::macos::env_guard();
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
        let _guard = crate::macos::env_guard();
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
        let _guard = crate::macos::env_guard();
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
    fn build_app_server_command_targets_runtime_codex_home() {
        let real_codex_path = PathBuf::from("/opt/homebrew/bin/codex");
        let runtime_codex_home = PathBuf::from("/tmp/codex-home");

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

        assert_eq!(
            command.get_program().to_string_lossy(),
            real_codex_path.to_string_lossy()
        );
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
    fn user_override_takes_priority_over_discovery_and_install_state() {
        let codex_home = temp_codex_home("user-override-priority");
        let managed_bin = codex_home.join("bin");
        let install_dir = codex_home.join("install");
        let user_dir = codex_home.join("user");
        fs::create_dir_all(&managed_bin).unwrap();
        fs::create_dir_all(&install_dir).unwrap();
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(managed_bin.join("codex"), "#!/bin/sh\n").unwrap();
        let install_path = install_dir.join("codex");
        let user_path = user_dir.join("codex");
        fs::write(&install_path, "#!/bin/sh\n").unwrap();
        fs::write(&user_path, "#!/bin/sh\n").unwrap();

        // Seed the install_state cache with `install_path` so without the
        // user override that path would win.
        let mut state = super::load_install_state(Some(&codex_home));
        state.real_codex_path = Some(install_path.to_string_lossy().into_owned());
        super::save_install_state(Some(&codex_home), &state);

        // No override yet → install_state wins.
        let (resolved, source) = resolve_real_codex_cli_with_source(Some(&codex_home)).unwrap();
        assert_eq!(resolved, install_path);
        assert_eq!(source, RealCodexPathSource::InstallState);

        // After setting the override → user path wins.
        set_user_codex_cli_path(Some(&codex_home), user_path.to_string_lossy().as_ref()).unwrap();
        let (resolved, source) = resolve_real_codex_cli_with_source(Some(&codex_home)).unwrap();
        assert_eq!(resolved, user_path);
        assert_eq!(source, RealCodexPathSource::UserOverride);

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn validate_user_codex_cli_path_rejects_empty_missing_and_managed_shim() {
        let codex_home = temp_codex_home("validate-user-path");
        let managed_bin = codex_home.join("bin");
        fs::create_dir_all(&managed_bin).unwrap();
        let managed_shim = managed_bin.join("codex");
        fs::write(&managed_shim, "#!/bin/sh\n").unwrap();

        let empty = validate_user_codex_cli_path(Some(&codex_home), "  ").unwrap_err();
        assert_eq!(empty.error_code, "CODEX_CLI_PATH_EMPTY");

        let missing = validate_user_codex_cli_path(
            Some(&codex_home),
            codex_home.join("nope/codex").to_string_lossy().as_ref(),
        )
        .unwrap_err();
        assert_eq!(missing.error_code, "CODEX_CLI_PATH_INVALID");

        let shim = validate_user_codex_cli_path(
            Some(&codex_home),
            managed_shim.to_string_lossy().as_ref(),
        )
        .unwrap_err();
        assert_eq!(shim.error_code, "CODEX_CLI_PATH_REJECTED");

        let _ = fs::remove_dir_all(&codex_home);
    }
}
