import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { showToast } from "@front-shared/render";

function requiredButton(id: string): HTMLButtonElement {
  const element = document.getElementById(id);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Missing required window control: ${id}`);
  }
  return element;
}

function setMaximizeState(button: HTMLButtonElement, maximized: boolean): void {
  button.classList.toggle("is-maximized", maximized);
  button.setAttribute("aria-label", maximized ? "Restore window" : "Maximize window");
  button.title = maximized ? "Restore" : "Maximize";
}

function reportWindowControlError(action: string, error: unknown): void {
  console.error(`Window control failed: ${action}`, error);
  showToast(`Failed to ${action} window.`, true);
}

export async function setupWindowControls(): Promise<void> {
  const minimizeButton = requiredButton("window-minimize-button");
  const maximizeButton = requiredButton("window-maximize-button");
  const closeButton = requiredButton("window-close-button");
  const titlebarSurface = document.getElementById("window-titlebar-drag-surface");

  if (!isTauri()) {
    for (const button of [minimizeButton, maximizeButton, closeButton]) {
      button.disabled = true;
      button.title = "Available in desktop app";
    }
    return;
  }

  const appWindow = getCurrentWindow();

  const syncMaximizeState = async (): Promise<void> => {
    try {
      setMaximizeState(maximizeButton, await appWindow.isMaximized());
    } catch {
      setMaximizeState(maximizeButton, false);
    }
  };

  const toggleWindowMaximize = async (): Promise<void> => {
    if (await appWindow.isMaximized()) {
      await appWindow.unmaximize();
    } else {
      await appWindow.maximize();
    }
  };

  const runWindowAction = async (
    action: string,
    operation: () => Promise<void>,
    after?: () => Promise<void>,
  ): Promise<void> => {
    try {
      await operation();
      if (after) {
        await after();
      }
    } catch (error) {
      reportWindowControlError(action, error);
    }
  };

  minimizeButton.addEventListener("click", () => {
    void runWindowAction("minimize", () => appWindow.minimize());
  });

  maximizeButton.addEventListener("click", () => {
    void runWindowAction("toggle", toggleWindowMaximize, syncMaximizeState);
  });

  closeButton.addEventListener("click", () => {
    void runWindowAction("close", () => appWindow.close());
  });

  titlebarSurface?.addEventListener("mousedown", (event) => {
    if (event.button !== 0) {
      return;
    }

    const target = event.target;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    if (target.closest(".window-controls")) {
      return;
    }

    if (event.detail === 2) {
      void runWindowAction("toggle", toggleWindowMaximize, syncMaximizeState);
      return;
    }

    void runWindowAction("drag", () => appWindow.startDragging());
  });

  const unlistenResize = await appWindow.onResized(() => {
    void syncMaximizeState();
  });
  globalThis.addEventListener("beforeunload", () => {
    void unlistenResize();
  });

  await syncMaximizeState();
}
