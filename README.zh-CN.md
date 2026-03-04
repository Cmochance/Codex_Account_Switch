# Codex 账号切换工具

这是一个可独立使用的本地脚本项目，用于实现MacOS端在多个 Codex 账号之间快速切换。

## 功能

- 多账号目录管理：`~/.codex/account_backup/<账号名>`
- 一条命令切换：`codex switch <账号名>`
- 自动记录当前账号：`.current_profile` + `.active_profile`
- 首次无标记时默认当前账号为 `a`
- 目标账号目录不存在时自动创建
- 切换前自动回写：先把 `~/.codex` 当前状态回写到上一个账号目录
- 统一替换流程：备份 -> 移除 -> 拷贝（新建/空目录跳过拷贝）
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
cd ~/.../Codex_Account_Switch # 进入项目目录
bash scripts/install.sh # 安装脚本到 ~/.codex，并添加命令别名到 shell 配置
source ~/.zshrc # 重新加载 shell 配置以使别名生效
```

## 使用

```bash
codex switch list # 列出所有账号
codex switch a    # 切换到账号 a
codex switch b    # 切换到账号 b
...
```

## 卸载

```bash
bash scripts/uninstall.sh # 同时删除安装到 ~/.codex 的脚本和所有账号数据
bash scripts/uninstall.sh --remove-script # 仅删除安装到 ~/.codex 的脚本，保留账号数据
source ~/.zshrc # 重新加载 shell 配置以移除命令别名
```
