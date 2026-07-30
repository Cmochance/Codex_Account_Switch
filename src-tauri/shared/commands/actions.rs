use crate::errors::CommandError;
use crate::models::{
    ActionResponse, AddProfilePayload, CodexCliRedetectResult, CodexCliStatus, OpenUrlPayload,
    ProfilePayload, ProxyConfig, RenameProfilePayload, SetCodexCliPathPayload,
    SetProxyConfigPayload, UpdateCheckPayload, UpdateCheckResponse, UpdateProfileBaseUrlPayload,
};

#[cfg(target_os = "macos")]
use crate::macos as platform_runtime;

#[cfg(not(target_os = "macos"))]
use crate::windows as platform_runtime;

#[tauri::command]
pub fn open_codex() -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_codex_app()?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened Codex.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn login_current_profile() -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::login_current_profile()?;
    Ok(ActionResponse {
        ok: true,
        message: "Logged in current profile.".to_string(),
        path: Some(path),
    })
}

/// Per-card login: drives `codex login` against a sandboxed CODEX_HOME so
/// the OAuth handshake writes a fresh `auth.json` for `payload.profile`,
/// even when that profile is not the currently active one. Avoids the
/// switch-then-login-then-switch round-trip the dashboard used to require.
///
/// Long-running (blocks until the user finishes the OAuth flow in the
/// browser), so it spawns onto the blocking runtime to keep Tauri's main
/// thread responsive.
#[tauri::command]
pub async fn login_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let profile = payload.profile;
    let path = tauri::async_runtime::spawn_blocking(move || {
        platform_runtime::actions::login_profile(&profile)
    })
    .await
    .map_err(|error| CommandError::new("LOGIN_FAILED", format!("Login task failed: {error}")))??;
    Ok(ActionResponse {
        ok: true,
        message: "Logged in profile.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub async fn refresh_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let profile = payload.profile;
    let path = tauri::async_runtime::spawn_blocking(move || {
        platform_runtime::actions::refresh_profile(&profile)
    })
    .await
    .map_err(|error| {
        CommandError::new("REFRESH_FAILED", format!("Refresh task failed: {error}"))
    })??;
    Ok(ActionResponse {
        ok: true,
        message: "Refreshed profile auth.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn rename_profile(payload: RenameProfilePayload) -> Result<ActionResponse, CommandError> {
    let path =
        platform_runtime::actions::rename_profile(&payload.profile, &payload.new_folder_name)?;
    Ok(ActionResponse {
        ok: true,
        message: "Renamed profile folder.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn delete_profile(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::delete_profile(&payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Deleted profile.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn clear_profile_account(payload: ProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::clear_profile_account(&payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Cleared profile account.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn update_profile_base_url(
    payload: UpdateProfileBaseUrlPayload,
) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::update_profile_base_url(
        &payload.profile,
        &payload.openai_base_url,
    )?;
    Ok(ActionResponse {
        ok: true,
        message: "Updated profile Base Url.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_profile_folder(
    app: tauri::AppHandle,
    payload: ProfilePayload,
) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_profile_folder(&app, &payload.profile)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened profile folder.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn add_profile(payload: AddProfilePayload) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::add_profile(
        &payload.folder_name,
        payload.openai_base_url.as_deref(),
    )?;
    Ok(ActionResponse {
        ok: true,
        message: "Created profile template.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_contact(app: tauri::AppHandle) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_contact(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened contact URL.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_releases(app: tauri::AppHandle) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_releases(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened releases URL.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub fn open_url(
    app: tauri::AppHandle,
    payload: OpenUrlPayload,
) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_url(&app, &payload.url)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened URL.".to_string(),
        path: Some(path),
    })
}

#[tauri::command]
pub async fn check_update(
    payload: UpdateCheckPayload,
) -> Result<UpdateCheckResponse, CommandError> {
    let update_url = payload.update_url;
    tauri::async_runtime::spawn_blocking(move || crate::shared::update::check_update(&update_url))
        .await
        .map_err(|error| {
            CommandError::new(
                "UPDATE_CHECK_TASK_FAILED",
                format!("Update check task failed: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn get_codex_cli_status() -> Result<CodexCliStatus, CommandError> {
    let codex_home = platform_runtime::paths::get_codex_home();
    Ok(crate::shared::codex_cli_path::get_codex_cli_status(
        platform_runtime::codex_cli_resolver(),
        &codex_home,
    ))
}

#[tauri::command]
pub fn set_codex_cli_path(payload: SetCodexCliPathPayload) -> Result<CodexCliStatus, CommandError> {
    let codex_home = platform_runtime::paths::get_codex_home();
    Ok(crate::shared::codex_cli_path::set_codex_cli_path(
        platform_runtime::codex_cli_resolver(),
        &codex_home,
        &payload.path,
    )?)
}

#[tauri::command]
pub fn clear_codex_cli_path() -> Result<CodexCliStatus, CommandError> {
    let codex_home = platform_runtime::paths::get_codex_home();
    Ok(crate::shared::codex_cli_path::clear_codex_cli_path(
        platform_runtime::codex_cli_resolver(),
        &codex_home,
    ))
}

/// Force a fresh codex CLI detection scan for the Settings auto-detect
/// button. Runs on the blocking pool because it probes each candidate
/// with `codex --version`, which can take a second or two per path and
/// would otherwise stall the UI thread.
#[tauri::command]
pub async fn redetect_codex_cli_path() -> Result<CodexCliRedetectResult, CommandError> {
    tauri::async_runtime::spawn_blocking(|| {
        let codex_home = platform_runtime::paths::get_codex_home();
        crate::shared::codex_cli_path::redetect_codex_cli_path(
            platform_runtime::codex_cli_resolver(),
            &codex_home,
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "CODEX_CLI_REDETECT_FAILED",
            format!("Redetect task failed: {error}"),
        )
    })
}

#[tauri::command]
pub fn cancel_codex_login() -> Result<bool, CommandError> {
    Ok(crate::shared::login_cancel::cancel_login_in_progress())
}

/// 返回当前生效的代理配置。仅 macOS 真正读取 `proxy_state.json`；
/// Windows / Linux 永远返回默认（直连）状态。
#[tauri::command]
pub fn get_proxy_config() -> Result<ProxyConfig, CommandError> {
    #[cfg(target_os = "macos")]
    {
        let state = crate::shared::proxy::read_proxy_state_cached();
        Ok(ProxyConfig {
            proxy_url: state.proxy_url,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(ProxyConfig::default())
    }
}

/// 保存代理配置。空字符串视为清空（直连）。立即丢弃已缓存的
/// reqwest client，使下一次 plan / quota 刷新走新代理。
///
/// URL 校验：用 `reqwest::Proxy::all` 试构建，失败返回
/// `INVALID_PROXY_URL`。支持 `http://` / `https://` / `socks5://`
/// / `socks5h://`。
#[tauri::command]
pub fn set_proxy_config(payload: SetProxyConfigPayload) -> Result<ProxyConfig, CommandError> {
    #[cfg(target_os = "macos")]
    {
        let trimmed = payload.proxy_url.trim();
        let next_state = if trimmed.is_empty() {
            crate::shared::proxy::ProxyState::default()
        } else {
            // 用 reqwest 校验 URL 是否可被解析为代理。这一步不发起
            // 任何网络请求，只是构造 Proxy 内部结构。
            reqwest::Proxy::all(trimmed).map_err(|error| {
                CommandError::new(
                    "INVALID_PROXY_URL",
                    format!(
                        "Invalid proxy URL {trimmed:?}: {error}. Expected http://, https://, socks5:// or socks5h://."
                    ),
                )
            })?;
            crate::shared::proxy::ProxyState {
                proxy_url: Some(trimmed.to_string()),
            }
        };
        crate::shared::proxy::set_proxy_state(None, next_state.clone());
        // 丢弃旧 client，下一次 build_http_client 按新配置重建。
        crate::shared::chatgpt_api::invalidate_http_client();
        Ok(ProxyConfig {
            proxy_url: next_state.proxy_url,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = payload;
        Err(CommandError::new(
            "PROXY_CONFIG_UNSUPPORTED",
            "Proxy configuration is only supported on macOS in this build.",
        ))
    }
}

/// 清空代理配置（恢复直连），并丢弃缓存的 reqwest client。
#[tauri::command]
pub fn clear_proxy_config() -> Result<ProxyConfig, CommandError> {
    #[cfg(target_os = "macos")]
    {
        crate::shared::proxy::clear_proxy_state(None);
        crate::shared::chatgpt_api::invalidate_http_client();
        Ok(ProxyConfig::default())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(ProxyConfig::default())
    }
}

#[tauri::command]
pub fn open_xiaohongshu(app: tauri::AppHandle) -> Result<ActionResponse, CommandError> {
    let path = platform_runtime::actions::open_xiaohongshu(&app)?;
    Ok(ActionResponse {
        ok: true,
        message: "Opened Xiaohongshu URL.".to_string(),
        path: Some(path),
    })
}
