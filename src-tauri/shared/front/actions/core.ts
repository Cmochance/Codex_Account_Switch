import { t } from "@front-shared/i18n";
import { state } from "@front-shared/state";
import {
  applyCurrentQuota,
  applySnapshot,
  buildDashboardViewModel,
} from "@front-shared/dashboard-view-model";
import {
  getCurrentLiveQuota,
  getProfilesSnapshot,
  refreshProfile,
} from "@front-shared/tauri";
import {
  applyLocale,
  renderCurrentCard,
  renderPaging,
  renderProfiles,
  renderShellOverview,
  renderShellRoute,
  routeFromLocation,
  showToast,
} from "@front-shared/render";

import { handleRefreshProfile, handleSwitchProfile } from "@front-shared/actions/handlers";
import {
  handleBaseUrlProfileClick,
  handleDeleteProfileClick,
  handleRenameProfileClick,
} from "@front-shared/actions/dialogs";

type ErrorWithCode = Error & {
  code?: string;
};

export function rerenderDashboard(): void {
  state.route = routeFromLocation();
  applyLocale();
  renderShellRoute();

  const dashboard = buildDashboardViewModel();
  if (!dashboard) {
    renderPaging({ has_previous: false, has_next: false, page: 1, total_pages: 1 });
    renderShellOverview(null);
    return;
  }

  renderProfiles(
    dashboard,
    handleDeleteProfileClick,
    handleRenameProfileClick,
    handleSwitchProfile,
    handleRefreshProfile,
    handleBaseUrlProfileClick,
  );
  renderCurrentCard(dashboard);
  renderPaging(dashboard.paging);
  renderShellOverview(dashboard);
}

export function isRefreshPending(profile: string): boolean {
  return state.refreshActiveProfile === profile || state.refreshQueue.includes(profile);
}

export async function runBlockingAction<T>(run: () => Promise<T>): Promise<T> {
  state.loading = true;
  rerenderDashboard();
  try {
    return await run();
  } finally {
    state.loading = false;
    rerenderDashboard();
  }
}

export async function refreshCurrentQuota(showError = false): Promise<void> {
  if (state.loading || !state.snapshot) {
    return;
  }

  try {
    applyCurrentQuota(await getCurrentLiveQuota());
    rerenderDashboard();
  } catch (error) {
    if (showError) {
      showToast(error instanceof Error ? error.message : "Failed to refresh quota.", true);
    }
  }
}

export async function refreshAllData(showError = true): Promise<void> {
  try {
    const [snapshot, currentQuota] = await Promise.all([
      getProfilesSnapshot(),
      getCurrentLiveQuota(),
    ]);

    applySnapshot(snapshot);
    applyCurrentQuota(currentQuota);
    rerenderDashboard();
  } catch (error) {
    if (showError) {
      showToast(error instanceof Error ? error.message : "Failed to load dashboard.", true);
    }
  }
}

function isExpiredProfileAuthError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false;
  }

  const code = (error as ErrorWithCode).code;
  if (code === "AUTH_REFRESH_RELOGIN_REQUIRED") {
    return true;
  }

  return /token_invalidated|refresh_token_reused|sign(?:ing)? in again|log out and sign in again/i.test(
    error.message,
  );
}

export function refreshProfileErrorMessage(error: unknown): string {
  if (isExpiredProfileAuthError(error)) {
    return t(state.locale, "profileRefreshRequiresLogin");
  }

  return error instanceof Error ? error.message : t(state.locale, "failedToRefreshProfile");
}

export async function drainRefreshQueue(): Promise<void> {
  if (state.refreshWorkerActive) {
    return;
  }

  state.refreshWorkerActive = true;
  try {
    while (state.refreshQueue.length > 0) {
      const profile = state.refreshQueue.shift();
      if (!profile) {
        continue;
      }

      state.refreshActiveProfile = profile;
      rerenderDashboard();
      try {
        await refreshProfile(profile);
        showToast(t(state.locale, "refreshedProfile", { profile }));
        await refreshAllData(false);
      } catch (error) {
        showToast(refreshProfileErrorMessage(error), true);
      } finally {
        state.refreshActiveProfile = null;
        rerenderDashboard();
      }
    }
  } finally {
    state.refreshWorkerActive = false;
    rerenderDashboard();
  }
}
