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

function optionalElement<T extends HTMLElement>(id: string): T | null {
  const element = document.getElementById(id);
  return element instanceof HTMLElement ? (element as T) : null;
}

const hasDeleteProfileUi = document.getElementById("delete-profile-dialog") instanceof HTMLDialogElement;
const hasNavUi = document.getElementById("nav-dashboard") instanceof HTMLElement;

export const elements = {
  // Navigation (optional for macOS compatibility)
  navDashboard: hasNavUi ? requiredElement<HTMLButtonElement>("nav-dashboard") : null as unknown as HTMLButtonElement,
  navProxy: hasNavUi ? requiredElement<HTMLButtonElement>("nav-proxy") : null as unknown as HTMLButtonElement,
  navSettings: hasNavUi ? requiredElement<HTMLButtonElement>("nav-settings") : null as unknown as HTMLButtonElement,
  navGuide: hasNavUi ? requiredElement<HTMLButtonElement>("nav-guide") : null as unknown as HTMLButtonElement,

  // Page sections (optional for macOS compatibility)
  pageDashboard: hasNavUi ? requiredElement<HTMLElement>("page-dashboard") : null as unknown as HTMLElement,
  pageProxy: hasNavUi ? requiredElement<HTMLElement>("page-proxy") : null as unknown as HTMLElement,
  pageSettings: hasNavUi ? requiredElement<HTMLElement>("page-settings") : null as unknown as HTMLElement,
  pageGuide: hasNavUi ? requiredElement<HTMLElement>("page-guide") : null as unknown as HTMLElement,

  // Dashboard
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

  // Proxy
  proxyHeading: requiredElement<HTMLHeadingElement>("proxy-heading"),
  proxyStatusText: requiredElement<HTMLSpanElement>("proxy-status-text"),
  proxyStatusDot: requiredElement<HTMLSpanElement>("proxy-status-indicator"),
  proxyPortInput: requiredElement<HTMLInputElement>("proxy-port-input"),
  proxyToggleButton: requiredElement<HTMLButtonElement>("proxy-toggle-button"),
  proxyLogLabel: requiredElement<HTMLSpanElement>("proxy-log-label"),
  proxyClearLogs: requiredElement<HTMLButtonElement>("proxy-clear-logs"),
  proxyLogs: requiredElement<HTMLDivElement>("proxy-logs"),
  statTotal: requiredElement<HTMLSpanElement>("stat-total"),
  statSuccess: requiredElement<HTMLSpanElement>("stat-success"),
  statFailed: requiredElement<HTMLSpanElement>("stat-failed"),
  statToday: requiredElement<HTMLSpanElement>("stat-today"),
  statTotalLabel: requiredElement<HTMLSpanElement>("stat-total-label"),
  statSuccessLabel: requiredElement<HTMLSpanElement>("stat-success-label"),
  statFailedLabel: requiredElement<HTMLSpanElement>("stat-failed-label"),
  statTodayLabel: requiredElement<HTMLSpanElement>("stat-today-label"),
  proxyPortLabel: requiredElement<HTMLSpanElement>("proxy-port-label"),

  // Settings
  settingsHeading: requiredElement<HTMLHeadingElement>("settings-heading"),
  appearanceHeading: requiredElement<HTMLHeadingElement>("appearance-heading"),
  proxySettingsHeading: requiredElement<HTMLHeadingElement>("proxy-settings-heading"),
  backupHeading: requiredElement<HTMLHeadingElement>("backup-heading"),
  aboutHeading: requiredElement<HTMLHeadingElement>("about-heading"),
  themeLabel: requiredElement<HTMLSpanElement>("theme-label"),
  languageLabel: requiredElement<HTMLSpanElement>("language-label"),
  proxyPortSettingLabel: requiredElement<HTMLSpanElement>("proxy-port-setting-label"),
  autoStartLabel: requiredElement<HTMLSpanElement>("auto-start-label"),
  themeLight: requiredElement<HTMLButtonElement>("theme-light"),
  themeDark: requiredElement<HTMLButtonElement>("theme-dark"),
  themeSystem: requiredElement<HTMLButtonElement>("theme-system"),
  settingsLocaleEn: requiredElement<HTMLButtonElement>("settings-locale-en"),
  settingsLocaleZh: requiredElement<HTMLButtonElement>("settings-locale-zh"),
  settingsProxyPort: requiredElement<HTMLInputElement>("settings-proxy-port"),
  autoStartToggle: requiredElement<HTMLInputElement>("auto-start-toggle"),
  exportConfig: requiredElement<HTMLButtonElement>("export-config"),
  importConfig: requiredElement<HTMLButtonElement>("import-config"),
  checkUpdate: requiredElement<HTMLButtonElement>("check-update"),

  // Quick Actions
  quickActionsHeading: requiredElement<HTMLHeadingElement>("quick-actions-heading"),
  quickProxy: requiredElement<HTMLButtonElement>("quick-proxy"),
  quickSettings: requiredElement<HTMLButtonElement>("quick-settings"),
  quickGuide: requiredElement<HTMLButtonElement>("quick-guide"),
  quickRefreshAll: requiredElement<HTMLButtonElement>("quick-refresh-all"),

  // Guide
  guideHeading: requiredElement<HTMLHeadingElement>("guide-heading"),
  guideStep1Title: requiredElement<HTMLHeadingElement>("guide-step-1-title"),
  guideStep1Desc: requiredElement<HTMLParagraphElement>("guide-step-1-desc"),
  guideStep2Title: requiredElement<HTMLHeadingElement>("guide-step-2-title"),
  guideStep2Desc: requiredElement<HTMLParagraphElement>("guide-step-2-desc"),
  guideStep3Title: requiredElement<HTMLHeadingElement>("guide-step-3-title"),
  guideStep3Desc: requiredElement<HTMLParagraphElement>("guide-step-3-desc"),
  guideAddProfile: requiredElement<HTMLButtonElement>("guide-add-profile"),
  guideBack: requiredElement<HTMLButtonElement>("guide-back"),

  // Dialogs
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
  bindProfileButtons("data-switch-profile", onSwitch);
}

export function renderPaging(
  paging: Pick<PagingInfo, "has_previous" | "has_next" | "page" | "total_pages">,
): void {
  elements.previousPageButton.disabled = state.loading || !paging.has_previous;
  elements.nextPageButton.disabled = state.loading || !paging.has_next;
  elements.pageIndicator.textContent = `${paging.page} / ${paging.total_pages}`;
}

export function renderNavigation(): void {
  if (!hasNavUi) {
    return;
  }

  elements.navDashboard.classList.toggle("is-active", state.currentPage === "dashboard");
  elements.navProxy.classList.toggle("is-active", state.currentPage === "proxy");
  elements.navSettings.classList.toggle("is-active", state.currentPage === "settings");
  elements.navGuide.classList.toggle("is-active", state.currentPage === "guide");

  elements.pageDashboard.hidden = state.currentPage !== "dashboard";
  elements.pageProxy.hidden = state.currentPage !== "proxy";
  elements.pageSettings.hidden = state.currentPage !== "settings";
  elements.pageGuide.hidden = state.currentPage !== "guide";
}

export function renderProxyPage(): void {
  elements.proxyHeading.textContent = t(state.locale, "proxyHeading");
  elements.proxyStatusText.textContent = state.proxyRunning
    ? t(state.locale, "proxyRunning")
    : t(state.locale, "proxyStopped");
  elements.proxyStatusDot.classList.toggle("is-running", state.proxyRunning);
  elements.proxyToggleButton.textContent = state.proxyRunning
    ? t(state.locale, "proxyStop")
    : t(state.locale, "proxyStart");
  elements.proxyPortLabel.textContent = t(state.locale, "proxyPortLabel");
  elements.proxyLogLabel.textContent = t(state.locale, "proxyLogs");
  elements.proxyClearLogs.textContent = t(state.locale, "clear");
  elements.statTotalLabel.textContent = t(state.locale, "statTotal");
  elements.statSuccessLabel.textContent = t(state.locale, "statSuccess");
  elements.statFailedLabel.textContent = t(state.locale, "statFailed");
  elements.statTodayLabel.textContent = t(state.locale, "statToday");

  elements.proxyLogs.innerHTML = state.proxyLogs
    .map((log) => `<div class="proxy-log-line">${escapeHtml(log)}</div>`)
    .join("");
  if (state.proxyLogs.length > 0) {
    elements.proxyLogs.scrollTop = elements.proxyLogs.scrollHeight;
  }
}

export function renderSettingsPage(): void {
  elements.settingsHeading.textContent = t(state.locale, "settingsHeading");
  elements.appearanceHeading.textContent = t(state.locale, "appearanceHeading");
  elements.proxySettingsHeading.textContent = t(state.locale, "proxySettingsHeading");
  elements.backupHeading.textContent = t(state.locale, "backupHeading");
  elements.aboutHeading.textContent = t(state.locale, "aboutHeading");
  elements.themeLabel.textContent = t(state.locale, "themeLabel");
  elements.languageLabel.textContent = t(state.locale, "languageLabel");
  elements.proxyPortSettingLabel.textContent = t(state.locale, "proxyPortLabel");
  elements.autoStartLabel.textContent = t(state.locale, "autoStartLabel");

  elements.themeLight.textContent = t(state.locale, "themeLight");
  elements.themeDark.textContent = t(state.locale, "themeDark");
  elements.themeSystem.textContent = t(state.locale, "themeSystem");
  elements.themeLight.classList.toggle("is-active", state.theme === "light");
  elements.themeDark.classList.toggle("is-active", state.theme === "dark");
  elements.themeSystem.classList.toggle("is-active", state.theme === "system");

  elements.settingsLocaleEn.textContent = t(state.locale, "languageEnglish");
  elements.settingsLocaleZh.textContent = t(state.locale, "languageChinese");
  elements.settingsLocaleEn.classList.toggle("is-active", state.locale === "en");
  elements.settingsLocaleZh.classList.toggle("is-active", state.locale === "zh-CN");

  elements.settingsProxyPort.value = String(state.proxyPort);
  elements.exportConfig.textContent = t(state.locale, "exportConfig");
  elements.importConfig.textContent = t(state.locale, "importConfig");
  elements.checkUpdate.textContent = t(state.locale, "checkUpdate");
}

export function renderGuidePage(): void {
  elements.guideHeading.textContent = t(state.locale, "guideHeading");
  elements.guideStep1Title.textContent = t(state.locale, "guideStep1Title");
  elements.guideStep1Desc.textContent = t(state.locale, "guideStep1Desc");
  elements.guideStep2Title.textContent = t(state.locale, "guideStep2Title");
  elements.guideStep2Desc.textContent = t(state.locale, "guideStep2Desc");
  elements.guideStep3Title.textContent = t(state.locale, "guideStep3Title");
  elements.guideStep3Desc.textContent = t(state.locale, "guideStep3Desc");
  elements.guideAddProfile.textContent = t(state.locale, "guideAddProfile");
  elements.guideBack.textContent = t(state.locale, "guideBack");
}

export function applyLocale(): void {
  document.documentElement.lang = state.locale;
  document.title = t(state.locale, "appTitle");

  elements.profilesHeading.textContent = t(state.locale, "profilesHeading");
  elements.currentSectionHeading.textContent = t(state.locale, "currentSession");
  elements.controlDeckHeading.textContent = t(state.locale, "controlDeck");
  elements.quickActionsHeading.textContent = t(state.locale, "quickActionsHeading");
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

  renderNavigation();
  renderProxyPage();
  renderSettingsPage();
  renderGuidePage();
}
