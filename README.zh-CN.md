# Codex 账号切换工具

这是一个将本地实际使用中的 Codex 账号切换流程整理成独立项目的 macOS 脚本工具。

## 功能

- 多账号目录管理：`~/.codex/account_backup/<账号名>`
- 一条命令切换：`codex switch <账号名>`
- 自动记录当前账号：`.current_profile` + `.active_profile`
- 每次切换前，先把 `~/.codex` 当前状态回写到当前激活账号目录
- 自动保存 `auth.json` 快照到 `_autosave/<时间戳>/auth.json`
- 如果 `Codex.app` 正在运行，脚本会先退出 app，再切换账号，最后重新拉起 app

## 重要说明

- 脚本安装时会默认创建名为a~e的四个初始文件夹并将当前登陆账号备份至a中
- 当前脚本不支持自动创建新的账号目录，若目标账号目录不存在，必须由用户提前手动创建后才能切换进入

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
├── scripts/
│   ├── codex-switch.sh
│   ├── install.sh
│   └── uninstall.sh
├── docs/
├── examples/
└── README.md
```

## 安装

```bash
cd ~/.../Codex_Account_Switch
bash scripts/install.sh
source ~/.zshrc
```

安装脚本会：

- 把 `scripts/codex-switch.sh` 复制到 `~/.codex/account_backup/codex-switch.sh`
- 创建 `~/.codex/account_backup/a` 到 `~/.codex/account_backup/d`
- 如果当前存在 `~/.codex/auth.json`，则默认备份到 `~/.codex/account_backup/a/auth.json`
- 在 `~/.zshrc` 中注入 `codex()` wrapper
- 非 `switch` 命令继续走用户自己环境中的 `codex` CLI

## 使用

```bash
codex switch list    # 列出当前所有可用账号
codex switch a    # 切换账号
codex switch b
```

## 卸载

```bash
bash scripts/uninstall.sh
bash scripts/uninstall.sh --remove-script
source ~/.zshrc
```

默认情况下，`uninstall.sh` 只删除 shell 中受管理的 wrapper，不会删除你的账号目录。账号目录如果要清理，需要你手动删除。
