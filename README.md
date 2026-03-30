# Codex Account Switch

中文文档: [README.zh-CN.md](./README.zh-CN.md)

This project packages the locally used Codex account switch workflow into a standalone shell utility for macOS.

## Features

- Multi-account profile management under `~/.codex/account_backup/<profile>`
- One-command switch via `codex switch <profile>`
- Active profile tracking with `.current_profile` and `.active_profile`
- Before each switch, current root state is written back to the active profile
- Automatic `auth.json` snapshot in `_autosave/<timestamp>/auth.json`
- If `Codex.app` is running, the script closes it before switching and relaunches it after the switch

## Important behavior

- The script does **not** auto-create new profile folders
- The target profile folder must already exist
- The target profile folder must already contain `auth.json`
- You must prepare profiles manually before switching
- `install.sh` creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- `install.sh` seeds only `a/auth.json` from the current `~/.codex/auth.json`

Example:

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

If you run `codex switch x` and `~/.codex/account_backup/x/auth.json` is missing, the script exits with an error instead of creating files for you.

## Project Structure

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

## Installation

```bash
cd ~/.../Codex_Account_Switch
bash scripts/install.sh
source ~/.zshrc
```

The installer:

- copies `scripts/codex-switch.sh` to `~/.codex/account_backup/codex-switch.sh`
- creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- copies the current `~/.codex/auth.json` to `~/.codex/account_backup/a/auth.json` when available
- injects a `codex()` wrapper into `~/.zshrc`
- leaves non-switch commands to the user's existing `codex` CLI in `PATH`

## Usage

```bash
codex switch list
codex switch a
codex switch b
```

## Uninstall

```bash
bash scripts/uninstall.sh
bash scripts/uninstall.sh --remove-script
source ~/.zshrc
```

`uninstall.sh` removes only the managed shell wrapper block by default. It does not delete your account folders unless you remove them manually.
