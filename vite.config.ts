import { fileURLToPath, URL } from "node:url";
import { readFileSync } from "node:fs";

import { defineConfig } from "vite";

const uiTarget = process.env.CODEX_UI_TARGET ?? (process.platform === "darwin" ? "macos" : "windows");
const root = uiTarget === "macos" ? "src-tauri/mac/front" : "src-tauri/win/front";

const packageJson = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"),
) as { version: string };
const appVersion = packageJson.version;

export default defineConfig({
  root,
  define: {
    __CODEX_UI_TARGET__: JSON.stringify(uiTarget),
    // Single source of truth for the app version that the front-end can
    // render in the Settings page (and anywhere else the version label
    // shows up). `package.json` → `version-sync.mjs` already drives the
    // Cargo manifest + lockfiles, so injecting from `package.json` here
    // keeps every surface in lock-step without a runtime IPC.
    __CODEX_APP_VERSION__: JSON.stringify(appVersion),
  },
  resolve: {
    alias: {
      "@front-shared": fileURLToPath(new URL("./src-tauri/shared/front", import.meta.url)),
      "@win-front": fileURLToPath(new URL("./src-tauri/win/front", import.meta.url)),
      "@mac-front": fileURLToPath(new URL("./src-tauri/mac/front", import.meta.url)),
    },
  },
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  preview: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "../../../dist/web",
    emptyOutDir: true,
  },
});
