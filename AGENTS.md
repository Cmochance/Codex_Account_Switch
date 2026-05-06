# Repository AGENTS

## Code Sharing Policy

This repository is now maintained by a single developer. Earlier guidance that
required Windows and macOS to be kept strictly independent is obsolete and has
been removed. The current policy favors sharing.

- Default to placing code under `src-tauri/shared/**` whenever the behavior can
  be expressed once for both platforms. Front-end (`src-tauri/shared/front/**`),
  cross-platform runtime logic, command surface, models, and on-disk formats
  all belong here.
- Keep `src-tauri/mac/**` and `src-tauri/win/**` as thin platform shells. They
  should only contain code that genuinely cannot be shared: OS-specific
  windowing, process integration, and the platform `hooks` implementations
  routed through `src-tauri/shared/platform/`.
- When a feature has divergent behavior on the two platforms, prefer expressing
  the differences via the existing `PlatformHooks` trait (or a new trait) in
  `shared/platform/hooks` rather than forking the implementation across `mac/`
  and `win/`.
- When porting an existing Windows-first feature, lift the common parts into
  `shared/` first, then leave only the platform-specific glue under `win/`.
- Refactors that move code from `mac/` or `win/` into `shared/` are encouraged
  whenever the diff confirms the logic is identical or trivially parameterizable.

## Build Notes

- Windows Vite/Tauri build commands should be run with escalation by default.
  Sandboxed builds in this repository are known to fail with `spawn EPERM`
  while starting `esbuild` from the Vite/Tauri `beforeBuildCommand`.
- The CLIProxyAPI sidecar is built out-of-tree via
  `scripts/build-cliproxy.sh` (macOS/Linux) or `scripts/build-cliproxy.ps1`
  (Windows). The output lands in `src-tauri/binaries/` (gitignored), and Tauri
  bundles it as an external resource at package time.
