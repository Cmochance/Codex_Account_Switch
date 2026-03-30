# Implementation Notes

## Data locations

- Root Codex state: `~/.codex`
- Profile backups: `~/.codex/account_backup/<profile>`
- Current profile pointer: `~/.codex/account_backup/.current_profile`
- Active marker per profile: `~/.codex/account_backup/<profile>/.active_profile`
- Auto snapshots: `~/.codex/account_backup/_autosave/<timestamp>/auth.json`

## Installation behavior

- `install.sh` creates `~/.codex/account_backup` if missing
- `install.sh` creates `~/.codex/account_backup/a` through `~/.codex/account_backup/d`
- If `~/.codex/auth.json` exists during install, it is copied to `~/.codex/account_backup/a/auth.json`

## Preconditions for switching

- The target profile directory must already exist
- The target profile directory must already contain `auth.json`

The switch script itself does not create profile folders or generate missing auth files.

## Switching algorithm

1. Validate that the target profile directory exists.
2. Validate that the target profile contains `auth.json`.
3. If `Codex.app` is running, terminate it before switching.
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
- `rsync` is preferred when available; otherwise `cp -R` is used.

## Shell wrapper behavior

The installer injects a `codex()` shell wrapper into `~/.zshrc`.

- `codex switch ...` routes to `~/.codex/account_backup/codex-switch.sh`
- Other `codex` commands continue to use the user's existing `codex` CLI in `PATH`
