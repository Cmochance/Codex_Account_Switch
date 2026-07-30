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
3. On Windows, restrict access to `%CODEX_HOME%\account_backup` and `%CODEX_HOME%\bin` with NTFS permissions:
   - `icacls %CODEX_HOME%\account_backup /inheritance:r /grant:r %USERNAME%:F`
4. Never push token files to Git repositories.
5. Avoid syncing backup folders to public cloud storage.

## Threat model summary

This project mainly performs local file operations. When reading plan/quota data it sends the OAuth access token over HTTPS to official ChatGPT/OpenAI endpoints and fetches quota metadata only — no prompts are sent and no models are invoked.

The reset-credit lookup sends only the credentials required for the account scope; the app persists just the available count, grant time, and expiry time — never card IDs or raw response bodies.

The optional proxy setting (macOS) stores the configured proxy URL in plain text in `proxy_state.json` under the runtime directory. If your proxy URL embeds credentials (`http://user:pass@host:port`), treat that file as a secret like the token files above.

Main risk is accidental token exposure through Git, screenshots, shared terminals, insecure backups, or loose Windows ACLs.
