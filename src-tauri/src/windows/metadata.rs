use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::models::{ProfileMetadata, QuotaSummary};

use super::paths::{
    get_backup_root, get_codex_home, get_profile_metadata_path, validate_profile_name,
};

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
    Ok(hydrate_profile_metadata(metadata, &profile_name, codex_home))
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
    use super::{
        load_profile_metadata, load_root_auth_metadata, sync_profile_metadata_from_auth,
        sync_profile_openai_base_url, sync_profile_quota,
    };
    use crate::models::{QuotaSummary, QuotaWindow};
    use base64::{engine::general_purpose, Engine as _};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-metadata-{name}-{unique}"))
    }

    fn encode_jwt_payload(payload: &str) -> String {
        format!(
            "header.{}.signature",
            general_purpose::URL_SAFE_NO_PAD.encode(payload.as_bytes())
        )
    }

    #[test]
    fn load_profile_metadata_falls_back_to_auth_email_when_profile_json_is_missing() {
        let codex_home = temp_codex_home("auth-fallback-name");
        let profile_dir = codex_home.join("account_backup").join("a");
        fs::create_dir_all(&profile_dir).unwrap();
        let id_token = encode_jwt_payload(
            r#"{"name":"Jane Doe","email":"jane@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro","chatgpt_subscription_active_until":"2030-01-15T00:00:00+00:00"}}"#,
        );
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"id_token":"{id_token}","account_id":"acct_123"}}}}"#),
        )
        .unwrap();

        let metadata = load_profile_metadata("a", Some(&codex_home));

        assert_eq!(metadata.folder_name.as_deref(), Some("a"));
        assert_eq!(metadata.account_label.as_deref(), Some("jane@example.com"));
        assert_eq!(metadata.plan_name.as_deref(), Some("pro"));
        assert_eq!(
            metadata.subscription_expires_at.as_deref(),
            Some("2030-01-15T00:00:00+00:00")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_profile_metadata_ignores_placeholder_auth_tokens() {
        let codex_home = temp_codex_home("placeholder-auth");
        let profile_dir = codex_home.join("account_backup").join("d");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"id_token":"replace-me","account_id":"replace-me"}}"#,
        )
        .unwrap();

        let metadata = load_profile_metadata("d", Some(&codex_home));

        assert_eq!(metadata.folder_name.as_deref(), Some("d"));
        assert_eq!(metadata.account_label, None);
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_profile_metadata_keeps_explicit_account_label() {
        let codex_home = temp_codex_home("explicit-label");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        let id_token = encode_jwt_payload(
            r#"{"name":"Jane Doe","email":"jane@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"plus","chatgpt_subscription_active_until":"2031-03-01T00:00:00+00:00"}}"#,
        );
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"id_token":"{id_token}","account_id":"acct_123"}}}}"#),
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"b","account_label":"Custom Label"}"#,
        )
        .unwrap();

        let metadata = load_profile_metadata("b", Some(&codex_home));

        assert_eq!(metadata.account_label.as_deref(), Some("Custom Label"));
        assert_eq!(metadata.plan_name.as_deref(), Some("plus"));
        assert_eq!(
            metadata.subscription_expires_at.as_deref(),
            Some("2031-03-01T00:00:00+00:00")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_profile_openai_base_url_persists_custom_base_url() {
        let codex_home = temp_codex_home("sync-base-url");
        let profile_dir = codex_home.join("account_backup").join("api");
        fs::create_dir_all(&profile_dir).unwrap();

        let metadata = sync_profile_openai_base_url(
            "api",
            Some("https://example.com/v1".to_string()),
            Some(&codex_home),
        )
        .unwrap();

        assert_eq!(
            metadata.openai_base_url.as_deref(),
            Some("https://example.com/v1")
        );
        let saved = load_profile_metadata("api", Some(&codex_home));
        assert_eq!(
            saved.openai_base_url.as_deref(),
            Some("https://example.com/v1")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_profile_metadata_overrides_profile_plan_with_auth_claims() {
        let codex_home = temp_codex_home("auth-plan-override");
        let profile_dir = codex_home.join("account_backup").join("c");
        fs::create_dir_all(&profile_dir).unwrap();
        let id_token = encode_jwt_payload(
            r#"{"email":"c@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"team","chatgpt_subscription_active_until":"2032-05-20T00:00:00+00:00"}}"#,
        );
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"id_token":"{id_token}","account_id":"acct_123"}}}}"#),
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"c","plan_name":"stale-plan","subscription_expires_at":"2020-01-01"}"#,
        )
        .unwrap();

        let metadata = load_profile_metadata("c", Some(&codex_home));

        assert_eq!(metadata.plan_name.as_deref(), Some("team"));
        assert_eq!(
            metadata.subscription_expires_at.as_deref(),
            Some("2032-05-20T00:00:00+00:00")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_root_auth_metadata_returns_plan_fields() {
        let codex_home = temp_codex_home("root-auth-metadata");
        fs::create_dir_all(&codex_home).unwrap();
        let id_token = encode_jwt_payload(
            r#"{"email":"root@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro","chatgpt_subscription_active_until":"2033-07-10T00:00:00+00:00"}}"#,
        );
        fs::write(
            codex_home.join("auth.json"),
            format!(r#"{{"tokens":{{"id_token":"{id_token}","account_id":"acct_root"}}}}"#),
        )
        .unwrap();

        let metadata = load_root_auth_metadata(Some(&codex_home)).unwrap();

        assert_eq!(metadata.account_label.as_deref(), Some("root@example.com"));
        assert_eq!(metadata.plan_name.as_deref(), Some("pro"));
        assert_eq!(
            metadata.subscription_expires_at.as_deref(),
            Some("2033-07-10T00:00:00+00:00")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_profile_metadata_from_auth_overwrites_stale_account_label_and_preserves_quota() {
        let codex_home = temp_codex_home("sync-profile-metadata");
        let profile_dir = codex_home.join("account_backup").join("sync");
        fs::create_dir_all(&profile_dir).unwrap();
        let id_token = encode_jwt_payload(
            r#"{"email":"fresh@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro","chatgpt_subscription_active_until":"2034-02-01T00:00:00+00:00"}}"#,
        );
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"id_token":"{id_token}","account_id":"acct_sync"}}}}"#),
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"sync","account_label":"Old Label","quota":{"five_hour":{"remaining_percent":55,"refresh_at":"2030-01-01T00:00:00Z"},"weekly":{"remaining_percent":72,"refresh_at":"2030-01-08T00:00:00Z"}}}"#,
        )
        .unwrap();

        let synced = sync_profile_metadata_from_auth("sync", Some(&codex_home)).unwrap();

        assert_eq!(synced.account_label.as_deref(), Some("fresh@example.com"));
        assert_eq!(synced.plan_name.as_deref(), Some("pro"));
        assert_eq!(
            synced.subscription_expires_at.as_deref(),
            Some("2034-02-01T00:00:00+00:00")
        );
        assert_eq!(synced.quota.five_hour.remaining_percent, Some(55));
        assert_eq!(synced.quota.weekly.remaining_percent, Some(72));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn sync_profile_quota_updates_quota_and_preserves_auth_fields() {
        let codex_home = temp_codex_home("sync-profile-quota");
        let profile_dir = codex_home.join("account_backup").join("quota");
        fs::create_dir_all(&profile_dir).unwrap();
        let id_token = encode_jwt_payload(
            r#"{"email":"quota@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro","chatgpt_subscription_active_until":"2035-02-01T00:00:00+00:00"}}"#,
        );
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"id_token":"{id_token}","account_id":"acct_quota"}}}}"#),
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"quota","account_label":"quota@example.com","plan_name":"pro"}"#,
        )
        .unwrap();

        let synced = sync_profile_quota(
            "quota",
            QuotaSummary {
                five_hour: QuotaWindow {
                    remaining_percent: Some(91),
                    refresh_at: Some("2035-02-01 05:00".to_string()),
                },
                weekly: QuotaWindow {
                    remaining_percent: Some(77),
                    refresh_at: Some("2035-02-08 00:00".to_string()),
                },
            },
            Some(1_777_777_777_000),
            Some(&codex_home),
        )
        .unwrap();

        assert_eq!(synced.account_label.as_deref(), Some("quota@example.com"));
        assert_eq!(synced.plan_name.as_deref(), Some("pro"));
        assert_eq!(synced.quota.five_hour.remaining_percent, Some(91));
        assert_eq!(synced.quota.weekly.remaining_percent, Some(77));
        assert_eq!(synced.quota_updated_at_ms, Some(1_777_777_777_000));
        let _ = fs::remove_dir_all(&codex_home);
    }
}
