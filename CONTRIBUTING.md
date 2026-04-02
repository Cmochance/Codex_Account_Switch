# Contributing

## Rules

1. Do not commit any real `auth.json` or token-like content.
2. Keep macOS shell scripts POSIX-friendly where possible and keep Windows tooling Python-only.
3. Preserve idempotent behavior for install/uninstall scripts on both platforms.
4. Document verification steps when changing switch logic.

## Development

```bash
bash -n macOS/*.sh
shellcheck macOS/*.sh
pytest
```

## Pull request checklist

- [ ] No secret files committed
- [ ] macOS shell scripts pass syntax check
- [ ] Windows Python tests pass
- [ ] README updated if behavior changed
