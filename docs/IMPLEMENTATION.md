# Implementation Notes

## Data locations

- Shared Codex state root: `CODEX_HOME` or `~/.codex`
- Profile backups: `~/.codex/account_backup/<profile>`
- Current profile pointer: `~/.codex/account_backup/.current_profile`
- Active marker per profile: `~/.codex/account_backup/<profile>/.active_profile`
- Auto snapshots: `~/.codex/account_backup/_autosave/<timestamp>/auth.json`
- Windows runtime files: `%CODEX_HOME%\account_backup\windows\`
- Windows command shim: `%CODEX_HOME%\bin\codex.cmd`

## Installation behavior

- `macOS/install.sh` creates `~/.codex/account_backup` if missing
- `macOS/install.sh` creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- If `~/.codex/auth.json` exists during macOS install, it is copied to `~/.codex/account_backup/a/auth.json`
- `windows/install.py` creates the same profile layout plus `%CODEX_HOME%\account_backup\windows\` and `%CODEX_HOME%\bin\`
- Windows install copies `windows/codex_switch.py` and `windows/common.py` into the runtime directory
- Windows install records `real_codex_path`, `managed_bin_dir`, `app_path`, and `path_added_by_installer` in `install_state.json`

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
- Windows uses Python `shutil` and replaces copied directories so the profile copy matches the current root state.

## macOS wrapper behavior

The macOS installer injects a `codex()` shell wrapper into `~/.zshrc`.

- `codex switch ...` routes to `~/.codex/account_backup/codex-switch.sh`
- Other `codex` commands continue to use the user's existing `codex` CLI in `PATH`

## Windows shim behavior

The Windows installer writes `%CODEX_HOME%\bin\codex.cmd` and appends `%CODEX_HOME%\bin` to the user PATH if needed.

- `codex switch ...` routes to `%CODEX_HOME%\account_backup\windows\codex_switch.py`
- Non-switch `codex` commands are forwarded to the previously resolved real Codex CLI path from `install_state.json`
- `%CODEX_HOME%\account_backup\windows` is reserved runtime state and excluded from profile listing / active-profile scans

## Windows app discovery

Windows app relaunch uses the first existing path from:

1. `%LOCALAPPDATA%\Programs\Codex\Codex.exe`
2. `%ProgramFiles%\Codex\Codex.exe`

If the app was running before the switch but neither path exists, the switch still succeeds and prints a warning.
