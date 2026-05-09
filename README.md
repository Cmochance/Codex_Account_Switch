# Codex 账号切换工具

Codex 账号切换工具是一个本地桌面应用，用来在同一台机器上管理多个 Codex 账号。它通过本地账号备份目录保存不同账号状态，支持切换当前账号、查看账号状态和额度信息，并提供 macOS 与 Windows 原生 Tauri 桌面界面。

`macOS-backup/` 里的 shell 脚本仍然保留，作为兼容旧流程的入口；当前主要方向是原生桌面端。

## 当前功能

- 仪表盘：显示当前账号、账号总数、可用账号和待登录账号数量。
- 账号页：每页 4 张账号卡片，支持切换、登录刷新、重命名、删除或清空、打开账号目录、编辑 Base URL。登录中点击同一按钮可取消（向 codex login 发送 SIGTERM / taskkill），适用于 OAuth 浏览器关闭后应用卡在等待回调的场景。
- 运行时页：先放置运行时可视化界面，后续再接入更多后端能力。
- 设置页：包含语言、主题、端口、开机自启占位、更新地址、配置备份、版本、许可证、检查更新、GitHub 入口，以及 **Codex CLI 路径**（显示当前路径与来源标签，自动定位失败或路径错误时随时手动指定，写入 `install_state.json` 的 `user_codex_path` 优先级最高）。
- 引导页：展示添加账号、登录、切换账号的基础流程。
- 支持多套浅色和深色主题、中英文界面，以及没有 Tauri API 时的本地预览数据。

部分设置项和运行时条目目前只完成前端界面，后续再接入后端。

## 平台支持

- macOS：原生 Tauri 桌面端，同时保留 `macOS-backup/` 下的兼容脚本。
- Windows：原生 Tauri 桌面端，通过 Release 中的 `.exe` 分发。

平台专属逻辑优先放在 `src-tauri/mac/**` 或 `src-tauri/win/**` 下。`src-tauri/shared/**` 只放共享前端模块、命令契约、运行时模型和中立的跨平台逻辑。

## 仓库结构

- `src-tauri/`：Rust 与 Tauri 应用根目录。
- `src-tauri/mac/front/`：macOS HTML 壳、样式和窗口控制。
- `src-tauri/win/front/`：Windows HTML 壳、样式和窗口控制。
- `src-tauri/shared/front/`：共享前端状态、渲染、动作、主题、国际化和 Tauri 桥接。
- `src-tauri/mac/runtime/`：macOS 运行时集成。
- `src-tauri/win/runtime/`：Windows 运行时集成。
- `src-tauri/shared/runtime/`：共享模型、账号数据处理、更新检查、路径、元数据和切换核心逻辑。
- `src-tauri/shared/commands/`：暴露给前端的 Tauri 命令处理层。
- `macOS-backup/`：旧 shell 流程和桌面桥接的兼容安装、卸载脚本。
- `examples/account_backup/demo/`：示例 `auth.json` 模板。
- `scripts/`：版本同步、macOS 产物布局和 macOS `.pkg` 生成脚本。

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
