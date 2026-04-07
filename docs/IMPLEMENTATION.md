# Implementation Notes

## Data locations

- Shared Codex state root: `CODEX_HOME` or `~/.codex`
- Profile backups: `~/.codex/account_backup/<profile>`
- Current profile pointer: `~/.codex/account_backup/.current_profile`
- Active marker per profile: `~/.codex/account_backup/<profile>/.active_profile`
- Auto snapshots: `~/.codex/account_backup/_autosave/<timestamp>/auth.json`
- Windows runtime files: `%CODEX_HOME%\account_backup\windows\`
- Windows command shim: `%CODEX_HOME%\bin\codex.cmd`

## Runtime shape

- macOS uses shell entrypoints under `macOS/`
- Windows desktop UI and CLI both use Rust/Tauri:
  - frontend under `src/`
    - controller/orchestration in `src/lib/actions.ts`
    - dashboard view-model shaping in `src/lib/dashboard-view-model.ts`
    - native invoke wrapper in `src/lib/tauri.ts`
  - native commands and CLI runtime under `src-tauri/`
- The desktop app does not use a local Python backend or HTTP server at runtime
- `windows/` and `tests/` remain in the repository as legacy compatibility assets while the Rust path is the primary runtime and regression target

## Installation behavior

- `macOS/install.sh` creates `~/.codex/account_backup` if missing
- `macOS/install.sh` creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- macOS install fills any missing `a`-`d` `auth.json` files from `examples/account_backup/demo/auth.json.example`
- If `~/.codex/auth.json` exists during macOS install, it is copied to `~/.codex/account_backup/a/auth.json`
- If a real root auth exists and no active profile is initialized yet, macOS install sets `a` as the active profile
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

The macOS installer injects a `codex()` shell wrapper into `~/.zshrc`.

- `codex switch ...` routes to `~/.codex/account_backup/codex-switch.sh`
- Other `codex` commands continue to use the user's existing `codex` CLI in `PATH`

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
- Python tests under `tests/` are supplemental legacy compatibility coverage and are not the default regression gate
