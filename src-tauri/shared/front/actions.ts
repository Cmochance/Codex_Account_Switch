import { resolveInitialLocale } from "@front-shared/i18n";
import { state } from "@front-shared/state";
import { applyTheme, resolveInitialTheme } from "@front-shared/theme";
import {
  applyLocale,
  elements,
  renderShellRoute,
  routeFromLocation,
} from "@front-shared/render";

import {
  refreshAllData,
  refreshCurrentQuota,
  rerenderDashboard,
} from "@front-shared/actions/core";
import {
  handleCheckUpdate,
  handleLoginCurrentProfile,
  handleOpenCodex,
  handleOpenContact,
  handleOpenCurrentFolder,
  handleOpenUpdateRelease,
  handleOpenXiaohongshu,
  setLocale,
  setLocaleFromValue,
  setThemeFromValue,
} from "@front-shared/actions/handlers";
import {
  closeBaseUrlDialog,
  closeDeleteProfileDialog,
  closeRenameProfileDialog,
  handleDeleteProfileAction,
  handleSubmitAddProfile,
  handleSubmitBaseUrl,
  handleSubmitRenameProfile,
  openAddProfileDialog,
} from "@front-shared/actions/dialogs";
import {
  handleApplyGatewaySettings,
  handleRecoverGateway,
  handleToggleGateway,
  loadGatewayStatus,
} from "@front-shared/actions/gateway";

export function bootstrap(): void {
  state.locale = resolveInitialLocale();
  state.theme = resolveInitialTheme();
  state.route = routeFromLocation();
  applyTheme(state.theme);
  applyLocale();
  renderShellRoute();

  window.addEventListener("hashchange", () => {
    state.route = routeFromLocation();
    renderShellRoute();
  });

  elements.previousPageButton.addEventListener("click", () => {
    state.page -= 1;
    rerenderDashboard();
  });
  elements.nextPageButton.addEventListener("click", () => {
    state.page += 1;
    rerenderDashboard();
  });
  elements.openCurrentFolderButton.addEventListener("click", () => {
    void handleOpenCurrentFolder();
  });
  elements.currentLoginButton.addEventListener("click", () => {
    void handleLoginCurrentProfile();
  });
  elements.openCodexButton.addEventListener("click", () => {
    void handleOpenCodex();
  });
  elements.settingsGithubButton.addEventListener("click", () => {
    void handleOpenContact();
  });
  elements.settingsCheckUpdateButton.addEventListener("click", () => {
    void handleCheckUpdate();
  });
  elements.updateDialogLaterButton.addEventListener("click", () => {
    elements.updateDialog.close();
  });
  elements.updateDialogOpenButton.addEventListener("click", () => {
    elements.updateDialog.close();
    void handleOpenUpdateRelease();
  });
  elements.starButton.addEventListener("click", () => {
    window.location.hash = "guide";
  });
  elements.xiaohongshuButton.addEventListener("click", () => {
    void handleOpenXiaohongshu();
  });
  elements.addProfilesButton.addEventListener("click", openAddProfileDialog);
  for (const button of elements.addProfileButtons) {
    button.addEventListener("click", openAddProfileDialog);
  }
  elements.cancelAddProfileButton.addEventListener("click", () => {
    elements.dialog.close();
  });
  elements.cancelRenameProfileButton.addEventListener("click", () => {
    closeRenameProfileDialog();
  });
  elements.cancelBaseUrlButton.addEventListener("click", () => {
    closeBaseUrlDialog();
  });
  elements.cancelDeleteProfileButton?.addEventListener("click", () => {
    closeDeleteProfileDialog();
  });
  elements.deleteProfileButton?.addEventListener("click", () => {
    void handleDeleteProfileAction("delete");
  });
  elements.clearProfileAccountButton?.addEventListener("click", () => {
    void handleDeleteProfileAction("clear");
  });
  elements.addProfileForm.addEventListener("submit", (event) => {
    void handleSubmitAddProfile(event as SubmitEvent);
  });
  elements.renameProfileForm.addEventListener("submit", (event) => {
    void handleSubmitRenameProfile(event as SubmitEvent);
  });
  elements.baseUrlForm.addEventListener("submit", (event) => {
    void handleSubmitBaseUrl(event as SubmitEvent);
  });
  elements.localeEnButton.addEventListener("click", () => {
    setLocale("en");
  });
  elements.localeZhButton.addEventListener("click", () => {
    setLocale("zh-CN");
  });
  for (const button of elements.localeButtons) {
    button.addEventListener("click", () => {
      setLocaleFromValue(button.dataset.setLocale);
    });
  }
  for (const button of elements.themeButtons) {
    button.addEventListener("click", () => {
      setThemeFromValue(button.dataset.themeOption);
    });
  }
  elements.gatewayToggleInput.addEventListener("change", () => {
    void handleToggleGateway(elements.gatewayToggleInput.checked);
  });
  elements.gatewayApplyButton.addEventListener("click", () => {
    void handleApplyGatewaySettings();
  });
  elements.gatewayRecoverButton.addEventListener("click", () => {
    void handleRecoverGateway();
  });
  window.setInterval(() => {
    void refreshCurrentQuota();
  }, 15_000);

  // Probe the gateway backend silently before starting the 5s poll. When the
  // forwarding feature is not wired up yet (e.g. the backend PR has not
  // landed), the probe fails quietly and the panel stays in its initial
  // state — manual toggle clicks still surface the underlying error.
  void loadGatewayStatus({ silent: true }).then((available) => {
    if (!available) {
      return;
    }
    window.setInterval(() => {
      void loadGatewayStatus({ silent: true });
    }, 5_000);
  });

  state.loading = true;
  rerenderDashboard();
  void refreshAllData().finally(() => {
    state.loading = false;
    rerenderDashboard();
    void handleCheckUpdate(true);
  });
}
