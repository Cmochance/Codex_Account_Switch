# Repository AGENTS

## Platform Isolation

- Windows and macOS should be developed independently whenever practical.
- Do not extract or expand shared modules for platform-specific behavior unless the shared code is strictly necessary for identical cross-platform contracts, on-disk formats, or version/build plumbing.
- When syncing a feature from one platform to the other, prefer implementing or adapting it under `src-tauri/win/**` or `src-tauri/mac/**` first. Use `src-tauri/shared/**` only for unavoidable neutral contracts.
