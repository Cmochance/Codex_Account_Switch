# Codex 账号切换工具

这是一个把本地多账号 Codex 切换流程整理成独立项目的工具仓库：macOS 使用 shell 脚本接管 `codex switch`，Windows 使用原生桌面端。

## 功能

- 登录后自动记录保存当前激活账号，通过切换按钮一键切换
- 如 Codex 桌面端正在运行，切换前自动关闭，切换后自动拉起
- Windows 控制面板支持切换、登录、打开目录、添加账号等操作

## 平台支持

- macOS：使用 [`macOS/`](./macOS) 下的 shell 脚本
- Windows：使用 release 中的 .exe 应用

## 仓库结构

- [`macOS/`](./macOS)：macOS 切换脚本、安装脚本、卸载脚本
- [`src/`](./src/)：Windows 桌面端前端壳
  - [`src/index.html`](./src/index.html)：窗口结构
  - [`src/main.ts`](./src/main.ts)：前端入口
  - [`src/styles.css`](./src/styles.css)：桌面端样式
  - [`src/lib/`](./src/lib/)：状态、渲染、view-model、Tauri 桥接、动作编排
- [`src-tauri/`](./src-tauri/)：Rust CLI、Tauri 命令、安装逻辑、窗口层
- [`examples/account_backup/demo/`](./examples/account_backup/demo/)：占位 `auth.json` 模板
- [`docs/`](./docs/)：实现说明和安全文档
- [`windows/`](./windows/)：历史说明目录

## macOS 安装

```bash
cd ~/.../Codex_Account_Switch
bash macOS/install.sh
source ~/.zshrc
```

macOS 安装脚本会：

- 把 `macOS/codex-switch.sh` 复制到 `~/.codex/account_backup/codex-switch.sh`
- 创建 `~/.codex/account_backup/a` 到 `~/.codex/account_backup/d`
- 为所有缺失的 `~/.codex/account_backup/<profile>/auth.json` 写入示例模板
- 如果当前存在 `~/.codex/auth.json`，则复制到 `~/.codex/account_backup/a/auth.json`
- 如果当前存在真实根目录 auth 且尚未设置激活账号，则初始化 `a` 为当前激活账号
- 在 `~/.zshrc` 中注入 `codex()` wrapper
- 非 `switch` 命令继续走现有 `PATH` 中的 `codex` CLI

## Windows 安装

- 在仓库 release 中下载最新版本 .exe 桌面端应用

## macOS 使用

```打开终端
codex switch list  列出当前账号列表
codex switch a     切换到目录 a 下的账号
codex switch b     切换到目录 b 下的账号
```

如果你要新增默认 `a` 到 `d` 之外的账号目录，需要先手动创建目标目录，并放入有效的 `auth.json`，然后再执行切换。

## macOS 卸载

```bash
bash macOS/uninstall.sh
bash macOS/uninstall.sh --remove-script
source ~/.zshrc
```

默认情况下，卸载脚本只删除受管理的命令接入层，不会删除你的账号目录。账号目录如果要清理，需要你手动删除。
