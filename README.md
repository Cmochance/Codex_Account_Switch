# Codex Account Switch

中文文档: [README.zh-CN.md](./README.zh-CN.md)

This project packages the locally used Codex account switch workflow into standalone tooling for both macOS and Windows.

## Features

- Multi-account profile management under `~/.codex/account_backup/<profile>`
- One-command switch via `codex switch <profile>`
- Active profile tracking with `.current_profile` and `.active_profile`
- Before each switch, current root state is written back to the active profile
- Automatic `auth.json` snapshot in `_autosave/<timestamp>/auth.json`
- If the Codex desktop app is running, the tool closes it before switching and relaunches it after the switch

## Platform support

- macOS: shell scripts under [`macOS/`](./macOS)
- Windows: Python tooling under [`windows/`](./windows)
- Runtime profile data stays under the same `CODEX_HOME`/`~/.codex` layout on both platforms

## Important behavior

- The script does **not** auto-create new profile folders
- The target profile folder must already exist
- The target profile folder must already contain `auth.json`
- You must prepare profiles manually before switching
- The installers create `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- The installers seed only `a/auth.json` from the current `~/.codex/auth.json`
- On Windows, `~/.codex/account_backup/windows` is reserved for runtime files and is not treated as a profile

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
├── macOS/
│   ├── codex-switch.sh
│   ├── install.sh
│   └── uninstall.sh
├── windows/
│   ├── codex_switch.py
│   ├── install.py
│   ├── uninstall.py
│   └── common.py
├── tests/
├── docs/
├── examples/
└── README.md
```

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
- leaves non-switch commands to the user's existing `codex` CLI in `PATH`

## Windows installation

```powershell
cd C:\...\Codex_Account_Switch
python windows\install.py
```

The Windows installer:

- copies `windows/codex_switch.py` and `windows/common.py` to `%CODEX_HOME%\account_backup\windows\`
- creates `%CODEX_HOME%\account_backup\a` through `%CODEX_HOME%\account_backup\d`
- writes the example auth template into any missing `%CODEX_HOME%\account_backup\<profile>\auth.json`
- copies the current `%CODEX_HOME%\auth.json` to `%CODEX_HOME%\account_backup\a\auth.json` when available
- initializes profile `a` as the active profile if a real root auth file exists and no active profile is set yet
- writes `%CODEX_HOME%\bin\codex.cmd`
- ensures `%CODEX_HOME%\bin` is first in the user PATH
- records the real Codex CLI path in `%CODEX_HOME%\account_backup\windows\install_state.json`

Open a new terminal after install so the PATH change is visible.

## Usage

```text
codex switch list
codex switch a
codex switch b
```

## Uninstall

macOS:

```bash
bash macOS/uninstall.sh
bash macOS/uninstall.sh --remove-script
source ~/.zshrc
```

Windows:

```powershell
python windows\uninstall.py
python windows\uninstall.py --remove-script
```

The default uninstall removes only the managed command hook. It does not delete your account folders unless you remove them manually.
