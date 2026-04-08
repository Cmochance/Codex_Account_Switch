# macOS 适配架构方案 B

## 目标

在保留当前 `src/` + `src-tauri/` 为主产品结构的前提下，为后续 macOS 原生适配预留清晰的平台分层。

本方案不建议把当前主代码迁移到项目根目录的 `windows/` 文件夹。原因是：

- 当前 Windows 主实现已经稳定收敛在 `src-tauri/src/windows/`
- `src-tauri` 已同时承载桌面 UI 入口和 CLI 入口，属于真正的产品运行时
- 根目录 `windows/` 现在更适合作为遗留说明位，而不是继续演化成主实现目录
- 未来如果引入 macOS 原生实现，最自然的落点是 `src-tauri/src/macos/`，而不是顶层 `macOS/`

## 总体原则

1. 保留 `src/` 作为跨平台前端 UI 层，不放平台特定逻辑。
2. 保留 `src-tauri/` 作为桌面端和 CLI 的统一运行时。
3. 把平台无关逻辑从 `src-tauri/src/windows/` 抽到 `src-tauri/src/shared/`。
4. Windows 和 macOS 只保留各自的安装、进程控制、应用启动、命令 shim 等平台差异逻辑。
5. 根目录 `macOS/` 只保留脚本兼容实现和迁移期工具，不承载未来 Rust 原生主实现。
6. 根目录 `windows/` 不再承载运行时代码，可以保留说明文档，后续视情况删除。

## 推荐目标结构

```text
Codex_Account_Switch/
├── src/
│   ├── main.ts
│   ├── styles.css
│   └── lib/
│       ├── actions.ts
│       ├── dashboard-view-model.ts
│       ├── i18n.ts
│       ├── render.ts
│       ├── state.ts
│       ├── tauri.ts
│       └── types.ts
├── src-tauri/
│   └── src/
│       ├── cli.rs
│       ├── errors.rs
│       ├── lib.rs
│       ├── main.rs
│       ├── models.rs
│       ├── windowing.rs
│       ├── commands/
│       │   ├── actions.rs
│       │   ├── dashboard.rs
│       │   ├── mod.rs
│       │   └── switch.rs
│       ├── shared/
│       │   ├── fs_ops.rs
│       │   ├── metadata.rs
│       │   ├── profiles.rs
│       │   ├── state_paths.rs
│       │   ├── switch_core.rs
│       │   ├── profiles_index.rs
│       │   ├── session_usage.rs
│       │   └── mod.rs
│       ├── windows/
│       │   ├── install.rs
│       │   ├── process.rs
│       │   ├── shell.rs
│       │   └── mod.rs
│       └── macos/
│           ├── install.rs
│           ├── process.rs
│           ├── shell.rs
│           └── mod.rs
├── macOS/
│   ├── codex-switch.sh
│   ├── install.sh
│   ├── uninstall.sh
│   └── WINDOWS_SPLIT_NOTE.md
├── windows/
│   └── README.md
└── docs/
```

## 各层职责

### `src/`

前端界面层，保持跨平台。

- 负责 UI 渲染、交互事件、状态展示
- 通过 Tauri invoke 调用原生命令
- 不直接感知 Windows 或 macOS 的路径和进程差异

### `src-tauri/src/commands/`

Tauri 命令入口层。

- 对前端暴露稳定命令接口
- 只做参数接收、错误映射、异步边界处理
- 不直接承载复杂业务逻辑

### `src-tauri/src/shared/`

平台无关核心逻辑层。

- `fs_ops.rs`
  - 目录覆盖
  - 备份目录同步
  - autosave 写入
  - marker 写入
- `metadata.rs`
  - `auth.json` / `profile.json` 元数据解析
  - 账号标签、套餐、订阅信息读取
- `profiles.rs`
  - 当前 profile 解析
  - profile 展示标题拼装
  - 到期天数计算
- `state_paths.rs`
  - `CODEX_HOME`
  - `account_backup`
  - `.current_profile`
  - `profile.json`
  - `_autosave`
  - 这类跨平台一致的数据路径规则
- `switch_core.rs`
  - 切换流程编排
  - 备份当前状态
  - 覆盖目标 profile
  - 更新 marker
  - 只通过 trait 或回调调用平台动作
- `profiles_index.rs`
  - profile 列表快照
  - quota 卡片数据组装
- `session_usage.rs`
  - 本地 session/quota 解析

### `src-tauri/src/windows/`

Windows 平台差异层。

- `install.rs`
  - 安装
  - 卸载
  - PATH shim
  - `codex.cmd`
  - runtime CLI 写入
- `process.rs`
  - Windows Store shell target
  - `tasklist` / `taskkill`
  - `where codex`
  - `explorer.exe`
- `shell.rs`
  - Windows 平台的 shell / 启动包装逻辑
  - 可以承接当前 `process.rs` 中和 CLI/启动有关但不属于公共逻辑的部分

### `src-tauri/src/macos/`

未来 macOS 原生差异层。

- `install.rs`
  - 安装到 `~/.codex/account_backup`
  - wrapper 注入
  - 可能的 LaunchServices/路径准备
- `process.rs`
  - `Codex.app` 发现
  - `open -a`
  - AppleScript 激活
  - 进程退出与重开
- `shell.rs`
  - 与 macOS 命令包装相关的差异逻辑

### 根目录 `macOS/`

兼容期脚本层。

- 保留现有 shell 安装、卸载、切换实现
- 作为迁移期 fallback
- 未来如果 Rust 原生 macOS 功能完善，可考虑降级为 legacy 目录

### 根目录 `windows/`

遗留占位层。

- 当前不建议继续放任何运行时代码
- 可暂时保留 `README.md`
- 后续确认不再需要后可以删除整个目录

## 现有文件迁移建议

### 保持不动

- `src/lib/actions.ts`
- `src/lib/dashboard-view-model.ts`
- `src/lib/render.ts`
- `src/lib/tauri.ts`
- `src/lib/state.ts`
- `src/lib/types.ts`
- `src-tauri/src/commands/*`
- `src-tauri/src/errors.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/windowing.rs`

### 从 `src-tauri/src/windows/` 迁移到 `src-tauri/src/shared/`

- `fs_ops.rs`
- `metadata.rs`
- `profiles.rs`
- `profiles_index.rs`
- `session_usage.rs`
- `paths.rs`
  - 建议重命名为 `state_paths.rs`

### Windows 专属保留在 `src-tauri/src/windows/`

- `install.rs`
- `process.rs`

### 需要拆分的文件

- `switch.rs`
  - 共享切换流程移入 `shared/switch_core.rs`
  - Windows 进程关闭/重开保留在 `windows/`
- `process.rs`
  - 可拆为 `windows/process.rs` 与 `windows/shell.rs`

## `switch_core` 设计建议

建议定义一个平台适配接口，让切换主流程不依赖具体平台命令。

```rust
trait PlatformHooks {
    fn app_is_running(&self) -> Result<bool, AppError>;
    fn stop_app_if_running(&self) -> Result<bool, AppError>;
    fn reopen_app_if_needed(&self, was_running: bool) -> Result<Vec<String>, AppError>;
    fn ensure_runtime_ready(&self) -> Result<(), AppError>;
}
```

共享切换流程只关心：

1. 校验 profile
2. 备份当前 root 状态
3. autosave
4. 覆盖目标 profile
5. 更新 marker
6. 调用平台 hook 停止/重启 app

这样后续 macOS 只需要实现自己的 hooks，而不需要复制整套切换逻辑。

## `lib.rs` 未来入口形态

当前 `lib.rs` 直接调用 `windows::*`：

- `windows::bootstrap::ensure_backup_initialized`
- `windows::bootstrap::sync_root_state_to_current_profile`
- `windows::profiles_index::load_profiles_index`

后续建议改为：

- 共享初始化调用 `shared::*`
- 平台特有初始化通过 `cfg(target_os = "...")` 分派

示意：

```rust
#[cfg(target_os = "windows")]
mod platform {
    pub use crate::windows::*;
}

#[cfg(target_os = "macos")]
mod platform {
    pub use crate::macos::*;
}
```

这样可以避免后面继续把 macOS 逻辑硬塞进 `windows` 模块。

## 实施阶段

### 第一阶段：目录分层，不改行为

- 新建 `src-tauri/src/shared/`
- 将共享模块拷贝或迁移过去
- `windows::*` 改为引用 `shared::*`
- 保证所有测试通过

目标：

- 仅做目录和 import 调整
- 不改变 CLI 和桌面行为

### 第二阶段：抽出平台 hook

- 新建 `shared/switch_core.rs`
- 把 Windows 中的切换主流程拆成共享核心 + 平台回调
- 保持当前 Windows 行为不变

目标：

- macOS 后续只需要补 hook，而不是复制切换流程

### 第三阶段：建立 `src-tauri/src/macos/`

- 先只放 `process.rs`
- 实现 `Codex.app` 发现与重开
- 不急着做安装器迁移

目标：

- 先建立原生 macOS 差异层骨架

### 第四阶段：决定顶层 `macOS/` 的归宿

有两条路：

- 如果 Rust 原生 macOS 成熟：
  - 顶层 `macOS/` 降级为 legacy
- 如果 shell 方案仍然稳定有效：
  - 顶层 `macOS/` 保留为独立兼容入口
  - `src-tauri/src/macos/` 只服务未来桌面端

## 风险与注意事项

### 不建议现在做的事

- 不要把 `src-tauri/src/windows/*` 直接搬到根目录 `windows/`
- 不要让根目录 `macOS/` 成为未来原生 Rust 主实现目录
- 不要把平台差异散落到 `commands/` 和 `lib.rs` 中

### 当前最大风险

当前很多模块名虽然在 `windows/` 下，但实际内容已经包含平台无关逻辑。如果不先抽 `shared/`，后续做 macOS 时大概率会发生：

- 复制 `windows` 模块成 `macos` 模块
- 共享逻辑在两个平台目录里逐步分叉
- 测试和文档同时漂移

## 验收标准

完成本方案第一轮结构调整后，应满足：

1. `src/` 前端层不包含平台差异代码。
2. `src-tauri/src/shared/` 能承载所有平台无关核心逻辑。
3. `src-tauri/src/windows/` 只剩 Windows 特有能力。
4. 可以无歧义地新增 `src-tauri/src/macos/`。
5. 根目录 `windows/` 不再承载运行时代码。
6. 文档里明确说明：
   - `src-tauri` 是主产品运行时
   - 顶层 `macOS/` 是脚本兼容层
   - 未来原生 macOS 在 `src-tauri/src/macos/`

## 最终建议

如果以 Windows 端为主体，当前总体结构可以保留，不需要把代码迁移到根目录 `windows/`。

正确的下一步不是“迁移到 `windows/`”，而是：

- 保留 `src/` + `src-tauri/` 作为主结构
- 继续把 `src-tauri/src/windows/` 里的共享逻辑抽到 `src-tauri/src/shared/`
- 在时机合适时新增 `src-tauri/src/macos/`
- 顶层 `macOS/` 作为迁移期和脚本兼容目录继续存在

这条路径对当前 Windows 主体最稳，对后续 macOS 适配也最省返工。
