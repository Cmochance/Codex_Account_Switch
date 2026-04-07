# Legacy Note

Windows Python scripts were removed.

Windows install, uninstall, shim forwarding, and `codex switch` CLI now live in the Rust/Tauri runtime under [`src-tauri/`](../src-tauri/).

Use:

```powershell
.\src-tauri\target\release\codex_switch.exe install
.\src-tauri\target\release\codex_switch.exe uninstall
```
