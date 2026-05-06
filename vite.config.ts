import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vite";

const uiTarget = process.env.CODEX_UI_TARGET ?? (process.platform === "darwin" ? "macos" : "windows");

export default defineConfig({
  root: "src-tauri/shared/front",
  define: {
    __CODEX_UI_TARGET__: JSON.stringify(uiTarget),
  },
  resolve: {
    alias: {
      "@front-shared": fileURLToPath(new URL("./src-tauri/shared/front", import.meta.url)),
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
