# Security

## Sensitive files

The following files may contain active authentication tokens:

- `~/.codex/auth.json`
- `~/.codex/account_backup/<profile>/auth.json`

Treat them as secrets.

## Recommended protections

1. Keep backup directory permission restricted:
   - `chmod 700 ~/.codex/account_backup`
2. Restrict each `auth.json` file:
   - `chmod 600 ~/.codex/account_backup/*/auth.json`
3. Never push token files to Git repositories.
4. Avoid syncing backup folders to public cloud storage.

## Threat model summary

This project only performs local file operations. It does not transmit tokens over network by design.

Main risk is accidental token exposure through Git, screenshots, shared terminals, or insecure backups.
