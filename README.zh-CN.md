# Codex 账号切换工具

这是一个将本地实际使用中的 Codex 账号切换流程整理成独立项目的跨平台工具，目前支持 macOS 和 Windows。

## 功能

- 多账号目录管理：`~/.codex/account_backup/<账号名>`
- 一条命令切换：`codex switch <账号名>`
- 自动记录当前账号：`.current_profile` + `.active_profile`
- 每次切换前，先把 `~/.codex` 当前状态回写到当前激活账号目录
- 自动保存 `auth.json` 快照到 `_autosave/<时间戳>/auth.json`
- 如果 Codex 桌面端正在运行，工具会先退出 app，再切换账号，最后重新拉起 app

## 平台支持

- macOS：使用 [`macOS/`](./macOS) 下的 shell 脚本
- Windows CLI / 安装工具：使用 [`windows/`](./windows) 下的 Python 脚本
- Windows 原生桌面端：使用 [`src/`](./src/) + [`src-tauri/`](./src-tauri/) 的 Tauri 实现
- 两个平台共用同一套 `CODEX_HOME` / `~/.codex` 数据目录协议

## 重要说明

- 安装脚本会默认创建名为 `a` 到 `d` 的四个初始文件夹，并将当前登录账号备份至 `a` 中
- 当前工具不支持自动创建新的账号目录，若目标账号目录不存在，必须由用户提前手动创建后才能切换进入
- Windows 下的 `~/.codex/account_backup/windows` 为运行时目录，不会被当作账号目录

codex账号目录示例：

```text
~/.codex/account_backup/
├── a/
│   └── auth.json
├── b/
│   └── auth.json
├── c/
│   └── auth.json
└── d/
    └── auth.json
```

如果你执行：

```bash
codex switch x
```

但 `~/.codex/account_backup/x/auth.json` 不存在，脚本会直接报错退出，不会帮你自动建目录或自动生成文件。

## 目录结构

```text
Codex_Account_Switch/
├── src/
├── src-tauri/
├── macOS/
│   ├── codex-switch.sh
│   ├── install.sh
│   └── uninstall.sh
├── windows/
│   ├── codex_switch.py
│   ├── install.py
│   ├── uninstall.py
│   └── common.py
├── tests/
├── docs/
├── examples/
└── README.md
```

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
- 如果当前存在 `~/.codex/auth.json`，则默认备份到 `~/.codex/account_backup/a/auth.json`
- 如果当前存在真实根目录 auth 且尚未设置激活账号，则初始化 `a` 为当前激活账号
- 在 `~/.zshrc` 中注入 `codex()` wrapper
- 非 `switch` 命令继续走用户自己环境中的 `codex` CLI

## Windows 安装

```powershell
cd C:\...\Codex_Account_Switch
python windows\install.py
```

Windows 安装脚本会：

- 把 `windows/codex_switch.py` 和 `windows/common.py` 复制到 `%CODEX_HOME%\account_backup\windows\`
- 创建 `%CODEX_HOME%\account_backup\a` 到 `%CODEX_HOME%\account_backup\d`
- 为所有缺失的 `%CODEX_HOME%\account_backup\<profile>\auth.json` 写入示例模板
- 如果当前存在 `%CODEX_HOME%\auth.json`，则默认备份到 `%CODEX_HOME%\account_backup\a\auth.json`
- 如果当前存在真实根目录 auth 且尚未设置激活账号，则初始化 `a` 为当前激活账号
- 生成 `%CODEX_HOME%\bin\codex.cmd`
- 确保 `%CODEX_HOME%\bin` 位于用户 PATH 的最前面
- 将真实 Codex CLI 路径记录到 `%CODEX_HOME%\account_backup\windows\install_state.json`

安装完成后请重新打开终端，使 PATH 更新生效。

## 使用

```text
codex switch list    # 列出当前所有可用账号
codex switch a    # 切换账号
codex switch b
```

## Windows 原生桌面端

当前仓库也包含一个基于 Tauri 的 Windows 原生控制面板：

- 前端：[`src/`](./src/)
- 原生命令和窗口层：[`src-tauri/`](./src-tauri/)

在 Windows 上本地运行：

```powershell
npm install
npm run tauri:dev
```

构建便携版 exe：

```powershell
npm install
npm run tauri:build
```

构建产物：

```text
src-tauri\target\release\codex_switch.exe
```

## 卸载

macOS：

```bash
bash macOS/uninstall.sh
bash macOS/uninstall.sh --remove-script
source ~/.zshrc
```

Windows：

```powershell
python windows\uninstall.py
python windows\uninstall.py --remove-script
```

默认情况下，卸载脚本只删除受管理的命令接入层，不会删除你的账号目录。账号目录如果要清理，需要你手动删除。
