# Implementation Notes

## Data locations

- Root Codex state: `~/.codex`
- Profile backups: `~/.codex/account_backup/<profile>`
- Current profile pointer: `~/.codex/account_backup/.current_profile`
- Active marker per profile: `~/.codex/account_backup/<profile>/.active_profile`
- Auto snapshots: `~/.codex/account_backup/_autosave/<timestamp>/auth.json`

## Switching algorithm

1. Validate target profile folder and `auth.json`.
2. Resolve previously active profile from pointer file or marker files.
3. Before switch, sync current root state back to previous profile.
4. Save timestamped auto snapshot of root `auth.json`.
5. Copy target profile files into root `~/.codex`.
6. Update active markers.

## File sync strategy

- Primary path uses `rsync` when available.
- Fallback uses `cp -R` for portability.
- `.active_profile` and `.DS_Store` are excluded from root copy operations.
