/// <reference types="vite/client" />

declare const __CODEX_UI_TARGET__: "windows" | "macos";
/// Build-time-injected app version. Mirrors `package.json` → propagated
/// via `version-sync.mjs` to `Cargo.toml` + lockfiles, so the front-end
/// settings row stays in lock-step with the binary version automatically.
declare const __CODEX_APP_VERSION__: string;
