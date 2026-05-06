use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};

use base64::Engine;
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};

use super::config::{
    force_root_openai_base_url, read_root_openai_base_url,
    sync_root_openai_base_url_for_current_profile,
};
use super::paths::{get_backup_root, get_codex_home, list_profile_dirs};

pub const GATEWAY_DIRNAME: &str = "gateway";
pub const GATEWAY_AUTHS_DIRNAME: &str = "auths";
pub const GATEWAY_CONFIG_FILENAME: &str = "config.yaml";
pub const GATEWAY_STATE_FILENAME: &str = "state.json";
pub const GATEWAY_LOG_FILENAME: &str = "cliproxy.log";
pub const GATEWAY_DEFAULT_PORT: u16 = 8317;
pub const GATEWAY_SIDECAR_BASE_NAME: &str = "cliproxy";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayState {
    pub enabled: bool,
    pub port: u16,
    pub session_affinity: bool,
    pub strategy: String,
    /// Snapshot of the root config.toml `openai_base_url` taken at the moment
    /// the gateway is first enabled. Restored on disable/recover so an existing
    /// externally-managed endpoint (e.g. another local proxy on a different
    /// port) is not silently clobbered.
    pub external_base_url_backup: Option<String>,
}

impl Default for GatewayState {
    fn default() -> Self {
        Self {
            enabled: false,
            port: GATEWAY_DEFAULT_PORT,
            session_affinity: true,
            strategy: "round-robin".to_string(),
            external_base_url_backup: None,
        }
    }
}

fn process_slot() -> &'static Mutex<Option<Child>> {
    static SLOT: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn last_error_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn lock_process() -> MutexGuard<'static, Option<Child>> {
    process_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_last_error(message: Option<String>) {
    let mut guard = last_error_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = message;
}

fn read_last_error() -> Option<String> {
    last_error_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub fn gateway_dir(codex_home: &Path) -> PathBuf {
    get_backup_root(Some(codex_home)).join(GATEWAY_DIRNAME)
}

pub fn gateway_auths_dir(codex_home: &Path) -> PathBuf {
    gateway_dir(codex_home).join(GATEWAY_AUTHS_DIRNAME)
}

pub fn gateway_config_path(codex_home: &Path) -> PathBuf {
    gateway_dir(codex_home).join(GATEWAY_CONFIG_FILENAME)
}

pub fn gateway_state_path(codex_home: &Path) -> PathBuf {
    gateway_dir(codex_home).join(GATEWAY_STATE_FILENAME)
}

pub fn gateway_log_path(codex_home: &Path) -> PathBuf {
    gateway_dir(codex_home).join(GATEWAY_LOG_FILENAME)
}

fn ensure_gateway_dirs(codex_home: &Path) -> AppResult<()> {
    let dirs = [gateway_dir(codex_home), gateway_auths_dir(codex_home)];
    for dir in dirs {
        fs::create_dir_all(&dir).map_err(|error| {
            AppError::new(
                "GATEWAY_DIR_FAILED",
                format!("Failed to create gateway dir {}: {error}", dir.display()),
            )
        })?;
    }
    Ok(())
}

fn read_state(codex_home: &Path) -> GatewayState {
    let path = gateway_state_path(codex_home);
    if path.is_file() {
        return fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<GatewayState>(&raw).ok())
            .unwrap_or_default();
    }

    // First-time bootstrap: seed the suggested port from any pre-existing
    // root openai_base_url so users running an external proxy on a
    // non-default port don't have to re-type it before enabling.
    let mut state = GatewayState::default();
    if let Some(port) = read_root_openai_base_url(Some(codex_home))
        .as_deref()
        .and_then(parse_port_from_url)
    {
        state.port = port;
    }
    state
}

fn parse_port_from_url(url: &str) -> Option<u16> {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host_part = after_scheme.split('/').next().unwrap_or("");
    let (_, port_str) = host_part.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;
    if port >= 1024 {
        Some(port)
    } else {
        None
    }
}

fn write_state(state: &GatewayState, codex_home: &Path) -> AppResult<()> {
    ensure_gateway_dirs(codex_home)?;
    let path = gateway_state_path(codex_home);
    let body = serde_json::to_string_pretty(state).map_err(|error| {
        AppError::new(
            "GATEWAY_STATE_SERIALIZE_FAILED",
            format!("Failed to serialize gateway state: {error}"),
        )
    })?;
    fs::write(&path, body).map_err(|error| {
        AppError::new(
            "GATEWAY_STATE_WRITE_FAILED",
            format!("Failed to write gateway state {}: {error}", path.display()),
        )
    })
}

pub fn proxy_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn write_config_yaml(state: &GatewayState, codex_home: &Path) -> AppResult<()> {
    let auth_dir = gateway_auths_dir(codex_home);
    let body = format!(
        concat!(
            "host: \"127.0.0.1\"\n",
            "port: {port}\n",
            "auth-dir: {auth_dir}\n",
            "api-keys: []\n",
            "remote-management:\n",
            "  allow-remote: false\n",
            "  secret-key: \"\"\n",
            "  disable-control-panel: true\n",
            "debug: false\n",
            "logging-to-file: false\n",
            "usage-statistics-enabled: false\n",
            "request-retry: 3\n",
            // The GUI owns account switching (see the pre-switch quota
            // writeback in switch_core). Letting cliproxy auto-switch on
            // quota exhaustion would race the GUI's snapshot and produce
            // misleading per-account quota cards. Keep both off.
            "quota-exceeded:\n",
            "  switch-project: false\n",
            "  switch-preview-model: false\n",
            "routing:\n",
            "  strategy: \"{strategy}\"\n",
            "  session-affinity: {affinity}\n",
        ),
        port = state.port,
        auth_dir = yaml_quote(&auth_dir.to_string_lossy()),
        strategy = state.strategy,
        affinity = state.session_affinity,
    );

    let path = gateway_config_path(codex_home);
    fs::write(&path, body).map_err(|error| {
        AppError::new(
            "GATEWAY_CONFIG_WRITE_FAILED",
            format!(
                "Failed to write gateway config {}: {error}",
                path.display()
            ),
        )
    })
}

fn base64_url_decode(value: &str) -> Option<Vec<u8>> {
    let cleaned = value.trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cleaned)
        .ok()
}

fn parse_jwt_claims(jwt: &str) -> (String, String) {
    let mut parts = jwt.split('.');
    let _header = parts.next();
    let payload = match parts.next() {
        Some(value) => value,
        None => return (String::new(), String::new()),
    };

    let bytes = match base64_url_decode(payload) {
        Some(value) => value,
        None => return (String::new(), String::new()),
    };

    let claims: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return (String::new(), String::new()),
    };

    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let exp = claims.get("exp").and_then(Value::as_i64).unwrap_or_default();
    let expired = if exp > 0 {
        Utc.timestamp_opt(exp, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    (email, expired)
}

fn convert_chatgpt_auth(profile_dir: &Path) -> Option<Value> {
    let raw = fs::read_to_string(profile_dir.join("auth.json")).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    let auth_mode = parsed
        .get("auth_mode")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !auth_mode.eq_ignore_ascii_case("chatgpt") {
        return None;
    }
    let tokens = parsed.get("tokens")?;
    let access = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let refresh = tokens
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = tokens
        .get("id_token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let account = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let last_refresh = parsed
        .get("last_refresh")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if access.is_empty() || refresh.is_empty() {
        return None;
    }
    let (email, expired) = parse_jwt_claims(id);
    Some(json!({
        "type": "codex",
        "id_token": id,
        "access_token": access,
        "refresh_token": refresh,
        "account_id": account,
        "last_refresh": last_refresh,
        "email": email,
        "expired": expired,
    }))
}

fn sync_auths(codex_home: &Path) -> AppResult<u32> {
    let target = gateway_auths_dir(codex_home);
    fs::create_dir_all(&target).map_err(|error| {
        AppError::new(
            "GATEWAY_AUTHS_DIR_FAILED",
            format!(
                "Failed to create gateway auths dir {}: {error}",
                target.display()
            ),
        )
    })?;

    // Track which files we wrote so stale ones can be cleaned up.
    let backup_root = get_backup_root(Some(codex_home));
    let mut active_names = Vec::new();
    let mut count = 0u32;

    for profile_dir in list_profile_dirs(&backup_root) {
        let profile_name = match profile_dir.file_name().and_then(|name| name.to_str()) {
            Some(value) => value.to_string(),
            None => continue,
        };
        let Some(token) = convert_chatgpt_auth(&profile_dir) else {
            continue;
        };
        let filename = format!("codex-{profile_name}.json");
        let dest = target.join(&filename);
        let body = match serde_json::to_vec_pretty(&token) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if fs::write(&dest, body).is_ok() {
            active_names.push(filename);
            count += 1;
        }
    }

    if let Ok(entries) = fs::read_dir(&target) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with("codex-") || !name.ends_with(".json") {
                continue;
            }
            if !active_names.iter().any(|active| active == name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    Ok(count)
}

fn count_auth_files(codex_home: &Path) -> u32 {
    fs::read_dir(gateway_auths_dir(codex_home))
        .map(|iter| {
            iter.flatten()
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .map(|name| name.ends_with(".json"))
                        .unwrap_or(false)
                })
                .count() as u32
        })
        .unwrap_or(0)
}

fn host_target_triple() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else {
        ""
    }
}

fn resolve_sidecar_path() -> Option<PathBuf> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let plain_name = format!("{GATEWAY_SIDECAR_BASE_NAME}{ext}");
    let triple = host_target_triple();
    let triple_name = if triple.is_empty() {
        plain_name.clone()
    } else {
        format!("{GATEWAY_SIDECAR_BASE_NAME}-{triple}{ext}")
    };

    if let Ok(env_path) = std::env::var("CODEX_SWITCH_SIDECAR") {
        let candidate = PathBuf::from(env_path);
        if is_real_binary(&candidate) {
            return Some(candidate);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for name in [&plain_name, &triple_name] {
                let candidate = parent.join(name);
                if is_real_binary(&candidate) {
                    return Some(candidate);
                }
            }
            // Sidecar may also live in a sibling resources dir under macOS bundle.
            if let Some(grandparent) = parent.parent() {
                for sibling in ["Resources", "resources", "../Resources"] {
                    for name in [&plain_name, &triple_name] {
                        let candidate = grandparent.join(sibling).join(name);
                        if is_real_binary(&candidate) {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
        // Dev fallback: walk up looking for src-tauri/binaries.
        let mut probe = exe.as_path();
        while let Some(parent) = probe.parent() {
            for name in [&triple_name, &plain_name] {
                let candidate = parent.join("src-tauri").join("binaries").join(name);
                if is_real_binary(&candidate) {
                    return Some(candidate);
                }
            }
            probe = parent;
        }
    }

    None
}

fn is_real_binary(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.len() > 0)
        .unwrap_or(false)
}

pub fn sidecar_available() -> bool {
    resolve_sidecar_path().is_some()
}

fn process_running(slot: &mut MutexGuard<'_, Option<Child>>) -> bool {
    if let Some(child) = slot.as_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {
                **slot = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        }
    } else {
        false
    }
}

fn open_log_file(codex_home: &Path) -> Option<fs::File> {
    let path = gateway_log_path(codex_home);
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

fn spawn_sidecar_locked(
    codex_home: &Path,
    slot: &mut MutexGuard<'_, Option<Child>>,
) -> AppResult<()> {
    if process_running(slot) {
        return Ok(());
    }

    let binary = resolve_sidecar_path().ok_or_else(|| {
        AppError::new(
            "GATEWAY_SIDECAR_MISSING",
            "CLIProxyAPI sidecar binary not found. Build it via scripts/build-cliproxy first.",
        )
    })?;
    let config_file = gateway_config_path(codex_home);
    if !config_file.is_file() {
        return Err(AppError::new(
            "GATEWAY_CONFIG_MISSING",
            format!("Missing gateway config at {}", config_file.display()),
        ));
    }

    let mut command = Command::new(&binary);
    command
        .arg("--config")
        .arg(&config_file)
        .stdin(Stdio::null());

    let log_handle = open_log_file(codex_home);
    match log_handle.as_ref().and_then(|file| file.try_clone().ok()) {
        Some(file) => {
            command.stdout(Stdio::from(file));
        }
        None => {
            command.stdout(Stdio::null());
        }
    }
    match log_handle.and_then(|file| file.try_clone().ok()) {
        Some(file) => {
            command.stderr(Stdio::from(file));
        }
        None => {
            command.stderr(Stdio::null());
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let child = command.spawn().map_err(|error| {
        AppError::new(
            "GATEWAY_SPAWN_FAILED",
            format!(
                "Failed to spawn sidecar at {}: {error}",
                binary.display()
            ),
        )
    })?;
    **slot = Some(child);
    Ok(())
}

fn stop_sidecar_locked(slot: &mut MutexGuard<'_, Option<Child>>) {
    if let Some(mut child) = slot.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn shutdown_only() {
    let mut slot = lock_process();
    stop_sidecar_locked(&mut slot);
}

/// Restore the root `openai_base_url` after the sidecar stops.
///
/// Prefers the backed-up external value captured at enable time. Falls back to
/// the per-profile derivation (matching the pre-gateway behavior) when no
/// backup is recorded.
fn restore_root_url(state: &GatewayState, codex_home: &Path) -> AppResult<()> {
    if let Some(backup) = state.external_base_url_backup.as_deref() {
        force_root_openai_base_url(Some(backup), Some(codex_home))
    } else {
        sync_root_openai_base_url_for_current_profile(Some(codex_home))
    }
}

fn build_status(codex_home: &Path, state: &GatewayState) -> AppResult<crate::models::GatewayStatus> {
    let mut slot = lock_process();
    let running = process_running(&mut slot);
    drop(slot);
    Ok(crate::models::GatewayStatus {
        enabled: state.enabled,
        running,
        port: state.port,
        endpoint: proxy_endpoint(state.port),
        session_affinity: state.session_affinity,
        strategy: state.strategy.clone(),
        active_auths: count_auth_files(codex_home),
        last_error: read_last_error(),
        sidecar_available: sidecar_available(),
        config_dir: gateway_dir(codex_home).to_string_lossy().to_string(),
    })
}

/// Return the current gateway status without mutating any state.
pub fn status(codex_home: Option<&Path>) -> AppResult<crate::models::GatewayStatus> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    ensure_gateway_dirs(&codex_home)?;
    let state = read_state(&codex_home);
    build_status(&codex_home, &state)
}

/// On application startup: if gateway state says enabled, spin up the sidecar
/// and re-apply the base URL override. Failure is recorded but not fatal so the
/// rest of the app keeps working.
pub fn restore_on_startup(codex_home: Option<&Path>) -> AppResult<()> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    ensure_gateway_dirs(&codex_home)?;
    let mut state = read_state(&codex_home);
    if !state.enabled {
        return Ok(());
    }
    match enable_internal(&codex_home, &mut state) {
        Ok(()) => {
            // Persist any backup that was newly captured (e.g. when migrating
            // from an older state.json that lacked the field).
            let _ = write_state(&state, &codex_home);
        }
        Err(error) => set_last_error(Some(error.message.clone())),
    }
    Ok(())
}

fn enable_internal(codex_home: &Path, state: &mut GatewayState) -> AppResult<()> {
    ensure_gateway_dirs(codex_home)?;
    sync_auths(codex_home)?;
    write_config_yaml(state, codex_home)?;
    {
        let mut slot = lock_process();
        spawn_sidecar_locked(codex_home, &mut slot)?;
    }
    if state.external_base_url_backup.is_none() {
        let current = read_root_openai_base_url(Some(codex_home));
        let self_endpoint = proxy_endpoint(state.port);
        // Skip the capture if the root already points at our proxy — that
        // means a prior enable already wrote it; treating it as "external"
        // would create a self-referential loop on the next disable.
        state.external_base_url_backup = current.filter(|value| value != &self_endpoint);
    }
    force_root_openai_base_url(Some(&proxy_endpoint(state.port)), Some(codex_home))?;
    set_last_error(None);
    Ok(())
}

/// Enable forwarding using the current persisted state (or defaults if none).
pub fn enable(codex_home: Option<&Path>) -> AppResult<crate::models::GatewayStatus> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    let mut state = read_state(&codex_home);
    state.enabled = true;
    enable_internal(&codex_home, &mut state)?;
    write_state(&state, &codex_home)?;
    build_status(&codex_home, &state)
}

/// Disable forwarding, stop the sidecar, and restore the per-profile base URL.
pub fn disable(codex_home: Option<&Path>) -> AppResult<crate::models::GatewayStatus> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    let mut state = read_state(&codex_home);
    state.enabled = false;
    shutdown_only();
    restore_root_url(&state, &codex_home)?;
    state.external_base_url_backup = None;
    write_state(&state, &codex_home)?;
    set_last_error(None);
    build_status(&codex_home, &state)
}

/// Update gateway settings. If forwarding is enabled, the sidecar is restarted
/// so the new config takes effect.
pub fn update_settings(
    payload: crate::models::GatewayUpdatePayload,
    codex_home: Option<&Path>,
) -> AppResult<crate::models::GatewayStatus> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    let mut state = read_state(&codex_home);
    if let Some(port) = payload.port {
        if port < 1024 {
            return Err(AppError::new(
                "GATEWAY_PORT_INVALID",
                format!("Port must be >= 1024, got {port}"),
            ));
        }
        state.port = port;
    }
    if let Some(value) = payload.session_affinity {
        state.session_affinity = value;
    }
    if let Some(strategy) = payload.strategy {
        let normalized = strategy.trim().to_ascii_lowercase();
        if !matches!(normalized.as_str(), "round-robin" | "fill-first") {
            return Err(AppError::new(
                "GATEWAY_STRATEGY_INVALID",
                format!("Unsupported routing strategy: {strategy}"),
            ));
        }
        state.strategy = normalized;
    }
    if state.enabled {
        // Restart with the new config.
        {
            let mut slot = lock_process();
            stop_sidecar_locked(&mut slot);
        }
        enable_internal(&codex_home, &mut state)?;
    }
    write_state(&state, &codex_home)?;
    build_status(&codex_home, &state)
}

/// Re-sync auths into the gateway directory. Useful after add/remove/refresh
/// operations on profiles while forwarding is active.
#[allow(dead_code)]
pub fn refresh_auths(codex_home: Option<&Path>) -> AppResult<u32> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    let state = read_state(&codex_home);
    if !state.enabled {
        return Ok(0);
    }
    sync_auths(&codex_home)
}

/// Gracefully shut down the sidecar (called on app exit). Does not change the
/// persisted enabled flag, so a restart will resume forwarding.
pub fn shutdown_for_exit() {
    let mut slot = lock_process();
    stop_sidecar_locked(&mut slot);
}

/// Best-effort recovery: ensure no sidecar is running and the base URL falls
/// back to either the captured external endpoint or per-profile values. Used
/// by the UI "reset" button.
pub fn force_recover(codex_home: Option<&Path>) -> AppResult<crate::models::GatewayStatus> {
    let codex_home = codex_home.map(Path::to_path_buf).unwrap_or_else(get_codex_home);
    let mut state = read_state(&codex_home);
    state.enabled = false;
    shutdown_only();
    match restore_root_url(&state, &codex_home) {
        Ok(()) => set_last_error(None),
        Err(error) => set_last_error(Some(error.message.clone())),
    }
    state.external_base_url_backup = None;
    write_state(&state, &codex_home)?;
    build_status(&codex_home, &state)
}

