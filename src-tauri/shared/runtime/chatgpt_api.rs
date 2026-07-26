//! Direct ChatGPT backend API client used to refresh per-profile quota
//! information without invoking the Codex CLI.
//!
//! Background: `~/.codex/sessions/<date>/<sid>.jsonl` only receives quota
//! data after Codex makes a real request. The legacy "refresh" path forces
//! that by running `codex exec ... "Reply with the single word OK."`, which
//! costs the user a tiny slice of their quota and depends on a working CLI
//! discovery pipeline. The Codex CLI itself, [Lampese/codex-switcher][1] and
//! [steipete/CodexBar][2] all read live quota directly from the same private
//! ChatGPT backend endpoint:
//!
//!   GET https://chatgpt.com/backend-api/wham/usage
//!     Authorization: Bearer <access_token>
//!     User-Agent: codex-cli/1.0.0
//!     chatgpt-account-id: <account_id>
//!
//! This module wraps that endpoint plus the OAuth refresh-token flow at
//! `https://auth.openai.com/oauth/token` (so we can transparently retry
//! once on 401), and converts the response into the project's existing
//! `QuotaSummary` shape so callers can use it as a drop-in replacement
//! for the JSONL parser.
//!
//! API-key profiles are not supported here — they go through the existing
//! per-profile flow. ChatGPT/OAuth profiles are.
//!
//! [1]: https://github.com/Lampese/codex-switcher/blob/main/src-tauri/src/api/usage.rs
//! [2]: https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/OpenAIWeb/OpenAIDashboardFetcher.swift

use std::path::Path;
use std::time::Duration;

use base64::Engine;
use chrono::{TimeZone, Utc};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::errors::{AppError, AppResult};
use crate::models::{QuotaSummary, QuotaWindow, RateLimitResetCredit, RateLimitResetCredits};

use super::paths::get_backup_root;

/// 进程内共享的 `reqwest::blocking::Client` 缓存。代理配置变更时
/// 通过 `invalidate_http_client` 置 None，下一次 `build_http_client`
/// 按当前 `ProxyState` 重建。
static SHARED_CLIENT: std::sync::Mutex<Option<Client>> = std::sync::Mutex::new(None);

// Lightweight atomic write — stage to a sibling temp file, fsync-free rename
// into place. v1.5.3's `fs_ops` predates the shared `atomic_write_bytes`
// helper; this inline version keeps `auth.json` writes from being torn while
// avoiding a larger backport of the 1.6.x fs_ops module.
fn atomic_write_bytes(target: &std::path::Path, contents: impl AsRef<[u8]>) -> AppResult<()> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AppError::new(
                    "FS_CREATE_FAILED",
                    format!(
                        "Failed to create parent directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }
    }
    let mut temp = target.to_path_buf();
    let suffix = format!(
        ".{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let mut name = temp
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(&suffix);
    temp.set_file_name(name);
    if let Err(error) = std::fs::write(&temp, contents.as_ref()) {
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::new(
            "FS_WRITE_FAILED",
            format!("Failed to stage write to {}: {error}", temp.display()),
        ));
    }
    std::fs::rename(&temp, target).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        AppError::new(
            "FS_WRITE_FAILED",
            format!(
                "Failed to publish atomic write to {}: {error}",
                target.display()
            ),
        )
    })
}

const ISSUER: &str = "https://auth.openai.com";
/// Public OAuth client id used by the ChatGPT desktop / Codex CLI flow.
/// Verified by inspecting the official `codex` CLI auth code and confirmed
/// against codex-switcher's `auth/token_refresh.rs`.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CHATGPT_BACKEND: &str = "https://chatgpt.com/backend-api";
const USAGE_PATH: &str = "/wham/usage";
const RESET_CREDITS_PATH: &str = "/wham/rate-limit-reset-credits";
/// Reserved for future use: surfaces the active subscription / plan tier
/// without relying on the id_token claims, useful when migrating between
/// `prolite` / `plus` / `pro` / `business`.
#[allow(dead_code)]
const ACCOUNTS_CHECK_PATH: &str = "/accounts/check/v4-2023-04-27";
const CODEX_USER_AGENT: &str = "codex-cli/1.0.0";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// How old the cached `last_plan_check_ms` can get before a refresh
/// path escalates to a full OAuth token rotation. Single source of
/// truth for the per-card Refresh button (`mac/win refresh_runtime`)
/// and for the bulk plan refresh (`commands/dashboard`) so a future
/// tuning change moves all three in lock-step.
pub const PLAN_FRESHNESS_TTL_MS: u64 = 6 * 60 * 60 * 1000;

/// Predicate for "this refresh failure means the user has to log in
/// again." Substring-checks the error code + message for the canonical
/// ChatGPT relogin signatures so callers in `refresh_runtime` and
/// `codex_app_server` can map the same set of inputs to the same
/// `AUTH_REFRESH_RELOGIN_REQUIRED` outcome — without this, the
/// HTTP fast-path was silently swallowing relogin errors and forcing
/// the user to wait through a redundant app-server RPC fallback to
/// see the same diagnostic.
pub fn looks_like_relogin_required(error_code: &str, message: &str) -> bool {
    if error_code == "AUTH_REFRESH_RELOGIN_REQUIRED" {
        return true;
    }
    let lowered = message.to_ascii_lowercase();
    lowered.contains("token_invalidated")
        || lowered.contains("refresh_token_reused")
        || lowered.contains("authentication token has been invalidated")
        || lowered.contains("refresh token has already been used")
        || lowered.contains("please try signing in again")
        || lowered.contains("please log out and sign in again")
}
/// Refresh the access token a little before its actual expiry so a 401
/// in-flight does not bubble up to the caller.
const EXPIRY_SKEW_SECONDS: i64 = 300;

/// Outcome of a single ChatGPT-API refresh round-trip.
///
/// `plan_type` is the live plan from `/wham/usage`'s response —
/// authoritative because OpenAI returns the user's *current* tier even
/// when the cached id_token claim hasn't rotated yet. Callers feed it
/// into `metadata::sync_profile_metadata_from_auth_and_quota` so the
/// dashboard shows plan changes within a refresh cycle instead of
/// waiting for an id_token re-issue.
///
/// `subscription_expires_at` is still derived from the id_token (the
/// `/wham/usage` payload doesn't carry an expiry) and only used by
/// callers that don't have a fresher source.
#[derive(Debug, Clone, Default)]
pub struct ChatGptApiSnapshot {
    pub quota: Option<QuotaSummary>,
    pub plan_type: Option<String>,
    #[allow(dead_code)]
    pub subscription_expires_at: Option<String>,
}

#[derive(Deserialize)]
struct OAuthRefreshResponse {
    #[serde(default)]
    id_token: Option<String>,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct RateLimitStatusPayload {
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    rate_limit: Option<RateLimitDetails>,
    #[serde(default)]
    rate_limit_reset_credits: Option<RateLimitResetCredits>,
}

#[derive(Deserialize)]
struct RateLimitDetails {
    #[serde(default)]
    primary_window: Option<RateLimitWindow>,
    #[serde(default)]
    secondary_window: Option<RateLimitWindow>,
}

#[derive(Deserialize)]
struct RateLimitWindow {
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    reset_at: Option<i64>,
    /// Length in minutes. OpenAI uses this to identify which window
    /// (5h vs weekly) the entry belongs to. Position in the
    /// primary/secondary pair is *usually* aligned with the size, but
    /// real `token_count` events have been observed where a weekly
    /// window arrived in the primary slot — see `quota_routing` for
    /// the mapping logic.
    #[serde(default)]
    window_minutes: Option<i64>,
}

/// Cheap predicate the caller can hit before paying for an HTTP round-trip.
pub fn profile_supports_api_refresh(profile_dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(profile_dir.join("auth.json")) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    let auth_mode = parsed
        .get("auth_mode")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    auth_mode.as_deref() == Some("chatgpt")
}

/// Run a ChatGPT-API refresh against the given profile. On success the
/// caller can persist `snapshot.quota` into `profile.json` and skip the
/// legacy `codex exec` round-trip.
///
/// Returns `Err` if the profile is not an OAuth profile, the auth file
/// is malformed, the network call fails, or the refreshed access token
/// still cannot reach the API. Callers in `refresh_runtime::refresh_profile`
/// treat any error here as a signal to fall back to the legacy path.
///
/// Equivalent to [`refresh_profile_via_api_with_options`] with
/// `force_token_rotation = false`. Used by the silent dashboard ticker
/// which prefers the cheap path (skip OAuth refresh when access_token
/// is still valid).
pub fn refresh_profile_via_api(
    profile_name: &str,
    codex_home: &Path,
) -> AppResult<ChatGptApiSnapshot> {
    refresh_profile_via_api_with_options(profile_name, codex_home, RefreshOptions::default())
}

/// Knobs for `refresh_profile_via_api_with_options`. Today the only knob
/// is `force_token_rotation`; future fields can land here without
/// reshaping every call site.
#[derive(Debug, Clone, Copy, Default)]
pub struct RefreshOptions {
    /// When `true`, always run the OAuth refresh round-trip before the
    /// usage call, even if the cached access_token is still valid. This
    /// is what user-initiated card refreshes want: the id_token claims
    /// (plan tier, subscription expiry) only rotate as a side-effect of
    /// the refresh endpoint, so without forcing one a quiet account can
    /// stay stale for hours.
    pub force_token_rotation: bool,
}

/// Like [`refresh_profile_via_api`] but lets the caller force an OAuth
/// refresh (rotating the id_token claims) regardless of access_token
/// expiry. User-initiated card refreshes use this so plan / subscription
/// fields move within a single click instead of waiting for the cached
/// access_token to actually expire.
pub fn refresh_profile_via_api_with_options(
    profile_name: &str,
    codex_home: &Path,
    options: RefreshOptions,
) -> AppResult<ChatGptApiSnapshot> {
    let profile_dir = get_backup_root(Some(codex_home)).join(profile_name);
    if !profile_dir.is_dir() {
        return Err(AppError::new(
            "PROFILE_NOT_FOUND",
            format!("Profile not found: {profile_name}"),
        ));
    }

    let auth = read_auth_file(&profile_dir)?;
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return Err(AppError::new(
            "PROFILE_NOT_OAUTH",
            "API refresh path only applies to ChatGPT/OAuth profiles.",
        ));
    }

    let client = build_http_client()?;
    let usage_result =
        fetch_usage_with_retry(&client, &profile_dir, &auth, options.force_token_rotation)?;
    let reset_credit_cards = match fetch_reset_credits(&client, &usage_result.auth) {
        Ok(cards) => Some(cards),
        Err(error) => {
            eprintln!("Reset credits detail unavailable: {error}");
            None
        }
    };
    let usage_payload = usage_result.payload;
    let plan_type = usage_payload
        .plan_type
        .clone()
        .filter(|value| !value.trim().is_empty());
    let subscription_expires_at =
        read_subscription_expiry_from_id_token(&usage_result.auth.id_token);
    let mut quota = quota_summary_from_payload(&usage_payload);
    match reset_credit_cards {
        Some(reset_credit_cards) => {
            let summary = quota.get_or_insert_with(QuotaSummary::default);
            let reset_credits = summary
                .rate_limit_reset_credits
                .get_or_insert_with(RateLimitResetCredits::default);
            if reset_credits.available_count.is_none() {
                reset_credits.available_count = u32::try_from(reset_credit_cards.len()).ok();
            }
            reset_credits.credits = reset_credit_cards;
        }
        None => {
            // Every API write-back path persists this snapshot wholesale
            // (`snapshot.quota.unwrap_or_default()`), so a transient detail
            // failure would wipe previously confirmed cards without this
            // backfill — the session-snapshot paths already preserve them
            // the same way (see `select_current_quota`).
            let stored = super::metadata::load_profile_metadata(profile_name, Some(codex_home));
            backfill_reset_credits_from_stored(&mut quota, &stored.quota);
        }
    }

    Ok(ChatGptApiSnapshot {
        quota,
        plan_type,
        subscription_expires_at,
    })
}

#[derive(Default, Debug, Clone)]
struct ProfileAuthFile {
    auth_mode: Option<String>,
    access_token: String,
    refresh_token: String,
    account_id: Option<String>,
    id_token: String,
}

fn read_auth_file(profile_dir: &Path) -> AppResult<ProfileAuthFile> {
    let auth_path = profile_dir.join("auth.json");
    let raw = std::fs::read_to_string(&auth_path).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_MISSING",
            format!("Failed to read {}: {error}", auth_path.display()),
        )
    })?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_INVALID",
            format!("Failed to parse {}: {error}", auth_path.display()),
        )
    })?;

    let auth_mode = parsed
        .get("auth_mode")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);

    let tokens = parsed.get("tokens").ok_or_else(|| {
        AppError::new("PROFILE_AUTH_INVALID", "auth.json missing `tokens` field.")
    })?;
    let take = |key: &str| -> String {
        tokens
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let access_token = take("access_token");
    let refresh_token = take("refresh_token");
    let id_token = take("id_token");
    let account_id_raw = take("account_id");
    let account_id = if account_id_raw.is_empty() {
        None
    } else {
        Some(account_id_raw)
    };

    Ok(ProfileAuthFile {
        auth_mode,
        access_token,
        refresh_token,
        account_id,
        id_token,
    })
}

fn build_http_client() -> AppResult<Client> {
    // Cache the successful build only — `reqwest::blocking::Client`
    // wraps an `Arc<Inner>` so `clone()` is cheap and reuses the TLS
    // pool, but caching a Build *error* would poison the cell for
    // the entire process lifetime.
    //
    // 用 `Mutex<Option<Client>>` 而不是 `OnceLock`，是为了支持代理
    // 配置变更后丢弃旧 client、按新配置重建（见
    // `invalidate_http_client`）。`OnceLock` 一旦填充无法清空，与
    // "改代理立即生效" 的需求冲突。
    let mut guard = match SHARED_CLIENT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(client) = guard.as_ref() {
        return Ok(client.clone());
    }
    let mut builder = Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(CODEX_USER_AGENT);
    // 仅 macOS 启用应用层代理配置（Windows / Linux 走 reqwest 默认
    // 的环境变量读取行为）。
    #[cfg(target_os = "macos")]
    {
        if let Some(url) = crate::shared::proxy::read_proxy_state_cached().effective_url() {
            // `reqwest::Proxy::all` 同时覆盖 http / https / ftp；
            // socks5 / socks5h URL 需要 reqwest 的 `socks` feature，
            // 已在 Cargo.toml 启用。
            match reqwest::Proxy::all(url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                }
                Err(error) => {
                    return Err(AppError::new(
                        "HTTP_CLIENT_BUILD_FAILED",
                        format!("Failed to apply proxy {url:?}: {error}"),
                    ));
                }
            }
        }
    }
    let new_client = builder.build().map_err(|error| {
        AppError::new(
            "HTTP_CLIENT_BUILD_FAILED",
            format!("Failed to build HTTP client: {error}"),
        )
    })?;
    *guard = Some(new_client.clone());
    Ok(new_client)
}

/// 丢弃已缓存的 HTTP client，使下一次 `build_http_client` 按当前
/// `ProxyState` 重建。`set_proxy_config` / `clear_proxy_config`
/// command 调用，确保代理变更立即对后续 plan / quota 刷新生效，无
/// 需重启 app。
pub fn invalidate_http_client() {
    if let Ok(mut guard) = SHARED_CLIENT.lock() {
        *guard = None;
    }
}

fn build_chatgpt_headers(access_token: &str, account_id: Option<&str>) -> AppResult<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(CODEX_USER_AGENT));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {access_token}")).map_err(|error| {
            AppError::new(
                "HTTP_HEADER_INVALID",
                format!("Invalid access token header: {error}"),
            )
        })?,
    );
    if let Some(id) = account_id {
        let name = HeaderName::from_static("chatgpt-account-id");
        let value = HeaderValue::from_str(id).map_err(|error| {
            AppError::new(
                "HTTP_HEADER_INVALID",
                format!("Invalid chatgpt-account-id header: {error}"),
            )
        })?;
        headers.insert(name, value);
    }
    Ok(headers)
}

fn fetch_usage_with_retry(
    client: &Client,
    profile_dir: &Path,
    auth: &ProfileAuthFile,
    force_token_rotation: bool,
) -> AppResult<UsageFetchResult> {
    // 1) Refresh proactively if the JWT is already expired or about to
    //    expire. Avoids a guaranteed 401 + retry pair. When the caller
    //    forces rotation, refresh unconditionally — id_token claims
    //    (plan tier, subscription expiry) only rotate as a side-effect
    //    of the OAuth refresh endpoint, so user-initiated refreshes
    //    must take this path even when the access_token is still valid.
    let mut working_auth = auth.clone();
    if force_token_rotation || access_token_expired(&working_auth.access_token) {
        working_auth = refresh_oauth_tokens(client, profile_dir, &working_auth)?;
    }

    // 2) First attempt.
    let response = send_usage_request(client, &working_auth)?;
    if response.status() != StatusCode::UNAUTHORIZED {
        return Ok(UsageFetchResult {
            payload: decode_usage_response(response)?,
            auth: working_auth,
        });
    }

    // 3) On 401, refresh once and retry. If the refresh itself fails, the
    //    caller falls back to the legacy `codex exec` path.
    let refreshed = refresh_oauth_tokens(client, profile_dir, &working_auth)?;
    let retry = send_usage_request(client, &refreshed)?;
    Ok(UsageFetchResult {
        payload: decode_usage_response(retry)?,
        auth: refreshed,
    })
}

struct UsageFetchResult {
    payload: RateLimitStatusPayload,
    auth: ProfileAuthFile,
}

fn send_usage_request(
    client: &Client,
    auth: &ProfileAuthFile,
) -> AppResult<reqwest::blocking::Response> {
    let headers = build_chatgpt_headers(&auth.access_token, auth.account_id.as_deref())?;
    client
        .get(format!("{CHATGPT_BACKEND}{USAGE_PATH}"))
        .headers(headers)
        .send()
        .map_err(|error| {
            AppError::new(
                "HTTP_REQUEST_FAILED",
                format!("ChatGPT usage request failed: {error}"),
            )
        })
}

fn fetch_reset_credits(
    client: &Client,
    auth: &ProfileAuthFile,
) -> AppResult<Vec<RateLimitResetCredit>> {
    let headers = build_chatgpt_headers(&auth.access_token, auth.account_id.as_deref())?;
    let response = client
        .get(format!("{CHATGPT_BACKEND}{RESET_CREDITS_PATH}"))
        .headers(headers)
        .send()
        .map_err(|error| {
            AppError::new(
                "HTTP_REQUEST_FAILED",
                format!("ChatGPT reset credits request failed: {error}"),
            )
        })?;
    let status = response.status();
    if let Some(error) = reset_credits_status_error(status) {
        return Err(error);
    }
    let payload: Value = response.json().map_err(|error| {
        AppError::new(
            "RESET_CREDITS_PARSE_FAILED",
            format!("Failed to parse reset credits response: {error}"),
        )
    })?;
    reset_credits_from_payload(&payload).ok_or_else(|| {
        AppError::new(
            "RESET_CREDITS_PARSE_FAILED",
            "Reset credits response is missing a `credits` array.",
        )
    })
}

fn reset_credits_status_error(status: StatusCode) -> Option<AppError> {
    if status == StatusCode::UNAUTHORIZED {
        return Some(AppError::new(
            "RESET_CREDITS_UNAUTHORIZED",
            "Reset credits endpoint returned 401; the access token or account scope may be stale.",
        ));
    }
    (!status.is_success()).then(|| {
        AppError::new(
            "RESET_CREDITS_HTTP_ERROR",
            format!("Reset credits endpoint returned HTTP {status}."),
        )
    })
}

fn reset_credits_from_payload(payload: &Value) -> Option<Vec<RateLimitResetCredit>> {
    let credits = payload.get("credits")?.as_array()?;
    Some(
        credits
            .iter()
            .map(|credit| RateLimitResetCredit {
                granted_at: credit.get("granted_at").and_then(timestamp_seconds),
                expires_at: credit.get("expires_at").and_then(timestamp_seconds),
            })
            .collect(),
    )
}

/// Carry previously confirmed reset-credit details over into a fresh
/// snapshot whose detail fetch failed. Only fires when the new snapshot
/// carries no reset-credit information at all: if `/wham/usage` reported a
/// fresh `available_count`, mixing it with stale stored card timestamps
/// could show a count that contradicts the listed cards.
fn backfill_reset_credits_from_stored(
    quota: &mut Option<QuotaSummary>,
    stored_quota: &QuotaSummary,
) {
    if quota
        .as_ref()
        .is_some_and(|summary| summary.rate_limit_reset_credits.is_some())
    {
        return;
    }
    let Some(stored) = stored_quota.rate_limit_reset_credits.clone() else {
        return;
    };
    quota
        .get_or_insert_with(QuotaSummary::default)
        .rate_limit_reset_credits = Some(stored);
}

fn timestamp_seconds(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
            .map(|timestamp| timestamp.timestamp())
    })
}

fn decode_usage_response(
    response: reqwest::blocking::Response,
) -> AppResult<RateLimitStatusPayload> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(AppError::new(
            "CHATGPT_USAGE_HTTP_ERROR",
            format!("Usage endpoint returned {status}: {body}"),
        ));
    }
    response.json::<RateLimitStatusPayload>().map_err(|error| {
        AppError::new(
            "CHATGPT_USAGE_PARSE_FAILED",
            format!("Failed to parse usage response: {error}"),
        )
    })
}

fn refresh_oauth_tokens(
    client: &Client,
    profile_dir: &Path,
    auth: &ProfileAuthFile,
) -> AppResult<ProfileAuthFile> {
    if auth.refresh_token.is_empty() {
        return Err(AppError::new(
            "OAUTH_REFRESH_MISSING_TOKEN",
            "Profile auth.json has no refresh_token.",
        ));
    }

    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            url_encode(&auth.refresh_token),
            url_encode(CLIENT_ID),
        ))
        .send()
        .map_err(|error| {
            AppError::new(
                "OAUTH_REFRESH_FAILED",
                format!("Failed to send OAuth refresh request: {error}"),
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(AppError::new(
            "OAUTH_REFRESH_HTTP_ERROR",
            format!("OAuth refresh returned {status}: {body}"),
        ));
    }
    let parsed: OAuthRefreshResponse = response.json().map_err(|error| {
        AppError::new(
            "OAUTH_REFRESH_PARSE_FAILED",
            format!("Failed to parse OAuth refresh response: {error}"),
        )
    })?;

    let next_id_token = parsed.id_token.unwrap_or_else(|| auth.id_token.clone());
    let next_refresh_token = parsed
        .refresh_token
        .unwrap_or_else(|| auth.refresh_token.clone());

    let next = ProfileAuthFile {
        auth_mode: auth.auth_mode.clone(),
        access_token: parsed.access_token,
        refresh_token: next_refresh_token,
        account_id: auth.account_id.clone(),
        id_token: next_id_token,
    };
    persist_refreshed_auth(profile_dir, &next)?;
    Ok(next)
}

fn persist_refreshed_auth(profile_dir: &Path, auth: &ProfileAuthFile) -> AppResult<()> {
    let auth_path = profile_dir.join("auth.json");
    let raw = std::fs::read_to_string(&auth_path).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_MISSING",
            format!("Failed to read {}: {error}", auth_path.display()),
        )
    })?;
    let mut parsed: Value = serde_json::from_str(&raw).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_INVALID",
            format!("Failed to parse {}: {error}", auth_path.display()),
        )
    })?;

    let tokens = parsed
        .get_mut("tokens")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            AppError::new(
                "PROFILE_AUTH_INVALID",
                "auth.json missing tokens object during refresh persist.",
            )
        })?;
    tokens.insert(
        "access_token".to_string(),
        Value::String(auth.access_token.clone()),
    );
    tokens.insert(
        "refresh_token".to_string(),
        Value::String(auth.refresh_token.clone()),
    );
    if !auth.id_token.is_empty() {
        tokens.insert("id_token".to_string(), Value::String(auth.id_token.clone()));
    }
    if let Some(parent) = parsed.as_object_mut() {
        parent.insert(
            "last_refresh".to_string(),
            Value::String(Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        );
    }

    let serialized = serde_json::to_vec_pretty(&parsed).map_err(|error| {
        AppError::new(
            "PROFILE_AUTH_SERIALIZE_FAILED",
            format!("Failed to re-serialize auth.json: {error}"),
        )
    })?;
    atomic_write_bytes(&auth_path, serialized)
}

fn quota_summary_from_payload(payload: &RateLimitStatusPayload) -> Option<QuotaSummary> {
    use super::quota_routing::{slot_from_reset_at, slot_from_window_minutes, QuotaSlot};

    let mut summary = QuotaSummary::default();
    summary.rate_limit_reset_credits = payload.rate_limit_reset_credits.clone();
    let mut any_data = summary.rate_limit_reset_credits.is_some();

    let now_secs = Utc::now().timestamp();

    // Position is just a fallback; `window_minutes` is authoritative.
    // OpenAI usually puts the 5h window in primary and weekly in
    // secondary, but `token_count` events have been observed with the
    // weekly window in the primary slot and secondary null. Routing by
    // size means a Team plan whose only enforced window is weekly
    // doesn't get its data labeled as 5h on the dashboard.
    //
    // When `window_minutes` is missing (observed on `/wham/usage`),
    // fall back to classifying by `reset_at` distance from now: a 5h
    // window resets within hours, a weekly window resets in days.
    if let Some(rate_limit) = payload.rate_limit.as_ref() {
        for (window, fallback) in [
            (rate_limit.primary_window.as_ref(), QuotaSlot::FiveHour),
            (rate_limit.secondary_window.as_ref(), QuotaSlot::Weekly),
        ] {
            let Some(window) = window else { continue };
            let mapped = quota_window_from_rate_limit(window);
            if mapped.remaining_percent.is_none() && mapped.refresh_at.is_none() {
                continue;
            }
            any_data = true;
            let slot = slot_from_window_minutes(window.window_minutes, fallback);
            let slot = if window.window_minutes.is_none() {
                slot_from_reset_at(window.reset_at, now_secs).unwrap_or(slot)
            } else {
                slot
            };
            match slot {
                QuotaSlot::FiveHour => summary.five_hour = mapped,
                QuotaSlot::Weekly => summary.weekly = mapped,
            }
        }
    }

    if !any_data {
        return None;
    }

    Some(summary)
}

fn quota_window_from_rate_limit(window: &RateLimitWindow) -> QuotaWindow {
    let remaining_percent = window
        .used_percent
        .map(|used| (100.0 - used).round().clamp(0.0, 100.0) as u8);
    let refresh_at = window.reset_at.and_then(|seconds| {
        Utc.timestamp_opt(seconds, 0).single().map(|datetime| {
            datetime
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
    });
    QuotaWindow {
        remaining_percent,
        refresh_at,
        reset_at_timestamp: window.reset_at,
    }
}

fn read_subscription_expiry_from_id_token(id_token: &str) -> Option<String> {
    let claims = parse_jwt_payload(id_token)?;
    let auth_claims = claims.get("https://api.openai.com/auth")?;
    let raw = auth_claims
        .get("chatgpt_subscription_active_until")
        .and_then(Value::as_str)?
        .to_string();
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

fn access_token_expired(access_token: &str) -> bool {
    match parse_jwt_exp(access_token) {
        Some(expiry) => expiry <= Utc::now().timestamp() + EXPIRY_SKEW_SECONDS,
        None => false,
    }
}

fn parse_jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn parse_jwt_exp(token: &str) -> Option<i64> {
    parse_jwt_payload(token)
        .as_ref()?
        .get("exp")
        .and_then(Value::as_i64)
}

fn url_encode(raw: &str) -> String {
    // Hand-rolled URL form-encoding (RFC 3986 unreserved set + percent-
    // encoding everything else). Avoids pulling in `urlencoding` for one
    // call site.
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_jwt(payload: &Value) -> String {
        let body = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(payload).unwrap());
        format!("header.{body}.sig")
    }

    #[test]
    fn parse_jwt_exp_round_trips() {
        let token = make_jwt(&serde_json::json!({"exp": 1_900_000_000_i64}));
        assert_eq!(parse_jwt_exp(&token), Some(1_900_000_000));
    }

    #[test]
    fn read_subscription_expiry_pulls_chatgpt_subscription_active_until() {
        let token = make_jwt(&serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "plus",
                "chatgpt_subscription_active_until": "2026-04-23T05:03:38+00:00"
            }
        }));
        assert_eq!(
            read_subscription_expiry_from_id_token(&token).as_deref(),
            Some("2026-04-23T05:03:38+00:00"),
        );
    }

    #[test]
    fn quota_summary_from_payload_maps_used_percent_to_remaining_percent() {
        let payload: RateLimitStatusPayload = serde_json::from_value(serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": { "used_percent": 36.5, "reset_at": 1_715_000_000 },
                "secondary_window": { "used_percent": 7.2, "reset_at": 1_716_000_000 }
            }
        }))
        .unwrap();
        let quota = quota_summary_from_payload(&payload).unwrap();
        // 100 - 36.5 = 63.5 → 64 after round / clamp
        assert_eq!(quota.five_hour.remaining_percent, Some(64));
        // 100 - 7.2 = 92.8 → 93
        assert_eq!(quota.weekly.remaining_percent, Some(93));
        assert!(quota.five_hour.refresh_at.is_some());
        assert!(quota.weekly.refresh_at.is_some());
    }

    #[test]
    fn quota_summary_from_payload_returns_none_for_empty_rate_limit_blob() {
        let payload: RateLimitStatusPayload = serde_json::from_value(serde_json::json!({
            "plan_type": "plus",
            "rate_limit": { "primary_window": null, "secondary_window": null }
        }))
        .unwrap();
        assert!(quota_summary_from_payload(&payload).is_none());
    }

    #[test]
    fn quota_summary_preserves_available_reset_credit_count() {
        let payload: RateLimitStatusPayload = serde_json::from_value(serde_json::json!({
            "plan_type": "pro",
            "rate_limit_reset_credits": { "available_count": 2 }
        }))
        .unwrap();

        let quota = quota_summary_from_payload(&payload).expect("reset credit count");
        let credits = quota
            .rate_limit_reset_credits
            .expect("reset credit summary");
        assert_eq!(credits.available_count, Some(2));
        assert!(credits.credits.is_empty());
    }

    #[test]
    fn reset_credit_details_preserve_grant_and_expiry_without_persisting_ids() {
        let payload = serde_json::json!({
            "credits": [{
                "id": "must-not-be-persisted",
                "granted_at": "2026-07-12T21:12:45.739898Z",
                "expires_at": "2026-08-11T21:12:45.739898Z"
            }, {
                "id": "also-discarded",
                "granted_at": 1_783_900_000_i64,
                "expires_at": 1_786_500_000_i64
            }]
        });

        let credits = reset_credits_from_payload(&payload).expect("reset credit details");
        assert_eq!(credits.len(), 2);
        assert_eq!(credits[0].granted_at, Some(1_783_890_765));
        assert_eq!(credits[0].expires_at, Some(1_786_482_765));
        assert_eq!(credits[1].granted_at, Some(1_783_900_000));
        assert_eq!(credits[1].expires_at, Some(1_786_500_000));
    }

    #[test]
    fn reset_credits_401_is_reported_without_response_body() {
        let error = reset_credits_status_error(StatusCode::UNAUTHORIZED).expect("401 error");
        assert_eq!(error.error_code, "RESET_CREDITS_UNAUTHORIZED");
        assert!(error.message.contains("access token"));
    }

    fn stored_quota_with_credits() -> QuotaSummary {
        QuotaSummary {
            rate_limit_reset_credits: Some(RateLimitResetCredits {
                available_count: Some(2),
                credits: vec![RateLimitResetCredit {
                    granted_at: Some(1_783_890_765),
                    expires_at: Some(1_786_482_765),
                }],
            }),
            ..QuotaSummary::default()
        }
    }

    #[test]
    fn backfill_restores_stored_reset_credits_when_detail_fetch_fails() {
        let mut quota = Some(QuotaSummary {
            five_hour: QuotaWindow {
                remaining_percent: Some(80),
                ..QuotaWindow::default()
            },
            ..QuotaSummary::default()
        });

        backfill_reset_credits_from_stored(&mut quota, &stored_quota_with_credits());

        let summary = quota.expect("quota kept");
        assert_eq!(summary.five_hour.remaining_percent, Some(80));
        let credits = summary.rate_limit_reset_credits.expect("stored credits");
        assert_eq!(credits.available_count, Some(2));
        assert_eq!(credits.credits[0].expires_at, Some(1_786_482_765));
    }

    #[test]
    fn backfill_keeps_fresh_usage_count_over_stale_stored_cards() {
        let mut quota = Some(QuotaSummary {
            rate_limit_reset_credits: Some(RateLimitResetCredits {
                available_count: Some(1),
                credits: Vec::new(),
            }),
            ..QuotaSummary::default()
        });

        backfill_reset_credits_from_stored(&mut quota, &stored_quota_with_credits());

        let credits = quota
            .expect("quota kept")
            .rate_limit_reset_credits
            .expect("fresh count kept");
        assert_eq!(credits.available_count, Some(1));
        assert!(credits.credits.is_empty());
    }

    #[test]
    fn backfill_creates_summary_when_snapshot_has_no_quota() {
        let mut quota = None;

        backfill_reset_credits_from_stored(&mut quota, &stored_quota_with_credits());

        let summary = quota.expect("summary created for stored credits");
        assert!(summary.five_hour.remaining_percent.is_none());
        let credits = summary.rate_limit_reset_credits.expect("stored credits");
        assert_eq!(credits.available_count, Some(2));
    }

    #[test]
    fn backfill_is_a_no_op_without_stored_credits() {
        let mut quota = None;

        backfill_reset_credits_from_stored(&mut quota, &QuotaSummary::default());

        assert!(quota.is_none());
    }

    #[test]
    fn quota_summary_routes_by_window_minutes_when_weekly_lands_in_primary_slot() {
        // Real session JSONL audit on the maintainer's machine surfaced
        // 2 events shaped like this — primary holds the weekly window,
        // secondary is null. Without `window_minutes`-based routing,
        // the dashboard would show the weekly remaining percent as if
        // it were the 5h budget and leave the weekly bar empty.
        let payload: RateLimitStatusPayload = serde_json::from_value(serde_json::json!({
            "plan_type": "team",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 12.0,
                    "reset_at": 1_716_000_000,
                    "window_minutes": 10080
                },
                "secondary_window": null
            }
        }))
        .unwrap();

        let quota = quota_summary_from_payload(&payload).unwrap();

        assert!(
            quota.five_hour.remaining_percent.is_none() && quota.five_hour.refresh_at.is_none(),
            "5h slot must stay empty when only the weekly window is present"
        );
        // 100 - 12 = 88
        assert_eq!(quota.weekly.remaining_percent, Some(88));
        assert!(quota.weekly.refresh_at.is_some());
    }

    #[test]
    fn quota_summary_routes_by_window_minutes_when_five_hour_lands_in_secondary_slot() {
        // Symmetric guard: a 5h window arriving in the secondary slot
        // (e.g. an upstream that puts the longer/older window first)
        // should still be routed to `five_hour`, not `weekly`.
        let payload: RateLimitStatusPayload = serde_json::from_value(serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {
                    "used_percent": 12.0,
                    "reset_at": 1_716_000_000,
                    "window_minutes": 10080
                },
                "secondary_window": {
                    "used_percent": 30.0,
                    "reset_at": 1_715_000_000,
                    "window_minutes": 300
                }
            }
        }))
        .unwrap();

        let quota = quota_summary_from_payload(&payload).unwrap();

        assert_eq!(quota.weekly.remaining_percent, Some(88));
        assert_eq!(quota.five_hour.remaining_percent, Some(70));
    }

    #[test]
    fn url_encode_handles_token_special_chars() {
        assert_eq!(url_encode("abc"), "abc");
        assert_eq!(url_encode("a b"), "a%20b");
        assert_eq!(url_encode("a+b/c=d"), "a%2Bb%2Fc%3Dd");
    }

    #[test]
    fn access_token_expired_treats_unparseable_tokens_as_fresh() {
        // An opaque (non-JWT) token can't be parsed, so we conservatively
        // assume it is *not* expired and let the server tell us via 401.
        assert!(!access_token_expired("not-a-jwt"));
    }
}
