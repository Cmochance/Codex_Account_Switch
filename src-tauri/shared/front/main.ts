import "./styles.css";

import { bootstrapDesktopShell } from "@front-shared/bootstrap-app";

document.documentElement.dataset.platform = __CODEX_UI_TARGET__;

async function loadWindowControls(): Promise<() => void | Promise<void>> {
  if (__CODEX_UI_TARGET__ === "windows") {
    const mod = await import("@front-shared/lib/window-controls");
    return mod.setupWindowControls;
  }
  return () => {};
}

void loadWindowControls().then((setup) => {
  bootstrapDesktopShell(setup);
});
