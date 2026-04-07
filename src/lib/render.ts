import type { DashboardViewModel, PagingInfo, QuotaSummary } from "./types";
import { t } from "./i18n";
import { state } from "./state";

function requiredElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!(element instanceof HTMLElement)) {
    throw new Error(`Missing required element: ${id}`);
  }
  return element as T;
}

export const elements = {
  profilesGrid: requiredElement<HTMLDivElement>("profiles-grid"),
  currentTitle: requiredElement<HTMLHeadingElement>("current-title"),
  currentPlan: requiredElement<HTMLParagraphElement>("current-plan"),
  currentQuotaPanel: requiredElement<HTMLDivElement>("current-quota-panel"),
  currentSessionLabel: requiredElement<HTMLParagraphElement>("current-session-label"),
  currentQuotaLabel: requiredElement<HTMLHeadingElement>("current-quota-label"),
  previousPageButton: requiredElement<HTMLButtonElement>("previous-page-button"),
  nextPageButton: requiredElement<HTMLButtonElement>("next-page-button"),
  currentLoginButton: requiredElement<HTMLButtonElement>("current-login-button"),
  openCurrentFolderButton: requiredElement<HTMLButtonElement>("open-current-folder-button"),
  openCodexButton: requiredElement<HTMLButtonElement>("open-codex-button"),
  contactButton: requiredElement<HTMLButtonElement>("contact-button"),
  addProfilesButton: requiredElement<HTMLButtonElement>("add-profiles-button"),
  dialog: document.getElementById("add-profile-dialog") as HTMLDialogElement,
  addProfileForm: requiredElement<HTMLFormElement>("add-profile-form"),
  cancelAddProfileButton: requiredElement<HTMLButtonElement>("cancel-add-profile-button"),
  submitAddProfileButton: requiredElement<HTMLButtonElement>("submit-add-profile-button"),
  dialogTitle: requiredElement<HTMLHeadingElement>("dialog-title"),
  dialogCopy: requiredElement<HTMLParagraphElement>("dialog-copy"),
  folderNameLabel: requiredElement<HTMLSpanElement>("folder-name-label"),
  folderNameInput: requiredElement<HTMLInputElement>("folder-name-input"),
  dialogError: requiredElement<HTMLParagraphElement>("dialog-error"),
  toast: requiredElement<HTMLDivElement>("toast"),
  localeToggleButton: requiredElement<HTMLButtonElement>("locale-toggle-button"),
};

function formatPercent(value: number | null): string {
  return value == null ? "--" : `${value}%`;
}

function formatRefresh(value: string | null): string {
  return value || "--";
}

function isFreePlan(planName: string | null | undefined): boolean {
  return (planName || "").trim().toLowerCase() === "free";
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function buildProfileTitleMarkup(folderName: string, displayTitle: string): string {
  const prefix = `${folderName} / `;
  let accountLabel = "--";

  if (displayTitle.startsWith(prefix)) {
    accountLabel = displayTitle.slice(prefix.length).trim() || "--";
  } else if (displayTitle.trim() && displayTitle.trim() !== folderName) {
    accountLabel = displayTitle.trim();
  }

  return `
    <span class="profile-title-folder">${escapeHtml(folderName)} /</span>
    <span class="profile-title-account">${escapeHtml(accountLabel)}</span>
  `;
}

export function planLine(planName: string | null, daysLeft: number | null): string {
  if (!planName && daysLeft == null) {
    return t(state.locale, "profileMetadataMissing");
  }

  if (planName && daysLeft != null) {
    return t(state.locale, "subscriptionDaysLeft", { plan: planName, days: daysLeft });
  }

  return planName || t(state.locale, "subscriptionFallback", { days: daysLeft ?? "--" });
}

export function buildQuotaMarkup(
  quota: QuotaSummary | null | undefined,
  statusClass = "",
  planName: string | null | undefined = null,
  hasAccountIdentity = true,
): string {
  const windows = [
    { key: "five_hour", label: t(state.locale, "fiveHourAllowance") },
    { key: "weekly", label: t(state.locale, "weeklyAllowance") },
  ] as const;
  const freePlan = isFreePlan(planName);
  const accountUnavailable = !hasAccountIdentity;

  return windows
    .map(({ key, label }) => {
      const entry = quota?.[key] ?? { remaining_percent: null, refresh_at: null };
      const unavailable = accountUnavailable || (freePlan && key === "five_hour");
      const percent = unavailable ? 0 : (entry.remaining_percent ?? 0);
      const quotaClass = unavailable ? "is-unavailable" : "";
      return `
        <section class="quota-group ${statusClass} ${quotaClass}">
          <div class="quota-row">
            <span class="quota-title">${label}</span>
            <span class="quota-value">${formatPercent(unavailable ? null : entry.remaining_percent)}</span>
          </div>
          <div class="quota-track">
            <div class="quota-fill" style="width: ${percent}%;"></div>
          </div>
          <div class="quota-refresh">${t(state.locale, "refresh", { value: formatRefresh(unavailable ? null : entry.refresh_at) })}</div>
        </section>
      `;
    })
    .join("");
}

export function showToast(message: string, isError = false): void {
  elements.toast.hidden = false;
  elements.toast.textContent = message;
  elements.toast.style.borderColor = isError ? "rgba(181, 82, 91, 0.55)" : "rgba(85, 112, 137, 0.42)";
  elements.toast.style.color = isError ? "#ffd7d8" : "#f4efe8";
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
  elements.currentTitle.textContent = current.display_title;
  elements.currentPlan.textContent = planLine(current.plan_name, current.subscription_days_left);
  elements.currentLoginButton.disabled = state.loading;
  elements.openCurrentFolderButton.disabled = false;
  elements.currentQuotaPanel.innerHTML = buildQuotaMarkup(
    dashboard.current_quota_card,
    "",
    current.plan_name,
    current.has_account_identity,
  );
}

export function renderProfiles(dashboard: DashboardViewModel, onSwitch: (profile: string) => void): void {
  if (!dashboard.profiles.length) {
    elements.profilesGrid.innerHTML =
      `<div class="empty-state">${t(state.locale, "profilesEmpty")}</div>`;
    return;
  }

  elements.profilesGrid.innerHTML = dashboard.profiles
    .map((profile) => {
      const disabled = !profile.auth_present || state.loading || profile.status === "current";
      const plan = planLine(profile.plan_name, profile.subscription_days_left);
      return `
        <article class="profile-card status-${profile.status}">
          <header class="profile-card-header">
            <div>
              <h3 class="profile-title">${buildProfileTitleMarkup(profile.folder_name, profile.display_title)}</h3>
              <p class="profile-plan">${plan}</p>
            </div>
            <button
              class="switch-icon-button"
              type="button"
              title="${disabled ? t(state.locale, "profileSwitchDisabled") : t(state.locale, "profileSwitchReady")}"
              data-switch-profile="${profile.folder_name}"
              ${disabled ? "disabled" : ""}
            >
              ${t(state.locale, "switch")}
            </button>
          </header>
          ${buildQuotaMarkup(
            profile.quota,
            `status-${profile.status}`,
            profile.plan_name,
            profile.has_account_identity,
          )}
        </article>
      `;
    })
    .join("");

  for (const button of elements.profilesGrid.querySelectorAll<HTMLButtonElement>("[data-switch-profile]")) {
    button.addEventListener("click", () => {
      const profile = button.dataset.switchProfile;
      if (profile) {
        void onSwitch(profile);
      }
    });
  }
}

export function renderPaging(paging: Pick<PagingInfo, "has_previous" | "has_next">): void {
  elements.previousPageButton.disabled = state.loading || !paging.has_previous;
  elements.nextPageButton.disabled = state.loading || !paging.has_next;
}

export function applyLocale(): void {
  document.documentElement.lang = state.locale;
  document.title = t(state.locale, "appTitle");

  elements.currentSessionLabel.textContent = t(state.locale, "currentSession");
  elements.currentQuotaLabel.textContent = t(state.locale, "currentQuota");
  elements.currentLoginButton.textContent = t(state.locale, "login");
  elements.openCurrentFolderButton.textContent = t(state.locale, "openFolder");
  elements.addProfilesButton.textContent = t(state.locale, "addProfiles");
  elements.openCodexButton.textContent = t(state.locale, "openCodex");
  elements.contactButton.textContent = t(state.locale, "contactUs");
  elements.previousPageButton.textContent = t(state.locale, "previous");
  elements.nextPageButton.textContent = t(state.locale, "next");
  elements.dialogTitle.textContent = t(state.locale, "addProfileTitle");
  elements.dialogCopy.innerHTML = t(state.locale, "addProfileCopy")
    .replace("auth.json", "<code>auth.json</code>")
    .replace("profile.json", "<code>profile.json</code>");
  elements.folderNameLabel.textContent = t(state.locale, "folderName");
  elements.cancelAddProfileButton.textContent = t(state.locale, "cancel");
  elements.submitAddProfileButton.textContent = t(state.locale, "create");
  elements.localeToggleButton.textContent =
    state.locale === "en" ? t(state.locale, "languageChinese") : t(state.locale, "languageEnglish");
}
