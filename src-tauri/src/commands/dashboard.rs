use crate::errors::CommandError;
use crate::models::{
    CurrentQuotaResponse, DashboardResponse, ProfilesSnapshotResponse, RuntimeSummary,
};
use crate::windows;

#[tauri::command]
pub fn get_dashboard(page: Option<u32>) -> Result<DashboardResponse, CommandError> {
    windows::dashboard::build_dashboard(page.unwrap_or(1), None).map_err(Into::into)
}

#[tauri::command]
pub fn get_profiles_snapshot() -> Result<ProfilesSnapshotResponse, CommandError> {
    windows::profiles_index::load_profiles_snapshot(None).map_err(Into::into)
}

#[tauri::command]
pub fn get_runtime_status() -> Result<RuntimeSummary, CommandError> {
    Ok(windows::dashboard::build_runtime_summary(None))
}

#[tauri::command]
pub fn get_current_live_quota() -> Result<CurrentQuotaResponse, CommandError> {
    windows::profiles_index::load_current_live_quota(None).map_err(Into::into)
}
