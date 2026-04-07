# Codex Account Switch

中文文档: [README.zh-CN.md](./README.zh-CN.md)

This project packages the locally used Codex account switch workflow into standalone tooling.

## Features

- Multi-account profile management under `~/.codex/account_backup/<profile>`
- One-command switch via `codex switch <profile>`
- Active profile tracking with `.current_profile` and `.active_profile`
- Before each switch, current root state is written back to the active profile
- Automatic `auth.json` snapshot in `_autosave/<timestamp>/auth.json`
- If the Codex desktop app is running, the tool closes it before switching and relaunches it after the switch

## Platform support

- macOS: shell scripts under [`macOS/`](./macOS)
- Windows native app and CLI under [`src/`](./src/) + [`src-tauri/`](./src-tauri/)
- Runtime profile data stays under the same `CODEX_HOME`/`~/.codex` layout on both platforms

Current desktop scope:

- The Tauri desktop client is Windows-only
- Windows desktop launch is resolved through the Microsoft Store shell target
- macOS desktop behavior is intentionally not implemented in `src-tauri/`; future direction is documented in [`macOS/WINDOWS_SPLIT_NOTE.md`](./macOS/WINDOWS_SPLIT_NOTE.md)

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
├── src/
│   └── lib/
│       ├── actions.ts
│       ├── dashboard-view-model.ts
│       └── tauri.ts
├── src-tauri/
├── windows/
├── macOS/
│   ├── codex-switch.sh
│   ├── install.sh
│   └── uninstall.sh
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
npm install
npm run tauri:build
.\src-tauri\target\release\codex_switch.exe install
```

The Windows installer:

- copies the built Rust CLI to `%CODEX_HOME%\account_backup\windows\codex_switch_cli.exe`
- creates `%CODEX_HOME%\account_backup\a` through `%CODEX_HOME%\account_backup\d`
- writes the example auth template into any missing `%CODEX_HOME%\account_backup\<profile>\auth.json`
- copies the current `%CODEX_HOME%\auth.json` to `%CODEX_HOME%\account_backup\a\auth.json` when available
- initializes profile `a` as the active profile if a real root auth file exists and no active profile is set yet
- writes `%CODEX_HOME%\bin\codex.cmd`
- ensures `%CODEX_HOME%\bin` is first in the user PATH
- records the real Codex CLI path in `%CODEX_HOME%\account_backup\windows\install_state.json` for shim/login command resolution

Open a new terminal after install so the PATH change is visible.

## Usage

```text
codex switch list
codex switch a
codex switch b
```

## Native Windows app

The repo now also contains a native Tauri desktop implementation for the Windows control panel:

- frontend source: [`src/`](./src/)
  - controller/orchestration: [`src/lib/actions.ts`](./src/lib/actions.ts)
  - local dashboard view-model: [`src/lib/dashboard-view-model.ts`](./src/lib/dashboard-view-model.ts)
  - native invoke wrapper: [`src/lib/tauri.ts`](./src/lib/tauri.ts)
- native shell and Rust commands: [`src-tauri/`](./src-tauri/)

Run locally on Windows:

```powershell
npm install
npm run tauri:dev
```

Build the portable executable on Windows:

```powershell
npm install
npm run tauri:build
```

Expected portable artifact:

```text
src-tauri\target\release\codex_switch.exe
```

## Testing

Primary regression baseline:

```powershell
npm test
```

Equivalent explicit Rust command:

```powershell
npm run test:rust
```

Legacy compatibility checks remain available while the historical Python coverage is still kept in the repo. They require `pytest` to be installed in the local Python environment:

```powershell
npm run test:python
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
.\src-tauri\target\release\codex_switch.exe uninstall
.\src-tauri\target\release\codex_switch.exe uninstall --remove-script
```

The default uninstall removes only the managed command hook. It does not delete your account folders unless you remove them manually.
