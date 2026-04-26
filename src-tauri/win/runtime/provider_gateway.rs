use std::collections::HashMap;
use std::convert::Infallible;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(not(test))]
use std::net::TcpListener;
#[cfg(not(test))]
use std::process::Command;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{stream, StreamExt};
use reqwest::header::{
    HeaderMap as ReqwestHeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE,
    USER_AGENT,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::shared::config::{profile_uses_api_key_auth, sync_root_openai_base_url_value};

use super::metadata::{load_profile_metadata, sync_profile_provider_protocol};
use super::model_mapping::{
    detect_profile_provider_protocol, fetch_profile_provider_models, load_profile_api_key,
    resolve_provider_target_model_for_request, PROVIDER_PROTOCOL_CHAT_COMPLETIONS,
    PROVIDER_PROTOCOL_COMPLETIONS, PROVIDER_PROTOCOL_MESSAGES,
};
use super::paths::{get_backup_root, get_codex_home, get_runtime_dir, validate_profile_name};
#[cfg(not(test))]
use super::process::hide_console_window;
use super::profiles::resolve_current_profile;

const GATEWAY_HOST: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
const GATEWAY_PORT_CANDIDATES: [u16; 8] = [48101, 48102, 48103, 48104, 48105, 48106, 48107, 48108];
const GATEWAY_HEALTH_PATH: &str = "/health";
const GATEWAY_STATE_FILENAME: &str = "provider_gateway_state.json";
const GATEWAY_RESPONSES_DIRNAME: &str = "provider_gateway_responses";
const KIMI_GATEWAY_DIAGNOSTICS_FILENAME: &str = "provider_gateway_kimi_diagnostics.jsonl";
#[cfg(not(test))]
const GATEWAY_START_TIMEOUT_SECS: u64 = 5;
const GATEWAY_REQUEST_TIMEOUT_SECS: u64 = 120;
const GATEWAY_USER_AGENT: &str = "Codex CLI";
const MESSAGES_API_KEY_HEADER: &str = "x-api-key";
const ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const ANTHROPIC_VERSION_VALUE: &str = "2023-06-01";
const DEFAULT_MESSAGES_MAX_TOKENS: u64 = 4096;
const GATEWAY_DIAGNOSTIC_BODY_PREVIEW_CHARS: usize = 600;
const EVENT_RESPONSE_CREATED: &str = "response.created";
const EVENT_RESPONSE_IN_PROGRESS: &str = "response.in_progress";
const EVENT_RESPONSE_OUTPUT_ITEM_ADDED: &str = "response.output_item.added";
const EVENT_RESPONSE_CONTENT_PART_ADDED: &str = "response.content_part.added";
const EVENT_RESPONSE_OUTPUT_TEXT_DELTA: &str = "response.output_text.delta";
const EVENT_RESPONSE_OUTPUT_TEXT_DONE: &str = "response.output_text.done";
const EVENT_RESPONSE_CONTENT_PART_DONE: &str = "response.content_part.done";
const EVENT_RESPONSE_OUTPUT_ITEM_DONE: &str = "response.output_item.done";
const EVENT_RESPONSE_COMPLETED: &str = "response.completed";
const UPSTREAM_BASE_SUFFIXES: [&str; 5] = [
    "/chat/completions",
    "/responses",
    "/models",
    "/completions",
    "/messages",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GatewayRuntimeState {
    port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredResponseRecord {
    id: String,
    previous_response_id: Option<String>,
    effective_instructions: Option<String>,
    request_messages: Vec<Value>,
    response: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
struct ResponsesGatewayRequest {
    model: String,
    stream: bool,
    previous_response_id: Option<String>,
    instructions: Option<String>,
    input: Option<Value>,
    messages: Option<Vec<Value>>,
    tools: Option<Vec<Value>>,
    tool_choice: Option<Value>,
    parallel_tool_calls: Option<bool>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    max_output_tokens: Option<u64>,
    max_completion_tokens: Option<u64>,
    metadata: Option<Value>,
    reasoning: Option<Value>,
    text: Option<Value>,
    response_format: Option<Value>,
    user: Option<String>,
    store: Option<bool>,
}

#[derive(Debug, Clone)]
struct PreparedGatewayRequest {
    upstream_body: Value,
    effective_instructions: Option<String>,
    request_messages: Vec<Value>,
    tool_name_registry: ToolNameRegistry,
}

#[derive(Debug, Clone)]
struct ActiveProfileContext {
    profile_name: String,
    base_url: String,
    provider_protocol: Option<String>,
    authorization_header: Option<String>,
    api_key: Option<String>,
}

#[derive(Debug, Clone)]
struct GatewayError {
    status: StatusCode,
    message: String,
}

impl GatewayError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "message": self.message,
                    "type": "provider_gateway_error",
                }
            })),
        )
            .into_response()
    }
}

#[derive(Clone)]
struct GatewayAppState {
    codex_home: PathBuf,
    http_client: Client,
    responses: Arc<Mutex<HashMap<String, StoredResponseRecord>>>,
}

#[derive(Debug, Clone, Default)]
struct ToolNameRegistry {
    original_to_sanitized: HashMap<String, String>,
    sanitized_to_original: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct StreamingAssistantMessageState {
    output_index: usize,
    item_id: String,
}

#[derive(Debug, Clone)]
enum KimiStreamingBlockState {
    Text {
        output_index: usize,
        item_id: String,
        content_index: usize,
    },
    ToolUse {
        call_id: String,
        name: String,
        partial_json: String,
    },
}

#[derive(Debug, Clone)]
struct UpstreamSseEvent {
    event: String,
    data: String,
}

#[derive(Debug, Default)]
struct UpstreamSseParser {
    pending: String,
    event_name: Option<String>,
    data_lines: Vec<String>,
}

#[derive(Debug, Default)]
struct KimiStreamingEventBatch {
    events: Vec<(String, Value)>,
    completed_response: Option<Value>,
}

#[derive(Debug, Clone)]
struct KimiStreamingTranslator {
    request: ResponsesGatewayRequest,
    response_id: String,
    created_at: i64,
    model: String,
    output_items: Vec<Value>,
    assistant_message: Option<StreamingAssistantMessageState>,
    blocks: HashMap<usize, KimiStreamingBlockState>,
    usage: Option<Value>,
    initial_events_sent: bool,
    completed: bool,
}

enum GatewayResponsePayload {
    Json(Value),
    Sse(Vec<Event>),
    Response(Response),
}

fn runtime_state_path(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(GATEWAY_STATE_FILENAME)
}

fn responses_store_dir(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(GATEWAY_RESPONSES_DIRNAME)
}

fn kimi_gateway_diagnostics_path(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join(KIMI_GATEWAY_DIAGNOSTICS_FILENAME)
}

fn response_record_path(codex_home: Option<&Path>, response_id: &str) -> PathBuf {
    responses_store_dir(codex_home).join(format!("{response_id}.json"))
}

fn gateway_base_url_for_port(port: u16) -> String {
    format!("http://{GATEWAY_HOST}:{port}/v1")
}

#[cfg(not(test))]
fn gateway_health_url_for_port(port: u16) -> String {
    format!("http://{GATEWAY_HOST}:{port}{GATEWAY_HEALTH_PATH}")
}

fn load_gateway_runtime_state(codex_home: Option<&Path>) -> GatewayRuntimeState {
    let path = runtime_state_path(codex_home);
    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return GatewayRuntimeState::default(),
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_gateway_runtime_state(
    codex_home: Option<&Path>,
    state: &GatewayRuntimeState,
) -> AppResult<()> {
    let path = runtime_state_path(codex_home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create provider gateway runtime directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(state).map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_STATE_INVALID",
            format!("Failed to serialize provider gateway state: {error}"),
        )
    })?;

    fs::write(&path, format!("{serialized}\n")).map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_STATE_WRITE_FAILED",
            format!(
                "Failed to write provider gateway state {}: {error}",
                path.display()
            ),
        )
    })
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .unwrap_or(0)
}

fn diagnostic_preview(value: Option<&str>) -> Option<String> {
    let trimmed = value.map(str::trim).filter(|value| !value.is_empty())?;
    let mut preview = trimmed
        .chars()
        .take(GATEWAY_DIAGNOSTIC_BODY_PREVIEW_CHARS)
        .collect::<String>();
    if trimmed.chars().count() > GATEWAY_DIAGNOSTIC_BODY_PREVIEW_CHARS {
        preview.push_str("...");
    }
    Some(preview)
}

fn append_kimi_gateway_diagnostic(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    event: &str,
    endpoint: Option<&str>,
    request_model: Option<&str>,
    status: Option<reqwest::StatusCode>,
    body: Option<&str>,
    error: Option<&str>,
) {
    let path = kimi_gateway_diagnostics_path(Some(&state.codex_home));
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let record = json!({
        "ts_ms": unix_timestamp_ms(),
        "event": event,
        "profile_name": profile.profile_name,
        "base_url": profile.base_url,
        "provider_protocol": profile.provider_protocol,
        "endpoint": endpoint,
        "request_model": request_model,
        "status": status.map(|value| value.as_u16()),
        "body_preview": diagnostic_preview(body),
        "error": diagnostic_preview(error),
    });

    let Ok(serialized) = serde_json::to_string(&record) else {
        return;
    };

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let _ = file.write_all(serialized.as_bytes());
    let _ = file.write_all(b"\n");
}

fn append_kimi_gateway_request_diagnostic(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request_model: Option<&str>,
    original_tools: &[String],
    forwarded_tools: &[String],
    image_count: usize,
) {
    let path = kimi_gateway_diagnostics_path(Some(&state.codex_home));
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let record = json!({
        "ts_ms": unix_timestamp_ms(),
        "event": "request_shape",
        "profile_name": profile.profile_name,
        "base_url": profile.base_url,
        "provider_protocol": profile.provider_protocol,
        "request_model": request_model,
        "request_tool_names": original_tools,
        "forwarded_tool_names": forwarded_tools,
        "input_image_count": image_count,
    });

    let Ok(serialized) = serde_json::to_string(&record) else {
        return;
    };

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let _ = file.write_all(serialized.as_bytes());
    let _ = file.write_all(b"\n");
}

fn count_request_images(messages: &[Value]) -> usize {
    messages
        .iter()
        .flat_map(|message| {
            message
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("input_image") | Some("image_url")
            )
        })
        .count()
}

#[cfg(not(test))]
fn is_port_available(port: u16) -> bool {
    TcpListener::bind((GATEWAY_HOST, port)).is_ok()
}

#[cfg(not(test))]
fn pick_gateway_port(codex_home: Option<&Path>) -> Option<u16> {
    let preferred = load_gateway_runtime_state(codex_home).port;
    preferred
        .filter(|port| is_port_available(*port))
        .or_else(|| {
            GATEWAY_PORT_CANDIDATES
                .into_iter()
                .find(|port| is_port_available(*port))
        })
}

#[cfg(not(test))]
fn healthcheck_gateway(port: u16) -> bool {
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    {
        Ok(value) => value,
        Err(_) => return false,
    };

    client
        .get(gateway_health_url_for_port(port))
        .send()
        .ok()
        .is_some_and(|response| response.status().is_success())
}

#[cfg(not(test))]
fn spawn_gateway_process(port: u16, codex_home: &Path) -> AppResult<()> {
    let current_exe = std::env::current_exe().map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_EXE_UNAVAILABLE",
            format!("Failed to resolve current executable for provider gateway: {error}"),
        )
    })?;

    let mut command = Command::new(current_exe);
    command.args(["gateway", "serve", "--port", &port.to_string()]);
    command.env("CODEX_HOME", codex_home);
    hide_console_window(&mut command).spawn().map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_LAUNCH_FAILED",
            format!("Failed to launch provider gateway: {error}"),
        )
    })?;

    Ok(())
}

pub fn ensure_gateway_running(codex_home: Option<&Path>) -> AppResult<u16> {
    let codex_home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home);
    #[cfg(test)]
    {
        let port = load_gateway_runtime_state(Some(&codex_home))
            .port
            .or_else(|| GATEWAY_PORT_CANDIDATES.first().copied())
            .ok_or_else(|| {
                AppError::new(
                    "PROVIDER_GATEWAY_PORT_UNAVAILABLE",
                    "Failed to allocate a local port for the provider gateway test stub.",
                )
            })?;
        save_gateway_runtime_state(Some(&codex_home), &GatewayRuntimeState { port: Some(port) })?;
        return Ok(port);
    }

    #[cfg(not(test))]
    {
        if let Some(port) = load_gateway_runtime_state(Some(&codex_home))
            .port
            .filter(|port| healthcheck_gateway(*port))
        {
            return Ok(port);
        }

        let port = pick_gateway_port(Some(&codex_home)).ok_or_else(|| {
            AppError::new(
                "PROVIDER_GATEWAY_PORT_UNAVAILABLE",
                "Failed to find a free local port for the provider gateway.",
            )
        })?;
        spawn_gateway_process(port, &codex_home)?;

        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(GATEWAY_START_TIMEOUT_SECS) {
            if healthcheck_gateway(port) {
                save_gateway_runtime_state(
                    Some(&codex_home),
                    &GatewayRuntimeState { port: Some(port) },
                )?;
                return Ok(port);
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        Err(AppError::new(
            "PROVIDER_GATEWAY_START_FAILED",
            "Provider gateway did not become ready in time.",
        ))
    }
}

fn load_cached_provider_protocol(profile_name: &str, codex_home: &Path) -> Option<String> {
    load_profile_metadata(profile_name, Some(codex_home))
        .provider_protocol
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn should_refresh_cached_provider_protocol(
    profile_name: &str,
    cached_protocol: &str,
    codex_home: &Path,
) -> bool {
    if cached_protocol != PROVIDER_PROTOCOL_CHAT_COMPLETIONS {
        return false;
    }

    load_normalized_profile_base_url(profile_name, codex_home)
        .is_some_and(|base_url| base_url.contains("api.kimi.com") && base_url.contains("/coding"))
}

fn resolve_provider_protocol(profile_name: &str, codex_home: &Path) -> AppResult<Option<String>> {
    if let Some(protocol) = load_cached_provider_protocol(profile_name, codex_home) {
        if !should_refresh_cached_provider_protocol(profile_name, &protocol, codex_home) {
            return Ok(Some(protocol));
        }
    }

    let detected = detect_profile_provider_protocol(profile_name, Some(codex_home))?;
    if detected.is_some() {
        sync_profile_provider_protocol(profile_name, detected.clone(), Some(codex_home))?;
    }
    Ok(detected)
}

fn load_normalized_profile_base_url(profile_name: &str, codex_home: &Path) -> Option<String> {
    load_profile_metadata(profile_name, Some(codex_home))
        .openai_base_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn sync_root_openai_base_url_for_profile(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<()> {
    let codex_home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    if !profile_uses_api_key_auth(&profile_name, Some(&codex_home))? {
        return sync_root_openai_base_url_value(None, Some(&codex_home));
    }

    let Some(base_url) = load_normalized_profile_base_url(&profile_name, &codex_home) else {
        return sync_root_openai_base_url_value(None, Some(&codex_home));
    };
    let _ = base_url;
    let port = ensure_gateway_running(Some(&codex_home))?;
    sync_root_openai_base_url_value(
        Some(gateway_base_url_for_port(port).as_str()),
        Some(&codex_home),
    )
}

fn build_chat_completion_endpoint_candidates(base_url: &str) -> Vec<String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push = |candidate: String| {
        if candidate.is_empty() || reqwest::Url::parse(&candidate).is_err() {
            return;
        }

        if !candidates
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&candidate))
        {
            candidates.push(candidate);
        }
    };

    if normalized.ends_with("/chat/completions") {
        push(normalized.to_string());
        return candidates;
    }

    push(format!("{normalized}/chat/completions"));
    if !normalized.ends_with("/v1") {
        push(format!("{normalized}/v1/chat/completions"));
    }

    for suffix in UPSTREAM_BASE_SUFFIXES {
        if let Some(base) = normalized.strip_suffix(suffix) {
            let base = base.trim_end_matches('/');
            push(format!("{base}/chat/completions"));
            push(format!("{base}/v1/chat/completions"));
        }
    }

    candidates
}

fn is_kimi_coding_base_url(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };

    let Some(host) = url.host_str() else {
        return false;
    };

    host.eq_ignore_ascii_case("api.kimi.com")
        && url
            .path()
            .split('/')
            .any(|segment| segment.eq_ignore_ascii_case("coding"))
}

fn uses_kimi_messages_gateway(profile: &ActiveProfileContext) -> bool {
    is_kimi_coding_base_url(&profile.base_url)
}

fn build_responses_endpoint_candidates(base_url: &str) -> Vec<String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push = |candidate: String| {
        if candidate.is_empty() || reqwest::Url::parse(&candidate).is_err() {
            return;
        }

        if !candidates
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&candidate))
        {
            candidates.push(candidate);
        }
    };

    if normalized.ends_with("/responses") {
        push(normalized.to_string());
        return candidates;
    }

    push(format!("{normalized}/responses"));
    if !normalized.ends_with("/v1") {
        push(format!("{normalized}/v1/responses"));
    }

    for suffix in UPSTREAM_BASE_SUFFIXES {
        if let Some(base) = normalized.strip_suffix(suffix) {
            let base = base.trim_end_matches('/');
            push(format!("{base}/responses"));
            push(format!("{base}/v1/responses"));
        }
    }

    candidates
}

fn build_messages_endpoint_candidates(base_url: &str) -> Vec<String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut push = |candidate: String| {
        if candidate.is_empty() || reqwest::Url::parse(&candidate).is_err() {
            return;
        }

        if !candidates
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&candidate))
        {
            candidates.push(candidate);
        }
    };

    if normalized.ends_with("/messages") {
        push(normalized.to_string());
        return candidates;
    }

    let kimi_coding = is_kimi_coding_base_url(normalized);
    if kimi_coding && !normalized.ends_with("/v1") {
        push(format!("{normalized}/v1/messages"));
        push(format!("{normalized}/messages"));
    } else {
        push(format!("{normalized}/messages"));
        if !normalized.ends_with("/v1") {
            push(format!("{normalized}/v1/messages"));
        }
    }

    for suffix in UPSTREAM_BASE_SUFFIXES {
        if let Some(base) = normalized.strip_suffix(suffix) {
            let base = base.trim_end_matches('/');
            if kimi_coding {
                push(format!("{base}/v1/messages"));
                push(format!("{base}/messages"));
            } else {
                push(format!("{base}/messages"));
                push(format!("{base}/v1/messages"));
            }
        }
    }

    candidates
}

fn api_key_from_authorization_header(authorization_header: Option<&str>) -> Option<String> {
    authorization_header
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
                .map(str::trim)
        })
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn prune_json_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys_to_remove = map
                .iter_mut()
                .filter_map(|(key, nested)| {
                    prune_json_nulls(nested);
                    nested.is_null().then(|| key.clone())
                })
                .collect::<Vec<_>>();
            for key in keys_to_remove {
                map.remove(&key);
            }
        }
        Value::Array(values) => {
            for nested in values.iter_mut() {
                prune_json_nulls(nested);
            }
            values.retain(|nested| !nested.is_null());
        }
        _ => {}
    }
}

fn response_id() -> String {
    format!("resp_{}", Uuid::new_v4().simple())
}

fn response_item_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn unix_timestamp_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .unwrap_or(0)
}

fn stored_response_from_disk(codex_home: &Path, response_id: &str) -> Option<StoredResponseRecord> {
    let raw = fs::read_to_string(response_record_path(Some(codex_home), response_id)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_response_record(codex_home: &Path, record: &StoredResponseRecord) -> AppResult<()> {
    let path = response_record_path(Some(codex_home), &record.id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create provider gateway response store {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(record).map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_STATE_INVALID",
            format!("Failed to serialize provider gateway response record: {error}"),
        )
    })?;

    fs::write(&path, format!("{serialized}\n")).map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_STATE_WRITE_FAILED",
            format!(
                "Failed to write provider gateway response record {}: {error}",
                path.display()
            ),
        )
    })
}

fn content_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn response_message_content_to_chat_content(content: &Value) -> Option<Value> {
    if let Some(text) = content_text(content) {
        return Some(Value::String(text));
    }

    let parts = content.as_array()?;
    let mut text_parts = Vec::new();
    let mut mixed_parts = Vec::new();
    for part in parts {
        let Some(part_type) = part.get("type").and_then(Value::as_str) else {
            continue;
        };
        match part_type {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    text_parts.push(text.to_string());
                    mixed_parts.push(json!({"type": "text", "text": text}));
                }
            }
            "input_image" | "image_url" => {
                let image_url = part
                    .pointer("/image_url/url")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("url").and_then(Value::as_str))
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let Some(image_url) = image_url {
                    mixed_parts.push(json!({
                        "type": "image_url",
                        "image_url": { "url": image_url },
                    }));
                }
            }
            _ => {}
        }
    }

    if mixed_parts.is_empty() {
        return None;
    }

    if mixed_parts
        .iter()
        .all(|part| part.get("type") == Some(&Value::String("text".to_string())))
    {
        return Some(Value::String(text_parts.join("\n")));
    }

    Some(Value::Array(mixed_parts))
}

fn sanitize_chat_message(mut message: Value) -> Option<Value> {
    let object = message.as_object_mut()?;
    let role = object
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .trim()
        .to_ascii_lowercase();
    let role = if role == "developer" {
        "system"
    } else {
        role.as_str()
    };
    object.insert("role".to_string(), Value::String(role.to_string()));

    let has_tool_calls = object
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|value| !value.is_empty());
    if !object.contains_key("content") && !has_tool_calls {
        object.insert("content".to_string(), Value::String(String::new()));
    }

    Some(message)
}

fn normalized_tool_name_base(name: &str) -> String {
    let mut normalized = name
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if normalized.is_empty() {
        normalized = "tool".to_string();
    }

    if !normalized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
    {
        normalized = format!("tool_{normalized}");
    }

    normalized
}

impl ToolNameRegistry {
    fn sanitized_name(&mut self, original_name: &str) -> String {
        let original_name = original_name.trim();
        if let Some(existing) = self.original_to_sanitized.get(original_name) {
            return existing.clone();
        }

        let base = normalized_tool_name_base(original_name);
        let mut candidate = base.clone();
        let mut suffix = 2_u32;
        while self
            .sanitized_to_original
            .get(&candidate)
            .is_some_and(|existing| existing != original_name)
        {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }

        self.original_to_sanitized
            .insert(original_name.to_string(), candidate.clone());
        self.sanitized_to_original
            .insert(candidate.clone(), original_name.to_string());
        candidate
    }

    fn original_name<'a>(&'a self, sanitized_name: &'a str) -> &'a str {
        self.sanitized_to_original
            .get(sanitized_name)
            .map(String::as_str)
            .unwrap_or(sanitized_name)
    }
}

impl UpstreamSseParser {
    fn push_chunk(&mut self, chunk: &str) -> Vec<UpstreamSseEvent> {
        self.pending.push_str(chunk);
        let mut events = Vec::new();

        while let Some(newline_index) = self.pending.find('\n') {
            let mut line = self.pending[..newline_index].to_string();
            self.pending.drain(..=newline_index);
            if line.ends_with('\r') {
                line.pop();
            }
            self.process_line(&line, &mut events);
        }

        events
    }

    fn finish(&mut self) -> Vec<UpstreamSseEvent> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.process_line(line.trim_end_matches('\r'), &mut events);
        }
        self.flush_event(&mut events);
        events
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<UpstreamSseEvent>) {
        if line.is_empty() {
            self.flush_event(events);
            return;
        }

        if let Some(value) = line.strip_prefix("event:") {
            self.event_name = Some(value.trim().to_string());
            return;
        }

        if let Some(value) = line.strip_prefix("data:") {
            self.data_lines.push(value.trim_start().to_string());
        }
    }

    fn flush_event(&mut self, events: &mut Vec<UpstreamSseEvent>) {
        if self.event_name.is_none() && self.data_lines.is_empty() {
            return;
        }

        events.push(UpstreamSseEvent {
            event: self
                .event_name
                .take()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "message".to_string()),
            data: self.data_lines.join("\n"),
        });
        self.data_lines.clear();
    }
}

impl KimiStreamingTranslator {
    fn new(request: &ResponsesGatewayRequest, model: String) -> Self {
        Self {
            request: request.clone(),
            response_id: response_id(),
            created_at: unix_timestamp_secs(),
            model,
            output_items: Vec::new(),
            assistant_message: None,
            blocks: HashMap::new(),
            usage: None,
            initial_events_sent: false,
            completed: false,
        }
    }

    fn initial_events(&mut self) -> Vec<(String, Value)> {
        if self.initial_events_sent {
            return Vec::new();
        }
        self.initial_events_sent = true;

        let stub = self.response_stub("in_progress");
        vec![
            (
                EVENT_RESPONSE_CREATED.to_string(),
                json!({
                    "type": EVENT_RESPONSE_CREATED,
                    "response": stub,
                }),
            ),
            (
                EVENT_RESPONSE_IN_PROGRESS.to_string(),
                json!({
                    "type": EVENT_RESPONSE_IN_PROGRESS,
                    "response": self.response_stub("in_progress"),
                }),
            ),
        ]
    }

    fn process_upstream_event(
        &mut self,
        event_name: &str,
        payload: &Value,
        tool_name_registry: &ToolNameRegistry,
    ) -> KimiStreamingEventBatch {
        let mut batch = KimiStreamingEventBatch::default();

        match event_name {
            "message_start" => {
                if let Some(model) = payload
                    .pointer("/message/model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    self.model = model.to_string();
                }
                if let Some(usage) = payload.pointer("/message/usage") {
                    self.usage = Some(build_usage_from_messages_stream_usage(usage));
                }
            }
            "content_block_start" => {
                let block_index =
                    payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let block = payload
                    .get("content_block")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match block_type {
                    "text" => {
                        let (output_index, item_id) = self.ensure_assistant_message(&mut batch);
                        let content_index =
                            self.add_text_part(output_index, &item_id, &mut batch.events);
                        self.blocks.insert(
                            block_index,
                            KimiStreamingBlockState::Text {
                                output_index,
                                item_id,
                                content_index,
                            },
                        );
                    }
                    "tool_use" => {
                        let call_id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| response_item_id("call"));
                        let name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        self.blocks.insert(
                            block_index,
                            KimiStreamingBlockState::ToolUse {
                                call_id,
                                name,
                                partial_json: String::new(),
                            },
                        );
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let block_index =
                    payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let Some(delta) = payload.get("delta") else {
                    return batch;
                };
                let delta_type = delta
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match delta_type {
                    "text_delta" => {
                        let text_delta = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let text_state =
                            self.blocks.get(&block_index).and_then(|state| match state {
                                KimiStreamingBlockState::Text {
                                    output_index,
                                    item_id,
                                    content_index,
                                } => Some((*output_index, item_id.clone(), *content_index)),
                                _ => None,
                            });
                        if let Some((output_index, item_id, content_index)) = text_state {
                            self.append_text_delta(
                                output_index,
                                &item_id,
                                content_index,
                                text_delta,
                                &mut batch.events,
                            );
                        }
                    }
                    "input_json_delta" => {
                        if let Some(KimiStreamingBlockState::ToolUse { partial_json, .. }) =
                            self.blocks.get_mut(&block_index)
                        {
                            partial_json.push_str(
                                delta
                                    .get("partial_json")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                            );
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let block_index =
                    payload.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let Some(block_state) = self.blocks.remove(&block_index) else {
                    return batch;
                };
                match block_state {
                    KimiStreamingBlockState::Text {
                        output_index,
                        item_id,
                        content_index,
                    } => {
                        self.finish_text_part(
                            output_index,
                            &item_id,
                            content_index,
                            &mut batch.events,
                        );
                    }
                    KimiStreamingBlockState::ToolUse {
                        call_id,
                        name,
                        partial_json,
                    } => {
                        if !name.trim().is_empty() {
                            self.finish_tool_use(
                                &call_id,
                                &name,
                                &partial_json,
                                tool_name_registry,
                                &mut batch.events,
                            );
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(usage) = payload.get("usage") {
                    self.usage = Some(build_usage_from_messages_stream_usage(usage));
                }
            }
            "message_stop" => {
                batch.completed_response = Some(self.finish_response(&mut batch.events));
            }
            _ => {}
        }

        batch
    }

    fn finish_if_needed(&mut self) -> KimiStreamingEventBatch {
        let mut batch = KimiStreamingEventBatch::default();
        if self.completed {
            return batch;
        }
        batch.completed_response = Some(self.finish_response(&mut batch.events));
        batch
    }

    fn ensure_assistant_message(&mut self, batch: &mut KimiStreamingEventBatch) -> (usize, String) {
        if let Some(existing) = self.assistant_message.as_ref() {
            return (existing.output_index, existing.item_id.clone());
        }

        let output_index = self.output_items.len();
        let item_id = response_item_id("msg");
        let item = json!({
            "id": item_id,
            "type": "message",
            "status": "in_progress",
            "role": "assistant",
            "content": [],
        });
        self.output_items.push(item.clone());
        self.assistant_message = Some(StreamingAssistantMessageState {
            output_index,
            item_id: item_id.clone(),
        });
        batch.events.push((
            EVENT_RESPONSE_OUTPUT_ITEM_ADDED.to_string(),
            json!({
                "type": EVENT_RESPONSE_OUTPUT_ITEM_ADDED,
                "response_id": self.response_id,
                "output_index": output_index,
                "item": item,
            }),
        ));

        (output_index, item_id)
    }

    fn add_text_part(
        &mut self,
        output_index: usize,
        item_id: &str,
        events: &mut Vec<(String, Value)>,
    ) -> usize {
        let Some(content) = self.output_items[output_index]
            .get_mut("content")
            .and_then(Value::as_array_mut)
        else {
            return 0;
        };
        let content_index = content.len();
        let part = json!({
            "type": "output_text",
            "text": "",
            "annotations": [],
        });
        content.push(part.clone());
        events.push((
            EVENT_RESPONSE_CONTENT_PART_ADDED.to_string(),
            json!({
                "type": EVENT_RESPONSE_CONTENT_PART_ADDED,
                "response_id": self.response_id,
                "output_index": output_index,
                "item_id": item_id,
                "content_index": content_index,
                "part": {
                    "type": "output_text",
                    "text": "",
                    "annotations": [],
                },
            }),
        ));
        content_index
    }

    fn append_text_delta(
        &mut self,
        output_index: usize,
        item_id: &str,
        content_index: usize,
        text_delta: &str,
        events: &mut Vec<(String, Value)>,
    ) {
        if text_delta.is_empty() {
            return;
        }

        if let Some(text) = self.output_items[output_index]
            .pointer_mut(&format!("/content/{content_index}/text"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
        {
            self.output_items[output_index]["content"][content_index]["text"] =
                Value::String(format!("{text}{text_delta}"));
        }

        events.push((
            EVENT_RESPONSE_OUTPUT_TEXT_DELTA.to_string(),
            json!({
                "type": EVENT_RESPONSE_OUTPUT_TEXT_DELTA,
                "response_id": self.response_id,
                "output_index": output_index,
                "item_id": item_id,
                "content_index": content_index,
                "delta": text_delta,
            }),
        ));
    }

    fn finish_text_part(
        &mut self,
        output_index: usize,
        item_id: &str,
        content_index: usize,
        events: &mut Vec<(String, Value)>,
    ) {
        let Some(part) = self.output_items[output_index]
            .pointer(&format!("/content/{content_index}"))
            .cloned()
        else {
            return;
        };
        let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
        events.push((
            EVENT_RESPONSE_OUTPUT_TEXT_DONE.to_string(),
            json!({
                "type": EVENT_RESPONSE_OUTPUT_TEXT_DONE,
                "response_id": self.response_id,
                "output_index": output_index,
                "item_id": item_id,
                "content_index": content_index,
                "text": text,
            }),
        ));
        events.push((
            EVENT_RESPONSE_CONTENT_PART_DONE.to_string(),
            json!({
                "type": EVENT_RESPONSE_CONTENT_PART_DONE,
                "response_id": self.response_id,
                "output_index": output_index,
                "item_id": item_id,
                "content_index": content_index,
                "part": part,
            }),
        ));
    }

    fn finish_tool_use(
        &mut self,
        call_id: &str,
        name: &str,
        partial_json: &str,
        tool_name_registry: &ToolNameRegistry,
        events: &mut Vec<(String, Value)>,
    ) {
        let parsed_arguments = parse_tool_arguments(partial_json);
        let item = json!({
            "id": response_item_id("fc"),
            "type": "function_call",
            "status": "completed",
            "call_id": call_id,
            "name": tool_name_registry.original_name(name),
            "arguments": serde_json::to_string(&parsed_arguments).unwrap_or_else(|_| "{}".to_string()),
        });
        let output_index = self.output_items.len();
        self.output_items.push(item.clone());
        events.push((
            EVENT_RESPONSE_OUTPUT_ITEM_ADDED.to_string(),
            json!({
                "type": EVENT_RESPONSE_OUTPUT_ITEM_ADDED,
                "response_id": self.response_id,
                "output_index": output_index,
                "item": item.clone(),
            }),
        ));
        events.push((
            EVENT_RESPONSE_OUTPUT_ITEM_DONE.to_string(),
            json!({
                "type": EVENT_RESPONSE_OUTPUT_ITEM_DONE,
                "response_id": self.response_id,
                "output_index": output_index,
                "item": item,
            }),
        ));
    }

    fn finish_response(&mut self, events: &mut Vec<(String, Value)>) -> Value {
        if self.completed {
            return self.final_response();
        }
        self.completed = true;

        if let Some(message) = self.assistant_message.take() {
            self.output_items[message.output_index]["status"] =
                Value::String("completed".to_string());
            let item = self.output_items[message.output_index].clone();
            events.push((
                EVENT_RESPONSE_OUTPUT_ITEM_DONE.to_string(),
                json!({
                    "type": EVENT_RESPONSE_OUTPUT_ITEM_DONE,
                    "response_id": self.response_id,
                    "output_index": message.output_index,
                    "item": item,
                }),
            ));
        }

        if self.output_items.is_empty() {
            self.output_items.push(json!({
                "id": response_item_id("msg"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "",
                    "annotations": [],
                }],
            }));
        }

        let final_response = self.final_response();
        events.push((
            EVENT_RESPONSE_COMPLETED.to_string(),
            json!({
                "type": EVENT_RESPONSE_COMPLETED,
                "response": final_response.clone(),
            }),
        ));
        final_response
    }

    fn final_response(&self) -> Value {
        let usage = self.usage.clone().unwrap_or_else(|| {
            json!({
                "input_tokens": 0,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens": 0,
                "output_tokens_details": { "reasoning_tokens": 0 },
                "total_tokens": 0,
            })
        });

        let mut response = json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": if self.completed { "completed" } else { "in_progress" },
            "model": self.model,
            "output": self.output_items,
            "parallel_tool_calls": self.request.parallel_tool_calls.unwrap_or(true),
            "tool_choice": self.request.tool_choice.clone().unwrap_or(Value::String("auto".to_string())),
            "store": self.request.store.unwrap_or(true),
            "usage": usage,
            "metadata": self.request.metadata.clone().unwrap_or_else(|| json!({})),
            "text": self
                .request
                .text
                .clone()
                .unwrap_or_else(|| json!({ "format": { "type": "text" } })),
            "temperature": self.request.temperature,
            "top_p": self.request.top_p,
            "previous_response_id": self.request.previous_response_id,
            "instructions": self.request.instructions,
            "max_output_tokens": self.request.max_output_tokens,
            "error": Value::Null,
            "incomplete_details": Value::Null,
        });

        if let Some(output_text) = output_text_from_output(&self.output_items) {
            response["output_text"] = Value::String(output_text);
        }

        response
    }

    fn response_stub(&self, status: &str) -> Value {
        let mut stub = self.final_response();
        stub["status"] = Value::String(status.to_string());
        stub["output"] = Value::Array(Vec::new());
        stub["usage"] = Value::Null;
        stub
    }
}

fn build_usage_from_messages_stream_usage(usage: &Value) -> Value {
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(input_tokens + output_tokens);

    json!({
        "input_tokens": input_tokens,
        "input_tokens_details": { "cached_tokens": 0 },
        "output_tokens": output_tokens,
        "output_tokens_details": { "reasoning_tokens": 0 },
        "total_tokens": total_tokens,
    })
}

fn output_text_from_output(output: &[Value]) -> Option<String> {
    output
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .and_then(|item| item.get("content").and_then(Value::as_array))
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .filter(|value| !value.is_empty())
}

fn rewrite_message_tool_names(message: &mut Value, tool_name_registry: &mut ToolNameRegistry) {
    let Some(tool_calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) else {
        return;
    };

    for tool_call in tool_calls {
        let Some(function) = tool_call.get_mut("function").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(name) = function.get("name").and_then(Value::as_str) else {
            continue;
        };

        let sanitized_name = tool_name_registry.sanitized_name(name);
        function.insert("name".to_string(), Value::String(sanitized_name));
    }
}

fn convert_response_input_item_to_chat_messages(item: &Value) -> Vec<Value> {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    match item_type.as_str() {
        "message" => {
            let role = item
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .trim()
                .to_ascii_lowercase();
            let Some(content) = item
                .get("content")
                .and_then(response_message_content_to_chat_content)
            else {
                return Vec::new();
            };
            sanitize_chat_message(json!({ "role": role, "content": content }))
                .into_iter()
                .collect()
        }
        "function_call" => {
            let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
            if name.trim().is_empty() {
                return Vec::new();
            }
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| response_item_id("call"));
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            sanitize_chat_message(json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                }]
            }))
            .into_iter()
            .collect()
        }
        "function_call_output" => {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if call_id.is_empty() {
                return Vec::new();
            }
            let output = item.get("output").cloned().unwrap_or(Value::Null);
            let content = output.as_str().map(str::to_string).unwrap_or_else(|| {
                serde_json::to_string(&output).unwrap_or_else(|_| String::new())
            });
            sanitize_chat_message(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": content,
            }))
            .into_iter()
            .collect()
        }
        _ => Vec::new(),
    }
}

fn build_chat_messages_from_input(input: &Value) -> Vec<Value> {
    if let Some(text) = content_text(input) {
        return sanitize_chat_message(json!({ "role": "user", "content": text }))
            .into_iter()
            .collect();
    }

    input
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(convert_response_input_item_to_chat_messages)
        .collect()
}

fn normalize_tool(tool: &Value, tool_name_registry: &mut ToolNameRegistry) -> Option<Value> {
    let object = tool.as_object()?;
    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function")
        .trim()
        .to_ascii_lowercase();

    if tool_type == "function" {
        let mut normalized_tool = tool.clone();
        let function = normalized_tool.get_mut("function")?.as_object_mut()?;
        let name = function.get("name").and_then(Value::as_str)?.trim();
        if name.is_empty() {
            return None;
        }

        let sanitized_name = tool_name_registry.sanitized_name(name);
        function.insert("name".to_string(), Value::String(sanitized_name));
        return Some(normalized_tool);
    }

    let name = object.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let sanitized_name = tool_name_registry.sanitized_name(name);

    Some(json!({
        "type": "function",
        "function": {
            "name": sanitized_name,
            "description": object.get("description").cloned().unwrap_or(Value::Null),
            "parameters": object.get("parameters").cloned().unwrap_or_else(|| json!({})),
        }
    }))
}

fn normalize_tool_choice(
    mut tool_choice: Value,
    tool_name_registry: &mut ToolNameRegistry,
) -> Value {
    let Some(object) = tool_choice.as_object_mut() else {
        return tool_choice;
    };

    if let Some(function) = object.get_mut("function").and_then(Value::as_object_mut) {
        if let Some(name) = function.get("name").and_then(Value::as_str) {
            let sanitized_name = tool_name_registry.sanitized_name(name);
            function.insert("name".to_string(), Value::String(sanitized_name));
        }
        return tool_choice;
    }

    if let Some(name) = object.get("name").and_then(Value::as_str) {
        let sanitized_name = tool_name_registry.sanitized_name(name);
        object.insert("name".to_string(), Value::String(sanitized_name));
    }

    tool_choice
}

fn normalize_response_format(request: &ResponsesGatewayRequest) -> Option<Value> {
    if let Some(value) = request.response_format.clone() {
        return Some(value);
    }

    let format = request.text.as_ref()?.get("format")?.clone();
    let format_type = format.get("type").and_then(Value::as_str)?.trim();
    if format_type.is_empty() {
        return None;
    }

    Some(format)
}

fn previous_record_messages(previous_record: &StoredResponseRecord) -> Vec<Value> {
    let mut messages = previous_record.request_messages.clone();
    if let Some(output_items) = previous_record.response.get("output").and_then(Value::as_array) {
        messages.extend(
            output_items
                .iter()
                .flat_map(convert_response_input_item_to_chat_messages),
        );
    }
    messages
}

fn merge_messages_with_previous(
    explicit_messages: Vec<Value>,
    previous_record: Option<&StoredResponseRecord>,
) -> Vec<Value> {
    let Some(previous_record) = previous_record else {
        return explicit_messages;
    };
    if explicit_messages.is_empty() {
        return previous_record_messages(previous_record);
    }

    let looks_incremental = explicit_messages.len() <= 2
        && explicit_messages.iter().all(|message| {
            matches!(
                message.get("role").and_then(Value::as_str),
                Some("user") | Some("tool")
            )
        });

    if !looks_incremental {
        return explicit_messages;
    }

    let mut merged = previous_record_messages(previous_record);
    merged.extend(explicit_messages);
    merged
}

fn prepend_instructions_message(messages: &mut Vec<Value>, instructions: &str) {
    if instructions.trim().is_empty() {
        return;
    }

    let already_present = messages
        .first()
        .and_then(Value::as_object)
        .is_some_and(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let content = message
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            matches!(role, "system" | "developer") && content.trim() == instructions.trim()
        });
    if already_present {
        return;
    }

    messages.insert(
        0,
        json!({
            "role": "system",
            "content": instructions.trim(),
        }),
    );
}

fn number_from_f64(value: f64) -> Option<Number> {
    Number::from_f64(value)
}

fn prepare_gateway_request(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request: &ResponsesGatewayRequest,
) -> AppResult<PreparedGatewayRequest> {
    let mut tool_name_registry = ToolNameRegistry::default();
    let previous_record = request
        .previous_response_id
        .as_deref()
        .and_then(|response_id| {
            state
                .responses
                .lock()
                .ok()
                .and_then(|responses| responses.get(response_id).cloned())
                .or_else(|| stored_response_from_disk(&state.codex_home, response_id))
        });

    let explicit_messages = if let Some(messages) = request.messages.clone() {
        messages
            .into_iter()
            .filter_map(sanitize_chat_message)
            .collect::<Vec<_>>()
    } else {
        request
            .input
            .as_ref()
            .map(build_chat_messages_from_input)
            .unwrap_or_default()
    };

    let effective_instructions = request
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            previous_record
                .as_ref()
                .and_then(|record| record.effective_instructions.clone())
        });

    let mut messages = merge_messages_with_previous(explicit_messages, previous_record.as_ref());
    if let Some(instructions) = effective_instructions.as_deref() {
        prepend_instructions_message(&mut messages, instructions);
    }
    for message in &mut messages {
        rewrite_message_tool_names(message, &mut tool_name_registry);
    }

    if messages.is_empty() {
        return Err(AppError::new(
            "PROVIDER_GATEWAY_INPUT_MISSING",
            "No request messages were available to forward to the upstream provider.",
        ));
    }
    let upstream_model = resolve_provider_target_model_for_request(
        &profile.profile_name,
        &request.model,
        Some(&state.codex_home),
    )?;

    let mut upstream = Map::new();
    upstream.insert("model".to_string(), Value::String(upstream_model));
    upstream.insert("messages".to_string(), Value::Array(messages.clone()));
    upstream.insert("stream".to_string(), Value::Bool(false));
    if let Some(tools) = request.tools.as_ref() {
        let normalized_tools = tools
            .iter()
            .filter_map(|tool| normalize_tool(tool, &mut tool_name_registry))
            .collect::<Vec<_>>();
        if !normalized_tools.is_empty() {
            upstream.insert("tools".to_string(), Value::Array(normalized_tools));
        }
    }
    if let Some(tool_choice) = request.tool_choice.clone() {
        upstream.insert(
            "tool_choice".to_string(),
            normalize_tool_choice(tool_choice, &mut tool_name_registry),
        );
    }
    if let Some(parallel_tool_calls) = request.parallel_tool_calls {
        upstream.insert(
            "parallel_tool_calls".to_string(),
            Value::Bool(parallel_tool_calls),
        );
    }
    if let Some(temperature) = request.temperature.and_then(number_from_f64) {
        upstream.insert("temperature".to_string(), Value::Number(temperature));
    }
    if let Some(top_p) = request.top_p.and_then(number_from_f64) {
        upstream.insert("top_p".to_string(), Value::Number(top_p));
    }
    if let Some(max_tokens) = request.max_output_tokens.or(request.max_completion_tokens) {
        upstream.insert(
            "max_tokens".to_string(),
            Value::Number(Number::from(max_tokens)),
        );
    }
    if let Some(response_format) = normalize_response_format(request) {
        upstream.insert("response_format".to_string(), response_format);
    }
    if let Some(user) = request
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        upstream.insert("user".to_string(), Value::String(user.to_string()));
    }

    Ok(PreparedGatewayRequest {
        upstream_body: Value::Object(upstream),
        effective_instructions,
        request_messages: messages,
        tool_name_registry,
    })
}

fn extract_chat_message_text(message: &Value) -> String {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return text.to_string();
    }

    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_block(text: &str) -> Value {
    json!({
        "type": "text",
        "text": text,
    })
}

fn messages_image_block_from_url(image_url: &str) -> Option<Value> {
    let trimmed = image_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(data_url) = trimmed.strip_prefix("data:") {
        let (metadata, data) = data_url.split_once(',')?;
        if !metadata.contains(";base64") || data.trim().is_empty() {
            return None;
        }

        let media_type = metadata
            .split(';')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("image/png");
        return Some(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data.trim(),
            }
        }));
    }

    Some(json!({
        "type": "image",
        "source": {
            "type": "url",
            "url": trimmed,
        }
    }))
}

fn parse_tool_arguments(arguments: &str) -> Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return json!({});
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(value) => json!({ "value": value }),
        Err(_) => json!({ "value": trimmed }),
    }
}

fn push_messages_content_message(messages: &mut Vec<Value>, role: &str, mut content: Vec<Value>) {
    if content.is_empty() {
        return;
    }

    if let Some(last_content) = messages
        .last_mut()
        .and_then(Value::as_object_mut)
        .filter(|message| message.get("role").and_then(Value::as_str) == Some(role))
        .and_then(|message| message.get_mut("content"))
        .and_then(Value::as_array_mut)
    {
        last_content.append(&mut content);
        return;
    }

    messages.push(json!({
        "role": role,
        "content": content,
    }));
}

fn push_messages_tool_result_message(messages: &mut Vec<Value>, tool_result: Value) {
    let can_append = messages
        .last()
        .and_then(Value::as_object)
        .is_some_and(|message| {
            if message.get("role").and_then(Value::as_str) != Some("user") {
                return false;
            }
            message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|content| {
                    !content.is_empty()
                        && content.iter().all(|part| {
                            part.get("type").and_then(Value::as_str) == Some("tool_result")
                        })
                })
        });

    if can_append {
        if let Some(last_content) = messages
            .last_mut()
            .and_then(Value::as_object_mut)
            .and_then(|message| message.get_mut("content"))
            .and_then(Value::as_array_mut)
        {
            last_content.push(tool_result);
            return;
        }
    }

    messages.push(json!({
        "role": "user",
        "content": [tool_result],
    }));
}

fn build_messages_text_blocks(message: &Value) -> Vec<Value> {
    let text = extract_chat_message_text(message);
    if text.trim().is_empty() {
        return Vec::new();
    }

    vec![text_block(&text)]
}

fn build_messages_content_blocks(message: &Value) -> Vec<Value> {
    let Some(content) = message.get("content") else {
        return build_messages_text_blocks(message);
    };

    if let Some(text) = content_text(content) {
        return vec![text_block(&text)];
    }

    let mut blocks = Vec::new();
    for part in content.as_array().into_iter().flatten() {
        let Some(part_type) = part.get("type").and_then(Value::as_str) else {
            continue;
        };
        match part_type {
            "input_text" | "output_text" | "text" => {
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    blocks.push(text_block(text));
                }
            }
            "input_image" | "image_url" => {
                let image_url = part
                    .pointer("/image_url/url")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("url").and_then(Value::as_str));
                if let Some(block) = image_url.and_then(messages_image_block_from_url) {
                    blocks.push(block);
                }
            }
            _ => {}
        }
    }

    if blocks.is_empty() {
        return build_messages_text_blocks(message);
    }

    blocks
}

fn build_messages_conversation(
    request_messages: &[Value],
    fallback_instructions: Option<&str>,
) -> (Option<String>, Vec<Value>) {
    let mut system_chunks = Vec::new();
    let mut messages = Vec::new();

    for message in request_messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .trim()
            .to_ascii_lowercase();

        match role.as_str() {
            "system" | "developer" => {
                let text = extract_chat_message_text(message);
                if !text.trim().is_empty() {
                    system_chunks.push(text);
                }
            }
            "assistant" => {
                let mut content = build_messages_text_blocks(message);
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for tool_call in tool_calls {
                        let name = tool_call
                            .pointer("/function/name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim();
                        if name.is_empty() {
                            continue;
                        }
                        let arguments = tool_call
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let call_id = tool_call
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| response_item_id("toolu"));
                        content.push(json!({
                            "type": "tool_use",
                            "id": call_id,
                            "name": name,
                            "input": parse_tool_arguments(arguments),
                        }));
                    }
                }
                push_messages_content_message(&mut messages, "assistant", content);
            }
            "tool" => {
                let tool_use_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if tool_use_id.is_empty() {
                    continue;
                }
                let tool_result = extract_chat_message_text(message);
                push_messages_tool_result_message(
                    &mut messages,
                    json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": tool_result,
                    }),
                );
            }
            _ => {
                push_messages_content_message(
                    &mut messages,
                    "user",
                    build_messages_content_blocks(message),
                );
            }
        }
    }

    if system_chunks.is_empty() {
        if let Some(instructions) = fallback_instructions
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            system_chunks.push(instructions.to_string());
        }
    }

    let system = (!system_chunks.is_empty()).then(|| system_chunks.join("\n\n"));
    (system, messages)
}

fn normalize_messages_tool_input_schema(schema: Option<&Value>) -> Value {
    match schema.cloned() {
        Some(Value::Object(_)) => schema.cloned().unwrap_or_else(|| json!({})),
        _ => json!({
            "type": "object",
            "properties": {},
        }),
    }
}

fn tool_name_from_definition(tool: &Value) -> Option<&str> {
    let object = tool.as_object()?;
    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function")
        .trim()
        .to_ascii_lowercase();

    if tool_type == "function" {
        return object
            .get("function")
            .and_then(Value::as_object)
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
    }

    object
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn messages_tool_definition_from_tool(
    tool: &Value,
    tool_name_registry: &mut ToolNameRegistry,
) -> Option<Value> {
    let object = tool.as_object()?;
    let tool_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("function")
        .trim()
        .to_ascii_lowercase();

    if tool_type == "function" {
        let function = object.get("function")?.as_object()?;
        let name = function.get("name").and_then(Value::as_str)?.trim();
        if name.is_empty() {
            return None;
        }
        let sanitized_name = tool_name_registry.sanitized_name(name);
        return Some(json!({
            "name": sanitized_name,
            "description": function.get("description").cloned().unwrap_or_else(|| Value::String(String::new())),
            "input_schema": normalize_messages_tool_input_schema(function.get("parameters")),
        }));
    }

    let name = object.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let sanitized_name = tool_name_registry.sanitized_name(name);
    Some(json!({
        "name": sanitized_name,
        "description": object.get("description").cloned().unwrap_or_else(|| Value::String(String::new())),
        "input_schema": normalize_messages_tool_input_schema(object.get("parameters")),
    }))
}

#[cfg_attr(not(test), allow(dead_code))]
fn build_messages_tools(tools: &[Value], tool_name_registry: &mut ToolNameRegistry) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| messages_tool_definition_from_tool(tool, tool_name_registry))
        .collect()
}

fn is_kimi_tool_enabled(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    if normalized.contains("tool_search") {
        return false;
    }
    true
}

fn build_kimi_messages_tools(
    tools: &[Value],
    tool_name_registry: &mut ToolNameRegistry,
) -> (Vec<Value>, Vec<String>, Vec<String>) {
    let mut forwarded = Vec::new();
    let mut original_names = Vec::new();
    let mut forwarded_names = Vec::new();

    for tool in tools {
        let Some(name) = tool_name_from_definition(tool) else {
            continue;
        };
        let name = name.to_string();
        original_names.push(name.clone());
        if !is_kimi_tool_enabled(&name) {
            continue;
        }
        let Some(normalized) = messages_tool_definition_from_tool(tool, tool_name_registry) else {
            continue;
        };
        forwarded_names.push(name);
        forwarded.push(normalized);
    }

    (forwarded, original_names, forwarded_names)
}
fn tool_choice_disables_tools(tool_choice: Option<&Value>) -> bool {
    match tool_choice {
        Some(Value::String(value)) => value.eq_ignore_ascii_case("none"),
        Some(Value::Object(object)) => object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("none")),
        _ => false,
    }
}

fn build_messages_tool_choice(
    tool_choice: Option<&Value>,
    tool_name_registry: &mut ToolNameRegistry,
) -> Option<Value> {
    match tool_choice {
        Some(Value::String(value)) if value.eq_ignore_ascii_case("auto") => {
            Some(json!({ "type": "auto" }))
        }
        Some(Value::String(value)) if value.eq_ignore_ascii_case("required") => {
            Some(json!({ "type": "any" }))
        }
        Some(Value::String(value)) if value.eq_ignore_ascii_case("none") => None,
        Some(Value::Object(object)) => {
            if let Some(function) = object.get("function").and_then(Value::as_object) {
                let name = function.get("name").and_then(Value::as_str)?.trim();
                if name.is_empty() {
                    return None;
                }
                let sanitized_name = tool_name_registry.sanitized_name(name);
                return Some(json!({
                    "type": "tool",
                    "name": sanitized_name,
                }));
            }

            let choice_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            if matches!(choice_type.as_str(), "auto" | "any") {
                return Some(json!({ "type": choice_type }));
            }
            if matches!(choice_type.as_str(), "required") {
                return Some(json!({ "type": "any" }));
            }
            if matches!(choice_type.as_str(), "none") {
                return None;
            }

            let name = object.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let sanitized_name = tool_name_registry.sanitized_name(name);
            Some(json!({
                "type": "tool",
                "name": sanitized_name,
            }))
        }
        _ => None,
    }
}

fn build_messages_upstream_body(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request: &ResponsesGatewayRequest,
    prepared: &PreparedGatewayRequest,
) -> Result<Value, GatewayError> {
    let upstream_model = resolve_provider_target_model_for_request(
        &profile.profile_name,
        &request.model,
        Some(&state.codex_home),
    )
    .map_err(|error| GatewayError::new(StatusCode::BAD_REQUEST, error.message))?;
    let (system, messages) = build_messages_conversation(
        &prepared.request_messages,
        prepared.effective_instructions.as_deref(),
    );
    if messages.is_empty() {
        return Err(GatewayError::new(
            StatusCode::BAD_REQUEST,
            "No request messages were available to forward to the upstream messages provider.",
        ));
    }

    let mut upstream = Map::new();
    upstream.insert("model".to_string(), Value::String(upstream_model));
    upstream.insert("messages".to_string(), Value::Array(messages));
    upstream.insert("stream".to_string(), Value::Bool(request.stream));
    upstream.insert(
        "max_tokens".to_string(),
        Value::Number(Number::from(
            request
                .max_output_tokens
                .or(request.max_completion_tokens)
                .unwrap_or(DEFAULT_MESSAGES_MAX_TOKENS),
        )),
    );
    if let Some(temperature) = request.temperature.and_then(number_from_f64) {
        upstream.insert("temperature".to_string(), Value::Number(temperature));
    }
    if let Some(top_p) = request.top_p.and_then(number_from_f64) {
        upstream.insert("top_p".to_string(), Value::Number(top_p));
    }

    let mut original_tool_names = Vec::new();
    let mut forwarded_tool_names = Vec::new();
    if !tool_choice_disables_tools(request.tool_choice.as_ref()) {
        if let Some(tools) = request.tools.as_ref() {
            let mut tool_name_registry = prepared.tool_name_registry.clone();
            let (normalized_tools, original_names, forwarded_names) =
                build_kimi_messages_tools(tools, &mut tool_name_registry);
            original_tool_names = original_names;
            forwarded_tool_names = forwarded_names;
            if !normalized_tools.is_empty() {
                upstream.insert("tools".to_string(), Value::Array(normalized_tools));
            }
            if let Some(tool_choice) =
                build_messages_tool_choice(request.tool_choice.as_ref(), &mut tool_name_registry)
            {
                upstream.insert("tool_choice".to_string(), tool_choice);
            }
        } else if let Some(tool_choice) = build_messages_tool_choice(
            request.tool_choice.as_ref(),
            &mut prepared.tool_name_registry.clone(),
        ) {
            upstream.insert("tool_choice".to_string(), tool_choice);
        }
    }
    if let Some(system) = system {
        upstream.insert("system".to_string(), Value::String(system));
    }

    append_kimi_gateway_request_diagnostic(
        state,
        profile,
        Some(request.model.as_str()),
        &original_tool_names,
        &forwarded_tool_names,
        count_request_images(&prepared.request_messages),
    );

    Ok(Value::Object(upstream))
}

fn build_output_items_from_chat_response(
    chat_response: &Value,
    tool_name_registry: &ToolNameRegistry,
) -> Vec<Value> {
    let message = chat_response
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = extract_chat_message_text(&message);
    let mut output = Vec::new();
    if !text.trim().is_empty() {
        output.push(json!({
            "id": response_item_id("msg"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text,
                "annotations": [],
            }],
        }));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let name = tool_call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if name.is_empty() {
                continue;
            }
            let arguments = tool_call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let call_id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| response_item_id("call"));
            output.push(json!({
                "id": response_item_id("fc"),
                "type": "function_call",
                "status": "completed",
                "call_id": call_id,
                "name": tool_name_registry.original_name(name),
                "arguments": arguments,
            }));
        }
    }

    if output.is_empty() {
        output.push(json!({
            "id": response_item_id("msg"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "",
                "annotations": [],
            }],
        }));
    }

    output
}

fn build_usage_from_chat_response(chat_response: &Value) -> Value {
    let prompt_tokens = chat_response
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completion_tokens = chat_response
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_tokens = chat_response
        .pointer("/usage/total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(prompt_tokens + completion_tokens);
    let reasoning_tokens = chat_response
        .pointer("/usage/completion_tokens_details/reasoning_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    json!({
        "input_tokens": prompt_tokens,
        "input_tokens_details": { "cached_tokens": 0 },
        "output_tokens": completion_tokens,
        "output_tokens_details": { "reasoning_tokens": reasoning_tokens },
        "total_tokens": total_tokens,
    })
}

fn build_output_items_from_messages_response(
    messages_response: &Value,
    tool_name_registry: &ToolNameRegistry,
) -> Vec<Value> {
    let mut output = Vec::new();
    let mut text_parts = Vec::new();

    if let Some(text) = messages_response.get("content").and_then(Value::as_str) {
        if !text.trim().is_empty() {
            text_parts.push(json!({
                "type": "output_text",
                "text": text,
                "annotations": [],
            }));
        }
    }

    if let Some(content) = messages_response.get("content").and_then(Value::as_array) {
        for part in content {
            let part_type = part.get("type").and_then(Value::as_str).unwrap_or_default();
            match part_type {
                "text" => {
                    if let Some(text) = part
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        text_parts.push(json!({
                            "type": "output_text",
                            "text": text,
                            "annotations": [],
                        }));
                    }
                }
                "tool_use" => {
                    let name = part
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim();
                    if name.is_empty() {
                        continue;
                    }
                    let call_id = part
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| response_item_id("call"));
                    let arguments = part.get("input").cloned().unwrap_or_else(|| json!({}));
                    output.push(json!({
                        "id": response_item_id("fc"),
                        "type": "function_call",
                        "status": "completed",
                        "call_id": call_id,
                        "name": tool_name_registry.original_name(name),
                        "arguments": serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string()),
                    }));
                }
                _ => {}
            }
        }
    }

    if !text_parts.is_empty() {
        output.insert(
            0,
            json!({
                "id": response_item_id("msg"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": text_parts,
            }),
        );
    }

    if output.is_empty() {
        output.push(json!({
            "id": response_item_id("msg"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "",
                "annotations": [],
            }],
        }));
    }

    output
}

fn build_usage_from_messages_response(messages_response: &Value) -> Value {
    let input_tokens = messages_response
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = messages_response
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    json!({
        "input_tokens": input_tokens,
        "input_tokens_details": { "cached_tokens": 0 },
        "output_tokens": output_tokens,
        "output_tokens_details": { "reasoning_tokens": 0 },
        "total_tokens": input_tokens + output_tokens,
    })
}

fn build_final_responses_object(
    request: &ResponsesGatewayRequest,
    chat_response: &Value,
    tool_name_registry: &ToolNameRegistry,
) -> (String, Value) {
    let response_id = response_id();
    let output = build_output_items_from_chat_response(chat_response, tool_name_registry);
    let usage = build_usage_from_chat_response(chat_response);
    let model = chat_response
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(request.model.as_str());

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": unix_timestamp_secs(),
        "status": "completed",
        "model": model,
        "output": output,
        "parallel_tool_calls": request.parallel_tool_calls.unwrap_or(true),
        "tool_choice": request.tool_choice.clone().unwrap_or(Value::String("auto".to_string())),
        "store": request.store.unwrap_or(true),
        "usage": usage,
        "metadata": request.metadata.clone().unwrap_or_else(|| json!({})),
        "text": request.text.clone().unwrap_or_else(|| json!({ "format": { "type": "text" } })),
        "temperature": request.temperature,
        "top_p": request.top_p,
        "previous_response_id": request.previous_response_id,
        "instructions": request.instructions,
        "max_output_tokens": request.max_output_tokens,
        "error": Value::Null,
        "incomplete_details": Value::Null,
    });

    let output_text = response
        .pointer("/output/0/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !output_text.is_empty() {
        response["output_text"] = Value::String(output_text);
    }

    (response_id, response)
}

fn build_final_responses_object_from_messages(
    request: &ResponsesGatewayRequest,
    messages_response: &Value,
    tool_name_registry: &ToolNameRegistry,
) -> (String, Value) {
    let response_id = response_id();
    let output = build_output_items_from_messages_response(messages_response, tool_name_registry);
    let usage = build_usage_from_messages_response(messages_response);
    let model = messages_response
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(request.model.as_str());

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": unix_timestamp_secs(),
        "status": "completed",
        "model": model,
        "output": output,
        "parallel_tool_calls": request.parallel_tool_calls.unwrap_or(true),
        "tool_choice": request.tool_choice.clone().unwrap_or(Value::String("auto".to_string())),
        "store": request.store.unwrap_or(true),
        "usage": usage,
        "metadata": request.metadata.clone().unwrap_or_else(|| json!({})),
        "text": request.text.clone().unwrap_or_else(|| json!({ "format": { "type": "text" } })),
        "temperature": request.temperature,
        "top_p": request.top_p,
        "previous_response_id": request.previous_response_id,
        "instructions": request.instructions,
        "max_output_tokens": request.max_output_tokens,
        "error": Value::Null,
        "incomplete_details": Value::Null,
    });

    let output_text = response
        .pointer("/output/0/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !output_text.is_empty() {
        response["output_text"] = Value::String(output_text);
    }

    (response_id, response)
}

fn response_stub_from_final(final_response: &Value, status: &str) -> Value {
    let mut stub = final_response.clone();
    stub["status"] = Value::String(status.to_string());
    stub["output"] = Value::Array(Vec::new());
    stub["usage"] = Value::Null;
    stub
}

fn build_sse_events(final_response: &Value) -> Vec<Event> {
    let response_id = final_response
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut events = vec![
        sse_event(
            EVENT_RESPONSE_CREATED,
            json!({
                "type": EVENT_RESPONSE_CREATED,
                "response": response_stub_from_final(final_response, "in_progress"),
            }),
        ),
        sse_event(
            EVENT_RESPONSE_IN_PROGRESS,
            json!({
                "type": EVENT_RESPONSE_IN_PROGRESS,
                "response": response_stub_from_final(final_response, "in_progress"),
            }),
        ),
    ];

    if let Some(output_items) = final_response.get("output").and_then(Value::as_array) {
        for (output_index, item) in output_items.iter().enumerate() {
            events.push(sse_event(
                EVENT_RESPONSE_OUTPUT_ITEM_ADDED,
                json!({
                    "type": EVENT_RESPONSE_OUTPUT_ITEM_ADDED,
                    "response_id": response_id,
                    "output_index": output_index,
                    "item": item,
                }),
            ));

            if item.get("type").and_then(Value::as_str) == Some("message") {
                if let Some(content_items) = item.get("content").and_then(Value::as_array) {
                    for (content_index, part) in content_items.iter().enumerate() {
                        let item_id = item.get("id").cloned().unwrap_or(Value::Null);
                        events.push(sse_event(
                            EVENT_RESPONSE_CONTENT_PART_ADDED,
                            json!({
                                "type": EVENT_RESPONSE_CONTENT_PART_ADDED,
                                "response_id": response_id,
                                "output_index": output_index,
                                "item_id": item_id,
                                "content_index": content_index,
                                "part": {
                                    "type": "output_text",
                                    "text": "",
                                    "annotations": [],
                                },
                            }),
                        ));

                        let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                        events.push(sse_event(
                            EVENT_RESPONSE_OUTPUT_TEXT_DELTA,
                            json!({
                                "type": EVENT_RESPONSE_OUTPUT_TEXT_DELTA,
                                "response_id": response_id,
                                "output_index": output_index,
                                "item_id": item_id,
                                "content_index": content_index,
                                "delta": text,
                            }),
                        ));
                        events.push(sse_event(
                            EVENT_RESPONSE_OUTPUT_TEXT_DONE,
                            json!({
                                "type": EVENT_RESPONSE_OUTPUT_TEXT_DONE,
                                "response_id": response_id,
                                "output_index": output_index,
                                "item_id": item_id,
                                "content_index": content_index,
                                "text": text,
                            }),
                        ));
                        events.push(sse_event(
                            EVENT_RESPONSE_CONTENT_PART_DONE,
                            json!({
                                "type": EVENT_RESPONSE_CONTENT_PART_DONE,
                                "response_id": response_id,
                                "output_index": output_index,
                                "item_id": item_id,
                                "content_index": content_index,
                                "part": part,
                            }),
                        ));
                    }
                }
            }

            events.push(sse_event(
                EVENT_RESPONSE_OUTPUT_ITEM_DONE,
                json!({
                    "type": EVENT_RESPONSE_OUTPUT_ITEM_DONE,
                    "response_id": response_id,
                    "output_index": output_index,
                    "item": item,
                }),
            ));
        }
    }

    events.push(sse_event(
        EVENT_RESPONSE_COMPLETED,
        json!({
            "type": EVENT_RESPONSE_COMPLETED,
            "response": final_response,
        }),
    ));
    events
}

fn sse_event(event_name: &str, payload: Value) -> Event {
    Event::default()
        .event(event_name)
        .data(serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()))
}

fn sse_event_bytes(event_name: &str, payload: Value) -> Bytes {
    let payload = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event_name}\ndata: {payload}\n\n"))
}

async fn send_kimi_streaming_batch(
    sender: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
    batch: KimiStreamingEventBatch,
) -> bool {
    for (event_name, payload) in batch.events {
        if sender
            .send(Ok(sse_event_bytes(&event_name, payload)))
            .await
            .is_err()
        {
            return false;
        }
    }

    true
}

async fn resolve_active_profile_context(
    state: &GatewayAppState,
    headers: &HeaderMap,
) -> Result<ActiveProfileContext, GatewayError> {
    let backup_root = get_backup_root(Some(&state.codex_home));
    let profile_name = resolve_current_profile(&backup_root).ok_or_else(|| {
        GatewayError::new(
            StatusCode::BAD_REQUEST,
            "No active profile is selected for the provider gateway.",
        )
    })?;
    let base_url =
        load_normalized_profile_base_url(&profile_name, &state.codex_home).ok_or_else(|| {
            GatewayError::new(
                StatusCode::BAD_REQUEST,
                "The active profile does not have a Base Url configured.",
            )
        })?;
    let provider_protocol = if is_kimi_coding_base_url(&base_url) {
        load_cached_provider_protocol(&profile_name, &state.codex_home)
            .or_else(|| Some(PROVIDER_PROTOCOL_MESSAGES.to_string()))
    } else {
        let protocol_profile_name = profile_name.clone();
        let protocol_codex_home = state.codex_home.clone();
        tokio::task::spawn_blocking(move || {
            resolve_provider_protocol(&protocol_profile_name, &protocol_codex_home)
        })
        .await
        .map_err(|error| {
            GatewayError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Provider protocol detection task failed: {error}"),
            )
        })?
        .map_err(|error| GatewayError::new(StatusCode::BAD_GATEWAY, error.message))?
    };

    let profile_api_key = load_profile_api_key(&profile_name, &state.codex_home);
    let authorization_header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            profile_api_key
                .clone()
                .map(|api_key| format!("Bearer {api_key}"))
        });
    let api_key = profile_api_key
        .or_else(|| api_key_from_authorization_header(authorization_header.as_deref()));

    Ok(ActiveProfileContext {
        profile_name,
        base_url,
        provider_protocol,
        authorization_header,
        api_key,
    })
}

fn build_upstream_headers(
    authorization_header: Option<&str>,
) -> Result<ReqwestHeaderMap, GatewayError> {
    let mut headers = ReqwestHeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static(GATEWAY_USER_AGENT));
    if let Some(authorization_header) = authorization_header {
        let value = HeaderValue::from_str(authorization_header).map_err(|error| {
            GatewayError::new(
                StatusCode::BAD_REQUEST,
                format!("Failed to build Authorization header for provider gateway: {error}"),
            )
        })?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

fn build_messages_upstream_headers(
    authorization_header: Option<&str>,
    api_key: Option<&str>,
    streaming: bool,
) -> Result<ReqwestHeaderMap, GatewayError> {
    let mut headers = build_upstream_headers(authorization_header)?;
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(if streaming {
            "text/event-stream"
        } else {
            "application/json"
        }),
    );
    headers.insert(
        HeaderName::from_static(ANTHROPIC_VERSION_HEADER),
        HeaderValue::from_static(ANTHROPIC_VERSION_VALUE),
    );

    if let Some(api_key) = api_key {
        headers.remove(AUTHORIZATION);
        let value = HeaderValue::from_str(api_key).map_err(|error| {
            GatewayError::new(
                StatusCode::BAD_REQUEST,
                format!("Failed to build x-api-key header for provider gateway: {error}"),
            )
        })?;
        headers.insert(HeaderName::from_static(MESSAGES_API_KEY_HEADER), value);
    }

    Ok(headers)
}

async fn request_upstream_chat_completion(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request_body: &Value,
) -> Result<Value, GatewayError> {
    let mut last_error: Option<GatewayError> = None;
    for endpoint in build_chat_completion_endpoint_candidates(&profile.base_url) {
        let response = state
            .http_client
            .post(&endpoint)
            .headers(build_upstream_headers(
                profile.authorization_header.as_deref(),
            )?)
            .json(request_body)
            .send()
            .await;

        let response = match response {
            Ok(value) => value,
            Err(error) => {
                last_error = Some(GatewayError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to reach upstream provider endpoint {endpoint}: {error}"),
                ));
                continue;
            }
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            last_error = Some(GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Upstream provider endpoint was not found: {endpoint}"),
            ));
            continue;
        }

        if !status.is_success() {
            return Err(GatewayError::new(
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                if body.trim().is_empty() {
                    format!("Upstream provider returned HTTP {status} from {endpoint}.")
                } else {
                    body
                },
            ));
        }

        return serde_json::from_str::<Value>(&body).map_err(|error| {
            GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse upstream provider response from {endpoint}: {error}"),
            )
        });
    }

    Err(last_error.unwrap_or_else(|| {
        GatewayError::new(
            StatusCode::BAD_GATEWAY,
            "No upstream chat/completions endpoint could be reached for this provider.",
        )
    }))
}

async fn request_upstream_messages(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request_body: &Value,
) -> Result<Value, GatewayError> {
    let mut last_error: Option<GatewayError> = None;
    let request_model = request_body.get("model").and_then(Value::as_str);
    for endpoint in build_messages_endpoint_candidates(&profile.base_url) {
        let response = state
            .http_client
            .post(&endpoint)
            .headers(build_messages_upstream_headers(
                profile.authorization_header.as_deref(),
                profile.api_key.as_deref(),
                false,
            )?)
            .json(request_body)
            .send()
            .await;

        let response = match response {
            Ok(value) => value,
            Err(error) => {
                append_kimi_gateway_diagnostic(
                    state,
                    profile,
                    "transport_error",
                    Some(&endpoint),
                    request_model,
                    None,
                    None,
                    Some(&error.to_string()),
                );
                last_error = Some(GatewayError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to reach upstream provider endpoint {endpoint}: {error}"),
                ));
                continue;
            }
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            append_kimi_gateway_diagnostic(
                state,
                profile,
                "endpoint_missing",
                Some(&endpoint),
                request_model,
                Some(status),
                Some(&body),
                None,
            );
            last_error = Some(GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Upstream provider endpoint was not found: {endpoint}"),
            ));
            continue;
        }

        if !status.is_success() {
            append_kimi_gateway_diagnostic(
                state,
                profile,
                "upstream_http_error",
                Some(&endpoint),
                request_model,
                Some(status),
                Some(&body),
                None,
            );
            return Err(GatewayError::new(
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                if body.trim().is_empty() {
                    format!("Upstream provider returned HTTP {status} from {endpoint}.")
                } else {
                    body
                },
            ));
        }

        let parsed = serde_json::from_str::<Value>(&body).map_err(|error| {
            append_kimi_gateway_diagnostic(
                state,
                profile,
                "response_parse_error",
                Some(&endpoint),
                request_model,
                Some(status),
                Some(&body),
                Some(&error.to_string()),
            );
            GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse upstream provider response from {endpoint}: {error}"),
            )
        })?;

        append_kimi_gateway_diagnostic(
            state,
            profile,
            "upstream_success",
            Some(&endpoint),
            request_model,
            Some(status),
            None,
            None,
        );
        return Ok(parsed);
    }

    append_kimi_gateway_diagnostic(
        state,
        profile,
        "no_reachable_endpoint",
        None,
        request_model,
        None,
        None,
        last_error.as_ref().map(|error| error.message.as_str()),
    );
    Err(last_error.unwrap_or_else(|| {
        GatewayError::new(
            StatusCode::BAD_GATEWAY,
            "No upstream messages endpoint could be reached for this provider.",
        )
    }))
}

async fn request_upstream_messages_stream(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request_body: &Value,
) -> Result<(String, reqwest::Response), GatewayError> {
    let mut last_error: Option<GatewayError> = None;
    let request_model = request_body.get("model").and_then(Value::as_str);
    for endpoint in build_messages_endpoint_candidates(&profile.base_url) {
        let response = state
            .http_client
            .post(&endpoint)
            .headers(build_messages_upstream_headers(
                profile.authorization_header.as_deref(),
                profile.api_key.as_deref(),
                true,
            )?)
            .json(request_body)
            .send()
            .await;

        let response = match response {
            Ok(value) => value,
            Err(error) => {
                append_kimi_gateway_diagnostic(
                    state,
                    profile,
                    "stream_transport_error",
                    Some(&endpoint),
                    request_model,
                    None,
                    None,
                    Some(&error.to_string()),
                );
                last_error = Some(GatewayError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to reach upstream provider endpoint {endpoint}: {error}"),
                ));
                continue;
            }
        };

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            append_kimi_gateway_diagnostic(
                state,
                profile,
                "stream_endpoint_missing",
                Some(&endpoint),
                request_model,
                Some(status),
                None,
                None,
            );
            last_error = Some(GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Upstream provider endpoint was not found: {endpoint}"),
            ));
            continue;
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            append_kimi_gateway_diagnostic(
                state,
                profile,
                "stream_upstream_http_error",
                Some(&endpoint),
                request_model,
                Some(status),
                Some(&body),
                None,
            );
            return Err(GatewayError::new(
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                if body.trim().is_empty() {
                    format!("Upstream provider returned HTTP {status} from {endpoint}.")
                } else {
                    body
                },
            ));
        }

        append_kimi_gateway_diagnostic(
            state,
            profile,
            "stream_upstream_open",
            Some(&endpoint),
            request_model,
            Some(status),
            None,
            None,
        );
        return Ok((endpoint, response));
    }

    append_kimi_gateway_diagnostic(
        state,
        profile,
        "stream_no_reachable_endpoint",
        None,
        request_model,
        None,
        None,
        last_error.as_ref().map(|error| error.message.as_str()),
    );
    Err(last_error.unwrap_or_else(|| {
        GatewayError::new(
            StatusCode::BAD_GATEWAY,
            "No upstream messages endpoint could be reached for this provider.",
        )
    }))
}

fn build_responses_proxy_request(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request: &ResponsesGatewayRequest,
) -> Result<Value, GatewayError> {
    let mut upstream_request = serde_json::to_value(request).map_err(|error| {
        GatewayError::new(
            StatusCode::BAD_REQUEST,
            format!("Failed to serialize responses request for proxying: {error}"),
        )
    })?;
    let upstream_model = resolve_provider_target_model_for_request(
        &profile.profile_name,
        &request.model,
        Some(&state.codex_home),
    )
    .map_err(|error| GatewayError::new(StatusCode::BAD_REQUEST, error.message))?;
    upstream_request["model"] = Value::String(upstream_model);
    prune_json_nulls(&mut upstream_request);
    Ok(upstream_request)
}

fn response_from_json_value(payload: Value) -> Response {
    Json(payload).into_response()
}

fn response_from_sse_stream<S>(stream: S) -> Response
where
    S: futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| {
            GatewayError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build streaming gateway response.",
            )
            .into_response()
        })
}

fn response_from_generated_sse_stream<S>(stream: S) -> Response
where
    S: futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| {
            GatewayError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build generated streaming gateway response.",
            )
            .into_response()
        })
}

async fn request_upstream_responses(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request: &ResponsesGatewayRequest,
) -> Result<GatewayResponsePayload, GatewayError> {
    let request_body = build_responses_proxy_request(state, profile, request)?;
    let mut last_error: Option<GatewayError> = None;
    for endpoint in build_responses_endpoint_candidates(&profile.base_url) {
        let response = state
            .http_client
            .post(&endpoint)
            .headers(build_upstream_headers(
                profile.authorization_header.as_deref(),
            )?)
            .json(&request_body)
            .send()
            .await;

        let response = match response {
            Ok(value) => value,
            Err(error) => {
                last_error = Some(GatewayError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("Failed to reach upstream provider endpoint {endpoint}: {error}"),
                ));
                continue;
            }
        };

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            last_error = Some(GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Upstream provider endpoint was not found: {endpoint}"),
            ));
            continue;
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(GatewayError::new(
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                if body.trim().is_empty() {
                    format!("Upstream provider returned HTTP {status} from {endpoint}.")
                } else {
                    body
                },
            ));
        }

        if request.stream {
            return Ok(GatewayResponsePayload::Response(response_from_sse_stream(
                response.bytes_stream(),
            )));
        }

        let parsed = response.json::<Value>().await.map_err(|error| {
            GatewayError::new(
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse upstream responses payload from {endpoint}: {error}"),
            )
        })?;
        return Ok(GatewayResponsePayload::Response(response_from_json_value(
            parsed,
        )));
    }

    Err(last_error.unwrap_or_else(|| {
        GatewayError::new(
            StatusCode::BAD_GATEWAY,
            "No upstream responses endpoint could be reached for this provider.",
        )
    }))
}

fn remember_response(
    state: &GatewayAppState,
    response_id: String,
    request: &ResponsesGatewayRequest,
    prepared: &PreparedGatewayRequest,
    response: &Value,
) {
    let record = StoredResponseRecord {
        id: response_id,
        previous_response_id: request.previous_response_id.clone(),
        effective_instructions: prepared.effective_instructions.clone(),
        request_messages: prepared.request_messages.clone(),
        response: response.clone(),
    };
    if let Ok(mut responses) = state.responses.lock() {
        responses.insert(record.id.clone(), record.clone());
    }
    let _ = save_response_record(&state.codex_home, &record);
}

async fn build_kimi_messages_streaming_response(
    state: &GatewayAppState,
    profile: &ActiveProfileContext,
    request: &ResponsesGatewayRequest,
    prepared: &PreparedGatewayRequest,
) -> Result<GatewayResponsePayload, GatewayError> {
    let upstream_request = build_messages_upstream_body(state, profile, request, prepared)?;
    let upstream_model = upstream_request
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(request.model.as_str())
        .to_string();
    let (endpoint, response) =
        request_upstream_messages_stream(state, profile, &upstream_request).await?;

    let state_for_task = state.clone();
    let profile_for_task = profile.clone();
    let request_for_task = request.clone();
    let prepared_for_task = prepared.clone();
    let endpoint_for_task = endpoint.clone();
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    tokio::spawn(async move {
        let mut translator = KimiStreamingTranslator::new(&request_for_task, upstream_model);
        let initial_batch = KimiStreamingEventBatch {
            events: translator.initial_events(),
            completed_response: None,
        };
        if !send_kimi_streaming_batch(&sender, initial_batch).await {
            return;
        }

        let mut parser = UpstreamSseParser::default();
        let mut stream = response.bytes_stream();
        let mut completed_response: Option<Value> = None;

        while let Some(next_chunk) = stream.next().await {
            let chunk = match next_chunk {
                Ok(value) => value,
                Err(error) => {
                    append_kimi_gateway_diagnostic(
                        &state_for_task,
                        &profile_for_task,
                        "stream_read_error",
                        Some(&endpoint_for_task),
                        Some(translator.model.as_str()),
                        None,
                        None,
                        Some(&error.to_string()),
                    );
                    break;
                }
            };

            let decoded = String::from_utf8_lossy(&chunk);
            for upstream_event in parser.push_chunk(&decoded) {
                let payload = if upstream_event.data.trim().is_empty() {
                    Value::Null
                } else {
                    match serde_json::from_str::<Value>(&upstream_event.data) {
                        Ok(value) => value,
                        Err(error) => {
                            append_kimi_gateway_diagnostic(
                                &state_for_task,
                                &profile_for_task,
                                "stream_event_parse_error",
                                Some(&endpoint_for_task),
                                Some(translator.model.as_str()),
                                None,
                                Some(&upstream_event.data),
                                Some(&error.to_string()),
                            );
                            continue;
                        }
                    }
                };

                let batch = translator.process_upstream_event(
                    &upstream_event.event,
                    &payload,
                    &prepared_for_task.tool_name_registry,
                );
                if let Some(final_response) = batch.completed_response.clone() {
                    completed_response = Some(final_response);
                }
                if !send_kimi_streaming_batch(&sender, batch).await {
                    return;
                }
                if completed_response.is_some() {
                    break;
                }
            }

            if completed_response.is_some() {
                break;
            }
        }

        if completed_response.is_none() {
            for upstream_event in parser.finish() {
                let payload = if upstream_event.data.trim().is_empty() {
                    Value::Null
                } else {
                    match serde_json::from_str::<Value>(&upstream_event.data) {
                        Ok(value) => value,
                        Err(error) => {
                            append_kimi_gateway_diagnostic(
                                &state_for_task,
                                &profile_for_task,
                                "stream_event_parse_error",
                                Some(&endpoint_for_task),
                                Some(translator.model.as_str()),
                                None,
                                Some(&upstream_event.data),
                                Some(&error.to_string()),
                            );
                            continue;
                        }
                    }
                };

                let batch = translator.process_upstream_event(
                    &upstream_event.event,
                    &payload,
                    &prepared_for_task.tool_name_registry,
                );
                if let Some(final_response) = batch.completed_response.clone() {
                    completed_response = Some(final_response);
                }
                if !send_kimi_streaming_batch(&sender, batch).await {
                    return;
                }
                if completed_response.is_some() {
                    break;
                }
            }
        }

        if completed_response.is_none() {
            let batch = translator.finish_if_needed();
            if let Some(final_response) = batch.completed_response.clone() {
                completed_response = Some(final_response);
            }
            if !send_kimi_streaming_batch(&sender, batch).await {
                return;
            }
        }

        if let Some(final_response) = completed_response {
            append_kimi_gateway_diagnostic(
                &state_for_task,
                &profile_for_task,
                "stream_completed",
                Some(&endpoint_for_task),
                final_response.get("model").and_then(Value::as_str),
                None,
                None,
                None,
            );
            remember_response(
                &state_for_task,
                translator.response_id.clone(),
                &request_for_task,
                &prepared_for_task,
                &final_response,
            );
        }
    });

    let stream = stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|item| (item, receiver))
    });

    Ok(GatewayResponsePayload::Response(
        response_from_generated_sse_stream(stream),
    ))
}

async fn build_gateway_response(
    state: &GatewayAppState,
    headers: &HeaderMap,
    request: ResponsesGatewayRequest,
) -> Result<GatewayResponsePayload, GatewayError> {
    let profile = resolve_active_profile_context(state, headers).await?;
    let uses_kimi_messages = uses_kimi_messages_gateway(&profile);
    if uses_kimi_messages {
        append_kimi_gateway_diagnostic(
            state,
            &profile,
            "route_selected",
            None,
            Some(request.model.as_str()),
            None,
            None,
            None,
        );
        let prepared = prepare_gateway_request(state, &profile, &request)
            .map_err(|error| GatewayError::new(StatusCode::BAD_REQUEST, error.message))?;
        if request.stream {
            return build_kimi_messages_streaming_response(state, &profile, &request, &prepared)
                .await;
        }
        let upstream_request = build_messages_upstream_body(state, &profile, &request, &prepared)?;
        let upstream_response =
            request_upstream_messages(state, &profile, &upstream_request).await?;
        let (response_id, final_response) = build_final_responses_object_from_messages(
            &request,
            &upstream_response,
            &prepared.tool_name_registry,
        );
        remember_response(state, response_id, &request, &prepared, &final_response);
        if request.stream {
            return Ok(GatewayResponsePayload::Sse(build_sse_events(
                &final_response,
            )));
        }

        return Ok(GatewayResponsePayload::Json(final_response));
    }
    if profile.provider_protocol.as_deref() == Some(PROVIDER_PROTOCOL_MESSAGES) {
        return Err(GatewayError::new(
            StatusCode::BAD_REQUEST,
            "The detected provider protocol `messages` is currently only supported for Kimi coding providers.",
        ));
    }
    if profile.provider_protocol.as_deref() == Some(PROVIDER_PROTOCOL_COMPLETIONS) {
        return Err(GatewayError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "The detected provider protocol `{}` is not yet supported by the local gateway.",
                profile.provider_protocol.as_deref().unwrap_or_default()
            ),
        ));
    }
    let prefers_chat_bridge =
        profile.provider_protocol.as_deref() == Some(PROVIDER_PROTOCOL_CHAT_COMPLETIONS);
    if !prefers_chat_bridge {
        match request_upstream_responses(state, &profile, &request).await {
            Ok(payload) => return Ok(payload),
            Err(error)
                if profile.provider_protocol.is_none()
                    && error.message.contains("endpoint was not found") => {}
            Err(error) => return Err(error),
        }
    }

    let prepared = prepare_gateway_request(state, &profile, &request)
        .map_err(|error| GatewayError::new(StatusCode::BAD_REQUEST, error.message))?;
    let upstream_response =
        request_upstream_chat_completion(state, &profile, &prepared.upstream_body).await?;
    let (response_id, final_response) =
        build_final_responses_object(&request, &upstream_response, &prepared.tool_name_registry);
    remember_response(state, response_id, &request, &prepared, &final_response);
    if request.stream {
        return Ok(GatewayResponsePayload::Sse(build_sse_events(
            &final_response,
        )));
    }

    Ok(GatewayResponsePayload::Json(final_response))
}

async fn health_handler() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn models_handler(State(state): State<GatewayAppState>) -> Response {
    let backup_root = get_backup_root(Some(&state.codex_home));
    let Some(profile_name) = resolve_current_profile(&backup_root) else {
        return GatewayError::new(
            StatusCode::BAD_REQUEST,
            "No active profile is selected for the provider gateway.",
        )
        .into_response();
    };

    let codex_home = state.codex_home.clone();
    let result = tokio::task::spawn_blocking(move || {
        fetch_profile_provider_models(&profile_name, Some(&codex_home))
    })
    .await;

    let response = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            return GatewayError::new(StatusCode::BAD_GATEWAY, error.message).into_response()
        }
        Err(error) => {
            return GatewayError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Provider gateway model fetch task failed: {error}"),
            )
            .into_response()
        }
    };

    let models = response
        .models
        .iter()
        .map(|model| {
            json!({
                "id": model,
                "object": "model",
                "owned_by": "provider",
            })
        })
        .collect::<Vec<_>>();

    Json(json!({
        "object": "list",
        "data": models,
        "provider_protocol": response.provider_protocol,
    }))
    .into_response()
}

async fn responses_handler(
    State(state): State<GatewayAppState>,
    headers: HeaderMap,
    Json(request): Json<ResponsesGatewayRequest>,
) -> Response {
    match build_gateway_response(&state, &headers, request).await {
        Ok(GatewayResponsePayload::Json(payload)) => Json(payload).into_response(),
        Ok(GatewayResponsePayload::Sse(events)) => {
            Sse::new(stream::iter(events.into_iter().map(Ok::<_, Infallible>))).into_response()
        }
        Ok(GatewayResponsePayload::Response(response)) => response,
        Err(error) => error.into_response(),
    }
}

async fn run_gateway_server_with_home(port: u16, codex_home: PathBuf) -> AppResult<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((GATEWAY_HOST, port)))
        .await
        .map_err(|error| {
            AppError::new(
                "PROVIDER_GATEWAY_BIND_FAILED",
                format!("Failed to bind provider gateway on {GATEWAY_HOST}:{port}: {error}"),
            )
        })?;
    save_gateway_runtime_state(Some(&codex_home), &GatewayRuntimeState { port: Some(port) })?;
    let app_state = GatewayAppState {
        codex_home,
        http_client: Client::builder()
            .timeout(Duration::from_secs(GATEWAY_REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|error| {
                AppError::new(
                    "PROVIDER_GATEWAY_CLIENT_FAILED",
                    format!("Failed to create provider gateway HTTP client: {error}"),
                )
            })?,
        responses: Arc::new(Mutex::new(HashMap::new())),
    };
    let router = Router::new()
        .route(GATEWAY_HEALTH_PATH, get(health_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/responses", post(responses_handler))
        .with_state(app_state);

    axum::serve(listener, router).await.map_err(|error| {
        AppError::new(
            "PROVIDER_GATEWAY_SERVE_FAILED",
            format!("Provider gateway stopped unexpectedly: {error}"),
        )
    })
}

pub fn run_gateway_cli(args: &[String], codex_home: Option<PathBuf>) -> Result<i32, AppError> {
    if !matches!(args.first().map(String::as_str), Some("serve")) {
        return Err(AppError::new(
            "PROVIDER_GATEWAY_USAGE",
            "Usage: codex_switch_cli gateway serve [--port <port>]",
        ));
    }

    let mut port = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--port" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(AppError::new(
                        "PROVIDER_GATEWAY_USAGE",
                        "Missing port after --port.",
                    ));
                };
                port = value.parse::<u16>().ok();
                index += 2;
            }
            _ => {
                index += 1;
            }
        }
    }

    let port = port.ok_or_else(|| {
        AppError::new(
            "PROVIDER_GATEWAY_USAGE",
            "Usage: codex_switch_cli gateway serve [--port <port>]",
        )
    })?;
    let codex_home = codex_home.unwrap_or_else(get_codex_home);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            AppError::new(
                "PROVIDER_GATEWAY_RUNTIME_FAILED",
                format!("Failed to create provider gateway runtime: {error}"),
            )
        })?;
    runtime.block_on(async move { run_gateway_server_with_home(port, codex_home).await })?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{
        build_chat_completion_endpoint_candidates, build_chat_messages_from_input,
        build_final_responses_object, build_kimi_messages_tools, build_messages_content_blocks,
        build_messages_conversation, build_messages_endpoint_candidates, build_messages_tools,
        build_output_items_from_chat_response, build_output_items_from_messages_response,
        build_usage_from_chat_response, build_usage_from_messages_response,
        build_kimi_messages_streaming_response, is_kimi_coding_base_url, normalize_tool,
        prepare_gateway_request, response_id, uses_kimi_messages_gateway, ActiveProfileContext,
        GatewayAppState, GatewayResponsePayload, KimiStreamingTranslator, ResponsesGatewayRequest,
        ToolNameRegistry, UpstreamSseParser, PROVIDER_PROTOCOL_MESSAGES,
    };
    use axum::{
        body::{to_bytes, Body},
        extract::State,
        http::{HeaderMap, StatusCode},
        response::Response,
        routing::post,
        Json, Router,
    };
    use reqwest::Client;
    use serde_json::{json, Value};
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone, Default)]
    struct MockMessagesServerState {
        requests: Arc<Mutex<Vec<Value>>>,
    }

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-provider-gateway-{name}-{unique}"))
    }

    fn write_profile_fixture(codex_home: &PathBuf, profile_name: &str, base_url: &str) {
        let profile_dir = codex_home.join("account_backup").join(profile_name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"test-key"}"#,
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            serde_json::to_string_pretty(&json!({
                "folder_name": profile_name,
                "openai_base_url": base_url,
                "provider_protocol": "messages"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn sse_response(body: &str) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn mock_messages_handler(
        State(state): State<MockMessagesServerState>,
        headers: HeaderMap,
        Json(payload): Json<Value>,
    ) -> Response {
        let captured = json!({
            "headers": {
                "x-api-key": headers.get("x-api-key").and_then(|value| value.to_str().ok()),
                "anthropic-version": headers.get("anthropic-version").and_then(|value| value.to_str().ok()),
                "user-agent": headers.get("user-agent").and_then(|value| value.to_str().ok()),
            },
            "body": payload,
        });

        let index = {
            let mut requests = state.requests.lock().unwrap();
            requests.push(captured);
            requests.len() - 1
        };

        match index {
            0 => sse_response(
                concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"kimi-for-coding\",\"usage\":{\"input_tokens\":11,\"output_tokens\":0}}}\n\n",
                    "event: content_block_start\n",
                    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tool_1\",\"name\":\"shell_command\",\"input\":{}}}\n\n",
                    "event: content_block_delta\n",
                    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"Get-ChildItem\\\",\\\"workdir\\\":\\\"D:\\\\\\\\Users\\\\\\\\32162\\\\\\\\Documents\\\\\\\\GitHub\\\\\\\\Codex_Account_Switch\\\"}\"}}\n\n",
                    "event: content_block_stop\n",
                    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                    "event: message_stop\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                ),
            ),
            1 => sse_response(
                concat!(
                    "event: message_start\n",
                    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"kimi-for-coding\",\"usage\":{\"input_tokens\":17,\"output_tokens\":3}}}\n\n",
                    "event: content_block_start\n",
                    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                    "event: content_block_delta\n",
                    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
                    "event: content_block_stop\n",
                    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                    "event: message_stop\n",
                    "data: {\"type\":\"message_stop\"}\n\n"
                ),
            ),
            _ => Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("unexpected extra request"))
                .unwrap(),
        }
    }

    #[test]
    fn chat_completion_endpoint_candidates_include_v1_fallback() {
        let candidates = build_chat_completion_endpoint_candidates("https://api.example.com");

        assert_eq!(
            candidates,
            vec![
                "https://api.example.com/chat/completions".to_string(),
                "https://api.example.com/v1/chat/completions".to_string(),
            ]
        );
    }

    #[test]
    fn messages_endpoint_candidates_include_v1_fallback() {
        let candidates = build_messages_endpoint_candidates("https://api.example.com");

        assert_eq!(
            candidates,
            vec![
                "https://api.example.com/messages".to_string(),
                "https://api.example.com/v1/messages".to_string(),
            ]
        );
    }

    #[test]
    fn kimi_messages_endpoint_candidates_prefer_v1_first() {
        let candidates = build_messages_endpoint_candidates("https://api.kimi.com/coding/");

        assert_eq!(
            candidates,
            vec![
                "https://api.kimi.com/coding/v1/messages".to_string(),
                "https://api.kimi.com/coding/messages".to_string(),
            ]
        );
    }

    #[test]
    fn kimi_messages_gateway_routes_only_for_kimi_coding_profiles() {
        let kimi_profile = ActiveProfileContext {
            profile_name: "Kimi".to_string(),
            base_url: "https://api.kimi.com/coding/".to_string(),
            provider_protocol: Some(PROVIDER_PROTOCOL_MESSAGES.to_string()),
            authorization_header: None,
            api_key: None,
        };
        let generic_profile = ActiveProfileContext {
            profile_name: "Other".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            provider_protocol: Some(PROVIDER_PROTOCOL_MESSAGES.to_string()),
            authorization_header: None,
            api_key: None,
        };

        assert!(is_kimi_coding_base_url(&kimi_profile.base_url));
        assert!(uses_kimi_messages_gateway(&kimi_profile));
        assert!(!uses_kimi_messages_gateway(&generic_profile));
    }

    #[test]
    fn upstream_sse_parser_collects_complete_events() {
        let mut parser = UpstreamSseParser::default();
        let chunk = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\"}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"he\"}}\n\n"
        );

        let events = parser.push_chunk(chunk);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "message_start");
        assert!(events[0].data.contains("\"message_start\""));
        assert_eq!(events[1].event, "content_block_delta");
        assert!(events[1].data.contains("\"text_delta\""));
    }

    #[test]
    fn kimi_streaming_translator_emits_incremental_text_events() {
        let request = ResponsesGatewayRequest {
            model: "gpt-5.2".to_string(),
            stream: true,
            ..ResponsesGatewayRequest::default()
        };
        let tool_name_registry = ToolNameRegistry::default();
        let mut translator = KimiStreamingTranslator::new(&request, "kimi-for-coding".to_string());

        let initial = translator.initial_events();
        assert_eq!(initial.len(), 2);

        let start_batch = translator.process_upstream_event(
            "message_start",
            &json!({
                "message": {
                    "model": "kimi-for-coding",
                    "usage": {
                        "input_tokens": 12,
                        "output_tokens": 0,
                        "total_tokens": 12
                    }
                }
            }),
            &tool_name_registry,
        );
        assert!(start_batch.events.is_empty());

        let block_start = translator.process_upstream_event(
            "content_block_start",
            &json!({
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
            &tool_name_registry,
        );
        assert_eq!(block_start.events.len(), 2);
        assert_eq!(block_start.events[0].0, "response.output_item.added");
        assert_eq!(block_start.events[1].0, "response.content_part.added");

        let delta = translator.process_upstream_event(
            "content_block_delta",
            &json!({
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": "hello"
                }
            }),
            &tool_name_registry,
        );
        assert_eq!(delta.events.len(), 1);
        assert_eq!(delta.events[0].0, "response.output_text.delta");
        assert_eq!(delta.events[0].1["delta"], "hello");

        let part_done = translator.process_upstream_event(
            "content_block_stop",
            &json!({ "index": 0 }),
            &tool_name_registry,
        );
        assert_eq!(part_done.events.len(), 2);
        assert_eq!(part_done.events[0].0, "response.output_text.done");
        assert_eq!(part_done.events[1].0, "response.content_part.done");

        let complete = translator.process_upstream_event(
            "message_stop",
            &json!({ "type": "message_stop" }),
            &tool_name_registry,
        );
        assert!(complete.completed_response.is_some());
        let final_response = complete.completed_response.expect("final response");
        assert_eq!(
            complete.events.last().map(|entry| entry.0.as_str()),
            Some("response.completed")
        );
        assert_eq!(final_response["output"][0]["type"], "message");
        assert_eq!(final_response["output"][0]["content"][0]["text"], "hello");
        assert_eq!(final_response["output_text"], "hello");
    }

    #[test]
    fn kimi_streaming_translator_builds_function_call_from_tool_use_stream() {
        let request = ResponsesGatewayRequest {
            model: "gpt-5.2".to_string(),
            stream: true,
            ..ResponsesGatewayRequest::default()
        };
        let mut tool_name_registry = ToolNameRegistry::default();
        tool_name_registry.sanitized_name("functions.shell_command");
        let mut translator = KimiStreamingTranslator::new(&request, "kimi-for-coding".to_string());

        let start = translator.process_upstream_event(
            "content_block_start",
            &json!({
                "index": 0,
                "content_block": {
                    "type": "tool_use",
                    "id": "tool_1",
                    "name": "functions_shell_command",
                    "input": {}
                }
            }),
            &tool_name_registry,
        );
        assert!(start.events.is_empty());

        let _ = translator.process_upstream_event(
            "content_block_delta",
            &json!({
                "index": 0,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"command\":\"dir\"}"
                }
            }),
            &tool_name_registry,
        );

        let done = translator.process_upstream_event(
            "content_block_stop",
            &json!({ "index": 0 }),
            &tool_name_registry,
        );
        assert_eq!(done.events.len(), 2);
        assert_eq!(done.events[0].0, "response.output_item.added");
        assert_eq!(done.events[1].0, "response.output_item.done");
        assert_eq!(done.events[1].1["item"]["type"], "function_call");
        assert_eq!(done.events[1].1["item"]["name"], "functions.shell_command");
        assert_eq!(
            done.events[1].1["item"]["arguments"],
            "{\"command\":\"dir\"}"
        );
    }

    #[test]
    fn response_input_messages_convert_function_calls_and_tool_outputs() {
        let messages = build_chat_messages_from_input(&json!([
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup",
                "arguments": "{\"city\":\"Tokyo\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "{\"temperature\":25}"
            }
        ]));

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "lookup");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn messages_content_blocks_preserve_input_images() {
        let content = build_messages_content_blocks(&json!({
            "role": "user",
            "content": [
                { "type": "input_text", "text": "look at this" },
                {
                    "type": "input_image",
                    "image_url": {
                        "url": "https://example.com/demo.png"
                    }
                }
            ]
        }));

        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "look at this");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "url");
        assert_eq!(content[1]["source"]["url"], "https://example.com/demo.png");
    }

    #[test]
    fn kimi_messages_tools_filter_only_tool_search() {
        let mut tool_name_registry = ToolNameRegistry::default();
        let (tools, original, forwarded) = build_kimi_messages_tools(
            &[
                json!({
                    "type": "function",
                    "function": {
                        "name": "shell_command",
                        "description": "run a command",
                        "parameters": { "type": "object" }
                    }
                }),
                json!({
                    "type": "function",
                    "function": {
                        "name": "tool_search_tool",
                        "description": "find tools",
                        "parameters": { "type": "object" }
                    }
                }),
                json!({
                    "type": "function",
                    "function": {
                        "name": "mcp__stitch__",
                        "description": "connector tool",
                        "parameters": { "type": "object" }
                    }
                })
            ],
            &mut tool_name_registry,
        );

        assert_eq!(
            original,
            vec![
                "shell_command".to_string(),
                "tool_search_tool".to_string(),
                "mcp__stitch__".to_string()
            ]
        );
        assert_eq!(
            forwarded,
            vec!["shell_command".to_string(), "mcp__stitch__".to_string()]
        );
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "shell_command");
        assert_eq!(tools[1]["name"], "mcp__stitch__");
    }

    #[test]
    fn tool_results_do_not_merge_into_plain_user_text_messages() {
        let (_, messages) = build_messages_conversation(
            &[
                json!({
                    "role": "user",
                    "content": "hello"
                }),
                json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "{\"ok\":true}"
                })
            ],
            None,
        );

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"][0]["type"], "text");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn chat_response_maps_to_responses_output_and_usage() {
        let mut tool_name_registry = ToolNameRegistry::default();
        tool_name_registry.sanitized_name("lookup");
        let chat_response = json!({
            "model": "Qwen/Qwen2.5-72B-Instruct",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "world",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{\"city\":\"Tokyo\"}"
                        }
                    }]
                }
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 7,
                "total_tokens": 19
            }
        });

        let output = build_output_items_from_chat_response(&chat_response, &tool_name_registry);
        let usage = build_usage_from_chat_response(&chat_response);

        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "world");
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(usage["input_tokens"], 12);
        assert_eq!(usage["output_tokens"], 7);
    }

    #[test]
    fn messages_conversation_converts_tool_calls_and_outputs() {
        let (system, messages) = build_messages_conversation(
            &[
                json!({
                    "role": "system",
                    "content": "follow the rules"
                }),
                json!({
                    "role": "user",
                    "content": "hello"
                }),
                json!({
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "arguments": "{\"city\":\"Tokyo\"}"
                        }
                    }]
                }),
                json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "{\"temperature\":25}"
                }),
            ],
            None,
        );

        assert_eq!(system.as_deref(), Some("follow the rules"));
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn messages_response_maps_to_responses_output_and_usage() {
        let mut tool_name_registry = ToolNameRegistry::default();
        tool_name_registry.sanitized_name("functions.shell_command");
        let messages_response = json!({
            "model": "kimi-for-coding",
            "content": [
                {
                    "type": "text",
                    "text": "done"
                },
                {
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "functions_shell_command",
                    "input": {
                        "command": "dir"
                    }
                }
            ],
            "usage": {
                "input_tokens": 21,
                "output_tokens": 9
            }
        });

        let output =
            build_output_items_from_messages_response(&messages_response, &tool_name_registry);
        let usage = build_usage_from_messages_response(&messages_response);

        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "done");
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(output[1]["name"], "functions.shell_command");
        assert_eq!(usage["input_tokens"], 21);
        assert_eq!(usage["output_tokens"], 9);
    }

    #[tokio::test]
    async fn kimi_messages_bridge_completes_two_turn_tool_roundtrip() {
        let codex_home = temp_codex_home("tool-roundtrip");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap();
        let upstream_base_url = format!("http://{upstream_addr}");
        write_profile_fixture(&codex_home, "api", &upstream_base_url);

        let mock_state = MockMessagesServerState::default();
        let router = Router::new()
            .route("/v1/messages", post(mock_messages_handler))
            .with_state(mock_state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let state = GatewayAppState {
            codex_home: codex_home.clone(),
            http_client: Client::builder().build().unwrap(),
            responses: Arc::new(Mutex::new(HashMap::new())),
        };
        let profile = ActiveProfileContext {
            profile_name: "api".to_string(),
            base_url: upstream_base_url.clone(),
            provider_protocol: Some(PROVIDER_PROTOCOL_MESSAGES.to_string()),
            authorization_header: None,
            api_key: Some("test-key".to_string()),
        };

        let first_request = ResponsesGatewayRequest {
            model: "gpt-5.2".to_string(),
            stream: true,
            messages: Some(vec![json!({
                "role": "user",
                "content": "list files"
            })]),
            tools: Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "shell_command",
                    "description": "run a command",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string" },
                            "workdir": { "type": "string" }
                        },
                        "required": ["command"]
                    }
                }
            })]),
            tool_choice: Some(Value::String("auto".to_string())),
            parallel_tool_calls: Some(true),
            store: Some(false),
            ..ResponsesGatewayRequest::default()
        };
        let first_prepared = prepare_gateway_request(&state, &profile, &first_request).unwrap();
        let first_payload =
            build_kimi_messages_streaming_response(&state, &profile, &first_request, &first_prepared)
                .await
                .unwrap();
        let first_response = match first_payload {
            GatewayResponsePayload::Response(response) => response,
            _ => panic!("expected streaming response payload"),
        };
        let first_bytes = to_bytes(first_response.into_body(), usize::MAX).await.unwrap();
        let first_text = String::from_utf8(first_bytes.to_vec()).unwrap();
        assert!(first_text.contains("response.output_item.done"));
        assert!(first_text.contains("\"name\":\"shell_command\""));

        let first_response_id = {
            let responses = state.responses.lock().unwrap();
            assert_eq!(responses.len(), 1);
            responses.keys().next().cloned().unwrap()
        };

        let second_request = ResponsesGatewayRequest {
            model: "gpt-5.2".to_string(),
            stream: true,
            previous_response_id: Some(first_response_id),
            input: Some(json!([{
                "type": "function_call_output",
                "call_id": "tool_1",
                "output": "{\"stdout\":\"ok\"}"
            }])),
            store: Some(false),
            ..ResponsesGatewayRequest::default()
        };
        let second_prepared = prepare_gateway_request(&state, &profile, &second_request).unwrap();
        let second_payload =
            build_kimi_messages_streaming_response(
                &state,
                &profile,
                &second_request,
                &second_prepared,
            )
            .await
            .unwrap();
        let second_response = match second_payload {
            GatewayResponsePayload::Response(response) => response,
            _ => panic!("expected streaming response payload"),
        };
        let second_bytes = to_bytes(second_response.into_body(), usize::MAX).await.unwrap();
        let second_text = String::from_utf8(second_bytes.to_vec()).unwrap();
        assert!(second_text.contains("response.completed"));
        assert!(second_text.contains("\"text\":\"done\""));

        let captured_requests = mock_state.requests.lock().unwrap().clone();
        assert_eq!(captured_requests.len(), 2);
        assert_eq!(
            captured_requests[0]["headers"]["x-api-key"],
            Value::String("test-key".to_string())
        );
        assert_eq!(
            captured_requests[0]["headers"]["anthropic-version"],
            Value::String("2023-06-01".to_string())
        );
        assert_eq!(
            captured_requests[0]["headers"]["user-agent"],
            Value::String("Codex CLI".to_string())
        );
        assert_eq!(
            captured_requests[0]["body"]["tools"][0]["name"],
            Value::String("shell_command".to_string())
        );
        assert_eq!(
            captured_requests[0]["body"]["messages"][0]["content"][0]["text"],
            Value::String("list files".to_string())
        );

        assert_eq!(
            captured_requests[1]["body"]["messages"][0]["content"][0]["text"],
            Value::String("list files".to_string())
        );
        assert_eq!(
            captured_requests[1]["body"]["messages"][1]["role"],
            Value::String("assistant".to_string())
        );
        assert_eq!(
            captured_requests[1]["body"]["messages"][1]["content"][0]["type"],
            Value::String("tool_use".to_string())
        );
        assert_eq!(
            captured_requests[1]["body"]["messages"][2]["role"],
            Value::String("user".to_string())
        );
        assert_eq!(
            captured_requests[1]["body"]["messages"][2]["content"][0]["type"],
            Value::String("tool_result".to_string())
        );
        assert_eq!(
            captured_requests[1]["body"]["messages"][2]["content"][0]["tool_use_id"],
            Value::String("tool_1".to_string())
        );

        server.abort();
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn messages_tools_sanitize_invalid_function_names() {
        let mut tool_name_registry = ToolNameRegistry::default();
        let tools = build_messages_tools(
            &[json!({
                "type": "function",
                "function": {
                    "name": "functions.shell_command",
                    "description": "run a command",
                    "parameters": { "type": "object" }
                }
            })],
            &mut tool_name_registry,
        );

        assert_eq!(tools[0]["name"], "functions_shell_command");
    }

    #[test]
    fn final_response_uses_responses_shape() {
        let tool_name_registry = ToolNameRegistry::default();
        let request = ResponsesGatewayRequest {
            model: "Qwen/Qwen2.5-72B-Instruct".to_string(),
            ..ResponsesGatewayRequest::default()
        };
        let chat_response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "done"
                }
            }]
        });

        let (id, response) =
            build_final_responses_object(&request, &chat_response, &tool_name_registry);

        assert!(id.starts_with("resp_"));
        assert_eq!(response["object"], "response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["output"][0]["content"][0]["text"], "done");
    }

    #[test]
    fn normalize_tool_sanitizes_invalid_function_names_for_chat_completions() {
        let mut tool_name_registry = ToolNameRegistry::default();
        let normalized = normalize_tool(
            &json!({
                "type": "function",
                "function": {
                    "name": "functions.shell_command",
                    "description": "run a command",
                    "parameters": { "type": "object" }
                }
            }),
            &mut tool_name_registry,
        )
        .expect("expected normalized tool");

        assert_eq!(
            normalized["function"]["name"],
            json!("functions_shell_command")
        );
    }

    #[test]
    fn generated_response_ids_use_responses_prefix() {
        assert!(response_id().starts_with("resp_"));
    }
}
