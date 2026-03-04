# Contributing

## Rules

1. Do not commit any real `auth.json` or token-like content.
2. Keep scripts POSIX-friendly where possible (project currently uses bash).
3. Preserve idempotent behavior for install/uninstall scripts.
4. Add or update smoke tests when changing switch logic.

## Development

```bash
bash scripts/smoke-test.sh
shellcheck scripts/*.sh
```

## Pull request checklist

- [ ] No secret files committed
- [ ] `scripts/smoke-test.sh` passes
- [ ] README updated if behavior changed
