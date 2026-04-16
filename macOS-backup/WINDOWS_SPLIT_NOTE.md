# macOS Split Note

The current Tauri desktop implementation is now Windows-only.

Windows behavior has been simplified to:
- launch Codex through the Microsoft Store shell target
- stop persisting desktop `app_path` into `install_state.json`
- remove local `.exe` discovery fallbacks from the Windows process layer

When a native macOS desktop client is implemented later, it will need its own app-resolution path again. That future macOS process layer should explicitly cover:
- locating `Codex.app` under `/Applications/Codex.app`
- optionally checking `~/Applications/Codex.app`
- activating an already running app through AppleScript
- reopening the app bundle after profile switching

This note exists so the macOS direction stays explicit instead of being inferred from old Windows code paths.
