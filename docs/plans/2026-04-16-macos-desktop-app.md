# macOS Desktop App Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reuse the existing Windows Tauri desktop app architecture to build a native macOS desktop app without forking the business logic into a second platform-specific codepath.

**Architecture:** Keep the Windows frontend shell under `src-tauri/win/front/`, move shared frontend bridge code under `src-tauri/shared/front/`, split the runtime into shared and platform modules, and add a native `src-tauri/mac/runtime/` layer for app lifecycle, shell integration, and installation. Preserve the retained `macOS-backup/` shell scripts as a fallback during migration, then retire them once the native flow is complete.

**Tech Stack:** Tauri 2, Rust, TypeScript, Vite, macOS shell integration, AppleScript / `open -a`, existing Codex CLI

## Current Status

- Completed: Phase 1 shared runtime extraction from `src-tauri/src/windows/` into `src-tauri/src/shared/`
- Completed: Phase 2 platform hook extraction through `src-tauri/src/platform/` and `src-tauri/src/shared/switch_core.rs`
- Completed: Phase 3 native macOS module skeleton in `src-tauri/src/macos/` plus compile-time routing from `lib.rs` and `cli.rs`
- Current macOS runtime coverage includes app open/activate, process stop/reopen, CLI shim install state, runtime CLI forwarding, bootstrap, and switch entry wiring
- Phase 4 completed: macOS window chrome uses a dedicated `src-tauri/mac/front/` shell while shared frontend bridge code lives under `src-tauri/shared/front/`
- Phase 5 completed: `macOS-backup/install.sh` and `macOS-backup/uninstall.sh` now remain as compatibility entrypoints that dispatch to native desktop or legacy shell implementations
- Remaining work is post-stabilization cleanup, including deciding when the legacy shell flow can be downgraded or retired

---

### Task 1: Freeze the cross-platform contract

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/dashboard.rs`
- Modify: `src-tauri/src/commands/actions.rs`
- Modify: `src-tauri/src/commands/switch.rs`
- Reference: `src-tauri/shared/front/tauri.ts`
- Reference: `src-tauri/shared/front/types.ts`

**Step 1: Record the platform-neutral command surface**

Use the existing Tauri commands as the stable contract for both Windows and macOS:

- `get_profiles_snapshot`
- `get_current_live_quota`
- `open_codex`
- `login_current_profile`
- `refresh_profile`
- `rename_profile`
- `update_profile_base_url`
- `open_profile_folder`
- `add_profile`
- `open_contact`
- `open_releases`
- `open_xiaohongshu`
- `switch_profile`

**Step 2: Keep frontend invocation names unchanged**

Do not rename the frontend bridge functions in `src-tauri/shared/front/tauri.ts`. The macOS work should remain transparent to the TypeScript layer.

**Step 3: Replace direct `windows::*` imports at the command boundary**

Introduce a platform adapter module and route command handlers through that adapter instead of importing `crate::windows` directly.

**Step 4: Verify preview mode still works**

Run the frontend in browser preview mode and confirm the mock branch in `src-tauri/shared/front/tauri.ts` still works without Tauri runtime access.

**Step 5: Commit**

Do not commit in this task unless explicitly requested.

### Task 2: Split shared runtime logic out of `windows/`

**Files:**
- Create: `src-tauri/src/shared/mod.rs`
- Create: `src-tauri/src/shared/paths.rs`
- Create: `src-tauri/src/shared/fs_ops.rs`
- Create: `src-tauri/src/shared/metadata.rs`
- Create: `src-tauri/src/shared/profiles.rs`
- Create: `src-tauri/src/shared/profiles_index.rs`
- Create: `src-tauri/src/shared/session_files.rs`
- Create: `src-tauri/src/shared/session_usage.rs`
- Create: `src-tauri/src/shared/config.rs`
- Modify: `src-tauri/src/windows/mod.rs`
- Modify: `src-tauri/src/windows/bootstrap.rs`
- Modify: `src-tauri/src/windows/profile_actions.rs`
- Modify: `src-tauri/src/windows/refresh_runtime.rs`
- Modify: `src-tauri/src/windows/switch.rs`

**Step 1: Move pure filesystem and metadata logic first**

Start with modules that do not depend on Windows process control:

- `paths.rs`
- `fs_ops.rs`
- `metadata.rs`
- `profiles.rs`
- `profiles_index.rs`
- `session_files.rs`
- `session_usage.rs`
- `config.rs`

**Step 2: Rename only when it improves semantics**

Keep `paths.rs` if you want minimum churn. Rename to `state_paths.rs` only if the refactor already touches every import site.

**Step 3: Re-export shared modules temporarily**

Have `windows/mod.rs` re-export shared modules during transition so Windows behavior does not change while imports are being moved.

**Step 4: Keep Windows-only code out of `shared/`**

Do not move:

- installer PATH mutation
- Windows Store app discovery
- `tasklist` / `taskkill`
- `explorer.exe`
- PowerShell-based Windows registry or AppX logic

**Step 5: Run Rust tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: Existing Rust tests pass with only import-path updates.

### Task 3: Extract a platform hook layer for lifecycle control

**Files:**
- Create: `src-tauri/src/platform/mod.rs`
- Create: `src-tauri/src/platform/hooks.rs`
- Create: `src-tauri/src/shared/switch_core.rs`
- Modify: `src-tauri/src/windows/process.rs`
- Modify: `src-tauri/src/windows/switch.rs`
- Modify: `src-tauri/src/windowing.rs`

**Step 1: Define the platform hook trait**

Create a trait with the lifecycle operations the shared switch flow actually needs:

```rust
pub trait PlatformHooks {
    fn open_or_activate_codex_app(&self) -> AppResult<String>;
    fn quit_codex_app_if_running(&self) -> AppResult<bool>;
    fn reopen_codex_app_if_needed(&self, app_was_running: bool) -> Vec<String>;
    fn run_codex_login(&self, codex_home: &Path) -> AppResult<()>;
    fn run_codex_auth_refresh(&self, cli_codex_home: &Path, runtime_codex_home: &Path) -> AppResult<()>;
    fn sync_on_window_close(&self) -> AppResult<()>;
}
```

**Step 2: Move switch orchestration into `shared/switch_core.rs`**

The shared switch flow should own:

1. profile validation
2. lock acquisition
3. backup of current root state
4. auth autosave
5. target overlay
6. `config.toml` base URL sync
7. active marker update
8. profile index rebuild
9. platform reopen call

**Step 3: Keep `windows/process.rs` focused on Windows behavior**

Windows should only implement the hook methods and discovery details.

**Step 4: Remove `crate::windows::*` from `windowing.rs`**

Window-close persistence should also go through the platform/shared layer.

**Step 5: Re-run focused tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml switch`

Expected: Switch behavior remains unchanged on Windows after the refactor.

### Task 4: Add a native macOS platform module

**Files:**
- Create: `src-tauri/src/macos/mod.rs`
- Create: `src-tauri/src/macos/process.rs`
- Create: `src-tauri/src/macos/install.rs`
- Create: `src-tauri/src/macos/cli_shim.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/cli.rs`

**Step 1: Implement macOS app discovery**

Support this order:

1. `/Applications/Codex.app`
2. `~/Applications/Codex.app`
3. `open -Ra Codex`

**Step 2: Implement macOS open / activate behavior**

Use:

- `open -a /Applications/Codex.app`
- fallback `open -a Codex`
- AppleScript activation when already running if needed

**Step 3: Implement macOS process stop / restart**

Match the current shell script behavior:

- `pgrep -x Codex`
- `pkill -TERM -x Codex`
- retry loop
- `pkill -KILL -x Codex` fallback

**Step 4: Implement macOS CLI forwarding**

Reuse the existing real-Codex discovery pattern, but write a macOS shim instead of `codex.cmd`.

**Step 5: Wire platform selection with `cfg(target_os = "...")`**

The crate root should select `windows` or `macos` through compile-time platform modules, not runtime branching spread across files.

### Task 5: Replace shell-only macOS install flow with native desktop install flow

**Files:**
- Modify: `macOS-backup/install.sh`
- Modify: `macOS-backup/uninstall.sh`
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `docs/IMPLEMENTATION.md`
- Modify: `src-tauri/src/macos/install.rs`

**Step 1: Decide migration mode**

Support both during transition:

- `legacy shell`: current `macOS-backup/codex-switch.sh`
- `desktop app`: native Tauri macOS build

**Step 2: Make `macOS-backup/install.sh` a compatibility bootstrap**

During migration, let the shell installer either:

- install the existing wrapper for fallback mode, or
- delegate to the native runtime installer once the macOS app is ready

**Step 3: Standardize data layout**

macOS native app must keep using:

- `~/.codex/account_backup/<profile>`
- `.current_profile`
- `.active_profile`
- `_autosave`
- `profile.json`

Do not introduce a second macOS-only storage root.

**Step 4: Document the migration boundary**

Explicitly say which features still rely on shell fallback, and when `macOS-backup/codex-switch.sh` can be considered legacy.

**Step 5: Manual verification**

Verify on macOS:

1. first-run bootstrap creates `a`-`d`
2. current `auth.json` seeds profile `a`
3. switching closes and reopens Codex.app
4. login writes refreshed `auth.json`
5. add/rename/open-folder/base-url all work

### Task 6: Adapt the desktop shell for macOS window conventions

**Files:**
- Modify: `src-tauri/mac/front/index.html`
- Modify: `src-tauri/mac/front/styles.css`
- Modify: `src-tauri/mac/front/lib/window-controls.ts`
- Modify: `src-tauri/tauri.conf.json`

**Step 1: Stop assuming Windows-style custom chrome everywhere**

The current HTML title bar and three right-side controls are Windows-centric.

**Step 2: Add platform-aware window controls**

Use native decorations on macOS if possible. If custom chrome is retained, move control placement and drag behavior behind a platform check.

**Step 3: Update Tauri window config**

Review:

- `decorations`
- `transparent`
- min size
- title-bar behavior

for macOS separately from Windows.

**Step 4: Verify interactions**

Check:

- drag region
- double-click maximize / zoom behavior
- close persistence on macOS
- button affordances not conflicting with native traffic lights

**Step 5: Manual UI QA**

Run: `npm run tauri:dev`

Expected: UI loads on macOS without broken title-bar interactions.

### Task 7: Preserve and extend the test baseline

**Files:**
- Modify: `src-tauri/src/windows/*.rs`
- Modify: `src-tauri/src/shared/*.rs`
- Create: `src-tauri/src/macos/tests.rs` or inline test modules
- Modify: `package.json`
- Modify: `docs/IMPLEMENTATION.md`

**Step 1: Keep existing Windows regression tests green**

Windows remains the baseline while the split happens.

**Step 2: Add macOS-safe unit coverage**

Add tests for:

- app bundle path resolution
- profile switching orchestration using mocked hooks
- profile storage bootstrap
- CLI forwarding path resolution

**Step 3: Avoid hardcoding Windows-only assumptions in shared tests**

Shared tests should use temp directories and injected hooks, not `tasklist`, `powershell`, or `.exe` expectations.

**Step 4: Run the full test command**

Run: `npm test`

Expected: Rust suite passes after platform split.

**Step 5: Update implementation notes**

Document the final split once the code is merged so `docs/IMPLEMENTATION.md` stays authoritative.

## Recommended Target Tree

```text
Codex_Account_Switch/
├── macOS-backup/
│   ├── codex-switch.sh
│   ├── install.sh
│   ├── uninstall.sh
│   └── WINDOWS_SPLIT_NOTE.md
├── src-tauri/
│   ├── win/
│   │   ├── front/
│   │   └── runtime/
│   ├── mac/
│   │   ├── front/
│   │   └── runtime/
│   ├── shared/
│   │   ├── front/
│   │   ├── commands/
│   │   ├── platform/
│   │   └── runtime/
│   └── src/
│       ├── lib.rs
│       └── main.rs
│       │   ├── metadata.rs
│       │   ├── paths.rs
│       │   ├── profiles.rs
│       │   ├── profiles_index.rs
│       │   ├── session_files.rs
│       │   ├── session_usage.rs
│       │   ├── switch_core.rs
│       │   └── mod.rs
│       ├── windows/
│       │   ├── install.rs
│       │   ├── mod.rs
│       │   └── process.rs
│       └── macos/
│           ├── cli_shim.rs
│           ├── install.rs
│           ├── mod.rs
│           └── process.rs
└── docs/
    └── plans/
        └── 2026-04-16-macos-desktop-app.md
```

## Risks To Control

- The biggest structural risk is copying `windows/` into `macos/` instead of extracting `shared/`.
- The biggest product risk is breaking the existing Windows flow while chasing macOS parity.
- The biggest UX risk is shipping macOS with Windows-style title-bar assumptions.
- The biggest migration risk is introducing a second profile storage layout on macOS.
