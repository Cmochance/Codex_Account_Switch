import { persistLocale, resolveInitialLocale, t, type Locale } from "@front-shared/i18n";
import { state } from "@front-shared/state";
import {
  applyCurrentQuota,
  applySnapshot,
  buildDashboardViewModel,
} from "@front-shared/dashboard-view-model";
import {
  fetchProfileProviderModels,
  addProfile,
  clearProfileAccount,
  deleteProfile,
  getCurrentLiveQuota,
  getProfilesSnapshot,
  loginCurrentProfile,
  openCodex,
  openContact,
  openReleases,
  openXiaohongshu,
  openProfileFolder,
  refreshProfile,
  renameProfile,
  switchProfile,
  updateProfileBaseUrl,
  updateProfileModelMappings,
} from "@front-shared/tauri";
import {
  applyLocale,
  elements,
  renderCurrentCard,
  renderPaging,
  renderProfiles,
  showToast,
} from "@front-shared/render";
import type {
  ModelMappingEntry,
  ProfileCard,
  ProviderModelListResponse,
} from "@front-shared/types";

type ErrorWithCode = Error & {
  code?: string;
};

const sourceModelOptions = [
  { value: "gpt-5.2", label: "GPT-5.2" },
  { value: "gpt-5.3-codex", label: "GPT-5.3-Codex" },
  { value: "gpt-5.4", label: "GPT-5.4" },
] as const;

const PROVIDER_PROTOCOL_RESPONSES = "responses";
const PROVIDER_PROTOCOL_CHAT_COMPLETIONS = "chat/completions";
const PROVIDER_PROTOCOL_MESSAGES = "messages";
const PROVIDER_PROTOCOL_COMPLETIONS = "completions";

const providerModelCache = new Map<string, ProviderModelListResponse>();

function rerenderDashboard(): void {
  applyLocale();

  const dashboard = buildDashboardViewModel();
  if (!dashboard) {
    renderPaging({ has_previous: false, has_next: false, page: 1, total_pages: 1 });
    return;
  }

  renderProfiles(
    dashboard,
    handleDeleteProfileClick,
    handleRenameProfileClick,
    handleSwitchProfile,
    handleRefreshProfile,
    handleBaseUrlProfileClick,
    handleModelMappingProfileClick,
  );
  renderCurrentCard(dashboard);
  renderPaging(dashboard.paging);
  if (elements.modelMappingDialog?.open) {
    renderProviderModelOptions();
    renderModelMappingRows();
    setModelMappingProviderType(modelMappingProviderProtocol);
    updateModelMappingDialogControls();
  }
}

let renameSourceProfile: string | null = null;
let baseUrlSourceProfile: string | null = null;
let deleteSourceProfile: string | null = null;
let modelMappingSourceProfile: string | null = null;
let modelMappingRows: ModelMappingEntry[] = [];
let modelMappingFetchPending = false;
let loadedProviderModels: string[] = [];
let modelMappingProviderProtocol: string | null = null;

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

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function currentProfileEntry(profile: string): ProfileCard | undefined {
  return state.snapshot?.profiles.find((entry) => entry.folder_name === profile);
}

function cloneMappings(mappings: ModelMappingEntry[]): ModelMappingEntry[] {
  return mappings.map((mapping) => ({
    source_model: mapping.source_model,
    target_model: mapping.target_model,
  }));
}

function defaultModelMappingRow(): ModelMappingEntry {
  return {
    source_model: sourceModelOptions[0]?.value ?? "gpt-5.2",
    target_model: "",
  };
}

function setModelMappingStatus(message: string, isError = false): void {
  if (!elements.modelMappingFetchStatus) {
    return;
  }

  elements.modelMappingFetchStatus.textContent = message;
  elements.modelMappingFetchStatus.style.color = isError ? "#a04949" : "";
}

function providerProtocolText(protocol: string | null): string {
  if (protocol === PROVIDER_PROTOCOL_RESPONSES) {
    return t(state.locale, "modelMappingProviderTypeResponses");
  }
  if (protocol === PROVIDER_PROTOCOL_CHAT_COMPLETIONS) {
    return t(state.locale, "modelMappingProviderTypeChatCompletions");
  }
  if (protocol === PROVIDER_PROTOCOL_MESSAGES) {
    return t(state.locale, "modelMappingProviderTypeMessages");
  }
  if (protocol === PROVIDER_PROTOCOL_COMPLETIONS) {
    return t(state.locale, "modelMappingProviderTypeCompletions");
  }
  if (protocol?.trim()) {
    return protocol;
  }
  return t(state.locale, "modelMappingProviderTypeUnknown");
}

function sourceOptionsForRow(sourceModel: string): ReadonlyArray<{ value: string; label: string }> {
  if (sourceModelOptions.some((option) => option.value === sourceModel)) {
    return sourceModelOptions;
  }

  if (!sourceModel.trim()) {
    return sourceModelOptions;
  }

  return [{ value: sourceModel, label: sourceModel }, ...sourceModelOptions];
}

function setModelMappingProviderType(protocol: string | null): void {
  modelMappingProviderProtocol = protocol;
  if (!elements.modelMappingProviderType) {
    return;
  }

  elements.modelMappingProviderType.hidden = false;
  elements.modelMappingProviderType.textContent = t(state.locale, "modelMappingProviderType", {
    type: providerProtocolText(protocol),
  });
}

function updateModelMappingDialogControls(): void {
  if (!elements.modelMappingDialog || !elements.modelMappingFetchButton) {
    return;
  }

  const profile = modelMappingSourceProfile ? currentProfileEntry(modelMappingSourceProfile) : null;
  const hasBaseUrl = Boolean(profile?.openai_base_url?.trim());
  elements.modelMappingFetchButton.disabled = state.loading || modelMappingFetchPending || !hasBaseUrl;
  elements.addModelMappingRowButton!.disabled = state.loading || modelMappingFetchPending;
  elements.submitModelMappingButton!.disabled = state.loading || modelMappingFetchPending;
  elements.cancelModelMappingButton!.disabled = modelMappingFetchPending;
}

function renderProviderModelOptions(): void {
  if (!elements.providerModelOptions) {
    return;
  }

  elements.providerModelOptions.innerHTML = loadedProviderModels
    .map((model) => `<option value="${escapeHtml(model)}"></option>`)
    .join("");
}

function bindModelMappingRowEvents(): void {
  if (!elements.modelMappingGrid) {
    return;
  }

  for (const select of elements.modelMappingGrid.querySelectorAll<HTMLSelectElement>("[data-model-mapping-source]")) {
    select.addEventListener("change", () => {
      const index = Number(select.getAttribute("data-model-mapping-source"));
      if (Number.isNaN(index) || !modelMappingRows[index]) {
        return;
      }
      modelMappingRows[index] = {
        ...modelMappingRows[index],
        source_model: select.value,
      };
    });
  }

  for (const input of elements.modelMappingGrid.querySelectorAll<HTMLInputElement>("[data-model-mapping-target]")) {
    input.addEventListener("input", () => {
      const index = Number(input.getAttribute("data-model-mapping-target"));
      if (Number.isNaN(index) || !modelMappingRows[index]) {
        return;
      }
      modelMappingRows[index] = {
        ...modelMappingRows[index],
        target_model: input.value,
      };
    });
  }

  for (const button of elements.modelMappingGrid.querySelectorAll<HTMLButtonElement>("[data-model-mapping-remove]")) {
    button.addEventListener("click", () => {
      const index = Number(button.getAttribute("data-model-mapping-remove"));
      if (Number.isNaN(index)) {
        return;
      }
      modelMappingRows = modelMappingRows.filter((_, currentIndex) => currentIndex !== index);
      renderModelMappingRows();
    });
  }
}

function renderModelMappingRows(): void {
  if (!elements.modelMappingGrid) {
    return;
  }

  if (!modelMappingRows.length) {
    elements.modelMappingGrid.innerHTML =
      `<div class="empty-state model-mapping-empty-state">${t(state.locale, "modelMappingEmpty")}</div>`;
    return;
  }

  const rowsMarkup = modelMappingRows
    .map((mapping, index) => {
      const sourceOptionsMarkup = sourceOptionsForRow(mapping.source_model)
        .map(
          (option) =>
            `<option value="${option.value}"${option.value === mapping.source_model ? " selected" : ""}>${option.label}</option>`,
        )
        .join("");

      return `
        <div class="model-mapping-row">
          <select class="model-mapping-select" data-model-mapping-source="${index}">
            ${sourceOptionsMarkup}
          </select>
          <input
            class="model-mapping-input"
            data-model-mapping-target="${index}"
            list="provider-model-options"
            placeholder="${escapeHtml(t(state.locale, "modelMappingTargetPlaceholder"))}"
            value="${escapeHtml(mapping.target_model)}"
          />
          <button
            class="ghost-button model-mapping-remove-button"
            type="button"
            data-model-mapping-remove="${index}"
          >
            ${t(state.locale, "modelMappingRemoveRow")}
          </button>
        </div>
      `;
    })
    .join("");

  elements.modelMappingGrid.innerHTML = rowsMarkup;
  bindModelMappingRowEvents();
}

function openModelMappingDialog(profile: string): void {
  if (!elements.modelMappingDialog || !elements.modelMappingDialogError) {
    return;
  }

  modelMappingSourceProfile = profile;
  modelMappingRows = cloneMappings(currentProfileEntry(profile)?.model_mappings ?? []);
  if (!modelMappingRows.length) {
    modelMappingRows = [defaultModelMappingRow()];
  }
  const cachedProviderModels = providerModelCache.get(profile);
  loadedProviderModels = [...(cachedProviderModels?.models ?? [])];
  modelMappingFetchPending = false;
  clearDialogError(elements.modelMappingDialogError);
  setModelMappingStatus("");
  setModelMappingProviderType(
    cachedProviderModels?.provider_protocol ?? currentProfileEntry(profile)?.provider_protocol ?? null,
  );
  renderProviderModelOptions();
  renderModelMappingRows();
  updateModelMappingDialogControls();
  elements.modelMappingDialog.showModal();
}

function closeModelMappingDialog(): void {
  modelMappingSourceProfile = null;
  modelMappingRows = [];
  loadedProviderModels = [];
  modelMappingFetchPending = false;
  modelMappingProviderProtocol = null;
  if (elements.modelMappingDialog) {
    elements.modelMappingDialog.close();
  }
}

function normalizeModelMappingsForSubmit(): ModelMappingEntry[] | null {
  const filteredRows = modelMappingRows
    .map((mapping) => ({
      source_model: mapping.source_model.trim(),
      target_model: mapping.target_model.trim(),
    }))
    .filter((mapping) => mapping.target_model);

  const seenSources = new Set<string>();
  for (const mapping of filteredRows) {
    const sourceKey = mapping.source_model.toLowerCase();
    if (seenSources.has(sourceKey)) {
      showDialogError(elements.modelMappingDialogError!, t(state.locale, "modelMappingDuplicateSource"));
      return null;
    }
    if (!mapping.target_model) {
      showDialogError(elements.modelMappingDialogError!, t(state.locale, "modelMappingTargetRequired"));
      return null;
    }
    seenSources.add(sourceKey);
  }

  return filteredRows;
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

function handleModelMappingProfileClick(profile: string): void {
  openModelMappingDialog(profile);
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

async function handleFetchProviderModels(): Promise<void> {
  if (!modelMappingSourceProfile || !elements.modelMappingDialogError) {
    return;
  }

  clearDialogError(elements.modelMappingDialogError);
  const profile = currentProfileEntry(modelMappingSourceProfile);
  if (!profile?.openai_base_url?.trim()) {
    showDialogError(elements.modelMappingDialogError, t(state.locale, "modelMappingMissingBaseUrl"));
    return;
  }

  modelMappingFetchPending = true;
  setModelMappingStatus("");
  setModelMappingProviderType(profile.provider_protocol ?? null);
  updateModelMappingDialogControls();
  try {
    const response = await fetchProfileProviderModels(modelMappingSourceProfile);
    loadedProviderModels = [...response.models];
    providerModelCache.set(modelMappingSourceProfile, {
      models: [...response.models],
      provider_protocol: response.provider_protocol ?? null,
      protocol_warning: response.protocol_warning ?? null,
    });
    renderProviderModelOptions();
    renderModelMappingRows();
    setModelMappingProviderType(response.provider_protocol ?? null);
    setModelMappingStatus(
      t(state.locale, "modelMappingFetchedModels", { count: response.models.length }),
    );
  } catch (error) {
    showDialogError(
      elements.modelMappingDialogError,
      error instanceof Error ? error.message : t(state.locale, "modelMappingFetchFailed"),
    );
    setModelMappingStatus(t(state.locale, "modelMappingFetchFailed"), true);
    setModelMappingProviderType(profile.provider_protocol ?? null);
  } finally {
    modelMappingFetchPending = false;
    updateModelMappingDialogControls();
  }
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

async function handleSubmitModelMappings(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  clearDialogError(elements.modelMappingDialogError!);

  const sourceProfile = modelMappingSourceProfile;
  if (!sourceProfile) {
    showDialogError(elements.modelMappingDialogError!, t(state.locale, "failedToSaveModelMappings"));
    return;
  }

  const normalizedMappings = normalizeModelMappingsForSubmit();
  if (!normalizedMappings) {
    return;
  }

  try {
    await runBlockingAction(async () => {
      await updateProfileModelMappings(sourceProfile, normalizedMappings);
      closeModelMappingDialog();
      showToast(
        normalizedMappings.length
          ? t(state.locale, "savedModelMappings", { profile: sourceProfile })
          : t(state.locale, "clearedModelMappings", { profile: sourceProfile }),
      );
      await refreshAllData();
    });
  } catch (error) {
    showDialogError(
      elements.modelMappingDialogError!,
      error instanceof Error ? error.message : t(state.locale, "failedToSaveModelMappings"),
    );
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
  elements.upgradeButton.addEventListener("click", () => {
    void handleOpenReleases();
  });
  elements.xiaohongshuButton.addEventListener("click", () => {
    void handleOpenXiaohongshu();
  });
  elements.addProfilesButton.addEventListener("click", openAddProfileDialog);
  elements.cancelAddProfileButton.addEventListener("click", () => {
    elements.dialog.close();
  });
  elements.cancelRenameProfileButton.addEventListener("click", () => {
    closeRenameProfileDialog();
  });
  elements.cancelBaseUrlButton.addEventListener("click", () => {
    closeBaseUrlDialog();
  });
  elements.cancelModelMappingButton?.addEventListener("click", () => {
    closeModelMappingDialog();
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
  elements.modelMappingForm?.addEventListener("submit", (event) => {
    void handleSubmitModelMappings(event as SubmitEvent);
  });
  elements.modelMappingFetchButton?.addEventListener("click", () => {
    void handleFetchProviderModels();
  });
  elements.addModelMappingRowButton?.addEventListener("click", () => {
    modelMappingRows = [...modelMappingRows, defaultModelMappingRow()];
    renderModelMappingRows();
  });
  elements.localeEnButton.addEventListener("click", () => {
    setLocale("en");
  });
  elements.localeZhButton.addEventListener("click", () => {
    setLocale("zh-CN");
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
