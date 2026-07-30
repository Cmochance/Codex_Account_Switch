//! 用户配置的 HTTP/HTTPS/SOCKS5 代理。
//!
//! 仅在 macOS 上启用：Windows 与 Linux 走 reqwest 默认行为（读环境
//! 变量代理），不做应用层代理配置。schema + IO 是平台无关的中性
//! 契约，按 AGENTS.md 平台隔离原则放在 shared/；reqwest 注入与
//! `apply_proxy_env` 的 `cfg(target_os = "macos")` 限定负责把行为
//! 收窄到 mac。
//!
//! 持久化到 `~/.codex/account_backup/<runtime_dir>/proxy_state.json`
//! （路径见 `paths::get_proxy_state_file`），全局共享，不区分 profile。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 解析 proxy_state.json 的磁盘路径。mac 走 mac 自己的 runtime dir
/// （`account_backup/macos/`），与 install_state.json 同目录；其他
/// 平台走 shared 的默认解析（实际为 no-op，因为代理配置仅 mac 启用）。
fn resolve_proxy_state_file(codex_home: Option<&Path>) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        // mac 的 cli_shim::get_proxy_state_file 需要 &Path（非 Option），
        // 用 get_codex_home() 取默认值。
        let home = codex_home
            .map(Path::to_path_buf)
            .unwrap_or_else(crate::shared::paths::get_codex_home);
        crate::macos::cli_shim::get_proxy_state_file(&home)
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::shared::paths::get_proxy_state_file(codex_home)
    }
}

/// 用户配置的代理。`proxy_url` 为空 / None 时表示直连。
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProxyState {
    /// 形如 `http://127.0.0.1:7890` / `https://host:port` /
    /// `socks5://host:port`。空或 None = 不走代理。
    #[serde(default)]
    pub proxy_url: Option<String>,
}

impl ProxyState {
    /// 规范化后的代理 URL：去首尾空白，空字符串视为 None。
    /// 调用方拿这个值决定是否注入 reqwest / 子进程 env。
    pub fn effective_url(&self) -> Option<&str> {
        self.proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

/// 进程内 ProxyState 缓存，避免每次 HTTP 调用都重读文件。
/// 首次访问时从磁盘加载；set/clear 后直接更新内存副本，使配置
/// 变更立即对后续读可见。
static PROXY_STATE_CACHE: Mutex<Option<ProxyState>> = Mutex::new(None);

/// 读取磁盘上的 proxy_state.json。文件不存在或解析失败时返回
/// 默认（直连）状态，与 `load_install_state` 的容错策略一致 ——
/// 让上层逻辑继续运行而不是把代理配置错误变成阻断性故障。
pub fn load_proxy_state(codex_home: Option<&Path>) -> ProxyState {
    let path = resolve_proxy_state_file(codex_home);
    let raw = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(_) => return ProxyState::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// 写入 proxy_state.json。失败时静默返回（与 `save_install_state`
/// 一致），调用方按 best-effort 处理。
pub fn save_proxy_state(codex_home: Option<&Path>, state: &ProxyState) {
    let path = resolve_proxy_state_file(codex_home);
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let Ok(serialized) = serde_json::to_string_pretty(state) else {
        return;
    };
    let _ = fs::write(path, format!("{serialized}\n"));
}

/// 读取当前生效的代理配置，使用进程内缓存避免重复 IO。
/// 首次调用从磁盘加载并缓存；`set_proxy_state` / `clear_proxy_state`
/// 调用后会刷新缓存。`chatgpt_api::build_http_client` 与
/// `apply_proxy_env` 都从这里取值。
pub fn read_proxy_state_cached() -> ProxyState {
    let mut guard = match PROXY_STATE_CACHE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        *guard = Some(load_proxy_state(None));
    }
    guard.clone().unwrap_or_default()
}

/// 写入新的代理配置并刷新缓存。`set_proxy_config` command 调用。
pub fn set_proxy_state(codex_home: Option<&Path>, state: ProxyState) {
    save_proxy_state(codex_home, &state);
    if let Ok(mut guard) = PROXY_STATE_CACHE.lock() {
        *guard = Some(state);
    }
}

/// 清空代理配置（恢复直连）并刷新缓存。`clear_proxy_config` 调用。
pub fn clear_proxy_state(codex_home: Option<&Path>) {
    set_proxy_state(codex_home, ProxyState::default());
}

/// 把当前代理 env 注入到子进程 Command。仅 macOS 启用 —— Windows
/// / Linux 的代理走 reqwest 默认行为与环境变量，不需要应用层注入。
///
/// 同时设置 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 是为了让
/// codex CLI（app-server / login）和 curl（检查更新）等各类子进程
/// 都能识别：不同工具读的 env 名不同，三管齐下覆盖最广。
#[cfg(target_os = "macos")]
pub fn apply_proxy_env(command: &mut Command) {
    if let Some(url) = read_proxy_state_cached().effective_url() {
        // 大小写各设一遍：curl 的 `http_proxy` 只认小写（历史 CGI 安全
        // 原因），reqwest / 多数工具认大写；两套全设覆盖面最广。
        for key in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            command.env(key, url);
        }
        // 显式排除 loopback：reqwest / curl 并**不**默认排除
        // 127.0.0.1，不设 NO_PROXY 会把子进程对本机地址的请求
        // （如 OAuth 回调自检）也送进代理。
        for key in ["NO_PROXY", "no_proxy"] {
            command.env(key, "localhost,127.0.0.1");
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_proxy_env(_command: &mut Command) {
    // Windows / Linux 不应用层注入代理。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_url_handles_empty_and_whitespace() {
        let direct = ProxyState::default();
        assert_eq!(direct.effective_url(), None);

        let blank = ProxyState {
            proxy_url: Some("   ".to_string()),
        };
        assert_eq!(blank.effective_url(), None);

        let real = ProxyState {
            proxy_url: Some("  http://127.0.0.1:7890  ".to_string()),
        };
        assert_eq!(real.effective_url(), Some("http://127.0.0.1:7890"));
    }

    #[test]
    fn load_missing_file_returns_default() {
        let temp = std::env::temp_dir().join(format!(
            "codex-proxy-test-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert_eq!(load_proxy_state(Some(&temp)), ProxyState::default());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let temp = std::env::temp_dir().join(format!(
            "codex-proxy-test-roundtrip-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state = ProxyState {
            proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
        };
        save_proxy_state(Some(&temp), &state);
        let loaded = load_proxy_state(Some(&temp));
        assert_eq!(loaded, state);
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn set_and_clear_proxy_state_updates_cache() {
        // 用一个临时路径初始化缓存，避免污染其他测试。
        let temp = std::env::temp_dir().join(format!(
            "codex-proxy-test-cache-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // 重置缓存确保本测试自洽。
        if let Ok(mut guard) = PROXY_STATE_CACHE.lock() {
            *guard = None;
        }
        let state = ProxyState {
            proxy_url: Some("http://proxy.local:8080".to_string()),
        };
        set_proxy_state(Some(&temp), state.clone());
        assert_eq!(read_proxy_state_cached(), state);

        clear_proxy_state(Some(&temp));
        assert_eq!(read_proxy_state_cached(), ProxyState::default());

        // 清缓存让后续测试不依赖本次结果。
        if let Ok(mut guard) = PROXY_STATE_CACHE.lock() {
            *guard = None;
        }
        let _ = std::fs::remove_dir_all(&temp);
    }
}
