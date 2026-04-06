import { state } from "./state";
import { addProfile, getDashboard, openCodex, openContact, openProfileFolder, switchProfile } from "./tauri";
import { elements, renderCurrentCard, renderPaging, renderProfiles, renderRuntime, showToast } from "./render";

async function loadDashboard(page = state.page): Promise<void> {
  state.loading = true;
  renderPaging({ has_previous: false, has_next: false });

  try {
    const dashboard = await getDashboard(page);
    state.loading = false;
    state.page = dashboard.paging.page;
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
    showToast(`Switched to ${profile}`);
    await loadDashboard(state.page);
  } catch (error) {
    showToast(error instanceof Error ? error.message : "Failed to switch profile.", true);
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
    showToast("Opened profile folder");
  } catch (error) {
    showToast(error instanceof Error ? error.message : "Failed to open profile folder.", true);
  }
}

async function handleOpenCodex(): Promise<void> {
  try {
    await openCodex();
    showToast("Opened Codex");
  } catch (error) {
    showToast(error instanceof Error ? error.message : "Failed to open Codex.", true);
  }
}

async function handleOpenContact(): Promise<void> {
  try {
    await openContact();
    showToast("Opened repository");
  } catch (error) {
    showToast(error instanceof Error ? error.message : "Failed to open repository.", true);
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
    elements.dialogError.textContent = "Folder name is required.";
    return;
  }

  try {
    await addProfile(folderName);
    elements.dialog.close();
    showToast(`Created profile ${folderName}`);
    await loadDashboard(state.page);
  } catch (error) {
    elements.dialogError.hidden = false;
    elements.dialogError.textContent = error instanceof Error ? error.message : "Failed to create profile.";
  }
}

export function bootstrap(): void {
  elements.previousPageButton.addEventListener("click", () => {
    void loadDashboard(state.page - 1);
  });
  elements.nextPageButton.addEventListener("click", () => {
    void loadDashboard(state.page + 1);
  });
  elements.openCurrentFolderButton.addEventListener("click", () => {
    void handleOpenCurrentFolder();
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

  void loadDashboard();
}
