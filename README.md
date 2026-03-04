# Codex Account Switch

This is a standalone local script project for quickly switching between multiple Codex accounts on macOS.

## Features

- Multi-account profile management: `~/.codex/account_backup/<profile>`
- One-command switch: `codex switch <profile>`
- Automatically tracks the active account: `.current_profile` + `.active_profile`
- First-time fallback: if no marker exists, current account defaults to `a`
- Auto-create target profile folder when it does not exist
- Auto sync before switching: writes current `~/.codex` state back to the previous profile
- Unified replacement flow: backup -> remove -> copy (new/empty profile skips copy)
- Auto snapshot: `_autosave/<timestamp>/auth.json`

## Project Structure

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

## Installation

```bash
cd ~/.../Codex_Account_Switch # Enter the project directory
bash scripts/install.sh # Install the script to ~/.codex and inject shell command wrapper
source ~/.zshrc # Reload shell config so the wrapper takes effect
```

## Usage

```bash
codex switch list # List all accounts
codex switch a    # Switch to account a
codex switch b    # Switch to account b
...
```

## Uninstall

```bash
bash scripts/uninstall.sh # Remove only the shell wrapper block
bash scripts/uninstall.sh --remove-script # Also remove installed script under ~/.codex
source ~/.zshrc # Reload shell config to remove the wrapper
```
