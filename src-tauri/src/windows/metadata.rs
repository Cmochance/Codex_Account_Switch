use base64::{engine::general_purpose, Engine as _};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::models::ProfileMetadata;

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

pub fn load_root_auth_metadata(codex_home: Option<&Path>) -> Option<AuthDerivedMetadata> {
    let auth_path = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("auth.json");
    load_auth_metadata_from_path(&auth_path)
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
        if metadata
            .account_label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            metadata.account_label = auth_metadata.account_label;
        }

        if auth_metadata.has_plan_claims {
            metadata.plan_name = auth_metadata.plan_name;
            metadata.subscription_expires_at = auth_metadata.subscription_expires_at;
        }
    }

    metadata
}

pub fn load_profile_metadata(profile_name: &str, codex_home: Option<&Path>) -> ProfileMetadata {
    let profile_name = match validate_profile_name(profile_name) {
        Ok(value) => value,
        Err(_) => return ProfileMetadata::with_folder_name(profile_name),
    };

    let metadata_path = get_profile_metadata_path(&profile_name, codex_home);
    let default_metadata = || {
        hydrate_profile_metadata(
            ProfileMetadata::with_folder_name(&profile_name),
            &profile_name,
            codex_home,
        )
    };

    let raw = match fs::read_to_string(metadata_path) {
        Ok(value) => value,
        Err(_) => return default_metadata(),
    };

    let metadata = match serde_json::from_str::<ProfileMetadata>(&raw)
        .ok()
        .and_then(ProfileMetadata::validate)
    {
        Some(value) => value,
        None => return default_metadata(),
    };

    hydrate_profile_metadata(metadata, &profile_name, codex_home)
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
    use super::{load_profile_metadata, load_root_auth_metadata};
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
}
