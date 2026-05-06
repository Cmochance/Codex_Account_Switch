import { persistLocale, t, type Locale } from "@front-shared/i18n";
import { state } from "@front-shared/state";
import {
  applyTheme,
  getThemeOption,
  isThemeId,
  persistTheme,
  type ThemeId,
} from "@front-shared/theme";
import {
  checkUpdate,
  loginCurrentProfile,
  openCodex,
  openContact,
  openProfileFolder,
  openReleases,
  openUrl,
  openXiaohongshu,
  switchProfile,
} from "@front-shared/tauri";
import {
  elements,
  renderThemeOptions,
  showToast,
  showUpdateDialog,
} from "@front-shared/render";

import {
  drainRefreshQueue,
  isRefreshPending,
  refreshAllData,
  rerenderDashboard,
  runBlockingAction,
} from "@front-shared/actions/core";

let pendingUpdateReleaseUrl: string | null = null;

export function setLocale(locale: Locale): void {
  if (state.locale === locale) {
    return;
  }

  state.locale = locale;
  persistLocale(locale);
  rerenderDashboard();
}

export function setLocaleFromValue(value: string | undefined): void {
  if (value === "en" || value === "zh-CN") {
    setLocale(value);
  }
}

export function setTheme(theme: ThemeId): void {
  if (state.theme === theme) {
    return;
  }

  state.theme = theme;
  applyTheme(theme);
  persistTheme(theme);
  renderThemeOptions();
  showToast(t(state.locale, "themeChanged", { theme: t(state.locale, getThemeOption(theme).nameKey) }));
}

export function setThemeFromValue(value: string | undefined): void {
  if (isThemeId(value)) {
    setTheme(value);
  }
}

export async function handleSwitchProfile(profile: string): Promise<void> {
  try {
    await runBlockingAction(async () => {
      await switchProfile(profile);
      showToast(t(state.locale, "switchedTo", { profile }));
      await refreshAllData();
    });
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToSwitchProfile"), true);
  }
}

export function handleRefreshProfile(profile: string): void {
  if (state.loading || isRefreshPending(profile)) {
    return;
  }

  state.refreshQueue.push(profile);
  rerenderDashboard();
  void drainRefreshQueue();
}

export async function handleOpenCurrentFolder(): Promise<void> {
  if (!state.currentProfile) {
    return;
  }

  try {
    await openProfileFolder(state.currentProfile);
    showToast(t(state.locale, "openedProfileFolder"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenProfileFolder"), true);
  }
}

export async function handleOpenCodex(): Promise<void> {
  try {
    await openCodex();
    showToast(t(state.locale, "openedCodex"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenCodex"), true);
  }
}

export async function handleLoginCurrentProfile(): Promise<void> {
  if (!state.currentProfile) {
    return;
  }

  try {
    await runBlockingAction(async () => {
      await loginCurrentProfile();
      showToast(t(state.locale, "loggedIn", { profile: state.currentProfile as string }));
      await refreshAllData();
    });
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToLogin"), true);
  }
}

export async function handleOpenContact(): Promise<void> {
  try {
    await openContact();
    showToast(t(state.locale, "openedRepository"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenRepository"), true);
  }
}

export async function handleOpenReleases(): Promise<void> {
  try {
    await openReleases();
    showToast(t(state.locale, "openedReleases"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenReleases"), true);
  }
}

export async function handleOpenUpdateRelease(): Promise<void> {
  const releaseUrl = pendingUpdateReleaseUrl;
  if (!releaseUrl) {
    await handleOpenReleases();
    return;
  }

  try {
    await openUrl(releaseUrl);
    showToast(t(state.locale, "openedReleases"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenReleases"), true);
  }
}

export async function handleCheckUpdate(silent = false): Promise<void> {
  if (!silent) {
    showToast(t(state.locale, "checkingUpdate"));
  }

  try {
    const update = await checkUpdate(elements.settingsUpdateUrlInput.value);
    if (update.has_update) {
      pendingUpdateReleaseUrl = update.release_url;
      showUpdateDialog(update);
      if (!silent) {
        showToast(t(state.locale, "updateAvailable", {
          current: update.current_version,
          latest: update.latest_version ?? "--",
        }));
      }
      return;
    }

    if (!silent) {
      showToast(t(state.locale, "updateAlreadyLatest", { current: update.current_version }));
    }
  } catch (error) {
    if (!silent) {
      showToast(error instanceof Error ? error.message : t(state.locale, "failedToCheckUpdate"), true);
    }
  }
}

export async function handleOpenXiaohongshu(): Promise<void> {
  try {
    await openXiaohongshu();
    showToast(t(state.locale, "openedXiaohongshu"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenXiaohongshu"), true);
  }
}
