use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::thread;
use std::time::{Duration, UNIX_EPOCH};

use reqwest::blocking::Client;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::models::{ModelMappingEntry, ProviderModelListResponse};

use super::config::{load_root_model_value, profile_uses_api_key_auth, sync_root_model_value};
use super::metadata::{load_profile_metadata, sync_profile_provider_protocol};
use super::paths::{
    get_backup_root, get_codex_home, get_root_config_path, get_runtime_dir, get_switch_lock_path,
    validate_profile_name,
};
use super::profiles::resolve_current_profile;

const DEFAULT_SOURCE_MODEL: &str = "gpt-5.4";
const LIVE_MODEL_SYNC_POLL_MS: u64 = 700;
const MODEL_DISCOVERY_TIMEOUT_SECS: u64 = 10;
const MODEL_DISCOVERY_USER_AGENT: &str = "codex-switch/model-discovery";
const MESSAGES_API_KEY_HEADER: &str = "x-api-key";
const ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const ANTHROPIC_VERSION_VALUE: &str = "2023-06-01";
pub(crate) const PROVIDER_PROTOCOL_RESPONSES: &str = "responses";
pub(crate) const PROVIDER_PROTOCOL_CHAT_COMPLETIONS: &str = "chat/completions";
pub(crate) const PROVIDER_PROTOCOL_MESSAGES: &str = "messages";
pub(crate) const PROVIDER_PROTOCOL_COMPLETIONS: &str = "completions";
const KIMI_FOR_CODING_MODEL: &str = "kimi-for-coding";
const PROVIDER_ENDPOINT_BASE_SUFFIXES: [&str; 4] = [
    "/chat/completions",
    "/responses",
    "/completions",
    "/embeddings",
];
const SOURCE_MODELS: [&str; 8] = [
    "gpt-5.4",
    "gpt-5.2-codex",
    "gpt-5.1-codex-max",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.3-codex-spark",
    "gpt-5.2",
    "gpt-5.1-codex-mini",
];
const MODEL_DISCOVERY_API_KEY_POINTERS: [&str; 12] = [
    "/api_key",
    "/apiKey",
    "/apikey",
    "/openai_api_key",
    "/openaiApiKey",
    "/credentials/api_key",
    "/credentials/apiKey",
    "/credentials/apikey",
    "/credentials/token",
    "/tokens/api_key",
    "/tokens/apiKey",
    "/tokens/access_token",
];
const MODEL_DISCOVERY_API_KEY_NAMES: [&str; 9] = [
    "api_key",
    "apiKey",
    "apikey",
    "openai_api_key",
    "openaiApiKey",
    "secret_key",
    "secretKey",
    "token",
    "access_token",
];
static LIVE_MODEL_SYNC_MONITOR: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtocolEndpointAvailability {
    Supported,
    Restricted,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtocolProbeKind {
    Responses,
    ChatCompletions,
    Messages,
    Completions,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
struct ModelSyncState {
    current_source_model: Option<String>,
    current_profile: Option<String>,
}

fn model_sync_state_path(codex_home: Option<&Path>) -> PathBuf {
    get_runtime_dir(codex_home).join("model_sync_state.json")
}

fn file_modified_ms(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as u64)
}

fn load_model_sync_state(codex_home: Option<&Path>) -> ModelSyncState {
    let path = model_sync_state_path(codex_home);
    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return ModelSyncState::default(),
    };

    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_model_sync_state(codex_home: Option<&Path>, state: &ModelSyncState) -> AppResult<()> {
    let path = model_sync_state_path(codex_home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create model sync state directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(state).map_err(|error| {
        AppError::new(
            "MODEL_SYNC_STATE_INVALID",
            format!("Failed to serialize model sync state: {error}"),
        )
    })?;

    fs::write(&path, format!("{serialized}\n")).map_err(|error| {
        AppError::new(
            "MODEL_SYNC_STATE_WRITE_FAILED",
            format!(
                "Failed to write model sync state {}: {error}",
                path.display()
            ),
        )
    })
}

fn normalize_secret(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("replace-me") {
        return None;
    }

    Some(trimmed.to_string())
}

fn matches_api_key_field_name(field_name: &str) -> bool {
    MODEL_DISCOVERY_API_KEY_NAMES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(field_name))
}

fn canonical_source_model(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    SOURCE_MODELS
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .copied()
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

fn load_profile_auth_json(profile_name: &str, codex_home: &Path) -> AppResult<Value> {
    let auth_path = get_backup_root(Some(codex_home))
        .join(profile_name)
        .join("auth.json");
    let raw = fs::read_to_string(&auth_path).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_READ_FAILED",
            format!("Failed to read auth.json {}: {error}", auth_path.display()),
        )
    })?;

    serde_json::from_str(&raw).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_PARSE_FAILED",
            format!("Failed to parse auth.json {}: {error}", auth_path.display()),
        )
    })
}

fn extract_api_key_recursive(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if matches_api_key_field_name(key) {
                    if let Some(secret) = value.as_str().and_then(normalize_secret) {
                        return Some(secret);
                    }
                }
            }

            for nested in map.values() {
                if let Some(secret) = extract_api_key_recursive(nested) {
                    return Some(secret);
                }
            }

            None
        }
        Value::Array(values) => values.iter().find_map(extract_api_key_recursive),
        _ => None,
    }
}

pub(crate) fn load_profile_api_key(profile_name: &str, codex_home: &Path) -> Option<String> {
    let parsed = load_profile_auth_json(profile_name, codex_home).ok()?;

    for pointer in MODEL_DISCOVERY_API_KEY_POINTERS {
        if let Some(secret) = parsed
            .pointer(pointer)
            .and_then(Value::as_str)
            .and_then(normalize_secret)
        {
            return Some(secret);
        }
    }

    extract_api_key_recursive(&parsed)
}

fn push_unique_string(values: &mut Vec<String>, next: String) {
    if !values.iter().any(|value| value.eq_ignore_ascii_case(&next)) {
        values.push(next);
    }
}

fn collect_model_ids(models: &mut Vec<String>, values: &[Value]) {
    for value in values {
        if let Some(model_id) = value
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| value.get("model").and_then(Value::as_str))
            .or_else(|| value.get("name").and_then(Value::as_str))
            .and_then(normalize_secret)
        {
            push_unique_string(models, model_id);
            continue;
        }

        if let Some(model_id) = value.as_str().and_then(normalize_secret) {
            push_unique_string(models, model_id);
        }
    }
}

fn extract_provider_models(payload: &Value) -> Vec<String> {
    let mut models = Vec::new();

    if let Some(values) = payload.get("data").and_then(Value::as_array) {
        collect_model_ids(&mut models, values);
    }
    if let Some(values) = payload.get("models").and_then(Value::as_array) {
        collect_model_ids(&mut models, values);
    }
    if let Some(values) = payload.get("result").and_then(Value::as_array) {
        collect_model_ids(&mut models, values);
    }
    if models.is_empty() {
        if let Some(values) = payload.as_array() {
            collect_model_ids(&mut models, values);
        }
    }

    models.sort_by_key(|value| value.to_ascii_lowercase());
    models
}

fn push_model_endpoint_candidate(candidates: &mut Vec<String>, candidate: String) {
    if candidate.is_empty() || reqwest::Url::parse(&candidate).is_err() {
        return;
    }

    if !candidates
        .iter()
        .any(|value| value.eq_ignore_ascii_case(&candidate))
    {
        candidates.push(candidate);
    }
}

fn build_model_endpoint_candidates(base_url: &str) -> Vec<String> {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    if normalized.ends_with("/models") {
        push_model_endpoint_candidate(&mut candidates, normalized.to_string());
        return candidates;
    }

    push_model_endpoint_candidate(&mut candidates, format!("{normalized}/models"));
    if !normalized.ends_with("/v1") {
        push_model_endpoint_candidate(&mut candidates, format!("{normalized}/v1/models"));
    }

    for suffix in PROVIDER_ENDPOINT_BASE_SUFFIXES {
        if let Some(base) = normalized.strip_suffix(suffix) {
            let base = base.trim_end_matches('/');
            push_model_endpoint_candidate(&mut candidates, format!("{base}/models"));
            push_model_endpoint_candidate(&mut candidates, format!("{base}/v1/models"));
        }
    }

    candidates
}

fn build_protocol_probe_endpoint_from_model_endpoint(
    model_endpoint: &str,
    endpoint_suffix: &str,
) -> Option<String> {
    let normalized = model_endpoint.trim().trim_end_matches('/');
    let probe_base = normalized.strip_suffix("/models")?.trim_end_matches('/');
    if probe_base.is_empty() {
        return None;
    }

    let endpoint = if probe_base.ends_with(endpoint_suffix) {
        probe_base.to_string()
    } else {
        format!("{probe_base}{endpoint_suffix}")
    };

    (reqwest::Url::parse(&endpoint).is_ok()).then_some(endpoint)
}

fn build_model_discovery_headers(api_key: Option<&str>) -> AppResult<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(MODEL_DISCOVERY_USER_AGENT),
    );

    if let Some(api_key) = api_key {
        let auth_value = HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|error| {
            AppError::new(
                "INVALID_API_KEY_HEADER",
                format!("Failed to build Authorization header: {error}"),
            )
        })?;
        headers.insert(AUTHORIZATION, auth_value);
    }

    Ok(headers)
}

fn build_protocol_probe_headers(api_key: Option<&str>) -> AppResult<HeaderMap> {
    let mut headers = build_model_discovery_headers(api_key)?;
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

fn build_messages_protocol_probe_headers(api_key: Option<&str>) -> AppResult<HeaderMap> {
    let mut headers = build_protocol_probe_headers(api_key)?;
    headers.insert(
        HeaderName::from_static(ANTHROPIC_VERSION_HEADER),
        HeaderValue::from_static(ANTHROPIC_VERSION_VALUE),
    );

    if let Some(api_key) = api_key {
        let api_key_value = HeaderValue::from_str(api_key).map_err(|error| {
            AppError::new(
                "INVALID_API_KEY_HEADER",
                format!("Failed to build x-api-key header: {error}"),
            )
        })?;
        headers.insert(
            HeaderName::from_static(MESSAGES_API_KEY_HEADER),
            api_key_value,
        );
    }

    Ok(headers)
}

fn classify_protocol_endpoint_availability(
    status: reqwest::StatusCode,
) -> ProtocolEndpointAvailability {
    match status {
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE => {
            ProtocolEndpointAvailability::Missing
        }
        reqwest::StatusCode::BAD_REQUEST
        | reqwest::StatusCode::UNAUTHORIZED
        | reqwest::StatusCode::FORBIDDEN
        | reqwest::StatusCode::METHOD_NOT_ALLOWED
        | reqwest::StatusCode::NOT_ACCEPTABLE
        | reqwest::StatusCode::CONFLICT
        | reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE
        | reqwest::StatusCode::UNPROCESSABLE_ENTITY
        | reqwest::StatusCode::TOO_MANY_REQUESTS => ProtocolEndpointAvailability::Supported,
        _ if status.is_success() => ProtocolEndpointAvailability::Supported,
        _ => ProtocolEndpointAvailability::Unknown,
    }
}

fn probe_model_name_for_endpoint(endpoint: &str) -> &str {
    if is_kimi_coding_base_url(endpoint) {
        return KIMI_FOR_CODING_MODEL;
    }

    "codex-switch-probe"
}

fn protocol_probe_body(kind: ProtocolProbeKind, endpoint: &str) -> Value {
    let model = probe_model_name_for_endpoint(endpoint);
    match kind {
        ProtocolProbeKind::Responses => serde_json::json!({
            "model": model,
            "input": "ping",
            "max_output_tokens": 1,
        }),
        ProtocolProbeKind::ChatCompletions => serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 1,
        }),
        ProtocolProbeKind::Messages => serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 1,
        }),
        ProtocolProbeKind::Completions => serde_json::json!({
            "model": model,
            "prompt": "ping",
            "max_tokens": 1,
        }),
    }
}

fn body_contains_access_terminated_error(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.contains("access_terminated_error")
        || trimmed.contains("only available for Coding Agents")
    {
        return true;
    }

    serde_json::from_str::<Value>(trimmed)
        .ok()
        .and_then(|payload| {
            payload
                .pointer("/error/type")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    payload
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("access_terminated_error")
                || value.contains("only available for Coding Agents")
        })
}

fn probe_protocol_endpoint(
    client: &Client,
    endpoint: &str,
    api_key: Option<&str>,
    kind: ProtocolProbeKind,
) -> AppResult<ProtocolEndpointAvailability> {
    let headers = if kind == ProtocolProbeKind::Messages {
        build_messages_protocol_probe_headers(api_key)?
    } else {
        build_protocol_probe_headers(api_key)?
    };
    let body = protocol_probe_body(kind, endpoint);
    let response = client.post(endpoint).headers(headers).json(&body).send();

    Ok(match response {
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            if body_contains_access_terminated_error(&body) {
                ProtocolEndpointAvailability::Restricted
            } else {
                classify_protocol_endpoint_availability(status)
            }
        }
        Err(_) => ProtocolEndpointAvailability::Unknown,
    })
}

fn provider_protocol_from_probe_states(
    responses_state: ProtocolEndpointAvailability,
    chat_completions_state: ProtocolEndpointAvailability,
    messages_state: ProtocolEndpointAvailability,
    completions_state: ProtocolEndpointAvailability,
) -> Option<String> {
    if responses_state == ProtocolEndpointAvailability::Supported {
        return Some(PROVIDER_PROTOCOL_RESPONSES.to_string());
    }

    if chat_completions_state == ProtocolEndpointAvailability::Supported {
        return Some(PROVIDER_PROTOCOL_CHAT_COMPLETIONS.to_string());
    }

    if messages_state == ProtocolEndpointAvailability::Supported {
        return Some(PROVIDER_PROTOCOL_MESSAGES.to_string());
    }

    (completions_state == ProtocolEndpointAvailability::Supported)
        .then(|| PROVIDER_PROTOCOL_COMPLETIONS.to_string())
}

fn protocol_warning_from_provider_protocol(provider_protocol: Option<&str>) -> Option<String> {
    let _ = provider_protocol;
    None
}

fn detect_provider_protocol_from_model_endpoint(
    client: &Client,
    model_endpoint: &str,
    api_key: Option<&str>,
) -> AppResult<Option<String>> {
    let Some(responses_endpoint) =
        build_protocol_probe_endpoint_from_model_endpoint(model_endpoint, "/responses")
    else {
        return Ok(None);
    };
    let Some(chat_completions_endpoint) =
        build_protocol_probe_endpoint_from_model_endpoint(model_endpoint, "/chat/completions")
    else {
        return Ok(None);
    };
    let Some(messages_endpoint) =
        build_protocol_probe_endpoint_from_model_endpoint(model_endpoint, "/messages")
    else {
        return Ok(None);
    };
    let Some(completions_endpoint) =
        build_protocol_probe_endpoint_from_model_endpoint(model_endpoint, "/completions")
    else {
        return Ok(None);
    };

    let responses_state = probe_protocol_endpoint(
        client,
        &responses_endpoint,
        api_key,
        ProtocolProbeKind::Responses,
    )?;
    let chat_completions_state = probe_protocol_endpoint(
        client,
        &chat_completions_endpoint,
        api_key,
        ProtocolProbeKind::ChatCompletions,
    )?;
    let messages_state = probe_protocol_endpoint(
        client,
        &messages_endpoint,
        api_key,
        ProtocolProbeKind::Messages,
    )?;
    let completions_state = probe_protocol_endpoint(
        client,
        &completions_endpoint,
        api_key,
        ProtocolProbeKind::Completions,
    )?;

    Ok(provider_protocol_from_probe_states(
        responses_state,
        chat_completions_state,
        messages_state,
        completions_state,
    ))
}

fn kimi_fallback_models(base_url: &str) -> Option<Vec<String>> {
    is_kimi_coding_base_url(base_url).then(|| vec![KIMI_FOR_CODING_MODEL.to_string()])
}

fn detect_provider_protocol_from_base_url(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
) -> AppResult<Option<String>> {
    for endpoint in build_model_endpoint_candidates(base_url) {
        let provider_protocol =
            detect_provider_protocol_from_model_endpoint(client, &endpoint, api_key)?;
        if provider_protocol.is_some() {
            return Ok(provider_protocol);
        }
    }

    Ok(None)
}

pub(crate) fn detect_profile_provider_protocol(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<Option<String>> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    if !profile_uses_api_key_auth(&profile_name, Some(&codex_home))? {
        return Ok(None);
    }

    let base_url = load_profile_metadata(&profile_name, Some(&codex_home))
        .openai_base_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(base_url) = base_url else {
        return Ok(None);
    };

    let api_key = load_profile_api_key(&profile_name, &codex_home);
    let client = Client::builder()
        .timeout(Duration::from_secs(MODEL_DISCOVERY_TIMEOUT_SECS))
        .build()
        .map_err(|error| {
            AppError::new(
                "PROVIDER_MODEL_LIST_FAILED",
                format!("Failed to create protocol detection client: {error}"),
            )
        })?;

    let provider_protocol =
        detect_provider_protocol_from_base_url(&client, &base_url, api_key.as_deref())?;
    if provider_protocol.is_some() {
        sync_profile_provider_protocol(
            &profile_name,
            provider_protocol.clone(),
            Some(&codex_home),
        )?;
    }

    Ok(provider_protocol)
}

fn request_provider_models(
    client: &Client,
    endpoint: &str,
    api_key: Option<&str>,
) -> AppResult<Vec<String>> {
    let response = client
        .get(endpoint)
        .headers(build_model_discovery_headers(api_key)?)
        .send()
        .map_err(|error| {
            AppError::new(
                "PROVIDER_MODEL_LIST_FAILED",
                format!("Failed to load models from {endpoint}: {error}"),
            )
        })?;

    let status = response.status();
    let body = response.text().map_err(|error| {
        AppError::new(
            "PROVIDER_MODEL_LIST_FAILED",
            format!("Failed to read model list response from {endpoint}: {error}"),
        )
    })?;

    if !status.is_success() {
        return Err(AppError::new(
            "PROVIDER_MODEL_LIST_FAILED",
            format!("Failed to load models from {endpoint}: HTTP {status}"),
        ));
    }

    let parsed = serde_json::from_str::<Value>(&body).map_err(|error| {
        AppError::new(
            "PROVIDER_MODEL_LIST_INVALID",
            format!("Failed to parse model list response from {endpoint}: {error}"),
        )
    })?;

    let models = extract_provider_models(&parsed);
    if models.is_empty() {
        return Err(AppError::new(
            "PROVIDER_MODEL_LIST_EMPTY",
            format!("No model identifiers were found in the response from {endpoint}."),
        ));
    }

    Ok(models)
}

fn infer_source_model_from_target(
    current_model: &str,
    model_mappings: &[ModelMappingEntry],
) -> Option<String> {
    let mut matches = model_mappings
        .iter()
        .filter(|mapping| mapping.target_model.eq_ignore_ascii_case(current_model))
        .filter_map(|mapping| canonical_source_model(&mapping.source_model))
        .map(str::to_string)
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();

    (matches.len() == 1).then(|| matches.remove(0))
}

fn resolve_source_model_for_api_profile(
    current_model: Option<&str>,
    model_mappings: &[ModelMappingEntry],
    state: &ModelSyncState,
) -> String {
    if let Some(model) = current_model.and_then(canonical_source_model) {
        return model.to_string();
    }

    if let Some(model) =
        current_model.and_then(|value| infer_source_model_from_target(value, model_mappings))
    {
        return model;
    }

    if let Some(model) = state
        .current_source_model
        .as_deref()
        .and_then(canonical_source_model)
    {
        return model.to_string();
    }

    DEFAULT_SOURCE_MODEL.to_string()
}

fn resolve_source_model_for_non_api_profile(
    current_model: Option<&str>,
    state: &ModelSyncState,
) -> String {
    if let Some(model) = current_model.and_then(canonical_source_model) {
        return model.to_string();
    }

    if let Some(model) = state
        .current_source_model
        .as_deref()
        .and_then(canonical_source_model)
    {
        return model.to_string();
    }

    DEFAULT_SOURCE_MODEL.to_string()
}

fn mapped_target_model(source_model: &str, model_mappings: &[ModelMappingEntry]) -> Option<String> {
    model_mappings
        .iter()
        .find(|mapping| mapping.source_model.eq_ignore_ascii_case(source_model))
        .map(|mapping| mapping.target_model.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn resolve_provider_target_model_for_request(
    profile_name: &str,
    requested_model: &str,
    codex_home: Option<&Path>,
) -> AppResult<String> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    let metadata = load_profile_metadata(&profile_name, Some(&codex_home));
    let requested_model = requested_model.trim();
    if !requested_model.is_empty() {
        if let Some(mapped_model) = mapped_target_model(requested_model, &metadata.model_mappings) {
            return Ok(mapped_model);
        }

        if let Some(target_model) = metadata
            .model_mappings
            .iter()
            .find(|mapping| mapping.target_model.eq_ignore_ascii_case(requested_model))
            .map(|mapping| mapping.target_model.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Ok(target_model);
        }

        return Ok(requested_model.to_string());
    }

    let state = load_model_sync_state(Some(&codex_home));
    let source_model = resolve_source_model_for_api_profile(None, &metadata.model_mappings, &state);
    Ok(mapped_target_model(&source_model, &metadata.model_mappings).unwrap_or(source_model))
}

pub fn sync_root_model_for_profile(profile_name: &str, codex_home: Option<&Path>) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = get_backup_root(Some(&codex_home)).join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let metadata = load_profile_metadata(&profile_name, Some(&codex_home));
    let mut state = load_model_sync_state(Some(&codex_home));
    let current_model = load_root_model_value(Some(&codex_home));
    let uses_api_key = profile_uses_api_key_auth(&profile_name, Some(&codex_home))?;
    let source_model = if uses_api_key {
        resolve_source_model_for_api_profile(
            current_model.as_deref(),
            &metadata.model_mappings,
            &state,
        )
    } else {
        resolve_source_model_for_non_api_profile(current_model.as_deref(), &state)
    };
    let desired_model = source_model.clone();

    sync_root_model_value(&desired_model, Some(&codex_home))?;
    state.current_source_model = Some(source_model);
    state.current_profile = Some(profile_name);
    save_model_sync_state(Some(&codex_home), &state)
}

pub fn sync_root_model_for_current_profile(codex_home: Option<&Path>) -> AppResult<()> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let Some(current_profile) = resolve_current_profile(&backup_root) else {
        return Ok(());
    };

    sync_root_model_for_profile(&current_profile, Some(&codex_home))
}

fn run_live_model_sync_monitor(codex_home: PathBuf) {
    let config_path = get_root_config_path(Some(&codex_home));
    let switch_lock_path = get_switch_lock_path(Some(&codex_home));
    let mut last_config_mtime = file_modified_ms(&config_path);

    loop {
        thread::sleep(Duration::from_millis(LIVE_MODEL_SYNC_POLL_MS));

        if switch_lock_path.exists() {
            continue;
        }

        let current_config_mtime = file_modified_ms(&config_path);
        if current_config_mtime == last_config_mtime {
            continue;
        }

        if let Err(error) = sync_root_model_for_current_profile(Some(&codex_home)) {
            eprintln!("codex-switch: live model sync failed: {}", error.message);
        }

        last_config_mtime = file_modified_ms(&config_path);
    }
}

pub fn ensure_live_model_sync_monitor() {
    LIVE_MODEL_SYNC_MONITOR.call_once(|| {
        let codex_home = get_codex_home();
        let _ = thread::Builder::new()
            .name("codex-switch-model-sync".to_string())
            .spawn(move || run_live_model_sync_monitor(codex_home));
    });
}

pub fn fetch_profile_provider_models(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<ProviderModelListResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let profile_name = validate_profile_name(profile_name)?;
    let profile_dir = get_backup_root(Some(&codex_home)).join(&profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let metadata = load_profile_metadata(&profile_name, Some(&codex_home));
    let base_url = metadata
        .openai_base_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::new(
                "PROFILE_BASE_URL_MISSING",
                "Set a Base Url for this profile before reading provider models.",
            )
        })?;

    let api_key = load_profile_api_key(&profile_name, &codex_home);
    let client = Client::builder()
        .timeout(Duration::from_secs(MODEL_DISCOVERY_TIMEOUT_SECS))
        .build()
        .map_err(|error| {
            AppError::new(
                "PROVIDER_MODEL_LIST_FAILED",
                format!("Failed to create model discovery client: {error}"),
            )
        })?;

    let mut provider_protocol =
        detect_provider_protocol_from_base_url(&client, &base_url, api_key.as_deref())?;
    if provider_protocol.is_some() {
        sync_profile_provider_protocol(
            &profile_name,
            provider_protocol.clone(),
            Some(&codex_home),
        )?;
    }

    let mut last_error: Option<AppError> = None;
    for endpoint in build_model_endpoint_candidates(&base_url) {
        match request_provider_models(&client, &endpoint, api_key.as_deref()) {
            Ok(models) => {
                if provider_protocol.is_none() {
                    provider_protocol = detect_provider_protocol_from_model_endpoint(
                        &client,
                        &endpoint,
                        api_key.as_deref(),
                    )?;
                }
                let protocol_warning =
                    protocol_warning_from_provider_protocol(provider_protocol.as_deref());
                if provider_protocol.is_some() {
                    sync_profile_provider_protocol(
                        &profile_name,
                        provider_protocol.clone(),
                        Some(&codex_home),
                    )?;
                }
                return Ok(ProviderModelListResponse {
                    models,
                    provider_protocol,
                    protocol_warning,
                });
            }
            Err(error) => last_error = Some(error),
        }
    }

    if api_key.is_none() && profile_uses_api_key_auth(&profile_name, Some(&codex_home))? {
        return Err(AppError::new(
            "PROFILE_API_KEY_MISSING",
            "No API key was found in auth.json for this API profile.",
        ));
    }

    if let Some(models) = kimi_fallback_models(&base_url) {
        let provider_protocol =
            provider_protocol.or_else(|| Some(PROVIDER_PROTOCOL_MESSAGES.to_string()));
        sync_profile_provider_protocol(
            &profile_name,
            provider_protocol.clone(),
            Some(&codex_home),
        )?;
        return Ok(ProviderModelListResponse {
            models,
            provider_protocol,
            protocol_warning: None,
        });
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::new(
            "PROVIDER_MODEL_LIST_FAILED",
            "Failed to load provider models from the configured Base Url.",
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        body_contains_access_terminated_error, build_model_endpoint_candidates,
        build_protocol_probe_endpoint_from_model_endpoint, classify_protocol_endpoint_availability,
        extract_provider_models, is_kimi_coding_base_url, kimi_fallback_models,
        load_model_sync_state, load_profile_api_key, provider_protocol_from_probe_states,
        resolve_provider_target_model_for_request, save_model_sync_state,
        sync_root_model_for_profile, ModelSyncState, ProtocolEndpointAvailability,
        PROVIDER_PROTOCOL_CHAT_COMPLETIONS, PROVIDER_PROTOCOL_MESSAGES,
    };
    use crate::windows::env_guard;
    use reqwest::StatusCode;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-model-mapping-{name}-{unique}"))
    }

    fn write_profile(
        codex_home: &PathBuf,
        profile_name: &str,
        auth_json: &str,
        profile_json: &str,
    ) {
        let profile_dir = codex_home.join("account_backup").join(profile_name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(profile_dir.join("auth.json"), auth_json).unwrap();
        fs::write(profile_dir.join("profile.json"), profile_json).unwrap();
    }

    #[test]
    fn sync_root_model_for_profile_applies_mapping_for_known_source_model() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("apply-mapping");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","api_key":"sk-test"}"#,
            r#"{"folder_name":"api","model_mappings":[{"source_model":"gpt-5.4","target_model":"provider-a"}]}"#,
        );
        fs::write(codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();

        sync_root_model_for_profile("api", Some(&codex_home)).unwrap();

        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.4\""));
        let state = load_model_sync_state(Some(&codex_home));
        assert_eq!(state.current_source_model.as_deref(), Some("gpt-5.4"));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_model_for_profile_infers_source_model_from_current_target() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("infer-source");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","api_key":"sk-test"}"#,
            r#"{"folder_name":"api","model_mappings":[{"source_model":"gpt-5.4-mini","target_model":"provider-mini"}]}"#,
        );
        fs::write(
            codex_home.join("config.toml"),
            "model = \"provider-mini\"\n",
        )
        .unwrap();

        sync_root_model_for_profile("api", Some(&codex_home)).unwrap();

        let state = load_model_sync_state(Some(&codex_home));
        assert_eq!(state.current_source_model.as_deref(), Some("gpt-5.4-mini"));
        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.4-mini\""));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_model_for_profile_prefers_target_inference_over_stale_state_for_api_key() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("prefer-inference");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","api_key":"sk-test"}"#,
            r#"{"folder_name":"api","model_mappings":[{"source_model":"gpt-5.4-mini","target_model":"provider-mini"}]}"#,
        );
        save_model_sync_state(
            Some(&codex_home),
            &ModelSyncState {
                current_source_model: Some("gpt-5.2".to_string()),
                current_profile: Some("chat".to_string()),
            },
        )
        .unwrap();
        fs::write(
            codex_home.join("config.toml"),
            "model = \"provider-mini\"\n",
        )
        .unwrap();

        sync_root_model_for_profile("api", Some(&codex_home)).unwrap();

        let state = load_model_sync_state(Some(&codex_home));
        assert_eq!(state.current_source_model.as_deref(), Some("gpt-5.4-mini"));
        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.4-mini\""));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_model_for_profile_restores_source_model_when_profile_is_not_api_key() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("restore-source");
        write_profile(
            &codex_home,
            "chat",
            r#"{"auth_mode":"chatgpt"}"#,
            r#"{"folder_name":"chat","model_mappings":[{"source_model":"gpt-5.4","target_model":"provider-a"}]}"#,
        );
        save_model_sync_state(
            Some(&codex_home),
            &ModelSyncState {
                current_source_model: Some("gpt-5.2".to_string()),
                current_profile: Some("api".to_string()),
            },
        )
        .unwrap();
        fs::write(codex_home.join("config.toml"), "model = \"provider-a\"\n").unwrap();

        sync_root_model_for_profile("chat", Some(&codex_home)).unwrap();

        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.2\""));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_root_model_for_profile_updates_mapping_after_live_source_change() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("live-source-change");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","api_key":"sk-test"}"#,
            r#"{"folder_name":"api","model_mappings":[{"source_model":"gpt-5.2","target_model":"provider-two"},{"source_model":"gpt-5.4","target_model":"provider-four"}]}"#,
        );
        save_model_sync_state(
            Some(&codex_home),
            &ModelSyncState {
                current_source_model: Some("gpt-5.4".to_string()),
                current_profile: Some("api".to_string()),
            },
        )
        .unwrap();
        fs::write(codex_home.join("config.toml"), "model = \"gpt-5.2\"\n").unwrap();

        sync_root_model_for_profile("api", Some(&codex_home)).unwrap();

        let state = load_model_sync_state(Some(&codex_home));
        assert_eq!(state.current_source_model.as_deref(), Some("gpt-5.2"));
        let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"gpt-5.2\""));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn resolve_provider_target_model_for_request_maps_source_model_to_target_model() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("resolve-request-source");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","api_key":"sk-test"}"#,
            r#"{"folder_name":"api","model_mappings":[{"source_model":"gpt-5.4","target_model":"provider-a"}]}"#,
        );

        let resolved =
            resolve_provider_target_model_for_request("api", "gpt-5.4", Some(&codex_home)).unwrap();

        assert_eq!(resolved, "provider-a");
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn resolve_provider_target_model_for_request_keeps_existing_target_model() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("resolve-request-target");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","api_key":"sk-test"}"#,
            r#"{"folder_name":"api","model_mappings":[{"source_model":"gpt-5.4","target_model":"provider-a"}]}"#,
        );

        let resolved =
            resolve_provider_target_model_for_request("api", "provider-a", Some(&codex_home))
                .unwrap();

        assert_eq!(resolved, "provider-a");
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_profile_api_key_accepts_uppercase_env_style_field_name() {
        let _guard = env_guard();
        let codex_home = temp_codex_home("uppercase-api-key");
        write_profile(
            &codex_home,
            "api",
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-test"}"#,
            r#"{"folder_name":"api"}"#,
        );

        let api_key = load_profile_api_key("api", &codex_home);

        assert_eq!(api_key.as_deref(), Some("sk-test"));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn extract_provider_models_handles_openai_compatible_response() {
        let payload = json!({
            "object": "list",
            "data": [
                { "id": "provider-a" },
                { "id": "provider-b" },
                { "id": "provider-a" }
            ]
        });

        let models = extract_provider_models(&payload);

        assert_eq!(
            models,
            vec!["provider-a".to_string(), "provider-b".to_string()]
        );
    }

    #[test]
    fn build_model_endpoint_candidates_adds_v1_fallback() {
        let endpoints = build_model_endpoint_candidates("https://example.com/openai");

        assert_eq!(
            endpoints,
            vec![
                "https://example.com/openai/models".to_string(),
                "https://example.com/openai/v1/models".to_string(),
            ]
        );
    }

    #[test]
    fn build_protocol_probe_endpoint_from_model_endpoint_reuses_loaded_v1_path() {
        let endpoint = build_protocol_probe_endpoint_from_model_endpoint(
            "https://api.siliconflow.cn/v1/models",
            "/responses",
        );

        assert_eq!(
            endpoint.as_deref(),
            Some("https://api.siliconflow.cn/v1/responses")
        );
    }

    #[test]
    fn classify_protocol_endpoint_availability_distinguishes_missing_and_supported() {
        assert_eq!(
            classify_protocol_endpoint_availability(StatusCode::NOT_FOUND),
            ProtocolEndpointAvailability::Missing
        );
        assert_eq!(
            classify_protocol_endpoint_availability(StatusCode::BAD_REQUEST),
            ProtocolEndpointAvailability::Supported
        );
        assert_eq!(
            classify_protocol_endpoint_availability(StatusCode::INTERNAL_SERVER_ERROR),
            ProtocolEndpointAvailability::Unknown
        );
    }

    #[test]
    fn provider_protocol_from_probe_states_detects_chat_completions_provider() {
        let provider_protocol = provider_protocol_from_probe_states(
            ProtocolEndpointAvailability::Missing,
            ProtocolEndpointAvailability::Supported,
            ProtocolEndpointAvailability::Missing,
            ProtocolEndpointAvailability::Missing,
        );

        assert_eq!(
            provider_protocol.as_deref(),
            Some(PROVIDER_PROTOCOL_CHAT_COMPLETIONS)
        );
    }

    #[test]
    fn provider_protocol_from_probe_states_detects_messages_provider() {
        let provider_protocol = provider_protocol_from_probe_states(
            ProtocolEndpointAvailability::Missing,
            ProtocolEndpointAvailability::Missing,
            ProtocolEndpointAvailability::Supported,
            ProtocolEndpointAvailability::Missing,
        );

        assert_eq!(
            provider_protocol.as_deref(),
            Some(PROVIDER_PROTOCOL_MESSAGES)
        );
    }

    #[test]
    fn provider_protocol_from_probe_states_skips_restricted_chat_completions() {
        let provider_protocol = provider_protocol_from_probe_states(
            ProtocolEndpointAvailability::Missing,
            ProtocolEndpointAvailability::Restricted,
            ProtocolEndpointAvailability::Supported,
            ProtocolEndpointAvailability::Missing,
        );

        assert_eq!(
            provider_protocol.as_deref(),
            Some(PROVIDER_PROTOCOL_MESSAGES)
        );
    }

    #[test]
    fn access_terminated_error_body_marks_protocol_as_restricted() {
        assert!(body_contains_access_terminated_error(
            r#"{"error":{"message":"Kimi For Coding is currently only available for Coding Agents","type":"access_terminated_error"}}"#
        ));
        assert!(!body_contains_access_terminated_error(
            r#"{"error":{"message":"invalid api key","type":"invalid_request_error"}}"#
        ));
    }

    #[test]
    fn kimi_base_url_supports_fallback_model_list() {
        assert!(is_kimi_coding_base_url("https://api.kimi.com/coding/v1"));
        assert_eq!(
            kimi_fallback_models("https://api.kimi.com/coding/v1"),
            Some(vec!["kimi-for-coding".to_string()])
        );
    }
}
