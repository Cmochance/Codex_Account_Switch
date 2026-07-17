# Implementation Notes

## Data locations

- Shared Codex state root: `CODEX_HOME` or `~/.codex`
- Profile backups: `~/.codex/account_backup/<profile>`
- Current profile pointer: `~/.codex/account_backup/.current_profile`
- Active marker per profile: `~/.codex/account_backup/<profile>/.active_profile`
- Auto snapshots: `~/.codex/account_backup/_autosave/<timestamp>/auth.json`
- macOS runtime files: `~/.codex/account_backup/macos/`
- macOS command shim: `~/.codex/bin/codex`
- Windows runtime files: `%CODEX_HOME%\account_backup\windows\`
- Windows command shim: `%CODEX_HOME%\bin\codex.cmd`

## Runtime shape

- macOS still keeps shell entrypoints under `macOS-backup/` as retained compatibility fallback
- Windows desktop UI and CLI both use Rust/Tauri:
  - Windows shell frontend under `src-tauri/win/front/`
  - shared frontend bridge code under `src-tauri/shared/front/`
    - controller/orchestration in `src-tauri/shared/front/actions.ts`
    - dashboard view-model shaping in `src-tauri/shared/front/dashboard-view-model.ts`
    - native invoke wrapper in `src-tauri/shared/front/tauri.ts`
- macOS desktop shell now has a separate frontend root under `src-tauri/mac/front/`
  - `src-tauri/mac/front/index.html` removes the Windows-style custom title bar
  - `src-tauri/mac/front/styles.css` carries macOS-specific shell styling overrides
  - `src-tauri/mac/front/lib/window-controls.ts` is intentionally a no-op because macOS uses native window controls
- native commands and CLI runtime stay under `src-tauri/`, but the source tree is now split by responsibility:
  - `src-tauri/win/front/` holds the Windows desktop shell
  - `src-tauri/mac/front/` holds the macOS desktop shell
  - `src-tauri/shared/front/` holds shared frontend bridge code and font assets
  - `src-tauri/win/runtime/` holds Windows-only bootstrap, install, process, refresh, and windowing code
  - `src-tauri/mac/runtime/` holds macOS-only bootstrap, CLI shim, install, process, switch, and windowing code
  - `src-tauri/shared/runtime/` holds platform-neutral profile, metadata, quota, path, config, CLI, error, and model logic
  - `src-tauri/shared/platform/` defines platform lifecycle hooks so window close, login, auth refresh, and app reopen can be routed without hard-coding those call sites to Windows modules
  - `src-tauri/shared/commands/` holds the Tauri command boundary used by both platforms
  - `src-tauri/src/` is now kept as the crate entry layer only, because Cargo expects `lib.rs` and `main.rs` there by default
- `src-tauri/shared/runtime/switch_core.rs` now owns the profile switch orchestration
- `src-tauri/mac/runtime/windowing.rs` now applies native macOS window decorations instead of reusing the Windows custom chrome assumptions
- `src-tauri/shared/runtime/cli.rs` now resolves to `windows::*` or `macos::*` through compile-time aliases instead of directly binding the CLI to Windows modules
- The desktop app does not use a separate local backend or HTTP server at runtime
- `windows/` remains only as a historical note directory while the Rust path is the primary runtime and regression target

## Quota 与 reset-credit 详情

- `chatgpt_api` 先刷新 `/wham/usage`，再使用最终的 access token 和可选的 `chatgpt-account-id` 请求 `GET /wham/rate-limit-reset-credits`。
- reset-credit 响应会归一化到 `QuotaSummary.rate_limit_reset_credits`；持久化内容只有可用数量、授予时间和过期时间，不保存卡片 ID 或原始响应体。
- 详情接口失败时不影响主 plan/quota 刷新；如果更新的 session 快照没有 reset-credit 字段，已确认的存储详情会继续保留。

## Installation behavior

- `macOS-backup/install.sh` is now a compatibility entrypoint with `auto`, `desktop`, and `legacy` modes
- In `auto` mode, `macOS-backup/install.sh` prefers the native desktop installer and falls back to the legacy shell installer if no native installer binary is available
- `macOS-backup/install-legacy.sh` keeps the original shell-based installation behavior
- `macOS-backup/install-desktop.sh` delegates to the native `codex_switch` installer and then wires `~/.codex/bin` into the shell
- Legacy macOS install creates `~/.codex/account_backup` if missing
- Legacy macOS install creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- Legacy macOS install fills any missing `a`-`d` `auth.json` files from `examples/account_backup/demo/auth.json.example`
- If `~/.codex/auth.json` exists during legacy macOS install, it is copied to `~/.codex/account_backup/a/auth.json`
- If a real root auth exists and no active profile is initialized yet, legacy macOS install sets `a` as the active profile
- `codex_switch.exe install` creates the same profile layout plus `%CODEX_HOME%\account_backup\windows\` and `%CODEX_HOME%\bin\`
- Windows install copies `codex_switch_cli.exe` into the runtime directory
- Windows install fills any missing `a`-`d` `auth.json` files from `examples/account_backup/demo/auth.json.example`
- If a real root `%CODEX_HOME%\auth.json` exists, Windows install overwrites `a/auth.json` with it
- If a real root auth exists and no active profile is initialized yet, Windows install sets `a` as the active profile
- Windows install records `real_codex_path` and `path_added_by_installer` in `install_state.json`

## Desktop app first-run bootstrap

- On desktop app startup, if `account_backup` is missing, the app initializes it automatically
- Bootstrap creates `a` through `d`
- Bootstrap writes placeholder `auth.json` files from `examples/account_backup/demo/auth.json.example`
- If root `auth.json` exists, it is copied into `a/auth.json`
- If root `auth.json` exists, bootstrap marks `a` as the active profile
- Bootstrap also refreshes `%CODEX_HOME%\account_backup\windows\install_state.json`

## Preconditions for switching

- The target profile directory must already exist
- The target profile directory must already contain `auth.json`

The switch script itself does not create profile folders or generate missing auth files.

## Switching algorithm

1. Validate that the target profile directory exists.
2. Validate that the target profile contains `auth.json`.
3. If the Codex desktop app is running, terminate it before switching.
4. Resolve current active profile from `.current_profile` or `.active_profile`.
5. Write current root state from `~/.codex` back into the active profile folder.
6. Save a timestamped snapshot of root `auth.json`.
7. Copy the target profile files into `~/.codex`.
8. Update `.current_profile` and `.active_profile`.
9. If the app was running before the switch, relaunch it.

## File sync strategy

- The profile backup step writes managed files from the root state back into the current profile.
- The root copy step overlays target profile files into `~/.codex`.
- Files absent from the target profile are not automatically removed from the root state.
- `.active_profile` and `.DS_Store` are excluded from copy operations.
- macOS prefers `rsync` when available; otherwise `cp -R` is used.
- Windows uses Rust filesystem operations and replaces copied directories so the profile copy matches the current root state.

## macOS wrapper behavior

- In desktop mode, the macOS compatibility installer injects a PATH hook into `~/.zshrc` so `~/.codex/bin/codex` is resolved before the existing CLI
- In legacy mode, the macOS installer injects a `codex()` shell wrapper into `~/.zshrc`
- Legacy `codex switch ...` routes to `~/.codex/account_backup/codex-switch.sh`
- Legacy non-switch `codex` commands continue to use the user's existing `codex` CLI in `PATH`

## Native macOS runtime behavior

- The native macOS runtime stores its install state in `~/.codex/account_backup/macos/install_state.json`
- The native macOS runtime copies `codex_switch_cli` into `~/.codex/account_backup/macos/`
- The native macOS runtime writes a managed `~/.codex/bin/codex` shim that forwards into the runtime CLI
- Native macOS CLI forwarding skips the managed shim when resolving the real `codex` binary from `PATH`
- Native macOS app activation first probes `/Applications/Codex.app`, then `~/Applications/Codex.app`, and otherwise falls back to `open -a Codex`
- If Codex is already running on macOS, the runtime first tries AppleScript activation before doing a fresh open
- Native macOS app shutdown uses `pgrep -x Codex`, `pkill -TERM -x Codex`, and finally `pkill -KILL -x Codex`
- Native macOS desktop UI now loads from `src-tauri/mac/front/` instead of the Windows shell under `src-tauri/win/front/`
- Native macOS windows restore system decorations and native title bar behavior during startup
- Native macOS bundle packaging is isolated in `src-tauri/tauri.macos.conf.json`, which enables `app` + `dmg` outputs without changing the default Windows/base Tauri config
- `npm run tauri:build:macos-dmg` moves the generated `.dmg` beside the `.app` under `src-tauri/target/release/bundle/macos/` to keep macOS artifacts in one folder
- `macOS-backup/uninstall.sh` mirrors the same compatibility split and chooses native desktop teardown when the desktop runtime is installed

## Windows shim behavior

The Windows installer writes `%CODEX_HOME%\bin\codex.cmd` and ensures `%CODEX_HOME%\bin` is first in the user PATH.

- `codex switch ...` routes to `%CODEX_HOME%\account_backup\windows\codex_switch_cli.exe shim switch ...`
- Non-switch `codex` commands are forwarded to the previously resolved real Codex CLI path from `install_state.json`
- `%CODEX_HOME%\account_backup\windows` is reserved runtime state and excluded from profile listing / active-profile scans

## Windows desktop app actions

- `Switch` writes current root state back to the active profile, snapshots `auth.json`, overlays the target profile into root state, updates active markers, and relaunches Codex if needed
- `Login` runs `codex login` against the current root `CODEX_HOME`, waits for login completion, then writes the refreshed root state back into the active profile
- `Open Codex` activates the Codex desktop app if already running, or launches it if not
- `Add Profiles` creates a new profile directory and writes template `auth.json` plus `profile.json`
- `Contact Us` opens the project GitHub repository

## Windows app discovery

Windows desktop app discovery first prefers the path recorded in `install_state.json`. If that is missing or invalid, it probes common install locations including:

1. `%LOCALAPPDATA%\Programs\Codex\Codex.exe`
2. `%LOCALAPPDATA%\Programs\OpenAI\Codex\Codex.exe`
3. `%LOCALAPPDATA%\Codex\Codex.exe`
4. `%LOCALAPPDATA%\OpenAI\Codex\Codex.exe`
5. `%ProgramFiles%\Codex\Codex.exe`
6. `%ProgramFiles%\OpenAI\Codex\Codex.exe`
7. directories under `%LOCALAPPDATA%\Programs` or `%ProgramFiles` whose names contain `codex`
8. Windows `App Paths\Codex.exe` registry entries

## Validation strategy

- Primary regression baseline is the Rust suite under `src-tauri/`
- Root command: `npm test`
- Equivalent direct command: `cargo test --manifest-path src-tauri/Cargo.toml`
