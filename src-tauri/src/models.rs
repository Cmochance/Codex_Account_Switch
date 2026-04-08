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
