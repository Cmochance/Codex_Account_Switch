# Changelog

## 1.6.0 - 2026-05-07

- 整合 macOS 与 Windows 前端：删除 `src-tauri/{mac,win}/front/**`，统一到 `src-tauri/shared/front/**`，平台差异通过 `__CODEX_UI_TARGET__` 注入。
- 重构 `actions.ts`（664 → 174 行），并拆出 `actions/{core,handlers,dialogs,gateway}.ts` 四个职责模块。
- 主页中文乱码修复：`--font-display` 与 `--font-body` 补充 Noto Sans SC / PingFang SC / Microsoft YaHei 等 CJK 兜底，Latin 字形仍优先使用 Cormorant Garamond / IBM Plex Sans。
- 大幅精简 UI 文案：移除 Runtime / Settings 页未对接的占位卡片与字段，删 ~25 个无引用 i18n key；前端产物 -17%。
- 新增「协议转发（Gateway）」：可选启用本地 [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) sidecar，把所有 ChatGPT/OAuth 账号挂在同一个本地端点后面统一路由，切号过程不再 quit/reopen Codex。
- Gateway 启用瞬间备份 `~/.codex/config.toml` 中的 `openai_base_url`，关闭/重置时优先还原；首次启用时按现有 `openai_base_url` 的端口推荐 Gateway 端口，避免与既有本地代理冲突。
- Gateway 启用状态下，所有账号修改命令（登录 / 刷新 / 重命名 / 删除 / 清空 / 添加 / 切换）会自动同步 sidecar 的 auth 目录；切号路径会跳过 quit/reopen Codex 生命周期。
- `~/.codex/config.toml` 的 `openai_base_url` 写入改用显式 TOML basic-string 转义，并切到 atomic write（`auth.json` overlay 也同步原子化），消除 JSON 与 TOML 转义差异隐患以及读侧拿到半写文件的竞态。
- Gateway 增加 TCP probe 区分 `is_enabled`（用户意图）与 `is_active`（实际监听）；sidecar 异常退出后切号路径自动回退到 per-profile sync，VSCode/Codex 扩展不会被卡在指向死端口的 base URL 上。陈旧 `.switch.lock` 自动清理，shutdown 给 sidecar 3s 退出超时。
- 额度刷新新增直连路径：OAuth profile 优先走 `https://chatgpt.com/backend-api/wham/usage`（带 OAuth 401 自动 refresh），失败再回退 `codex exec`。不再每次刷新都消耗用户的实际额度，也摆脱对真实 codex CLI 二进制的依赖。
- 打包：通过 `src-tauri/tauri.sidecar.conf.json` 的 Tauri `externalBin` 自动按 Rust 目标三元组挑 sidecar 二进制；`tauri:build*` 各脚本前置串入 `build:sidecar[:windows]` 并显式加载该覆盖配置，普通 Cargo 校验不再依赖 generated sidecar。
- `scripts/build-cliproxy.{sh,ps1}` 在 `docs/CLIProxyAPI` 缺失时自动 `git clone`，可通过 `CLIPROXY_REPO_URL` 覆盖；`build-cliproxy.sh` 按传入 triple 推导 GOOS/GOARCH，不再误用 host 架构。
- 新增 GitHub Actions 自动构建工作流：PR 上跑 Linux CI 检查；tag `v*` 推送或手动 dispatch 时矩阵打包 macOS arm64 / macOS x86_64 / Windows，并把 `.dmg / .pkg / .exe` 一键挂到 GitHub Release（draft）。
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
