use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::models::{ProfileMetadata, QuotaSummary};

use super::paths::{
    get_backup_root, get_codex_home, get_profile_metadata_path, validate_profile_name,
};

const PAID_PLAN_NAME: &str = "paid";

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
    let tokens = auth.tokens?;
    let mut metadata = AuthDerivedMetadata::default();

    let claims = tokens
        .id_token
        .as_deref()
        .and_then(decode_token_claims)
        .or_else(|| tokens.access_token.as_deref().and_then(decode_token_claims));

    if let Some(claims) = claims {
        metadata.account_label = normalized_value(claims.email);
        if let Some(auth_claims) = claims.auth {
            metadata.has_plan_claims = true;
            metadata.plan_name = normalized_value(auth_claims.chatgpt_plan_type);
            metadata.subscription_expires_at =
                normalized_value(auth_claims.chatgpt_subscription_active_until);
        }
    }

    if metadata.account_label.is_none() {
        metadata.account_label = normalized_value(tokens.account_id);
    }

    if metadata.account_label.is_some() || metadata.has_plan_claims {
        Some(metadata)
    } else {
        None
    }
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

fn quota_has_five_hour_window(quota: &QuotaSummary) -> bool {
    quota.five_hour.remaining_percent.is_some() || quota.five_hour.refresh_at.is_some()
}

fn apply_paid_fallback_for_free_plan(metadata: &mut ProfileMetadata) {
    if is_free_plan(metadata.plan_name.as_deref()) && quota_has_five_hour_window(&metadata.quota) {
        metadata.plan_name = Some(PAID_PLAN_NAME.to_string());
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
        apply_paid_fallback_for_free_plan(metadata);
    }
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

pub fn sync_profile_metadata_from_auth(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> Result<ProfileMetadata, crate::errors::AppError> {
    let auth_metadata = validate_profile_name(profile_name)
        .ok()
        .and_then(|profile_name| load_auth_metadata(&profile_name, codex_home));
    update_profile_metadata(profile_name, codex_home, |metadata| {
        if let Some(auth_metadata) = auth_metadata {
            apply_auth_metadata(metadata, auth_metadata, true);
        }
    })
}

pub fn sync_profile_metadata_from_auth_and_quota(
    profile_name: &str,
    quota: QuotaSummary,
    quota_updated_at_ms: Option<u64>,
    codex_home: Option<&Path>,
) -> Result<ProfileMetadata, crate::errors::AppError> {
    let auth_metadata = validate_profile_name(profile_name)
        .ok()
        .and_then(|profile_name| load_auth_metadata(&profile_name, codex_home));
    update_profile_metadata(profile_name, codex_home, move |metadata| {
        metadata.quota = quota;
        metadata.quota_updated_at_ms = quota_updated_at_ms;
        if let Some(auth_metadata) = auth_metadata {
            apply_auth_metadata(metadata, auth_metadata, true);
        }
    })
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
            },
            ..QuotaSummary::default()
        }
    }

    #[test]
    fn free_plan_with_five_hour_quota_displays_paid() {
        let mut metadata = ProfileMetadata {
            quota: quota_with_five_hour(),
            ..ProfileMetadata::default()
        };

        apply_auth_metadata(&mut metadata, auth_plan("free"), true);

        assert_eq!(metadata.plan_name.as_deref(), Some("paid"));
    }

    #[test]
    fn free_plan_without_five_hour_quota_stays_free() {
        let mut metadata = ProfileMetadata::default();

        apply_auth_metadata(&mut metadata, auth_plan("free"), true);

        assert_eq!(metadata.plan_name.as_deref(), Some("free"));
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
}
