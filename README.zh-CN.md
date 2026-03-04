# Codex 账号切换工具

这是一个可独立使用的本地脚本项目，用于在多个 Codex ChatGPT 账号之间快速切换。

## 功能

- 多账号目录管理：`~/.codex/account_backup/<账号名>`
- 一条命令切换：`codex switch <账号名>`
- 自动记录当前账号：`.current_profile` + `.active_profile`
- 切换前自动回写：先把 `~/.codex` 当前状态回写到上一个账号目录
- 自动快照：`_autosave/<时间戳>/auth.json`

## 目录结构

```text
Codex_Account_Switch/
├── scripts/
│   ├── codex-switch.sh
│   ├── install.sh
│   ├── uninstall.sh
│   └── smoke-test.sh
├── docs/
├── examples/
└── README.md
```

## 安装

```bash
cd ~/alysechen/Github/Codex_Account_Switch
bash scripts/install.sh
source ~/.zshrc
```

只安装脚本，不改 shell：

```bash
bash scripts/install.sh --no-shell
```

## 使用

```bash
codex switch list
codex switch a
codex switch b
```

## 卸载

```bash
bash scripts/uninstall.sh
# 同时删除安装到 ~/.codex 的脚本
bash scripts/uninstall.sh --remove-script
source ~/.zshrc
```

## 安全提醒

- `auth.json` 包含敏感 token，禁止提交到 Git。
- 建议权限：
  - `chmod 700 ~/.codex/account_backup`
  - `chmod 600 ~/.codex/account_backup/*/auth.json`
