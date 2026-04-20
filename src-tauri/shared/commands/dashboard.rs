use crate::errors::CommandError;
use crate::models::{CurrentQuotaResponse, ProfilesSnapshotResponse};

#[cfg(target_os = "macos")]
use crate::macos as platform_runtime;

#[cfg(not(target_os = "macos"))]
use crate::windows as platform_runtime;

#[tauri::command]
pub fn get_profiles_snapshot() -> Result<ProfilesSnapshotResponse, CommandError> {
    platform_runtime::profiles_index::load_profiles_snapshot(None).map_err(Into::into)
}

#[tauri::command]
pub fn get_current_live_quota() -> Result<CurrentQuotaResponse, CommandError> {
    platform_runtime::profiles_index::load_current_live_quota(None).map_err(Into::into)
}
