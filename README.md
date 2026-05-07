# Codex 账号切换工具

[![GitHub stars](https://img.shields.io/github/stars/Cmochance/Codex_Account_Switch?style=social)](https://github.com/Cmochance/Codex_Account_Switch/stargazers)
[![License](https://img.shields.io/github/license/Cmochance/Codex_Account_Switch)](LICENSE.txt)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri)](https://v2.tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/Cmochance/Codex_Account_Switch/total?label=downloads)](https://github.com/Cmochance/Codex_Account_Switch/releases)

Codex 账号切换工具是一个面向 **OpenAI Codex CLI** 的本地账号管理桌面应用。它在同一台机器上保存多个 Codex 账号的本地备份，提供 macOS 与 Windows 原生 Tauri 桌面界面，支持一键切换当前账号、查看账号状态与额度，并通过本地 [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) sidecar 实现「免重启切号」的协议转发（Gateway）。

和 `farion1231/cc-switch` 这类偏 Anthropic / Claude Code 的工具不同，本项目专注 OpenAI Codex CLI 的多账号生命周期：登录、刷新、备份、切换、额度查询都在同一个桌面应用里完成；启用 Gateway 后所有 ChatGPT/OAuth 账号挂在同一个本地端点背后，切号过程不再 quit/relaunch Codex 进程。

`macOS-backup/` 里的 shell 脚本仍然保留，作为兼容旧流程的入口；当前主线方向是原生桌面端。

## 项目状态

- 当前版本：**v1.6.0**(Gateway 稳定版，引入 CLIProxyAPI sidecar + 直连 ChatGPT-API 额度刷新)
- 已验证账号类型：ChatGPT (OAuth)、API Key (per-profile `openai_base_url`)
- 平台：macOS arm64 / Windows x86_64 由 GitHub Actions `release.yml` 工作流统一打包，macOS x86_64 因 GitHub 上游 Intel runner 排队不再随版本发布
- 数据位置：账号备份固定在 `~/.codex/account_backup/`，Gateway 状态在 `account_backup/gateway/state.json`
- v1.6.x 链路稳定性改动：Gateway 启用瞬间快照 `openai_base_url` 防误覆写；切号 / 登录 / 刷新 / 重命名 / 删除 / 添加自动 best-effort 同步 sidecar；Switch 路径在 Gateway 开启时跳过 quit/reopen Codex；陈旧 `.switch.lock`（>60s）自动清理

> 如果使用过程中出现问题，欢迎提交 PR 协助作者完善，会及时处理，非常感谢。

### 更新日志

逐版本变更详见 [GitHub Releases](https://github.com/Cmochance/Codex_Account_Switch/releases)。

## 当前功能

- 仪表盘：显示当前账号、账号总数、可用账号和待登录账号数量。
- 账号页：每页 4 张账号卡片，支持切换、登录刷新、重命名、删除或清空、打开账号目录、编辑 Base URL。
- 运行时页：协议转发（Gateway）开关。开启后所有 ChatGPT/OAuth 账号经由本地 CLIProxyAPI sidecar 统一路由，切号过程不再重启 Codex。详见下文「协议转发」章节。
- 设置页：包含语言、主题、端口、开机自启占位、更新地址、配置备份、版本、许可证、检查更新和 GitHub 入口。
- 引导页：展示添加账号、登录、切换账号的基础流程。
- 多套浅色 / 深色主题、中英文界面，以及没有 Tauri API 时的本地预览数据。

部分设置项目前只完成前端界面，后续再接入后端。

## 平台支持

- macOS：原生 Tauri 桌面端（arm64 由 release 链路分发；Intel 用户可本地用 `npm run tauri:build:macos-release` 自行打包），同时保留 `macOS-backup/` 下的兼容脚本。
- Windows：原生 Tauri 桌面端，通过 Release 中的 `.exe` 分发。

代码默认放在 `src-tauri/shared/**`（前端、命令、运行时模型、跨平台逻辑都在这里）。`src-tauri/mac/**` 与 `src-tauri/win/**` 仅承载真正的平台壳代码：窗口装饰、进程接入、平台专属的 `PlatformHooks` 实现。

## 仓库结构

- `src-tauri/`：Rust 与 Tauri 应用根目录。
- `src-tauri/shared/front/`：共享前端 HTML 壳、样式、窗口控件、状态、渲染、动作、主题、国际化和 Tauri 桥接。`__CODEX_UI_TARGET__` 在编译期注入区分 macOS / Windows。
- `src-tauri/shared/runtime/`：共享模型、账号数据处理、更新检查、路径、元数据、切换核心逻辑、协议转发（Gateway）。
- `src-tauri/shared/commands/`：暴露给前端的 Tauri 命令处理层。
- `src-tauri/shared/platform/`：`PlatformHooks` 抽象，被 mac/win 各自的实现注入。
- `src-tauri/mac/runtime/`：macOS 平台壳（窗口、进程、平台 hooks）。
- `src-tauri/win/runtime/`：Windows 平台壳（窗口、进程、平台 hooks）。
- `src-tauri/binaries/`：打包时由 Tauri `externalBin` 拷入安装包的 sidecar 二进制（gitignored；由 `scripts/build-cliproxy.*` 生成）。
- `macOS-backup/`：旧 shell 流程和桌面桥接的兼容安装、卸载脚本。
- `examples/account_backup/demo/`：示例 `auth.json` 模板。
- `scripts/`：版本同步、macOS 产物布局、macOS `.pkg` 生成、CLIProxyAPI sidecar 编译脚本。

生成文件不进仓库。前端网页构建输出放在 `dist/web/`，macOS 桌面端打包产物直接落到 `dist/` 根目录。

## 下载

最新已发布版本在 GitHub Release：

```text
https://github.com/Cmochance/Codex_Account_Switch/releases/latest
```

推荐普通用户直接下载：

- `codex_switch_<版本>_aarch64.dmg` / `.pkg`：macOS Apple Silicon
- `codex_switch_<版本>_x64-setup.exe`：Windows NSIS 安装版

macOS Intel 用户由于上游 macos-13 runner 排队严重，请自行 `git clone` 后用 `npm run tauri:build:macos-release` 本地打包；Apple Developer ID notarize 仍是后续工作。

## 基本用法

1. 启动 Codex 账号切换工具，弹出桌面窗口。
2. 在账号页点击右上角加号，按引导登录或导入一个 Codex 账号。
3. 重复添加多个账号；点击账号卡片即可切换为当前账号，Codex CLI 将使用对应的 `auth.json`。
4. 如需「免重启切号」体验，进入运行时页打开 **Gateway**：本机起一个 CLIProxyAPI sidecar，所有 ChatGPT/OAuth 账号挂在同一个本地端点（默认 `http://127.0.0.1:8317/v1`）背后；之后切号不再重启 Codex 进程。
5. 设置页可调整语言、主题、端口、备份位置等。

## 开发

安装依赖：

```bash
npm install
```

启动前端和 Tauri 开发应用：

```bash
npm run tauri:dev
```

只构建前端：

```bash
npm run build
```

运行 Rust 测试：

```bash
npm run test:rust
```

## 协议转发（Gateway）

Gateway 是「免重启切号」的实现路径。开启后会在本机起一个 [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) sidecar，把所有 ChatGPT/OAuth 账号挂在同一个本地端点（默认 `http://127.0.0.1:8317/v1`）背后统一路由。Codex 一直只跟这个本地端点通话，切号时只换底层 auth 文件，进程不再被 quit/relaunch。

工作机制要点：

- 启用瞬间会读取 `~/.codex/config.toml` 当前的 `openai_base_url` 并存到 `account_backup/gateway/state.json` 的 `external_base_url_backup`；关闭或重置时优先把这个值写回。这样如果你机器上原本就跑着别的本地代理（比如 `:18080`），不会被悄悄覆写。
- 首次启用时会读取已有 `openai_base_url` 中的端口作为 Gateway 推荐端口，与现有部署对齐。
- 启用状态下，所有账号修改（登录 / 刷新 / 重命名 / 删除 / 清空 / 添加 / 切换）都会自动调用 `gateway::refresh_auths_best_effort` 把最新 auth.json 同步给 sidecar，无需手动重启 Gateway。
- 启用状态下的 Switch 路径会跳过 `quit_codex_app_if_running` 与 `reopen_codex_app_if_needed`，Codex 进程不会被打断。
- API Key 类型的账号目前不走 Gateway，仍按原来的 per-profile `openai_base_url` 流程切号。

构建前置：

- 需要 Go 工具链（`go` 在 PATH 里）。
- `scripts/build-cliproxy.sh`（macOS / Linux）和 `scripts/build-cliproxy.ps1`（Windows）会在 `docs/CLIProxyAPI` 缺失时自动 `git clone --depth 1` 上游仓库；可以用 `CLIPROXY_REPO_URL` 环境变量指向自建镜像。
- 产物落到 `src-tauri/binaries/cliproxy-<rust-target-triple>[.exe]`。打包脚本通过 `src-tauri/tauri.sidecar.conf.json` 中的 Tauri `externalBin` 按目标三元组挑选对应的二进制并放进安装包。
- 所有打包用 `npm run tauri:build*` 都已在前置步骤调用 `npm run build:sidecar[:windows]`，干净的克隆 / CI runner 也能直接打包；普通 `cargo check` / `cargo test` 不需要先生成 sidecar。

`docs/CLIProxyAPI` 已在 `.gitignore` 里（整个 `docs/` 目录都不入库），不会污染主仓库历史。

## 桌面端打包

Tauri 2 没有提供 bundle 输出目录配置。macOS 打包脚本会在运行 Tauri 前，把 `src-tauri/target/release/bundle/macos` 和 `src-tauri/target/release/bundle/dmg` 准备成指向 `dist/` 的链接。Tauri 仍然写入它自己的固定 bundle 路径，但实际 `.app` 和 `.dmg` 会直接落到 `dist/`；`.pkg` 生成脚本也直接写入 `dist/`。

本地测试阶段只打包 `.app`：

```bash
npm run tauri:build:macos-app
```

`.app` 写入 `dist/` 后，会删除构建过程中生成的裸 macOS 可执行中间产物。

最终发布阶段打包 `.dmg` 和 `.pkg`：

```bash
npm run tauri:build:macos-release
```

最终发布流程会把 `.app` 作为打包输入，随后从 `dist/` 中移除 `.app`，只把发布用安装包保留在 `dist/` 根目录。`dist/` 根目录里的旧版本 `.dmg` 和 `.pkg` 会移动到 `dist/history/` 下对应的版本命名文件夹中；重复构建当前版本时会替换当前根目录安装包，不再额外备份。

预期本地产物结构：

```text
dist/
  codex_switch_<version>_<arch>.dmg
  codex_switch_<version>_<arch>.pkg
  history/
    v<old-version>/
      ...
  web/
    ...
```

## 版本与发布

- 项目版本保存在 `package.json`，并同步到 Tauri 和 Cargo 元数据。
- GitHub Release 标签使用完整语义版本号，例如 `v1.6.0`。
- 每个补丁版本单独创建一个 Release 标签。
- 不要把补丁版本产物上传到旧的两段式标签下，例如不要继续把 `1.5.x` 上传到 `1.5`。
- macOS 安装包作为 Release 资产发布，不提交到 Git。
- 默认遵循「先发 draft，确认后再转 Latest」的流程：tag push 触发 `.github/workflows/build.yml` → build matrix → `softprops/action-gh-release` 以 `draft: true` 创建草稿；确认后用 `gh release edit vX.Y.Z --draft=false --latest` 转正。

常用命令：

```bash
npm run version:sync
npm run version:set -- 1.6.1
```

## macOS 兼容脚本

通过兼容入口安装：

```bash
bash macOS-backup/install.sh
source ~/.zshrc
```

支持三种模式：

- `auto`：优先尝试原生桌面运行时，找不到时回退到 legacy shell 流程。
- `desktop`：强制使用原生桌面运行时。
- `legacy`：强制使用原来的 shell 版 `codex-switch.sh` 流程。

卸载：

```bash
bash macOS-backup/uninstall.sh
bash macOS-backup/uninstall.sh --remove-script
source ~/.zshrc
```

卸载脚本只移除受管理的命令接入层，不会删除账号备份目录；账号目录需要时手动清理。

## Windows 安装

在仓库 Releases 页面下载最新 Windows `.exe`。

## English Quick Start

Codex Account Switch is a local desktop app for managing multiple **OpenAI Codex CLI** accounts on the same machine. It keeps each account's `auth.json` in a local backup directory and exposes a native Tauri UI on macOS and Windows for one-click switching, login refresh, quota queries, and a "no-restart switching" Gateway powered by a local [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) sidecar.

Unlike `farion1231/cc-switch` and similar Anthropic / Claude Code-oriented tools, this project focuses on the full Codex CLI account lifecycle (login, refresh, backup, switch, quota) in a single desktop app. With the Gateway enabled, all ChatGPT / OAuth profiles sit behind one local endpoint and switching no longer needs to quit/relaunch the Codex process.

### Project status

- Current version: **v1.6.0** (Gateway-stable line, introducing the CLIProxyAPI sidecar and direct ChatGPT-API quota refresh)
- Validated profile types: ChatGPT (OAuth), API Key (per-profile `openai_base_url`)
- Platforms: macOS arm64 and Windows x86_64 are built by the GitHub Actions `release.yml` workflow. macOS x86_64 is no longer shipped per-release because upstream macos-13 runners are heavily backlogged — Intel users should build locally with `npm run tauri:build:macos-release`.
- Data location: account backups live in `~/.codex/account_backup/`; Gateway state is at `account_backup/gateway/state.json`.

### Getting started

1. Download the latest installer from [GitHub Releases](https://github.com/Cmochance/Codex_Account_Switch/releases/latest), or build locally with `npm run tauri:build:macos-release` / `:windows`.
2. Launch the app — a native desktop window appears.
3. On the **Accounts** page, click the top-right `+` and follow the guide to log in or import a Codex account. Repeat for each profile.
4. Click any account card to switch — Codex CLI will use the corresponding `auth.json`.
5. For no-restart switching, open the **Runtime** page and toggle **Gateway** on: the app spawns a local CLIProxyAPI sidecar (default `http://127.0.0.1:8317/v1`) and routes all ChatGPT / OAuth profiles through it. Subsequent switches no longer restart Codex.

### What it does

- Manages multiple Codex CLI accounts via per-profile snapshots under `~/.codex/account_backup/`.
- One-click switch / login refresh / rename / delete / clear / open profile dir / edit base URL on the Accounts page.
- Gateway mode (CLIProxyAPI sidecar) for "no-restart switching" — Codex stays connected to a stable local endpoint while underlying auth files are swapped.
- Snapshots and restores the user's original `openai_base_url` so an existing local proxy is never silently overwritten.
- Best-effort `gateway::refresh_auths_best_effort` after every account mutation keeps the sidecar in sync without manual restart.
- Light / dark themes, Chinese / English UI, and a local preview mode when Tauri APIs are unavailable.

### Security notes

- Account data is stored only on the local machine. Exported configs / backups may contain API keys or auth material — keep them on trusted devices.
- The Gateway sidecar binds to `127.0.0.1` only and never hijacks the system proxy.
- macOS notarization and Windows Authenticode signing are not yet in place; use the published installer SHA hashes (when added) to verify downloads.

## 故障排查

### 切号后 VSCode / Codex 扩展无法连接

绝大多数情况下问题出在 Gateway（协议转发）状态。打开 GUI 的 Runtime 页查看「转发」面板：

- **`Off` 但 VSCode 仍连不上**：检查 `~/.codex/config.toml` 的 `openai_base_url` 是否还指向某个 `127.0.0.1:<端口>`。如果是，说明之前启用 Gateway 时备份的外部 endpoint 没成功还原 — 把这一行手动删掉，然后重启 VSCode。
- **`On` 但状态徽章是红色 / 警告 "未监听"**：sidecar 进程死了（系统 OOM、端口被抢占、二进制不存在等）。点击「重置」让本应用还原 root URL，再点切换让 VSCode 用直连 OpenAI；想恢复转发就重新打开开关。
- **`On` 状态正常但仍报错**：可能是 sidecar 还没读到新切号的 auth.json（≤5s 窗口）。等几秒重试；持续失败查看 `~/.codex/account_backup/gateway/cliproxy.log`。

### 切号失败：`SWITCH_IN_PROGRESS`

旧版 GUI 强退后可能留下 `~/.codex/account_backup/.switch.lock`。1.6.0 起会自动清理超过 60 秒的陈旧锁；如果你在更早的版本碰到这个错误，手动删除该文件即可。

### Gateway 端口冲突

Gateway 默认在首次启用时复用 `openai_base_url` 中的端口。如果该端口被占用：

```bash
lsof -i :8317
```

或在 Windows：

```powershell
netstat -ano | findstr :8317
```

发现占用后，关闭占用进程，或在运行时页面修改 Gateway 端口后重启。

### Windows 提示未知发布者

当前 Windows 构建还没有 Authenticode 代码签名证书，所以 Windows 可能提示未知发布者。Release 页面提供安装包，可用文件哈希（如有）校验下载完整性。

## 账号数据

账号数据只保存在本机。导出的配置或账号备份可能包含 API Key 或认证数据，只应保存在可信设备上。

## 致谢

本项目站在前人的肩膀上：

- **[CC-Switch](https://github.com/farion1231/cc-switch)** 提供了「轻量桌面 + 一键切换 API 提供商」的产品形态启发。
- **[CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)** 提供了 Gateway 模式下的本地多账号统一路由能力 —— 本项目通过 Tauri `externalBin` 把它作为 sidecar 嵌入安装包，是「免重启切号」体验的关键。
- **[Tauri](https://tauri.app/)** 提供了桌面壳的全部基础设施 —— 单二进制打包、native webview、tray、IPC、单实例插件、自定义 URI scheme。
- **[OpenAI Codex CLI](https://github.com/openai/codex)** 是本项目服务的目标 CLI 本身；本项目只是它的多账号管理外壳，所有 Codex CLI 行为以官方实现为准。

本项目专注 OpenAI Codex CLI 多账号管理，不是 OpenAI、Anthropic、CC-Switch 或 `farion1231/cc-switch` 的官方项目，也不复用它们的商标、Logo 或发布身份。

## 许可证

MIT License。完整文本见 [LICENSE.txt](LICENSE.txt)。
