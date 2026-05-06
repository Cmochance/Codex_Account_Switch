use crate::errors::CommandError;
use crate::models::{GatewayStatus, GatewayUpdatePayload};
use crate::shared::gateway;

#[tauri::command]
pub fn get_gateway_status() -> Result<GatewayStatus, CommandError> {
    gateway::status(None).map_err(CommandError::from)
}

#[tauri::command]
pub async fn enable_gateway() -> Result<GatewayStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(|| gateway::enable(None))
        .await
        .map_err(|error| {
            CommandError::new(
                "GATEWAY_ENABLE_TASK_FAILED",
                format!("Gateway enable task failed: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn disable_gateway() -> Result<GatewayStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(|| gateway::disable(None))
        .await
        .map_err(|error| {
            CommandError::new(
                "GATEWAY_DISABLE_TASK_FAILED",
                format!("Gateway disable task failed: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn update_gateway_settings(
    payload: GatewayUpdatePayload,
) -> Result<GatewayStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || gateway::update_settings(payload, None))
        .await
        .map_err(|error| {
            CommandError::new(
                "GATEWAY_UPDATE_TASK_FAILED",
                format!("Gateway update task failed: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn recover_gateway() -> Result<GatewayStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(|| gateway::force_recover(None))
        .await
        .map_err(|error| {
            CommandError::new(
                "GATEWAY_RECOVER_TASK_FAILED",
                format!("Gateway recover task failed: {error}"),
            )
        })?
        .map_err(CommandError::from)
}
