# Codex Account Switch

中文文档: [README.zh-CN.md](./README.zh-CN.md)

This repository packages a local multi-account Codex switching workflow: macOS uses shell scripts to handle `codex switch`, and Windows uses a native desktop app.

## Features

- Automatically save the current active account after login, then switch accounts with one click
- Close the Codex desktop app before switching and relaunch it afterward when needed
- Use the Windows control panel for switching, login, opening folders, adding profiles, and related actions

## Platform support

- macOS: shell scripts under [`macOS/`](./macOS)
- Windows: the `.exe` desktop application from the repository release

## Repository layout

- [`macOS/`](./macOS): macOS switch script, installer, and uninstaller
- [`src/`](./src/): Windows desktop frontend shell
  - [`src/index.html`](./src/index.html): window markup
  - [`src/main.ts`](./src/main.ts): frontend entry
  - [`src/styles.css`](./src/styles.css): desktop UI styling
  - [`src/lib/`](./src/lib/): view-model, rendering, state, Tauri bridge, and actions
- [`src-tauri/`](./src-tauri/): Rust CLI, Tauri commands, installation logic, and windowing
- [`examples/account_backup/demo/`](./examples/account_backup/demo/): placeholder `auth.json` template
- [`docs/`](./docs/): implementation and security notes
- [`windows/`](./windows/): historical Windows note

## macOS installation

```bash
cd ~/.../Codex_Account_Switch
bash macOS/install.sh
source ~/.zshrc
```

The macOS installer:

- copies `macOS/codex-switch.sh` to `~/.codex/account_backup/codex-switch.sh`
- creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- writes the example auth template into any missing `~/.codex/account_backup/<profile>/auth.json`
- copies the current `~/.codex/auth.json` to `~/.codex/account_backup/a/auth.json` when available
- initializes profile `a` as the active profile if a real root auth file exists and no active profile is set yet
- injects a `codex()` wrapper into `~/.zshrc`
- leaves non-switch commands to the existing `codex` CLI in `PATH`

## Windows installation

- Download the latest `.exe` desktop application from this repository's Releases page

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
bash macOS/uninstall.sh
bash macOS/uninstall.sh --remove-script
source ~/.zshrc
```

The default uninstall removes only the managed command hook. It does not delete your account folders unless you remove them manually.
