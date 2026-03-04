# Codex Account Switch

中文文档: [README.zh-CN.md](./README.zh-CN.md)

A standalone shell-based utility to switch between multiple Codex ChatGPT account sessions by swapping `auth.json` and profile-managed files under `~/.codex`.

## Why this project

Codex usage quota can run out on one account while you still need to work. This project helps you:

- Keep multiple account backups under `~/.codex/account_backup/<profile>`
- Switch with one command: `codex switch <profile>`
- Preserve real-time login state updates
- Mark the active profile and sync state back before every switch

## Core behavior

When you switch from profile `A` to `B`:

1. Resolve current active profile from:
   - `~/.codex/account_backup/.current_profile`
   - or `.active_profile` marker in each profile folder
2. Backup current root state (`~/.codex`) back into previous profile (`A`)
3. Save an extra auto snapshot to `~/.codex/account_backup/_autosave/<timestamp>/auth.json`
4. Copy target profile (`B`) files into `~/.codex`
5. Set marker:
   - `~/.codex/account_backup/.current_profile`
   - `~/.codex/account_backup/B/.active_profile`

This keeps each account folder continuously updated with the latest login state.

## Project structure

```text
Codex_Account_Switch/
├── scripts/
│   ├── codex-switch.sh      # Main switch logic
│   ├── install.sh           # Install script + zsh wrapper
│   ├── uninstall.sh         # Remove managed wrapper block
│   └── smoke-test.sh        # Local behavior test
├── docs/
│   ├── SECURITY.md
│   └── IMPLEMENTATION.md
├── examples/
│   └── account_backup/demo/auth.json.example
├── .gitignore
├── CHANGELOG.md
├── CONTRIBUTING.md
├── LICENSE
└── README.md
```

## Prerequisites

- macOS or Linux
- `bash`
- `codex` CLI installed
- Multiple backup profiles under `~/.codex/account_backup/`

Example:

```text
~/.codex/account_backup/
├── a/auth.json
├── b/auth.json
├── c/auth.json
└── d/auth.json
```

## Installation

```bash
cd ~/alysechen/Github/Codex_Account_Switch
bash scripts/install.sh
source ~/.zshrc
```

If you only want the script without shell wrapper injection:

```bash
bash scripts/install.sh --no-shell
```

## Usage

```bash
codex switch list
codex switch a
codex switch b
```

Direct script usage:

```bash
~/.codex/account_backup/codex-switch.sh list
~/.codex/account_backup/codex-switch.sh c
```

## Uninstall

```bash
cd ~/alysechen/Github/Codex_Account_Switch
bash scripts/uninstall.sh
# Optional: also remove installed switch script
bash scripts/uninstall.sh --remove-script
source ~/.zshrc
```

## Security notes

- `auth.json` contains sensitive tokens. Do not commit real account data.
- Keep `~/.codex/account_backup` private (`chmod 700` recommended).
- This tool is for personal account management on your own device.

## Local test

```bash
cd ~/alysechen/Github/Codex_Account_Switch
bash scripts/smoke-test.sh
```

## License

MIT License. See [LICENSE](./LICENSE).
