# Codex 账号切换工具

Codex 账号切换工具是一个本地桌面应用，用来在同一台机器上管理多个 Codex 账号。它通过本地账号备份目录保存不同账号状态，支持切换当前账号、查看账号状态和额度信息，并提供 macOS 与 Windows 原生 Tauri 桌面界面。

`macOS-backup/` 里的 shell 脚本仍然保留，作为兼容旧流程的入口；当前主要方向是原生桌面端。

## 当前功能

- 仪表盘：显示当前账号、账号总数、可用账号和待登录账号数量。
- 账号页：每页 4 张账号卡片，支持切换、登录刷新、重命名、删除或清空、打开账号目录、编辑 Base URL。
- 运行时页：协议转发（Gateway）开关。开启后所有 ChatGPT/OAuth 账号经由本地 [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) sidecar 统一路由，切号过程不再重启 Codex。详见下文「协议转发」章节。
- 设置页：包含语言、主题、端口、开机自启占位、更新地址、配置备份、版本、许可证、检查更新和 GitHub 入口。
- 引导页：展示添加账号、登录、切换账号的基础流程。
- 支持多套浅色和深色主题、中英文界面，以及没有 Tauri API 时的本地预览数据。

部分设置项目前只完成前端界面，后续再接入后端。

## 平台支持

- macOS：原生 Tauri 桌面端，同时保留 `macOS-backup/` 下的兼容脚本。
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
- GitHub Release 标签使用完整语义版本号，例如 `1.5.3`。
- 每个补丁版本单独创建一个 Release 标签。
- 不要把补丁版本产物上传到旧的两段式标签下，例如不要继续把 `1.5.x` 上传到 `1.5`。
- macOS 安装包作为 Release 资产发布，不提交到 Git。

常用命令：

```bash
npm run version:sync
npm run version:set -- 1.5.4
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

## 账号数据

账号数据只保存在本机。导出的配置或账号备份可能包含 API Key 或认证数据，只应保存在可信设备上。
