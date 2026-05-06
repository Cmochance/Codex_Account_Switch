use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct QuotaWindow {
    pub remaining_percent: Option<u8>,
    pub refresh_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct QuotaSummary {
    pub five_hour: QuotaWindow,
    pub weekly: QuotaWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProfileMetadata {
    pub folder_name: Option<String>,
    pub account_label: Option<String>,
    pub plan_name: Option<String>,
    pub subscription_expires_at: Option<String>,
    pub openai_base_url: Option<String>,
    pub quota: QuotaSummary,
    pub quota_updated_at_ms: Option<u64>,
}

impl ProfileMetadata {
    pub fn with_folder_name(folder_name: &str) -> Self {
        Self {
            folder_name: Some(folder_name.to_string()),
            ..Self::default()
        }
    }

    pub fn validate(self) -> Option<Self> {
        let five_hour_ok = self
            .quota
            .five_hour
            .remaining_percent
            .map_or(true, |value| value <= 100);
        let weekly_ok = self
            .quota
            .weekly
            .remaining_percent
            .map_or(true, |value| value <= 100);

        if five_hour_ok && weekly_ok {
            Some(self)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileCard {
    pub folder_name: String,
    pub display_title: String,
    pub account_label: Option<String>,
    pub status: String,
    pub auth_present: bool,
    pub has_account_identity: bool,
    pub plan_name: Option<String>,
    pub subscription_days_left: Option<i64>,
    pub openai_base_url: Option<String>,
    pub quota: QuotaSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentCard {
    pub folder_name: String,
    pub display_title: String,
    pub account_label: Option<String>,
    pub has_account_identity: bool,
    pub plan_name: Option<String>,
    pub subscription_days_left: Option<i64>,
    pub profile_folder_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProfileIndexEntry {
    pub folder_name: String,
    pub account_label: Option<String>,
    pub has_account_identity: bool,
    pub plan_name: Option<String>,
    pub subscription_expires_at: Option<String>,
    pub openai_base_url: Option<String>,
    pub auth_present: bool,
    pub stored_quota: QuotaSummary,
    pub stored_quota_updated_at_ms: Option<u64>,
    pub auth_mtime_ms: Option<u64>,
    pub auth_size: Option<u64>,
    pub profile_mtime_ms: Option<u64>,
    pub profile_size: Option<u64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProfilesIndex {
    pub schema_version: u32,
    pub updated_at: String,
    pub current_profile: Option<String>,
    pub profiles: Vec<ProfileIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesSnapshotResponse {
    pub page_size: u32,
    pub profiles: Vec<ProfileCard>,
    pub current_card: Option<CurrentCard>,
    pub current_quota_card: Option<QuotaSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentQuotaResponse {
    pub profile: Option<String>,
    pub quota: Option<QuotaSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilePayload {
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProfilePayload {
    pub folder_name: String,
    pub openai_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameProfilePayload {
    pub profile: String,
    pub new_folder_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProfileBaseUrlPayload {
    pub profile: String,
    pub openai_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckPayload {
    pub update_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenUrlPayload {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResponse {
    pub ok: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub has_update: bool,
    pub release_url: Option<String>,
    pub notes: Option<String>,
    pub checked_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchResponse {
    pub ok: bool,
    pub profile: String,
    pub message: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResponse {
    pub ok: bool,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStatus {
    /// User intent: forwarding should be on. Persisted in `state.json`.
    pub enabled: bool,
    /// Whether this app instance currently owns a sidecar `Child` handle.
    /// May be `false` even when something is listening on the port (e.g.
    /// after a GUI restart with an orphan sidecar).
    pub running: bool,
    /// True when a TCP probe to `127.0.0.1:port` succeeds. This is the
    /// authoritative signal for "is forwarding actually serving traffic
    /// right now?" and is what downstream Codex clients ultimately depend
    /// on. The UI surfaces this so users can tell `enabled but listening = false`
    /// (sidecar died) apart from `enabled and listening` (healthy).
    pub listening: bool,
    pub port: u16,
    pub endpoint: String,
    pub session_affinity: bool,
    pub strategy: String,
    pub active_auths: u32,
    pub last_error: Option<String>,
    pub sidecar_available: bool,
    pub config_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GatewayUpdatePayload {
    pub port: Option<u16>,
    pub session_affinity: Option<bool>,
    pub strategy: Option<String>,
}
