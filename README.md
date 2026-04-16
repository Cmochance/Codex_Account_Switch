# Codex Account Switch

中文文档: [README.zh-CN.md](./README.zh-CN.md)

This repository packages a local multi-account Codex switching workflow: macOS now keeps the retained command-line flow under `macOS-backup/`, with `macOS-backup/install.sh` acting as a compatibility entry that prefers the native desktop runtime when available and falls back to the legacy shell flow, while Windows uses a native desktop app.

## Features

- Automatically save the current active account after login, then switch accounts with one click
- Close the Codex desktop app before switching and relaunch it afterward when needed
- Use the Windows control panel for switching, login, opening folders, adding profiles, and related actions

## Platform support

- macOS: compatibility shell scripts under [`macOS-backup/`](./macOS-backup)
- Windows: the `.exe` desktop application from the repository release

## Repository layout

- [`macOS-backup/`](./macOS-backup): retained macOS compatibility installer, native desktop bridge scripts, and legacy shell fallback
- [`src-tauri/`](./src-tauri/): Rust/Tauri runtime root
  - [`src-tauri/win/front/`](./src-tauri/win/front/): Windows desktop frontend shell
  - [`src-tauri/mac/front/`](./src-tauri/mac/front/): macOS desktop frontend shell
  - [`src-tauri/shared/front/`](./src-tauri/shared/front/): shared frontend bridge modules and font assets
  - [`src-tauri/win/runtime/`](./src-tauri/win/runtime/): Windows-only runtime code
  - [`src-tauri/mac/runtime/`](./src-tauri/mac/runtime/): macOS-only runtime code
  - [`src-tauri/shared/runtime/`](./src-tauri/shared/runtime/): shared CLI, models, errors, and runtime logic
  - [`src-tauri/shared/platform/`](./src-tauri/shared/platform/): cross-platform lifecycle hook layer
  - [`src-tauri/shared/commands/`](./src-tauri/shared/commands/): shared Tauri command handlers
  - [`src-tauri/src/`](./src-tauri/src/): crate entrypoints kept for Cargo/Tauri conventions
- [`examples/account_backup/demo/`](./examples/account_backup/demo/): placeholder `auth.json` template
- [`docs/`](./docs/): implementation and security notes
- [`windows/`](./windows/): historical Windows note

## macOS installation

```bash
cd ~/.../Codex_Account_Switch
bash macOS-backup/install.sh
source ~/.zshrc
```

The compatibility installer supports three modes:

- `auto` (default): prefer the native desktop installer when a macOS runtime binary is available, otherwise fall back to the legacy shell installer
- `desktop`: require the native desktop installer path and fail if it cannot be found
- `legacy`: force the original shell-based `codex-switch.sh` flow

Examples:

```bash
bash macOS-backup/install.sh --mode auto
bash macOS-backup/install.sh --mode desktop
bash macOS-backup/install.sh --mode legacy
```

In desktop mode, the installer:

- delegates to the native `codex_switch` installer
- keeps the real runtime under `~/.codex/account_backup/macos/`
- writes the managed `~/.codex/bin/codex` shim through the native runtime
- injects a PATH hook into `~/.zshrc` so the managed shim is available in the shell

In legacy mode, the installer keeps the original shell behavior:

- copies `macOS-backup/codex-switch.sh` to `~/.codex/account_backup/codex-switch.sh`
- creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- writes the example auth template into any missing `~/.codex/account_backup/<profile>/auth.json`
- copies the current `~/.codex/auth.json` to `~/.codex/account_backup/a/auth.json` when available
- initializes profile `a` as the active profile if a real root auth file exists and no active profile is set yet
- injects a `codex()` wrapper into `~/.zshrc`

## Windows installation

- Download the latest `.exe` desktop application from this repository's Releases page

## Local macOS packaging

To generate a drag-install `.dmg` locally:

```bash
npm run tauri:build:macos-dmg
```

The build outputs land at:

- `src-tauri/target/release/bundle/macos/codex_switch.app`
- `src-tauri/target/release/bundle/macos/codex_switch_*.dmg`

## macOS usage

```text
Open Terminal
codex switch list   List available accounts
codex switch a      Switch to the account under folder a
codex switch b      Switch to the account under folder b
```

If you want to add profiles beyond the default `a` through `d`, create the target folder manually first, put a valid `auth.json` inside it, and then run the switch command.

## macOS uninstall

```bash
bash macOS-backup/uninstall.sh
bash macOS-backup/uninstall.sh --remove-script
source ~/.zshrc
```

The compatibility uninstaller also supports `--mode auto|desktop|legacy`.

- In desktop mode it removes the native shell hook first, then delegates to the native runtime uninstaller.
- In legacy mode it removes the original shell wrapper block.
- The default uninstall removes only managed command hooks. It does not delete your account folders unless you remove them manually.
