# Contributing

## Rules

1. Do not commit any real `auth.json` or token-like content.
2. Keep scripts POSIX-friendly where possible (project currently uses bash).
3. Preserve idempotent behavior for install/uninstall scripts.
4. Document verification steps when changing switch logic.

## Development

```bash
bash -n scripts/*.sh
shellcheck scripts/*.sh
```

## Pull request checklist

- [ ] No secret files committed
- [ ] Shell scripts pass syntax check
- [ ] README updated if behavior changed
