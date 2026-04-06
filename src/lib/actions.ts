import { persistLocale, resolveInitialLocale, t, type Locale } from "./i18n";
import { state } from "./state";
import {
  addProfile,
  getDashboard,
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
  renderRuntime,
  showToast,
} from "./render";

function rerenderDashboard(): void {
  applyLocale();

  if (!state.dashboard) {
    renderPaging({ has_previous: false, has_next: false });
    return;
  }

  renderRuntime(state.dashboard.runtime);
  renderProfiles(state.dashboard, handleSwitchProfile);
  renderCurrentCard(state.dashboard);
  renderPaging(state.dashboard.paging);
}

function setLocale(locale: Locale): void {
  if (state.locale === locale) {
    return;
  }

  state.locale = locale;
  persistLocale(locale);
  rerenderDashboard();
}

async function loadDashboard(page = state.page): Promise<void> {
  state.loading = true;
  renderPaging({ has_previous: false, has_next: false });

  try {
    const dashboard = await getDashboard(page);
    state.loading = false;
    state.page = dashboard.paging.page;
    state.dashboard = dashboard;
    renderRuntime(dashboard.runtime);
    renderProfiles(dashboard, handleSwitchProfile);
    renderCurrentCard(dashboard);
    renderPaging(dashboard.paging);
  } catch (error) {
    state.loading = false;
    showToast(error instanceof Error ? error.message : "Failed to load dashboard.", true);
    renderPaging({ has_previous: false, has_next: false });
  } finally {
    elements.openCurrentFolderButton.disabled = !state.currentProfile;
  }
}

async function handleSwitchProfile(profile: string): Promise<void> {
  try {
    state.loading = true;
    await switchProfile(profile);
    showToast(t(state.locale, "switchedTo", { profile }));
    await loadDashboard(state.page);
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToSwitchProfile"), true);
  } finally {
    state.loading = false;
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
    renderPaging({ has_previous: false, has_next: false });
    await loginCurrentProfile();
    showToast(t(state.locale, "loggedIn", { profile: state.currentProfile }));
    await loadDashboard(state.page);
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "failedToLogin"), true);
  } finally {
    state.loading = false;
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
    await addProfile(folderName);
    elements.dialog.close();
    showToast(t(state.locale, "createdProfile", { profile: folderName }));
    await loadDashboard(state.page);
  } catch (error) {
    elements.dialogError.hidden = false;
    elements.dialogError.textContent = error instanceof Error ? error.message : t(state.locale, "failedToCreateProfile");
  }
}

export function bootstrap(): void {
  state.locale = resolveInitialLocale();
  applyLocale();

  elements.previousPageButton.addEventListener("click", () => {
    void loadDashboard(state.page - 1);
  });
  elements.nextPageButton.addEventListener("click", () => {
    void loadDashboard(state.page + 1);
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

  void loadDashboard();
}
