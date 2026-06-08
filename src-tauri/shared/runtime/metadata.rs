use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::models::{ProfileMetadata, QuotaSummary};

use super::paths::{
    get_backup_root, get_codex_home, get_profile_metadata_path, validate_profile_name,
};

/// Plan name written when the id_token says `free` but quota data
/// implies an active paid window — i.e. the cached id_token is stale
/// relative to ground truth and we have no authoritative `plan_type`
/// from the API yet. The front-end maps this token to a localized
/// "Unknown paid plan" label and prompts the user to re-login. Kept
/// distinct from a real "paid" tier so the UI can flag the uncertainty
/// instead of impersonating one of OpenAI's actual plan names.
pub(super) const UNKNOWN_PAID_PLAN_NAME: &str = "unknown_paid";

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<AuthTokens>,
}

#[derive(Deserialize)]
struct AuthTokens {
    access_token: Option<String>,
    id_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Deserialize)]
struct ChatGptAuthClaims {
    chatgpt_plan_type: Option<String>,
    chatgpt_subscription_active_until: Option<String>,
}

#[derive(Deserialize)]
struct IdTokenClaims {
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/auth")]
    auth: Option<ChatGptAuthClaims>,
    /// Older / variant id_token shapes seen in the wild that carry
    /// `chatgpt_plan_type` at the JWT payload root instead of inside the
    /// nested `https://api.openai.com/auth` claim. Mirrored from
    /// `steipete/CodexBar`'s defensive read so we don't lose the plan
    /// when OpenAI shifts the field around. Subscription expiry is read
    /// at the same level for the same reason.
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
    #[serde(default)]
    chatgpt_subscription_active_until: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AuthDerivedMetadata {
    pub account_label: Option<String>,
    pub plan_name: Option<String>,
    pub subscription_expires_at: Option<String>,
    pub has_plan_claims: bool,
}

fn normalized_value(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("replace-me"))
            .then(|| trimmed.to_string())
    })
}

fn decode_token_claims(token: &str) -> Option<IdTokenClaims> {
    let payload = token.split('.').nth(1)?;
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice::<IdTokenClaims>(&decoded).ok()
}

fn load_auth_metadata_from_path(auth_path: &Path) -> Option<AuthDerivedMetadata> {
    let raw = fs::read_to_string(auth_path).ok()?;
    let auth = serde_json::from_str::<AuthFile>(&raw).ok()?;
    let mut metadata = AuthDerivedMetadata::default();

    if let Some(tokens) = &auth.tokens {
        let claims = tokens
            .id_token
            .as_deref()
            .and_then(decode_token_claims)
            .or_else(|| tokens.access_token.as_deref().and_then(decode_token_claims));

        if let Some(claims) = claims {
            metadata.account_label = normalized_value(claims.email);
            let nested_plan = claims
                .auth
                .as_ref()
                .and_then(|auth| normalized_value(auth.chatgpt_plan_type.clone()));
            let nested_expiry = claims
                .auth
                .as_ref()
                .and_then(|auth| normalized_value(auth.chatgpt_subscription_active_until.clone()));
            let top_level_plan = normalized_value(claims.chatgpt_plan_type);
            let top_level_expiry = normalized_value(claims.chatgpt_subscription_active_until);

            let plan_name = nested_plan.or(top_level_plan);
            let subscription_expires_at = nested_expiry.or(top_level_expiry);

            // `has_plan_claims` is the signal apply_auth_metadata uses to
            // decide whether to overwrite stored plan_name. It must remain
            // true even when only the top-level fallback yielded a value,
            // so a JWT shape change doesn't silently leave plan stale.
            if claims.auth.is_some() || plan_name.is_some() || subscription_expires_at.is_some() {
                metadata.has_plan_claims = true;
                metadata.plan_name = plan_name;
                metadata.subscription_expires_at = subscription_expires_at;
            }
        }

        if metadata.account_label.is_none() {
            metadata.account_label = normalized_value(tokens.account_id.clone());
        }
    }

    Some(metadata)
}

/// Stable account identity for an on-disk `auth.json`: the OAuth `account_id`
/// and the id_token / access_token `email` claim, whichever are present.
///
/// Carries both (rather than a single fingerprint) because one account can
/// present an email-only auth before a refresh adds `account_id` — matching on
/// a single prefixed string would then treat the same account as a stranger.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountIdentity {
    pub account_id: Option<String>,
    pub email: Option<String>,
}

impl AccountIdentity {
    /// Two identities refer to the same OpenAI account when they share a
    /// non-empty `account_id` OR a non-empty `email`. Each field is globally
    /// unique to one account, so OR-matching can never merge two distinct
    /// accounts; but it *does* keep a legacy email-only slot matching the same
    /// account after a later refresh writes `account_id`.
    pub fn same_account(&self, other: &AccountIdentity) -> bool {
        if let (Some(left), Some(right)) = (&self.account_id, &other.account_id) {
            if left == right {
                return true;
            }
        }
        if let (Some(left), Some(right)) = (&self.email, &other.email) {
            if left.eq_ignore_ascii_case(right) {
                return true;
            }
        }
        false
    }

    /// Human label for prompts: email preferred, else the account id.
    pub fn label(&self) -> Option<String> {
        self.email.clone().or_else(|| self.account_id.clone())
    }
}

/// Load the account identity from an `auth.json`. Returns `None` only when
/// neither `account_id` nor `email` is resolvable — placeholder cards
/// (`replace-me`), apikey-mode auth (no `tokens`), or an unreadable / absent
/// file. Callers MUST treat `None` as "identity unknown" (preserve legacy
/// behavior) rather than as a mismatch, so apikey / placeholder profiles keep
/// refreshing normally.
pub fn load_account_identity_from_path(auth_path: &Path) -> Option<AccountIdentity> {
    let raw = fs::read_to_string(auth_path).ok()?;
    let auth = serde_json::from_str::<AuthFile>(&raw).ok()?;
    let tokens = auth.tokens?;

    let account_id = normalized_value(tokens.account_id.clone());
    let email = tokens
        .id_token
        .as_deref()
        .and_then(decode_token_claims)
        .or_else(|| tokens.access_token.as_deref().and_then(decode_token_claims))
        .and_then(|claims| normalized_value(claims.email));

    let identity = AccountIdentity { account_id, email };
    (identity != AccountIdentity::default()).then_some(identity)
}

/// True only when `auth.json` is a genuine empty placeholder — it parses and
/// carries no usable credentials of any kind (no OAuth tokens, no API key). The
/// switch / bootstrap write-back uses this to decide whether a marked slot may
/// receive a drifted login.
///
/// Conservative by design: a missing / unreadable / malformed file, an API-key
/// card (`auth_mode = "apikey"` or a non-empty `OPENAI_API_KEY`), or any real
/// OAuth auth all return `false`. This is what keeps a drifted OAuth account
/// from being seated on top of an API-key card's real credentials — `None`
/// identity means "no OAuth identity," which is NOT the same as "empty slot".
pub fn auth_is_empty_placeholder(auth_path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(auth_path) else {
        return false;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return true;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return false;
    };

    // API-key card → real, non-OAuth credentials. Never seatable.
    if value.get("auth_mode").and_then(serde_json::Value::as_str) == Some("apikey") {
        return false;
    }
    if value
        .get("OPENAI_API_KEY")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|key| !key.trim().is_empty())
    {
        return false;
    }

    // Any usable OAuth token material → real card. Placeholder seeds use the
    // `replace-me` sentinel, which doesn't count.
    if let Some(tokens) = value.get("tokens") {
        let has_real_token = ["access_token", "id_token", "refresh_token", "account_id"]
            .iter()
            .any(|field| {
                tokens
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty() && !value.eq_ignore_ascii_case("replace-me"))
            });
        if has_real_token {
            return false;
        }
    }

    true
}

fn load_auth_metadata(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> Option<AuthDerivedMetadata> {
    let auth_path = get_backup_root(codex_home)
        .join(profile_name)
        .join("auth.json");
    load_auth_metadata_from_path(&auth_path)
}

#[allow(dead_code)]
pub fn load_root_auth_metadata(codex_home: Option<&Path>) -> Option<AuthDerivedMetadata> {
    let auth_path = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("auth.json");
    load_auth_metadata_from_path(&auth_path)
}

fn load_stored_profile_metadata(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> Option<ProfileMetadata> {
    let metadata_path = get_profile_metadata_path(profile_name, codex_home);
    let raw = fs::read_to_string(metadata_path).ok()?;
    serde_json::from_str::<ProfileMetadata>(&raw)
        .ok()
        .and_then(ProfileMetadata::validate)
}

fn load_or_init_profile_metadata(profile_name: &str, codex_home: Option<&Path>) -> ProfileMetadata {
    load_stored_profile_metadata(profile_name, codex_home)
        .unwrap_or_else(|| ProfileMetadata::with_folder_name(profile_name))
}

fn is_free_plan(plan_name: Option<&str>) -> bool {
    plan_name
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("free"))
}

/// True when *either* rate-limit window has data. Both 5h and weekly
/// signal "this account has a paid quota allotment" — Plus / Pro
/// surface 5h, Team / Enterprise surface weekly. The previous version
/// of this helper only checked 5h, which silently mis-classified
/// weekly-only accounts whose id_token still claimed `free`.
fn quota_has_paid_window(quota: &QuotaSummary) -> bool {
    let five_hour_present =
        quota.five_hour.remaining_percent.is_some() || quota.five_hour.refresh_at.is_some();
    let weekly_present =
        quota.weekly.remaining_percent.is_some() || quota.weekly.refresh_at.is_some();
    five_hour_present || weekly_present
}

fn apply_paid_fallback_for_free_plan(metadata: &mut ProfileMetadata) {
    if is_free_plan(metadata.plan_name.as_deref()) && quota_has_paid_window(&metadata.quota) {
        metadata.plan_name = Some(UNKNOWN_PAID_PLAN_NAME.to_string());
    }
}

fn apply_auth_metadata(
    metadata: &mut ProfileMetadata,
    auth_metadata: AuthDerivedMetadata,
    overwrite_account_label: bool,
) {
    let AuthDerivedMetadata {
        account_label,
        plan_name,
        subscription_expires_at,
        has_plan_claims,
    } = auth_metadata;

    if overwrite_account_label {
        if let Some(account_label) = account_label {
            metadata.account_label = Some(account_label);
        }
    } else if metadata
        .account_label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        metadata.account_label = account_label;
    }

    if has_plan_claims {
        metadata.plan_name = plan_name;
        metadata.subscription_expires_at = subscription_expires_at;
        // The free→unknown_paid fallback is intentionally NOT applied
        // here. It used to live at this layer but became wrong once
        // the API plan_type override path was added — when the API
        // confirms "free", we must respect that even if cached quota
        // still shows a paid window left over from before a downgrade.
        // Callers run the fallback themselves at the end of their sync
        // flow when (and only when) no API plan_type is available.
    }
}

/// Apply the free→unknown_paid heuristic to a metadata blob whose
/// plan_name was derived purely from the id_token (no fresher API
/// signal available). Idempotent and safe to call on metadata where
/// plan_name is None or already paid — both no-op.
fn apply_paid_fallback_when_api_unavailable(metadata: &mut ProfileMetadata) {
    apply_paid_fallback_for_free_plan(metadata)
}

fn hydrate_profile_metadata(
    mut metadata: ProfileMetadata,
    profile_name: &str,
    codex_home: Option<&Path>,
) -> ProfileMetadata {
    if metadata.folder_name.is_none() {
        metadata.folder_name = Some(profile_name.to_string());
    }

    if let Some(auth_metadata) = load_auth_metadata(profile_name, codex_home) {
        apply_auth_metadata(&mut metadata, auth_metadata, false);
    }

    metadata
}

fn update_profile_metadata<F>(
    profile_name: &str,
    codex_home: Option<&Path>,
    updater: F,
) -> Result<ProfileMetadata, crate::errors::AppError>
where
    F: FnOnce(&mut ProfileMetadata),
{
    let profile_name = validate_profile_name(profile_name)?;
    let mut metadata = load_or_init_profile_metadata(&profile_name, codex_home);
    metadata.folder_name = Some(profile_name.clone());
    updater(&mut metadata);
    save_profile_metadata(&profile_name, &metadata, codex_home)?;
    Ok(hydrate_profile_metadata(
        metadata,
        &profile_name,
        codex_home,
    ))
}

pub fn load_profile_metadata(profile_name: &str, codex_home: Option<&Path>) -> ProfileMetadata {
    let profile_name = match validate_profile_name(profile_name) {
        Ok(value) => value,
        Err(_) => return ProfileMetadata::with_folder_name(profile_name),
    };

    let metadata = load_or_init_profile_metadata(&profile_name, codex_home);

    hydrate_profile_metadata(metadata, &profile_name, codex_home)
}

/// Re-derive the auth-backed slice of `ProfileMetadata` (account label,
/// plan tier, subscription expiry) from disk and persist it.
///
/// `api_plan_type_override`, when present and non-empty, replaces the
/// id_token-derived `plan_name`. Callers that just refreshed the
/// ChatGPT-API `wham/usage` payload pass `Some(plan_type)` so the
/// authoritative live tier wins over a possibly-stale id_token claim;
/// the legacy `codex exec` path and the post-login flow pass `None`
/// because they have no fresher source.
///
/// This is the plan-side half of the D1 split — the quota half is
/// `sync_profile_quota`. The two operations used to live in a single
/// `sync_profile_metadata_from_auth_and_quota` helper, but plan info
/// and quota usage move on entirely different cadences (plan: weeks
/// between renewals; quota: minutes within a 5-hour window) and the
/// callers that needed both could just chain the two writes. Splitting
/// keeps each function's contract small and keeps API plan overrides
/// from being shoehorned alongside unrelated quota arguments.
pub fn sync_profile_metadata_from_auth(
    profile_name: &str,
    api_plan_type_override: Option<String>,
    codex_home: Option<&Path>,
) -> Result<ProfileMetadata, crate::errors::AppError> {
    let auth_metadata = validate_profile_name(profile_name)
        .ok()
        .and_then(|profile_name| load_auth_metadata(&profile_name, codex_home));
    let api_plan_normalized = api_plan_type_override.and_then(|raw| {
        let trimmed = raw.trim();
        (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("replace-me"))
            .then(|| trimmed.to_string())
    });
    let now_ms = current_time_ms();
    update_profile_metadata(profile_name, codex_home, move |metadata| {
        let mut plan_confirmed = false;
        if let Some(auth_metadata) = auth_metadata {
            // `has_plan_claims` here means the id_token actually carried
            // plan info (nested or top-level), which counts as a
            // confirmed plan check — distinct from `quota_updated_at_ms`
            // since plan moves on a different cadence than usage.
            plan_confirmed |= auth_metadata.has_plan_claims;
            apply_auth_metadata(metadata, auth_metadata, true);
        }
        if let Some(api_plan) = api_plan_normalized {
            // API plan_type is authoritative. The id_token claim can
            // lag behind real plan changes (downgrades especially: a
            // user who moved Plus → Free can still see "plus" in
            // their cached id_token claim until the next OAuth-issuer
            // re-issue, which may never come during a refresh-token
            // round-trip). The /wham/usage response always reflects
            // the current backend state, so when it says "free" we
            // trust it even if the cached `metadata.quota` still has
            // a paid window left over from before the downgrade.
            metadata.plan_name = Some(api_plan);
            plan_confirmed = true;
        } else {
            // No API plan_type to lean on. Fall back to the heuristic
            // that flips a stale "free" claim to `unknown_paid` when
            // the cached quota looks paid. This is the right call only
            // when we have *no* fresher source — once an API answer is
            // available it's authoritative regardless of what the cached
            // quota says.
            apply_paid_fallback_when_api_unavailable(metadata);
        }
        if plan_confirmed {
            metadata.last_plan_check_ms = now_ms;
        }
    })
}

/// Wall-clock millis since the Unix epoch, returning `None` only when
/// the system clock is set before 1970 (effectively impossible on
/// shipping macOS / Windows hardware).
fn current_time_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
}

pub fn sync_profile_quota(
    profile_name: &str,
    quota: QuotaSummary,
    quota_updated_at_ms: Option<u64>,
    codex_home: Option<&Path>,
) -> Result<ProfileMetadata, crate::errors::AppError> {
    update_profile_metadata(profile_name, codex_home, move |metadata| {
        metadata.quota = quota;
        metadata.quota_updated_at_ms = quota_updated_at_ms;
    })
}

pub fn sync_profile_openai_base_url(
    profile_name: &str,
    openai_base_url: Option<String>,
    codex_home: Option<&Path>,
) -> Result<ProfileMetadata, crate::errors::AppError> {
    update_profile_metadata(profile_name, codex_home, move |metadata| {
        metadata.openai_base_url = openai_base_url;
    })
}

pub fn save_profile_metadata(
    profile_name: &str,
    metadata: &ProfileMetadata,
    codex_home: Option<&Path>,
) -> Result<(), crate::errors::AppError> {
    let profile_name = validate_profile_name(profile_name)?;
    let metadata_path = get_profile_metadata_path(&profile_name, codex_home);
    let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
        crate::errors::AppError::new(
            "PROFILE_METADATA_INVALID",
            format!("Failed to serialize metadata: {error}"),
        )
    })?;

    fs::write(metadata_path, format!("{serialized}\n")).map_err(|error| {
        crate::errors::AppError::new(
            "PROFILE_METADATA_WRITE_FAILED",
            format!("Failed to write metadata: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::QuotaWindow;

    fn auth_plan(plan_name: &str) -> AuthDerivedMetadata {
        AuthDerivedMetadata {
            plan_name: Some(plan_name.to_string()),
            has_plan_claims: true,
            ..AuthDerivedMetadata::default()
        }
    }

    fn quota_with_five_hour() -> QuotaSummary {
        QuotaSummary {
            five_hour: QuotaWindow {
                remaining_percent: Some(99),
                refresh_at: Some("2026-04-21 13:37".to_string()),
                ..QuotaWindow::default()
            },
            ..QuotaSummary::default()
        }
    }

    #[test]
    fn free_plan_with_five_hour_quota_displays_unknown_paid_when_api_unavailable() {
        let mut metadata = ProfileMetadata {
            quota: quota_with_five_hour(),
            ..ProfileMetadata::default()
        };

        // The shape of the production sync flow when no API plan_type
        // is available: apply_auth_metadata writes plan_name from the
        // id_token, then the caller runs the paid-fallback helper.
        apply_auth_metadata(&mut metadata, auth_plan("free"), true);
        apply_paid_fallback_when_api_unavailable(&mut metadata);

        // The id_token says "free" but quota data implies an active
        // paid window — flag as `unknown_paid` so the UI prompts the
        // user to re-login instead of inventing a tier name.
        assert_eq!(metadata.plan_name.as_deref(), Some("unknown_paid"));
    }

    #[test]
    fn free_plan_without_five_hour_quota_stays_free() {
        let mut metadata = ProfileMetadata::default();

        apply_auth_metadata(&mut metadata, auth_plan("free"), true);
        apply_paid_fallback_when_api_unavailable(&mut metadata);

        assert_eq!(metadata.plan_name.as_deref(), Some("free"));
    }

    #[test]
    fn apply_auth_metadata_does_not_fold_quota_into_plan_anymore() {
        // Regression: `apply_auth_metadata` used to inline the
        // paid-fallback heuristic. After the API-authoritative split
        // it must NOT — otherwise an API answer like "free" coming
        // through `sync_profile_metadata_from_auth` would be flipped
        // to `unknown_paid` even when the API explicitly confirmed the
        // downgrade. Pin that the function leaves quota out of plan.
        let mut metadata = ProfileMetadata {
            quota: quota_with_five_hour(),
            ..ProfileMetadata::default()
        };

        apply_auth_metadata(&mut metadata, auth_plan("free"), true);

        assert_eq!(
            metadata.plan_name.as_deref(),
            Some("free"),
            "apply_auth_metadata must mirror the id_token claim verbatim — \
             the paid fallback is the caller's job once it knows whether \
             an API plan_type is available"
        );
    }

    #[test]
    fn returned_paid_tiers_are_not_reclassified() {
        for plan_name in ["plus", "pro"] {
            let mut metadata = ProfileMetadata {
                quota: quota_with_five_hour(),
                ..ProfileMetadata::default()
            };

            apply_auth_metadata(&mut metadata, auth_plan(plan_name), true);

            assert_eq!(metadata.plan_name.as_deref(), Some(plan_name));
        }
    }

    /// `apply_paid_fallback_for_free_plan` is exercised through
    /// `apply_auth_metadata` above, but the API-plan override path
    /// in `sync_profile_metadata_from_auth_and_quota` runs the same
    /// helper after substituting the plan. Verify the helper itself
    /// still flips a stale "free" claim to `unknown_paid` when quota is
    /// present, so the API-plan override path inherits that behavior
    /// without needing its own dedicated test infrastructure (which
    /// would require a real on-disk profile.json).
    #[test]
    fn paid_fallback_flips_stale_free_to_unknown_paid_when_quota_present() {
        let mut metadata = ProfileMetadata {
            plan_name: Some("free".to_string()),
            quota: quota_with_five_hour(),
            ..ProfileMetadata::default()
        };

        apply_paid_fallback_for_free_plan(&mut metadata);

        assert_eq!(
            metadata.plan_name.as_deref(),
            Some("unknown_paid"),
            "free plan with active quota window must be flagged as \
             unknown_paid so the UI can prompt re-login"
        );
    }

    #[test]
    fn paid_fallback_leaves_explicit_paid_tier_untouched() {
        let mut metadata = ProfileMetadata {
            plan_name: Some("pro".to_string()),
            quota: quota_with_five_hour(),
            ..ProfileMetadata::default()
        };

        apply_paid_fallback_for_free_plan(&mut metadata);

        assert_eq!(
            metadata.plan_name.as_deref(),
            Some("pro"),
            "non-free plans must never be rewritten by the fallback"
        );
    }

    #[test]
    fn paid_fallback_leaves_free_plan_alone_without_quota() {
        let mut metadata = ProfileMetadata {
            plan_name: Some("free".to_string()),
            ..ProfileMetadata::default()
        };

        apply_paid_fallback_for_free_plan(&mut metadata);

        assert_eq!(
            metadata.plan_name.as_deref(),
            Some("free"),
            "free plan without quota signal must stay free"
        );
    }

    #[test]
    fn paid_fallback_flips_stale_free_when_only_weekly_window_present() {
        // Team / Enterprise accounts may surface only a weekly window
        // (no 5h budget enforcement). Before the routing fix, the only
        // way `apply_paid_fallback_for_free_plan` would trigger was a
        // 5h signal — so a Team account whose id_token still claimed
        // `free` could keep showing "Free" with weekly quota visible.
        let mut metadata = ProfileMetadata {
            plan_name: Some("free".to_string()),
            quota: QuotaSummary {
                five_hour: QuotaWindow::default(),
                weekly: QuotaWindow {
                    remaining_percent: Some(82),
                    refresh_at: Some("2026-05-15 12:00".to_string()),
                    ..QuotaWindow::default()
                },
            },
            ..ProfileMetadata::default()
        };

        apply_paid_fallback_for_free_plan(&mut metadata);

        assert_eq!(
            metadata.plan_name.as_deref(),
            Some("unknown_paid"),
            "weekly-only quota implies a paid tier just as much as 5h does"
        );
    }

    /// Assemble a JWT-like string from a JSON payload. Header and
    /// signature are placeholders; only the payload matters for our
    /// claim decoding. Base64-url-no-pad to match the production
    /// decoding path.
    fn synthesize_jwt(payload_json: &str) -> String {
        let header = general_purpose::URL_SAFE_NO_PAD.encode(b"{}");
        let payload = general_purpose::URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
        format!("{header}.{payload}.signature")
    }

    fn write_auth_with_id_token(dir: &Path, id_token: &str) {
        let auth_json = format!(
            "{{\"tokens\":{{\"id_token\":{}}}}}",
            serde_json::Value::String(id_token.to_string())
        );
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("auth.json"), auth_json).unwrap();
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("codex-switch-metadata-{name}-{unique}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn nested_chatgpt_plan_type_is_preferred_when_present() {
        let payload = r#"{
            "email": "user@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "pro",
                "chatgpt_subscription_active_until": "2099-01-01T00:00:00+00:00"
            },
            "chatgpt_plan_type": "free",
            "chatgpt_subscription_active_until": "1999-01-01T00:00:00+00:00"
        }"#;
        let dir = temp_dir("nested-wins");
        write_auth_with_id_token(&dir, &synthesize_jwt(payload));

        let derived = load_auth_metadata_from_path(&dir.join("auth.json"))
            .expect("auth metadata should parse");

        assert_eq!(derived.plan_name.as_deref(), Some("pro"));
        assert_eq!(
            derived.subscription_expires_at.as_deref(),
            Some("2099-01-01T00:00:00+00:00")
        );
    }

    #[test]
    fn top_level_chatgpt_plan_type_is_used_when_nested_missing() {
        // Mirrors what we observe when OpenAI changes the JWT shape and
        // drops the nested `https://api.openai.com/auth` claim — without
        // this fallback the dashboard would silently lose the plan.
        let payload = r#"{
            "email": "user@example.com",
            "chatgpt_plan_type": "plus",
            "chatgpt_subscription_active_until": "2099-12-31T23:59:59+00:00"
        }"#;
        let dir = temp_dir("top-level-fallback");
        write_auth_with_id_token(&dir, &synthesize_jwt(payload));

        let derived = load_auth_metadata_from_path(&dir.join("auth.json"))
            .expect("auth metadata should parse");

        assert!(
            derived.has_plan_claims,
            "top-level plan presence must still set has_plan_claims"
        );
        assert_eq!(derived.plan_name.as_deref(), Some("plus"));
        assert_eq!(
            derived.subscription_expires_at.as_deref(),
            Some("2099-12-31T23:59:59+00:00")
        );
    }

    #[test]
    fn apikey_auth_mode_returns_some_derived_metadata() {
        let dir = temp_dir("apikey-auth-mode");
        let auth_json = r#"{"auth_mode":"apikey"}"#;
        std::fs::write(dir.join("auth.json"), auth_json).unwrap();

        let derived = load_auth_metadata_from_path(&dir.join("auth.json"))
            .expect("apikey auth metadata should parse and return Some");

        assert_eq!(derived.account_label, None);
        assert_eq!(derived.plan_name, None);
        assert_eq!(derived.subscription_expires_at, None);
        assert!(!derived.has_plan_claims);
    }

    #[test]
    fn missing_claims_and_account_id_returns_some_empty_metadata() {
        let dir = temp_dir("missing-claims-and-acc-id");
        // A placeholder tokens object (e.g. for reverse proxies / developer cards)
        // with empty/missing or dummy values should return Some with None fields.
        let auth_json = r#"{
            "tokens": {
                "access_token": "replace-me",
                "id_token": "replace-me",
                "account_id": "replace-me"
            }
        }"#;
        std::fs::write(dir.join("auth.json"), auth_json).unwrap();

        let derived = load_auth_metadata_from_path(&dir.join("auth.json"))
            .expect("placeholder auth metadata should parse and return Some");

        assert_eq!(derived.account_label, None);
        assert_eq!(derived.plan_name, None);
        assert_eq!(derived.subscription_expires_at, None);
        assert!(!derived.has_plan_claims);
    }

    #[test]
    fn account_identity_captures_account_id_and_email() {
        // Both account_id and the id_token email are captured.
        let dir = temp_dir("identity-account-id");
        let id_token = synthesize_jwt(r#"{"email":"user@example.com"}"#);
        let auth = format!(
            "{{\"tokens\":{{\"account_id\":\"acct_123\",\"id_token\":{}}}}}",
            serde_json::Value::String(id_token)
        );
        std::fs::write(dir.join("auth.json"), auth).unwrap();
        let id = load_account_identity_from_path(&dir.join("auth.json")).unwrap();
        assert_eq!(id.account_id.as_deref(), Some("acct_123"));
        assert_eq!(id.email.as_deref(), Some("user@example.com"));

        // No account_id → email-only identity.
        let dir2 = temp_dir("identity-email-fallback");
        write_auth_with_id_token(&dir2, &synthesize_jwt(r#"{"email":"who@example.com"}"#));
        let id2 = load_account_identity_from_path(&dir2.join("auth.json")).unwrap();
        assert_eq!(id2.account_id, None);
        assert_eq!(id2.email.as_deref(), Some("who@example.com"));

        // apikey mode (no tokens) → no resolvable identity.
        let dir3 = temp_dir("identity-apikey");
        std::fs::write(dir3.join("auth.json"), r#"{"auth_mode":"apikey"}"#).unwrap();
        assert_eq!(load_account_identity_from_path(&dir3.join("auth.json")), None);

        // Placeholder account_id (`replace-me`) with no email → no identity.
        let dir4 = temp_dir("identity-placeholder");
        std::fs::write(
            dir4.join("auth.json"),
            r#"{"tokens":{"account_id":"replace-me"}}"#,
        )
        .unwrap();
        assert_eq!(load_account_identity_from_path(&dir4.join("auth.json")), None);
    }

    #[test]
    fn account_identity_same_account_matches_on_either_field() {
        // Email-only identity vs. account_id+email for the same account → same.
        let email_only = AccountIdentity {
            account_id: None,
            email: Some("user@example.com".to_string()),
        };
        let with_account_id = AccountIdentity {
            account_id: Some("acct_1".to_string()),
            email: Some("user@example.com".to_string()),
        };
        assert!(email_only.same_account(&with_account_id));
        assert!(with_account_id.same_account(&email_only));

        // Same account_id, different/absent email → still same.
        let id_a = AccountIdentity {
            account_id: Some("acct_1".to_string()),
            email: None,
        };
        let id_b = AccountIdentity {
            account_id: Some("acct_1".to_string()),
            email: Some("x@y.com".to_string()),
        };
        assert!(id_a.same_account(&id_b));

        // Different account_id and different email → distinct accounts.
        let other = AccountIdentity {
            account_id: Some("acct_2".to_string()),
            email: Some("other@example.com".to_string()),
        };
        assert!(!with_account_id.same_account(&other));

        // No shared identifiable field → cannot prove same; treat as distinct.
        let acct_only = AccountIdentity {
            account_id: Some("acct_3".to_string()),
            email: None,
        };
        let mail_only = AccountIdentity {
            account_id: None,
            email: Some("z@z.com".to_string()),
        };
        assert!(!acct_only.same_account(&mail_only));
    }

    #[test]
    fn auth_is_empty_placeholder_only_for_credential_free_slots() {
        let dir = temp_dir("placeholder-detect");
        let path = dir.join("auth.json");

        // Genuine placeholder: `replace-me` tokens only.
        std::fs::write(
            &path,
            r#"{"tokens":{"access_token":"replace-me","account_id":"replace-me"}}"#,
        )
        .unwrap();
        assert!(auth_is_empty_placeholder(&path), "replace-me seed is seatable");

        // Empty file → seatable.
        std::fs::write(&path, "  \n").unwrap();
        assert!(auth_is_empty_placeholder(&path), "empty file is seatable");

        // API-key card (auth_mode) → NOT a placeholder.
        std::fs::write(&path, r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#).unwrap();
        assert!(!auth_is_empty_placeholder(&path), "apikey card is not seatable");

        // Bare OPENAI_API_KEY without auth_mode → NOT a placeholder.
        std::fs::write(&path, r#"{"OPENAI_API_KEY":"sk-y"}"#).unwrap();
        assert!(!auth_is_empty_placeholder(&path), "raw api key is not seatable");

        // Real OAuth token material → NOT a placeholder.
        std::fs::write(&path, r#"{"tokens":{"account_id":"acct_real"}}"#).unwrap();
        assert!(!auth_is_empty_placeholder(&path), "real oauth is not seatable");

        // Malformed JSON → conservative false (don't overwrite the unknown).
        std::fs::write(&path, "{not json").unwrap();
        assert!(!auth_is_empty_placeholder(&path), "malformed is not seatable");

        // Missing file → false.
        assert!(!auth_is_empty_placeholder(&dir.join("nope.json")));
    }
}
