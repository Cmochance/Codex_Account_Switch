# Codex 账号切换工具

这是一个把本地多账号 Codex 切换流程整理成独立项目的工具仓库：macOS 现在把保留的命令行流程收口到 `macOS-backup/`，其中 `macOS-backup/install.sh` 作为兼容入口，优先接入原生桌面运行时，找不到原生安装器时再回退到 legacy shell 方案；Windows 使用原生桌面端。

## 功能

- 登录后自动记录保存当前激活账号，通过切换按钮一键切换
- 如 Codex 桌面端正在运行，切换前自动关闭，切换后自动拉起
- Windows 控制面板支持切换、登录、打开目录、添加账号等操作

## 平台支持

- macOS：使用 [`macOS-backup/`](./macOS-backup) 下的兼容 shell 脚本
- Windows：使用 release 中的 .exe 应用

## 仓库结构

- [`macOS-backup/`](./macOS-backup)：保留的 macOS 兼容安装入口、原生桌面桥接脚本、legacy shell 兜底脚本
- [`src-tauri/`](./src-tauri/)：Rust / Tauri 运行时根目录
  - [`src-tauri/win/front/`](./src-tauri/win/front/)：Windows 桌面端前端壳
  - [`src-tauri/mac/front/`](./src-tauri/mac/front/)：macOS 桌面端前端壳
  - [`src-tauri/shared/front/`](./src-tauri/shared/front/)：共享前端桥接模块与字体资源
  - [`src-tauri/win/runtime/`](./src-tauri/win/runtime/)：Windows 专属运行时代码
  - [`src-tauri/mac/runtime/`](./src-tauri/mac/runtime/)：macOS 专属运行时代码
  - [`src-tauri/shared/runtime/`](./src-tauri/shared/runtime/)：共享 CLI、模型、错误与运行时逻辑
  - [`src-tauri/shared/platform/`](./src-tauri/shared/platform/)：跨平台生命周期 hook 层
  - [`src-tauri/shared/commands/`](./src-tauri/shared/commands/)：共享 Tauri 命令处理层
  - [`src-tauri/src/`](./src-tauri/src/)：为 Cargo / Tauri 保留的 crate 入口层
- [`examples/account_backup/demo/`](./examples/account_backup/demo/)：占位 `auth.json` 模板
- [`docs/`](./docs/)：实现说明和安全文档
- [`windows/`](./windows/)：历史说明目录

## macOS 安装

```bash
cd ~/.../Codex_Account_Switch
bash macOS-backup/install.sh
source ~/.zshrc
```

兼容安装入口支持三种模式：

- `auto`：默认模式，优先尝试原生桌面安装器，找不到时回退到 legacy shell 安装
- `desktop`：强制使用原生桌面安装器，找不到就直接报错
- `legacy`：强制使用原来的 shell 安装流程

示例：

```bash
bash macOS-backup/install.sh --mode auto
bash macOS-backup/install.sh --mode desktop
bash macOS-backup/install.sh --mode legacy
```

`desktop` 模式下，安装入口会：

- 委托原生 `codex_switch` 安装器执行安装
- 把原生运行时保存在 `~/.codex/account_backup/macos/`
- 由原生运行时写入受管理的 `~/.codex/bin/codex` shim
- 在 `~/.zshrc` 中注入 PATH 钩子，让 shell 优先使用受管理 shim

`legacy` 模式下，安装入口保留原有 shell 行为：

- 把 `macOS-backup/codex-switch.sh` 复制到 `~/.codex/account_backup/codex-switch.sh`
- 创建 `~/.codex/account_backup/a` 到 `~/.codex/account_backup/d`
- 为所有缺失的 `~/.codex/account_backup/<profile>/auth.json` 写入示例模板
- 如果当前存在 `~/.codex/auth.json`，则复制到 `~/.codex/account_backup/a/auth.json`
- 如果当前存在真实根目录 auth 且尚未设置激活账号，则初始化 `a` 为当前激活账号
- 在 `~/.zshrc` 中注入 `codex()` wrapper

## Windows 安装

- 在仓库 release 中下载最新版本 .exe 桌面端应用

## macOS 本地打包

如果你要生成可拖拽安装的 `.dmg`：

```bash
npm run tauri:build:macos-dmg
```

构建完成后，产物默认位于：

- `src-tauri/target/release/bundle/macos/codex_switch.app`
- `src-tauri/target/release/bundle/macos/codex_switch_*.dmg`

## macOS 使用

```打开终端
codex switch list  列出当前账号列表
codex switch a     切换到目录 a 下的账号
codex switch b     切换到目录 b 下的账号
```

如果你要新增默认 `a` 到 `d` 之外的账号目录，需要先手动创建目标目录，并放入有效的 `auth.json`，然后再执行切换。

## macOS 卸载

```bash
bash macOS-backup/uninstall.sh
bash macOS-backup/uninstall.sh --remove-script
source ~/.zshrc
```

兼容卸载入口同样支持 `--mode auto|desktop|legacy`。

- `desktop` 模式会先清理原生 shell 钩子，再委托原生运行时卸载。
- `legacy` 模式会移除旧的 shell wrapper 块。
- 默认情况下，卸载脚本只删除受管理的命令接入层，不会删除你的账号目录。账号目录如果要清理，需要你手动删除。
