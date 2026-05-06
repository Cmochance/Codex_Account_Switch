# Changelog

## 1.6.0 - Unreleased

- 整合 macOS 与 Windows 前端：删除 `src-tauri/{mac,win}/front/**`，统一到 `src-tauri/shared/front/**`，平台差异通过 `__CODEX_UI_TARGET__` 注入。
- 重构 `actions.ts`（595 → 71 行）拆成 `actions/{core,handlers,dialogs,gateway}.ts` 四个职责模块。
- 新增「协议转发（Gateway）」：可选启用本地 [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) sidecar，把所有 ChatGPT/OAuth 账号挂在同一个本地端点后面，切号过程不再 quit/reopen Codex。
- Gateway 启用瞬间备份 `~/.codex/config.toml` 中的 `openai_base_url`，关闭/重置时优先还原；首次启用时按现有 `openai_base_url` 的端口推荐 Gateway 端口，避免与既有本地代理冲突。
- Gateway 启用状态下，所有账号修改命令（登录 / 刷新 / 重命名 / 删除 / 清空 / 添加 / 切换）会自动同步 sidecar 的 auth 目录；切号路径会跳过 quit/reopen Codex 生命周期。
- `~/.codex/config.toml` 的 `openai_base_url` 写入改用显式 TOML basic-string 转义，消除 JSON 与 TOML 转义差异隐患。
- 打包：通过 Tauri `externalBin` 自动按 Rust 目标三元组挑 sidecar 二进制；`tauri:build*` 各脚本前置串入 `build:sidecar[:windows]`，干净克隆也能直接打包。
- `scripts/build-cliproxy.{sh,ps1}` 在 `docs/CLIProxyAPI` 缺失时自动 `git clone`，可通过 `CLIPROXY_REPO_URL` 覆盖；`build-cliproxy.sh` 按传入 triple 推导 GOOS/GOARCH，不再误用 host 架构。
- AGENTS.md 调整为单人维护策略：默认把代码放进 `src-tauri/shared/**`，平台目录只承载真正的平台壳。
- 远端开启 main 分支保护：仅允许通过 PR 合入，禁用 force push 与分支删除，要求线性历史。

## 1.5.3 - 2026-04-27

- Added real update checks against the configured GitHub latest-release JSON endpoint.
- Added automatic new-version prompting when the latest release is newer than the running app.
- Fixed update version parsing so historical two-part tags such as `1.5` compare as `1.5.0` instead of failing.
- Added macOS `.pkg` packaging alongside `.app` and `.dmg`.
- Standardized future release records on full three-part semantic versions.

## 1.5.2 - 2026-04-21

- Windows installer uploaded as `codex_switch_1.5.2_x64-setup.exe`.
- Historical note: this asset was uploaded under the non-standard GitHub Release tag `1.5`.

## 1.5.1 - 2026-04-21

- Windows installer uploaded as `codex_switch_1.5.1_x64-setup.exe`.
- Historical note: this asset was uploaded under the non-standard GitHub Release tag `1.5`.

## 1.5.0 - 2026-04-20

- Added normalized installation and version-control release.
- Uploaded macOS DMG and Windows installer assets.
- Historical note: the GitHub Release tag was created as `1.5`; future release tags should use full semantic versions such as `1.5.3`.

## 1.4.2 - 2026-04-16

- Added the new local release after GitHub tag `1.4.1`.
- Added macOS drag-install DMG packaging and kept the generated `.dmg` beside the `.app`.
- Fixed macOS profile enumeration so the runtime `macos/` directory no longer appears as an empty account card.
- Improved macOS real `codex` CLI discovery for GUI refresh/login flows by falling back to the bundled `Codex.app` CLI when shell resolution is unavailable.
- Removed leftover preview mock account names from the shared frontend bridge.

## 1.4.1 - 2026-04-15

- Resolve merge conflicts.

## 1.4.0 - 2026-04-15

- Resolve merge conflicts.

## 1.3.2 - 2026-04-08

- 更新额度刷新流程和优化逻辑。
- 添加卡片 Base URL。
- 精简代码。

## 1.3.1 - 2026-04-08

- 更新额度刷新流程和优化逻辑。
- 添加卡片 Base URL。
- 精简代码。

## 1.2.4 - 2026-04-08

- 代码清理。

## 1.2.0 - 2026-04-08

- 移除 workflow。

## 1.1.5 - 2026-04-07

- 逻辑完善与界面美化。

## 1.1.4 - 2026-04-07

- 添加自动挂载 release。

## 1.1.0 - 2026-04-07

- Windows 优化完善。

## 0.1.1 - 2026-03-30

- Synced the repository script with the locally used production script.
- Removed the old auto-create profile behavior from the repository project.
- Kept the repository installer generic: non-switch commands continue to use the user's existing CLI.
- Updated README and implementation notes to require pre-created profile folders and `auth.json`.
- Removed local smoke-test instructions and the repository test script from the public project surface.

## 0.1.0 - 2026-03-04

- Initial standalone project extraction.
