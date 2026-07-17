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

本项目主要执行本地文件操作；读取 plan/quota 时，会通过 HTTPS 向 ChatGPT/OpenAI 官方 endpoint 发送 OAuth access token，仅获取额度元数据，不会发送 prompt 或调用模型。

reset-credit 查询只发送账号范围所需的认证信息，应用只保存可用数量、授予时间和过期时间，不保存卡片 ID 或原始响应体。

Main risk is accidental token exposure through Git, screenshots, shared terminals, insecure backups, or loose Windows ACLs.
