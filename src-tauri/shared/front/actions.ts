import { persistLocale, resolveInitialLocale, t, type Locale } from "@front-shared/i18n";
import { state } from "@front-shared/state";
import {
  applyTheme,
  getThemeOption,
  isThemeId,
  persistTheme,
  resolveInitialTheme,
  type ThemeId,
} from "@front-shared/theme";
import {
  applyCurrentQuota,
  applySnapshot,
  buildDashboardViewModel,
} from "@front-shared/dashboard-view-model";
import {
  addProfile,
  checkUpdate,
  clearProfileAccount,
  deleteProfile,
  getCurrentLiveQuota,
  getProfilesSnapshot,
  loginCurrentProfile,
  openCodex,
  openContact,
  openReleases,
  openUrl,
  openXiaohongshu,
  openProfileFolder,
  refreshProfile,
  renameProfile,
  switchProfile,
  updateProfileBaseUrl,
} from "@front-shared/tauri";
import {
  applyLocale,
  elements,
  renderCurrentCard,
  renderPaging,
  renderProfiles,
  renderShellOverview,
  renderShellRoute,
  renderThemeOptions,
  routeFromLocation,
  showUpdateDialog,
  showToast,
} from "@front-shared/render";

type ErrorWithCode = Error & {
  code?: string;
};

function rerenderDashboard(): void {
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

let renameSourceProfile: string | null = null;
let baseUrlSourceProfile: string | null = null;
let pendingUpdateReleaseUrl: string | null = null;
let deleteSourceProfile: string | null = null;

function isRefreshPending(profile: string): boolean {
  return state.refreshActiveProfile === profile || state.refreshQueue.includes(profile);
}

function clearDialogError(element: HTMLParagraphElement): void {
  element.hidden = true;
  element.textContent = "";
}

function showDialogError(element: HTMLParagraphElement, message: string): void {
  element.hidden = false;
  element.textContent = message;
}

function openTextDialog(options: {
  dialog: HTMLDialogElement;
  form: HTMLFormElement;
  error: HTMLParagraphElement;
  input: HTMLInputElement;
  value?: string;
}): void {
  clearDialogError(options.error);
  options.form.reset();
  options.input.value = options.value ?? "";
  options.dialog.showModal();
  options.input.focus();
  options.input.select();
}

async function runBlockingAction<T>(run: () => Promise<T>): Promise<T> {
  state.loading = true;
  rerenderDashboard();
  try {
    return await run();
  } finally {
    state.loading = false;
    rerenderDashboard();
  }
}

function setLocale(locale: Locale): void {
  if (state.locale === locale) {
    return;
  }

  state.locale = locale;
  persistLocale(locale);
  rerenderDashboard();
}

function setLocaleFromValue(value: string | undefined): void {
  if (value === "en" || value === "zh-CN") {
    setLocale(value);
  }
}

function setTheme(theme: ThemeId): void {
  if (state.theme === theme) {
    return;
  }

  state.theme = theme;
  applyTheme(theme);
  persistTheme(theme);
  renderThemeOptions();
  showToast(t(state.locale, "themeChanged", { theme: t(state.locale, getThemeOption(theme).nameKey) }));
}

function setThemeFromValue(value: string | undefined): void {
  if (isThemeId(value)) {
    setTheme(value);
  }
}

async function refreshCurrentQuota(showError = false): Promise<void> {
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

async function refreshAllData(showError = true): Promise<void> {
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

function refreshProfileErrorMessage(error: unknown): string {
  if (isExpiredProfileAuthError(error)) {
    return t(state.locale, "profileRefreshRequiresLogin");
  }

  return error instanceof Error ? error.message : t(state.locale, "failedToRefreshProfile");
}

async function handleSwitchProfile(profile: string): Promise<void> {
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

async function drainRefreshQueue(): Promise<void> {
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

function handleRefreshProfile(profile: string): void {
  if (state.loading || isRefreshPending(profile)) {
    return;
  }

  state.refreshQueue.push(profile);
  rerenderDashboard();
  void drainRefreshQueue();
}

function handleRenameProfileClick(profile: string): void {
  renameSourceProfile = profile;
  openTextDialog({
    dialog: elements.renameDialog,
    form: elements.renameProfileForm,
    error: elements.renameDialogError,
    input: elements.renameFolderNameInput,
    value: profile,
  });
}

function handleBaseUrlProfileClick(profile: string): void {
  const currentBaseUrl =
    state.snapshot?.profiles.find((entry) => entry.folder_name === profile)?.openai_base_url ?? "";
  baseUrlSourceProfile = profile;
  openTextDialog({
    dialog: elements.baseUrlDialog,
    form: elements.baseUrlForm,
    error: elements.baseUrlDialogError,
    input: elements.baseUrlInput,
    value: currentBaseUrl,
  });
}

function handleDeleteProfileClick(profile: string): void {
  if (!elements.deleteProfileDialog || !elements.deleteProfileDialogError) {
    return;
  }

  deleteSourceProfile = profile;
  clearDialogError(elements.deleteProfileDialogError);
  elements.deleteProfileDialog.showModal();
}

async function handleOpenCurrentFolder(): Promise<void> {
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

async function handleOpenCodex(): Promise<void> {
  try {
    await openCodex();
    showToast(t(state.locale, "openedCodex"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenCodex"), true);
  }
}

async function handleLoginCurrentProfile(): Promise<void> {
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

async function handleOpenContact(): Promise<void> {
  try {
    await openContact();
    showToast(t(state.locale, "openedRepository"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenRepository"), true);
  }
}

async function handleOpenReleases(): Promise<void> {
  try {
    await openReleases();
    showToast(t(state.locale, "openedReleases"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenReleases"), true);
  }
}

async function handleOpenUpdateRelease(): Promise<void> {
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

async function handleCheckUpdate(silent = false): Promise<void> {
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

async function handleOpenXiaohongshu(): Promise<void> {
  try {
    await openXiaohongshu();
    showToast(t(state.locale, "openedXiaohongshu"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToOpenXiaohongshu"), true);
  }
}

function openAddProfileDialog(): void {
  openTextDialog({
    dialog: elements.dialog,
    form: elements.addProfileForm,
    error: elements.dialogError,
    input: elements.folderNameInput,
  });
}

function closeRenameProfileDialog(): void {
  renameSourceProfile = null;
  elements.renameDialog.close();
}

function closeBaseUrlDialog(): void {
  baseUrlSourceProfile = null;
  elements.baseUrlDialog.close();
}

function closeDeleteProfileDialog(): void {
  deleteSourceProfile = null;
  elements.deleteProfileDialog?.close();
}

async function handleSubmitAddProfile(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  clearDialogError(elements.dialogError);

  const folderName = elements.folderNameInput.value.trim();
  const openaiBaseUrl = elements.addBaseUrlInput.value.trim();
  if (!folderName) {
    showDialogError(elements.dialogError, t(state.locale, "folderNameRequired"));
    return;
  }

  try {
    await runBlockingAction(async () => {
      await addProfile(folderName, openaiBaseUrl || null);
      elements.dialog.close();
      showToast(t(state.locale, "createdProfile", { profile: folderName }));
      await refreshAllData();
    });
  } catch (error) {
    showDialogError(
      elements.dialogError,
      error instanceof Error ? error.message : t(state.locale, "failedToCreateProfile"),
    );
  }
}

async function handleSubmitRenameProfile(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  clearDialogError(elements.renameDialogError);

  const sourceProfile = renameSourceProfile;
  const nextFolderName = elements.renameFolderNameInput.value.trim();
  if (!nextFolderName) {
    showDialogError(elements.renameDialogError, t(state.locale, "folderNameRequired"));
    return;
  }
  if (!sourceProfile) {
    showDialogError(elements.renameDialogError, t(state.locale, "failedToRenameProfile"));
    return;
  }
  if (nextFolderName === sourceProfile) {
    closeRenameProfileDialog();
    return;
  }

  try {
    await runBlockingAction(async () => {
      await renameProfile(sourceProfile, nextFolderName);
      closeRenameProfileDialog();
      showToast(t(state.locale, "renamedProfile", { from: sourceProfile, to: nextFolderName }));
      await refreshAllData();
    });
  } catch (error) {
    showDialogError(
      elements.renameDialogError,
      error instanceof Error ? error.message : t(state.locale, "failedToRenameProfile"),
    );
  }
}

async function handleSubmitBaseUrl(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  clearDialogError(elements.baseUrlDialogError);

  const sourceProfile = baseUrlSourceProfile;
  const nextBaseUrl = elements.baseUrlInput.value.trim();
  if (!sourceProfile) {
    showDialogError(elements.baseUrlDialogError, t(state.locale, "failedToSaveBaseUrl"));
    return;
  }

  try {
    await runBlockingAction(async () => {
      await updateProfileBaseUrl(sourceProfile, nextBaseUrl);
      closeBaseUrlDialog();
      showToast(
        nextBaseUrl
          ? t(state.locale, "savedBaseUrl", { profile: sourceProfile })
          : t(state.locale, "clearedBaseUrl", { profile: sourceProfile }),
      );
      await refreshAllData();
    });
  } catch (error) {
    showDialogError(
      elements.baseUrlDialogError,
      error instanceof Error ? error.message : t(state.locale, "failedToSaveBaseUrl"),
    );
  }
}

async function handleDeleteProfileAction(action: "delete" | "clear"): Promise<void> {
  const sourceProfile = deleteSourceProfile;
  const errorElement = elements.deleteProfileDialogError;
  if (!errorElement) {
    return;
  }

  clearDialogError(errorElement);
  if (!sourceProfile) {
    showDialogError(errorElement, t(state.locale, "failedToDeleteProfile"));
    return;
  }

  try {
    await runBlockingAction(async () => {
      if (action === "delete") {
        await deleteProfile(sourceProfile);
        closeDeleteProfileDialog();
        showToast(t(state.locale, "deletedProfile", { profile: sourceProfile }));
      } else {
        await clearProfileAccount(sourceProfile);
        closeDeleteProfileDialog();
        showToast(t(state.locale, "clearedProfileAccount", { profile: sourceProfile }));
      }
      await refreshAllData();
    });
  } catch (error) {
    showDialogError(
      errorElement,
      error instanceof Error ? error.message : t(state.locale, "failedToDeleteProfile"),
    );
  }
}

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
  window.setInterval(() => {
    void refreshCurrentQuota();
  }, 15_000);

  state.loading = true;
  rerenderDashboard();
  void refreshAllData().finally(() => {
    state.loading = false;
    rerenderDashboard();
    void handleCheckUpdate(true);
  });
}
