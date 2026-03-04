# Implementation Notes

## Data locations

- Root Codex state: `~/.codex`
- Profile backups: `~/.codex/account_backup/<profile>`
- Current profile pointer: `~/.codex/account_backup/.current_profile`
- Active marker per profile: `~/.codex/account_backup/<profile>/.active_profile`
- Auto snapshots: `~/.codex/account_backup/_autosave/<timestamp>/auth.json`

## Switching algorithm

1. Ensure target profile folder exists (auto-create if missing).
2. Resolve previously active profile from pointer file or marker files.
   - First-time fallback: if no marker exists but `~/.codex/auth.json` exists, default to profile `a`.
3. Backup current root state (`~/.codex`) back to the previous profile.
4. Save timestamped auto snapshot of root `auth.json`.
5. Replace root managed files using three steps: remove old files, then copy target profile files.
   - For a newly created/empty target profile, copy step is skipped.
6. Update active markers.

## File sync strategy

- Replacement flow is unified as: backup -> remove -> copy.
- Primary path uses `rsync` when available.
- Fallback uses `cp -R` for portability.
- `.active_profile` and `.DS_Store` are excluded from root copy operations.
