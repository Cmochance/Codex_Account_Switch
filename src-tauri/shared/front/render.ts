import type {
  CurrentCard,
  DashboardViewModel,
  PagingInfo,
  ProfileCard,
  QuotaSummary,
  QuotaWindow,
} from "@front-shared/types";
import { t } from "@front-shared/i18n";
import { state } from "@front-shared/state";

const isWindowsUiTarget = __CODEX_UI_TARGET__ === "windows";

function requiredElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!(element instanceof HTMLElement)) {
    throw new Error(`Missing required element: ${id}`);
  }
  return element as T;
}

const hasDeleteProfileUi = document.getElementById("delete-profile-dialog") instanceof HTMLDialogElement;
const hasModelMappingUi =
  isWindowsUiTarget && document.getElementById("model-mapping-dialog") instanceof HTMLDialogElement;

export const elements = {
  profilesHeading: requiredElement<HTMLHeadingElement>("profiles-heading"),
  profilesGrid: requiredElement<HTMLDivElement>("profiles-grid"),
  pageIndicator: requiredElement<HTMLSpanElement>("page-indicator"),
  previousPageButton: requiredElement<HTMLButtonElement>("previous-page-button"),
  nextPageButton: requiredElement<HTMLButtonElement>("next-page-button"),
  currentSectionHeading: requiredElement<HTMLHeadingElement>("current-section-heading"),
  currentTitle: requiredElement<HTMLHeadingElement>("current-title"),
  currentPlan: requiredElement<HTMLParagraphElement>("current-plan"),
  currentQuotaPanel: requiredElement<HTMLDivElement>("current-quota-panel"),
  currentLoginButton: requiredElement<HTMLButtonElement>("current-login-button"),
  openCurrentFolderButton: requiredElement<HTMLButtonElement>("open-current-folder-button"),
  controlDeckHeading: requiredElement<HTMLHeadingElement>("control-deck-heading"),
  addProfilesButton: requiredElement<HTMLButtonElement>("add-profiles-button"),
  openCodexButton: requiredElement<HTMLButtonElement>("open-codex-button"),
  contactButton: requiredElement<HTMLButtonElement>("contact-button"),
  upgradeButton: requiredElement<HTMLButtonElement>("upgrade-button"),
  starButton: requiredElement<HTMLButtonElement>("star-button"),
  xiaohongshuButton: requiredElement<HTMLButtonElement>("xiaohongshu-button"),
  localeEnButton: requiredElement<HTMLButtonElement>("locale-en-button"),
  localeZhButton: requiredElement<HTMLButtonElement>("locale-zh-button"),
  quotaMonitorLabel: requiredElement<HTMLSpanElement>("quota-monitor-label"),
  dialog: document.getElementById("add-profile-dialog") as HTMLDialogElement,
  addProfileForm: requiredElement<HTMLFormElement>("add-profile-form"),
  cancelAddProfileButton: requiredElement<HTMLButtonElement>("cancel-add-profile-button"),
  submitAddProfileButton: requiredElement<HTMLButtonElement>("submit-add-profile-button"),
  dialogTitle: requiredElement<HTMLHeadingElement>("dialog-title"),
  dialogCopy: requiredElement<HTMLParagraphElement>("dialog-copy"),
  folderNameLabel: requiredElement<HTMLSpanElement>("folder-name-label"),
  folderNameInput: requiredElement<HTMLInputElement>("folder-name-input"),
  addBaseUrlLabel: requiredElement<HTMLSpanElement>("add-base-url-label"),
  addBaseUrlInput: requiredElement<HTMLInputElement>("add-base-url-input"),
  addBaseUrlCopy: requiredElement<HTMLSpanElement>("add-base-url-copy"),
  dialogError: requiredElement<HTMLParagraphElement>("dialog-error"),
  renameDialog: document.getElementById("rename-profile-dialog") as HTMLDialogElement,
  renameProfileForm: requiredElement<HTMLFormElement>("rename-profile-form"),
  renameDialogTitle: requiredElement<HTMLHeadingElement>("rename-dialog-title"),
  renameDialogCopy: requiredElement<HTMLParagraphElement>("rename-dialog-copy"),
  renameFolderNameLabel: requiredElement<HTMLSpanElement>("rename-folder-name-label"),
  renameFolderNameInput: requiredElement<HTMLInputElement>("rename-folder-name-input"),
  renameDialogError: requiredElement<HTMLParagraphElement>("rename-dialog-error"),
  cancelRenameProfileButton: requiredElement<HTMLButtonElement>("cancel-rename-profile-button"),
  submitRenameProfileButton: requiredElement<HTMLButtonElement>("submit-rename-profile-button"),
  deleteProfileDialog: hasDeleteProfileUi
    ? requiredElement<HTMLDialogElement>("delete-profile-dialog")
    : null,
  deleteProfileDialogTitle: hasDeleteProfileUi
    ? requiredElement<HTMLHeadingElement>("delete-profile-dialog-title")
    : null,
  deleteProfileDialogCopy: hasDeleteProfileUi
    ? requiredElement<HTMLParagraphElement>("delete-profile-dialog-copy")
    : null,
  deleteProfileDialogError: hasDeleteProfileUi
    ? requiredElement<HTMLParagraphElement>("delete-profile-dialog-error")
    : null,
  deleteProfileButton: hasDeleteProfileUi
    ? requiredElement<HTMLButtonElement>("delete-profile-button")
    : null,
  clearProfileAccountButton: hasDeleteProfileUi
    ? requiredElement<HTMLButtonElement>("clear-profile-account-button")
    : null,
  cancelDeleteProfileButton: hasDeleteProfileUi
    ? requiredElement<HTMLButtonElement>("cancel-delete-profile-button")
    : null,
  baseUrlDialog: document.getElementById("base-url-dialog") as HTMLDialogElement,
  baseUrlForm: requiredElement<HTMLFormElement>("base-url-form"),
  baseUrlDialogTitle: requiredElement<HTMLHeadingElement>("base-url-dialog-title"),
  baseUrlDialogCopy: requiredElement<HTMLParagraphElement>("base-url-dialog-copy"),
  baseUrlLabel: requiredElement<HTMLSpanElement>("base-url-label"),
  baseUrlInput: requiredElement<HTMLInputElement>("base-url-input"),
  baseUrlDialogError: requiredElement<HTMLParagraphElement>("base-url-dialog-error"),
  cancelBaseUrlButton: requiredElement<HTMLButtonElement>("cancel-base-url-button"),
  submitBaseUrlButton: requiredElement<HTMLButtonElement>("submit-base-url-button"),
  modelMappingDialog: hasModelMappingUi
    ? requiredElement<HTMLDialogElement>("model-mapping-dialog")
    : null,
  modelMappingForm: hasModelMappingUi
    ? requiredElement<HTMLFormElement>("model-mapping-form")
    : null,
  modelMappingDialogTitle: hasModelMappingUi
    ? requiredElement<HTMLHeadingElement>("model-mapping-dialog-title")
    : null,
  modelMappingDialogCopy: hasModelMappingUi
    ? requiredElement<HTMLParagraphElement>("model-mapping-dialog-copy")
    : null,
  modelMappingDialogHint: hasModelMappingUi
    ? requiredElement<HTMLParagraphElement>("model-mapping-dialog-hint")
    : null,
  modelMappingSourceLabel: hasModelMappingUi
    ? requiredElement<HTMLSpanElement>("model-mapping-source-label")
    : null,
  modelMappingTargetLabel: hasModelMappingUi
    ? requiredElement<HTMLSpanElement>("model-mapping-target-label")
    : null,
  modelMappingGrid: hasModelMappingUi
    ? requiredElement<HTMLDivElement>("model-mapping-grid")
    : null,
  providerModelOptions: hasModelMappingUi
    ? requiredElement<HTMLDataListElement>("provider-model-options")
    : null,
  modelMappingFetchButton: hasModelMappingUi
    ? requiredElement<HTMLButtonElement>("model-mapping-fetch-button")
    : null,
  addModelMappingRowButton: hasModelMappingUi
    ? requiredElement<HTMLButtonElement>("add-model-mapping-row-button")
    : null,
  modelMappingFetchStatus: hasModelMappingUi
    ? requiredElement<HTMLSpanElement>("model-mapping-fetch-status")
    : null,
  modelMappingProviderType: hasModelMappingUi
    ? requiredElement<HTMLParagraphElement>("model-mapping-provider-type")
    : null,
  modelMappingDialogError: hasModelMappingUi
    ? requiredElement<HTMLParagraphElement>("model-mapping-dialog-error")
    : null,
  cancelModelMappingButton: hasModelMappingUi
    ? requiredElement<HTMLButtonElement>("cancel-model-mapping-button")
    : null,
  submitModelMappingButton: hasModelMappingUi
    ? requiredElement<HTMLButtonElement>("submit-model-mapping-button")
    : null,
  toast: requiredElement<HTMLDivElement>("toast"),
};

function formatPercent(value: number | null): string {
  return value == null ? "--" : `${value}%`;
}

function formatRefresh(value: string | null): string {
  return value || "--";
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function bindProfileButtons(attribute: string, handler: (profile: string) => void): void {
  for (const button of elements.profilesGrid.querySelectorAll<HTMLButtonElement>(`[${attribute}]`)) {
    button.addEventListener("click", () => {
      const profile = button.getAttribute(attribute);
      if (profile) {
        void handler(profile);
      }
    });
  }
}

function isProfileUnavailable(profile: Pick<ProfileCard, "auth_present" | "has_account_identity" | "status">): boolean {
  return profile.status === "missing_auth" || !profile.auth_present || !profile.has_account_identity;
}

function normalizeDisplayParts(
  entry: Pick<ProfileCard | CurrentCard, "folder_name" | "display_title" | "account_label">,
): { folder: string; account: string } {
  const folder = entry.folder_name.trim();
  const account = entry.account_label?.trim() ?? "";

  if (account) {
    return { folder, account };
  }

  const rawTitle = entry.display_title?.trim() ?? "";
  const parts = rawTitle.split(" / ").map((value) => value.trim()).filter(Boolean);
  if (parts.length >= 2) {
    return {
      folder: parts[0] ?? folder,
      account: parts[parts.length - 1] ?? "",
    };
  }

  return { folder, account: rawTitle };
}

function profileDisplayTitle(entry: Pick<ProfileCard, "folder_name" | "display_title" | "account_label">): string {
  const { folder, account } = normalizeDisplayParts(entry);
  if (folder && account && folder !== account) {
    return `${folder} · ${account}`;
  }

  return account || folder || "--";
}

function currentDisplayTitle(entry: Pick<CurrentCard, "folder_name" | "display_title" | "account_label">): string {
  const { folder, account } = normalizeDisplayParts(entry);
  return account || folder || "--";
}

function formatPlanName(planName: string): string {
  return planName.replace(/\b([a-z])/g, (match) => match.toUpperCase());
}

export function planLine(planName: string | null, daysLeft: number | null): string {
  const formattedPlanName = planName ? formatPlanName(planName) : null;

  if (!planName && daysLeft == null) {
    return t(state.locale, "profileMetadataMissing");
  }

  if (formattedPlanName && daysLeft != null) {
    return t(state.locale, "subscriptionDaysLeft", { plan: formattedPlanName, days: daysLeft });
  }

  return formattedPlanName || t(state.locale, "subscriptionFallback", { days: daysLeft ?? "--" });
}

function buildMetricLineMarkup(
  label: string,
  entry: QuotaWindow | undefined,
  fillVariant: "blue" | "pink",
  unavailable: boolean,
  layout: "profile" | "current",
): string {
  const percent = unavailable ? 0 : (entry?.remaining_percent ?? 0);
  const metricClass = layout === "current" ? "current-quota-metric" : "profile-quota-metric";
  const lineClass = layout === "current" ? "current-quota-line" : "profile-quota-line";
  const titleClass = layout === "current" ? "current-quota-title" : "profile-quota-title";
  const refreshClass = layout === "current" ? "current-quota-refresh" : "profile-quota-refresh";
  const valueClass = layout === "current" ? "current-quota-value" : "profile-quota-value";
  const fillClass = unavailable ? "gray" : fillVariant;

  return `
    <section class="${metricClass}${unavailable ? " is-unavailable" : ""}">
      <div class="${lineClass}">
        <span class="${titleClass}">${escapeHtml(label)}</span>
        <span class="${refreshClass}">${escapeHtml(formatRefresh(entry?.refresh_at ?? null))}</span>
        <span class="${valueClass}">${escapeHtml(formatPercent(unavailable ? null : entry?.remaining_percent ?? null))}</span>
      </div>
      <div class="quota-track">
        <div class="quota-fill quota-fill--${fillClass}" style="width: ${percent}%;"></div>
      </div>
    </section>
  `;
}

function buildProfileQuotaMarkup(profile: ProfileCard): string {
  const unavailable = isProfileUnavailable(profile);
  const quota = profile.quota;

  return `
    <div class="profile-quota-stack">
      ${buildMetricLineMarkup(t(state.locale, "fiveHourAllowance"), quota?.five_hour, "blue", unavailable, "profile")}
      ${buildMetricLineMarkup(t(state.locale, "weeklyAllowance"), quota?.weekly, "pink", unavailable, "profile")}
    </div>
  `;
}

function buildCurrentQuotaMarkup(
  quota: QuotaSummary | null | undefined,
  hasAccountIdentity: boolean,
): string {
  const unavailable = !hasAccountIdentity;

  return `
    <div class="current-quota-stack">
      ${buildMetricLineMarkup(t(state.locale, "fiveHourAllowance"), quota?.five_hour, "blue", unavailable, "current")}
      ${buildMetricLineMarkup(t(state.locale, "weeklyAllowance"), quota?.weekly, "pink", unavailable, "current")}
    </div>
  `;
}

export function showToast(message: string, isError = false): void {
  elements.toast.hidden = false;
  elements.toast.textContent = message;
  elements.toast.style.borderColor = isError ? "rgba(190, 95, 86, 0.44)" : "rgba(197, 227, 236, 0.8)";
  elements.toast.style.color = isError ? "#8f3b35" : "#52555f";
  window.clearTimeout((showToast as typeof showToast & { timeoutId?: number }).timeoutId);
  (showToast as typeof showToast & { timeoutId?: number }).timeoutId = window.setTimeout(() => {
    elements.toast.hidden = true;
  }, 3200);
}

export function renderCurrentCard(dashboard: DashboardViewModel): void {
  const current = dashboard.current_card;
  if (!current) {
    elements.currentTitle.textContent = t(state.locale, "noActiveProfile");
    elements.currentPlan.textContent = t(state.locale, "switchToStart");
    elements.currentLoginButton.disabled = true;
    elements.openCurrentFolderButton.disabled = true;
    state.currentProfile = null;
    elements.currentQuotaPanel.innerHTML =
      `<div class="empty-state">${t(state.locale, "quotaWillAppear")}</div>`;
    return;
  }

  state.currentProfile = current.folder_name;
  elements.currentTitle.textContent = currentDisplayTitle(current);
  elements.currentPlan.textContent = planLine(current.plan_name, current.subscription_days_left);
  elements.currentLoginButton.disabled = state.loading;
  elements.openCurrentFolderButton.disabled = false;
  elements.currentQuotaPanel.innerHTML = buildCurrentQuotaMarkup(
    dashboard.current_quota_card,
    current.has_account_identity,
  );
}

export function renderProfiles(
  dashboard: DashboardViewModel,
  onDelete: (profile: string) => void,
  onRename: (profile: string) => void,
  onSwitch: (profile: string) => void,
  onRefresh: (profile: string) => void,
  onBaseUrl: (profile: string) => void,
  onModelMapping: (profile: string) => void,
): void {
  if (!dashboard.profiles.length) {
    elements.profilesGrid.innerHTML =
      `<div class="empty-state profiles-empty-state">${t(state.locale, "profilesEmpty")}</div>`;
    return;
  }

  elements.profilesGrid.innerHTML = dashboard.profiles
    .map((profile) => {
      const refreshRunning = state.refreshActiveProfile === profile.folder_name;
      const refreshQueued =
        !refreshRunning && state.refreshQueue.includes(profile.folder_name);
      const refreshPending = refreshRunning || refreshQueued;
      const deleteDisabled =
        state.loading || refreshPending || profile.status === "current";
      const renameDisabled =
        state.loading || refreshPending || profile.status === "current";
      const refreshDisabled =
        !profile.auth_present || state.loading || refreshPending;
      const baseDisabled = state.loading || refreshPending;
      const switchDisabled =
        !profile.auth_present || state.loading || refreshPending || profile.status === "current";
      const unavailable = isProfileUnavailable(profile);
      const refreshTitle = refreshRunning
        ? t(state.locale, "profileRefreshRunning")
        : refreshQueued
          ? t(state.locale, "profileRefreshQueued")
          : refreshDisabled
            ? t(state.locale, "profileRefreshDisabled")
            : t(state.locale, "profileRefreshReady");

      return `
        <article class="profile-card status-${profile.status}${unavailable ? " is-unavailable-card" : ""}">
          <div class="profile-title-wrap">
            <p class="profile-title-account">${escapeHtml(profileDisplayTitle(profile))}</p>
            <p class="profile-plan">${escapeHtml(planLine(profile.plan_name, profile.subscription_days_left))}</p>
          </div>

          ${buildProfileQuotaMarkup(profile)}

          <div class="profile-card-actions${isWindowsUiTarget ? " profile-card-actions--windows" : ""}">
            ${
              hasDeleteProfileUi
                ? `<button
                    class="profile-action-button profile-action-button-danger"
                    type="button"
                    title="${deleteDisabled ? t(state.locale, "profileDeleteDisabled") : t(state.locale, "profileDeleteReady")}"
                    data-delete-profile="${profile.folder_name}"
                    ${deleteDisabled ? "disabled" : ""}
                  >
                    ${t(state.locale, "deleteButton")}
                  </button>`
                : ""
            }
            <button
              class="profile-action-button"
              type="button"
              title="${renameDisabled ? t(state.locale, "profileRenameDisabled") : t(state.locale, "profileRenameReady")}"
              data-rename-profile="${profile.folder_name}"
              ${renameDisabled ? "disabled" : ""}
            >
              ${t(state.locale, "rename")}
            </button>
            <button
              class="profile-action-button"
              type="button"
              title="${refreshTitle}"
              data-refresh-profile="${profile.folder_name}"
              ${refreshDisabled ? "disabled" : ""}
            >
              ${
                refreshPending
                  ? '<span class="button-spinner" aria-hidden="true"></span>'
                  : t(state.locale, "refreshButton")
              }
            </button>
            <button
              class="profile-action-button"
              type="button"
              title="${t(state.locale, "profileBaseReady")}"
              data-base-url-profile="${profile.folder_name}"
              ${baseDisabled ? "disabled" : ""}
            >
              ${t(state.locale, "baseButton")}
            </button>
            ${
              hasModelMappingUi
                ? `<button
                    class="profile-action-button"
                    type="button"
                    title="${t(state.locale, "profileModelReady")}"
                    data-model-mapping-profile="${profile.folder_name}"
                    ${baseDisabled ? "disabled" : ""}
                  >
                    ${t(state.locale, "modelButton")}
                  </button>`
                : ""
            }
            <button
              class="profile-action-button"
              type="button"
              title="${switchDisabled ? t(state.locale, "profileSwitchDisabled") : t(state.locale, "profileSwitchReady")}"
              data-switch-profile="${profile.folder_name}"
              ${switchDisabled ? "disabled" : ""}
            >
              ${t(state.locale, "switch")}
            </button>
          </div>
        </article>
      `;
    })
    .join("");

  if (hasDeleteProfileUi) {
    bindProfileButtons("data-delete-profile", onDelete);
  }
  bindProfileButtons("data-rename-profile", onRename);
  bindProfileButtons("data-refresh-profile", onRefresh);
  bindProfileButtons("data-base-url-profile", onBaseUrl);
  if (hasModelMappingUi) {
    bindProfileButtons("data-model-mapping-profile", onModelMapping);
  }
  bindProfileButtons("data-switch-profile", onSwitch);
}

export function renderPaging(
  paging: Pick<PagingInfo, "has_previous" | "has_next" | "page" | "total_pages">,
): void {
  elements.previousPageButton.disabled = state.loading || !paging.has_previous;
  elements.nextPageButton.disabled = state.loading || !paging.has_next;
  elements.pageIndicator.textContent = `${paging.page} / ${paging.total_pages}`;
}

export function applyLocale(): void {
  document.documentElement.lang = state.locale;
  document.title = t(state.locale, "appTitle");

  elements.profilesHeading.textContent = t(state.locale, "profilesHeading");
  elements.currentSectionHeading.textContent = t(state.locale, "currentSession");
  elements.controlDeckHeading.textContent = t(state.locale, "controlDeck");
  elements.currentLoginButton.textContent = t(state.locale, "login");
  elements.openCurrentFolderButton.textContent = t(state.locale, "openFolder");
  elements.addProfilesButton.textContent = t(state.locale, "addProfiles");
  elements.openCodexButton.textContent = t(state.locale, "openCodex");
  elements.contactButton.textContent = t(state.locale, "contactUs");
  elements.upgradeButton.textContent = t(state.locale, "upgrade");
  elements.starButton.textContent = t(state.locale, "star");
  elements.xiaohongshuButton.textContent = t(state.locale, "xiaohongshu");
  elements.previousPageButton.textContent = t(state.locale, "previous");
  elements.nextPageButton.textContent = t(state.locale, "next");
  elements.quotaMonitorLabel.textContent = t(state.locale, "quotaMonitor");
  elements.localeEnButton.textContent = t(state.locale, "languageEnglish");
  elements.localeZhButton.textContent = t(state.locale, "languageChinese");
  elements.localeEnButton.classList.toggle("is-active", state.locale === "en");
  elements.localeZhButton.classList.toggle("is-active", state.locale === "zh-CN");
  elements.localeEnButton.setAttribute("aria-pressed", state.locale === "en" ? "true" : "false");
  elements.localeZhButton.setAttribute("aria-pressed", state.locale === "zh-CN" ? "true" : "false");
  elements.dialogTitle.textContent = t(state.locale, "addProfileTitle");
  elements.dialogCopy.innerHTML = t(state.locale, "addProfileCopy")
    .replace("auth.json", "<code>auth.json</code>")
    .replace("profile.json", "<code>profile.json</code>");
  elements.renameDialogTitle.textContent = t(state.locale, "renameProfileTitle");
  elements.renameDialogCopy.textContent = t(state.locale, "renameProfileCopy");
  if (hasDeleteProfileUi) {
    elements.deleteProfileDialogTitle!.textContent = t(state.locale, "deleteProfileTitle");
    elements.deleteProfileDialogCopy!.textContent = t(state.locale, "deleteProfileCopy");
    elements.deleteProfileButton!.textContent = t(state.locale, "deleteCard");
    elements.clearProfileAccountButton!.textContent = t(state.locale, "clearAccount");
    elements.cancelDeleteProfileButton!.textContent = t(state.locale, "cancel");
  }
  const baseUrlCopy = t(state.locale, isWindowsUiTarget ? "baseUrlWindowsCopy" : "baseUrlCopy");
  elements.baseUrlDialogTitle.textContent = t(state.locale, "baseUrlTitle");
  elements.baseUrlDialogCopy.textContent = baseUrlCopy;
  if (hasModelMappingUi) {
    elements.modelMappingDialogTitle!.textContent = t(state.locale, "modelMappingTitle");
    elements.modelMappingDialogCopy!.textContent = t(state.locale, "modelMappingCopy");
    elements.modelMappingDialogHint!.textContent = t(state.locale, "modelMappingHint");
    elements.modelMappingSourceLabel!.textContent = t(state.locale, "modelMappingSourceLabel");
    elements.modelMappingTargetLabel!.textContent = t(state.locale, "modelMappingTargetLabel");
    elements.modelMappingFetchButton!.textContent = t(state.locale, "modelMappingReadModels");
    elements.addModelMappingRowButton!.textContent = t(state.locale, "modelMappingAddRow");
    elements.cancelModelMappingButton!.textContent = t(state.locale, "cancel");
    elements.submitModelMappingButton!.textContent = t(state.locale, "save");
  }
  elements.folderNameLabel.textContent = t(state.locale, "folderName");
  elements.addBaseUrlLabel.textContent = t(state.locale, "baseUrlLabel");
  elements.addBaseUrlInput.placeholder = t(state.locale, "baseUrlPlaceholder");
  elements.addBaseUrlCopy.textContent = baseUrlCopy;
  elements.renameFolderNameLabel.textContent = t(state.locale, "folderName");
  elements.baseUrlLabel.textContent = t(state.locale, "baseUrlLabel");
  elements.baseUrlInput.placeholder = t(state.locale, "baseUrlPlaceholder");
  elements.cancelAddProfileButton.textContent = t(state.locale, "cancel");
  elements.submitAddProfileButton.textContent = t(state.locale, "create");
  elements.cancelRenameProfileButton.textContent = t(state.locale, "cancel");
  elements.submitRenameProfileButton.textContent = t(state.locale, "rename");
  elements.cancelBaseUrlButton.textContent = t(state.locale, "cancel");
  elements.submitBaseUrlButton.textContent = t(state.locale, "save");
}
