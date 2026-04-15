# Contributing

## Rules

1. Do not commit any real `auth.json` or token-like content.
2. Keep macOS shell scripts POSIX-friendly where possible and keep Windows tooling in the Rust/Tauri runtime.
3. Preserve idempotent behavior for install/uninstall scripts on both platforms.
4. Document verification steps when changing switch logic.

## Development

```bash
bash -n macOS/*.sh
shellcheck macOS/*.sh
npm test
```

## Pull request checklist

- [ ] No secret files committed
- [ ] macOS shell scripts pass syntax check
- [ ] Rust tests pass
- [ ] README updated if behavior changed
