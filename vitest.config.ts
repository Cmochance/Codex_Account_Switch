import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vitest/config";

// Standalone config (not merged into vite.config.ts): the Vite `root`
// points at the per-platform front-end directory, so an inherited root
// would never scan the shared front-end tests.
export default defineConfig({
  resolve: {
    alias: {
      "@front-shared": fileURLToPath(new URL("./src-tauri/shared/front", import.meta.url)),
      "@win-front": fileURLToPath(new URL("./src-tauri/win/front", import.meta.url)),
      "@mac-front": fileURLToPath(new URL("./src-tauri/mac/front", import.meta.url)),
    },
  },
  test: {
    include: ["src-tauri/**/front/**/*.test.ts"],
  },
});
