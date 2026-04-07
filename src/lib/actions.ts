import { persistLocale, resolveInitialLocale, t, type Locale } from "./i18n";
import { state } from "./state";
import {
  applyCurrentQuota,
  applySnapshot,
  buildDashboardViewModel,
} from "./dashboard-view-model";
import {
  addProfile,
  getCurrentLiveQuota,
  getProfilesSnapshot,
  loginCurrentProfile,
  openCodex,
  openContact,
  openProfileFolder,
  switchProfile,
} from "./tauri";
import {
  applyLocale,
  elements,
  renderCurrentCard,
  renderPaging,
  renderProfiles,
  showToast,
} from "./render";

function rerenderDashboard(): void {
  applyLocale();

  const dashboard = buildDashboardViewModel();
  if (!dashboard) {
    renderPaging({ has_previous: false, has_next: false });
    return;
  }

  renderProfiles(dashboard, handleSwitchProfile);
  renderCurrentCard(dashboard);
  renderPaging(dashboard.paging);
}

function setLocale(locale: Locale): void {
  if (state.locale === locale) {
    return;
  }

  state.locale = locale;
  persistLocale(locale);
  rerenderDashboard();
}

async function refreshProfilesSnapshot(showError = false): Promise<void> {
  if (state.loading) {
    return;
  }

  try {
    applySnapshot(await getProfilesSnapshot());
    rerenderDashboard();
  } catch (error) {
    if (showError) {
      showToast(error instanceof Error ? error.message : "Failed to load profiles.", true);
    }
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

async function handleSwitchProfile(profile: string): Promise<void> {
  try {
    state.loading = true;
    rerenderDashboard();
    await switchProfile(profile);
    showToast(t(state.locale, "switchedTo", { profile }));
    await refreshAllData();
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToSwitchProfile"), true);
  } finally {
    state.loading = false;
    rerenderDashboard();
  }
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
    state.loading = true;
    rerenderDashboard();
    await loginCurrentProfile();
    showToast(t(state.locale, "loggedIn", { profile: state.currentProfile }));
    await refreshAllData();
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToLogin"), true);
  } finally {
    state.loading = false;
    rerenderDashboard();
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

function openAddProfileDialog(): void {
  elements.dialogError.hidden = true;
  elements.dialogError.textContent = "";
  elements.addProfileForm.reset();
  elements.dialog.showModal();
  elements.folderNameInput.focus();
}

async function handleSubmitAddProfile(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  elements.dialogError.hidden = true;
  elements.dialogError.textContent = "";

  const folderName = elements.folderNameInput.value.trim();
  if (!folderName) {
    elements.dialogError.hidden = false;
    elements.dialogError.textContent = t(state.locale, "folderNameRequired");
    return;
  }

  try {
    state.loading = true;
    rerenderDashboard();
    await addProfile(folderName);
    elements.dialog.close();
    showToast(t(state.locale, "createdProfile", { profile: folderName }));
    await refreshAllData();
  } catch (error) {
    elements.dialogError.hidden = false;
    elements.dialogError.textContent = error instanceof Error ? error.message : t(state.locale, "failedToCreateProfile");
  } finally {
    state.loading = false;
    rerenderDashboard();
  }
}

export function bootstrap(): void {
  state.locale = resolveInitialLocale();
  applyLocale();

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
  elements.contactButton.addEventListener("click", () => {
    void handleOpenContact();
  });
  elements.addProfilesButton.addEventListener("click", openAddProfileDialog);
  elements.cancelAddProfileButton.addEventListener("click", () => {
    elements.dialog.close();
  });
  elements.addProfileForm.addEventListener("submit", (event) => {
    void handleSubmitAddProfile(event as SubmitEvent);
  });
  elements.localeToggleButton.addEventListener("click", () => {
    setLocale(state.locale === "en" ? "zh-CN" : "en");
  });
  window.setInterval(() => {
    void refreshCurrentQuota();
  }, 15_000);

  state.loading = true;
  rerenderDashboard();
  void refreshAllData().finally(() => {
    state.loading = false;
    rerenderDashboard();
  });
}
