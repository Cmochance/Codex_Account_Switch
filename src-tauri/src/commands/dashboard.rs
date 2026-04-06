use crate::errors::CommandError;
use crate::models::DashboardResponse;
use crate::windows;

#[tauri::command]
pub fn get_dashboard(page: Option<u32>) -> Result<DashboardResponse, CommandError> {
    windows::dashboard::build_dashboard(page.unwrap_or(1), None).map_err(Into::into)
}
