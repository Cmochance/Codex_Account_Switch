import type { DashboardResponse, PagingInfo, QuotaSummary, RuntimeSummary } from "./types";
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
  runtimeStatus: requiredElement<HTMLSpanElement>("runtime-status"),
  runtimeAutosave: requiredElement<HTMLSpanElement>("runtime-autosave"),
  previousPageButton: requiredElement<HTMLButtonElement>("previous-page-button"),
  nextPageButton: requiredElement<HTMLButtonElement>("next-page-button"),
  openCurrentFolderButton: requiredElement<HTMLButtonElement>("open-current-folder-button"),
  openCodexButton: requiredElement<HTMLButtonElement>("open-codex-button"),
  contactButton: requiredElement<HTMLButtonElement>("contact-button"),
  addProfilesButton: requiredElement<HTMLButtonElement>("add-profiles-button"),
  dialog: document.getElementById("add-profile-dialog") as HTMLDialogElement,
  addProfileForm: requiredElement<HTMLFormElement>("add-profile-form"),
  cancelAddProfileButton: requiredElement<HTMLButtonElement>("cancel-add-profile-button"),
  folderNameInput: requiredElement<HTMLInputElement>("folder-name-input"),
  dialogError: requiredElement<HTMLParagraphElement>("dialog-error"),
  toast: requiredElement<HTMLDivElement>("toast"),
};

export function iconMarkup(): string {
  return `
    <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
      <path d="M7 17L17 7"></path>
      <path d="M9 7h8v8"></path>
    </svg>
  `;
}

function formatPercent(value: number | null): string {
  return value == null ? "--" : `${value}%`;
}

function formatRefresh(value: string | null): string {
  return value || "--";
}

export function planLine(planName: string | null, daysLeft: number | null): string {
  if (!planName && daysLeft == null) {
    return "Profile metadata not configured";
  }

  if (planName && daysLeft != null) {
    return `${planName} • ${daysLeft} days left`;
  }

  return planName || `Subscription • ${daysLeft} days left`;
}

export function buildQuotaMarkup(quota: QuotaSummary | null | undefined, statusClass = ""): string {
  const windows = [
    { key: "five_hour", label: "5h allowance" },
    { key: "weekly", label: "Weekly allowance" },
  ] as const;

  return windows
    .map(({ key, label }) => {
      const entry = quota?.[key] ?? { remaining_percent: null, refresh_at: null };
      const percent = entry.remaining_percent ?? 0;
      return `
        <section class="quota-group ${statusClass}">
          <div class="quota-row">
            <span class="quota-title">${label}</span>
            <span class="quota-value">${formatPercent(entry.remaining_percent)}</span>
          </div>
          <div class="quota-track">
            <div class="quota-fill" style="width: ${percent}%;"></div>
          </div>
          <div class="quota-refresh">Refresh ${formatRefresh(entry.refresh_at)}</div>
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

export function renderCurrentCard(dashboard: DashboardResponse): void {
  const current = dashboard.current_card;
  if (!current) {
    elements.currentTitle.textContent = "No active profile";
    elements.currentPlan.textContent = "Switch a profile to start";
    elements.openCurrentFolderButton.disabled = true;
    state.currentProfile = null;
    elements.currentQuotaPanel.innerHTML =
      '<div class="empty-state">Quota data will appear after a profile is configured.</div>';
    return;
  }

  state.currentProfile = current.folder_name;
  elements.currentTitle.textContent = current.display_title;
  elements.currentPlan.textContent = planLine(current.plan_name, current.subscription_days_left);
  elements.openCurrentFolderButton.disabled = false;
  elements.currentQuotaPanel.innerHTML = buildQuotaMarkup(dashboard.current_quota_card);
}

export function renderRuntime(runtime: RuntimeSummary): void {
  elements.runtimeStatus.textContent = runtime.codex_running ? "Codex running" : "Codex not running";
  elements.runtimeStatus.classList.toggle("is-running", runtime.codex_running);
  elements.runtimeStatus.classList.toggle("is-stopped", !runtime.codex_running);
  elements.runtimeAutosave.textContent = runtime.last_autosave_at
    ? `Autosave ${runtime.last_autosave_at}`
    : "Autosave unavailable";
}

export function renderProfiles(dashboard: DashboardResponse, onSwitch: (profile: string) => void): void {
  if (!dashboard.profiles.length) {
    elements.profilesGrid.innerHTML =
      '<div class="empty-state">No profiles found. Use Add Profiles to create the first backup folder.</div>';
    return;
  }

  elements.profilesGrid.innerHTML = dashboard.profiles
    .map((profile) => {
      const disabled = !profile.auth_present || state.loading;
      const plan = planLine(profile.plan_name, profile.subscription_days_left);
      return `
        <article class="profile-card status-${profile.status}">
          <header class="profile-card-header">
            <div>
              <h3 class="profile-title">${profile.display_title}</h3>
              <p class="profile-plan">${plan}</p>
            </div>
            <button
              class="switch-icon-button"
              type="button"
              title="${disabled ? "Profile is not switchable" : "Switch to this profile"}"
              data-switch-profile="${profile.folder_name}"
              ${disabled ? "disabled" : ""}
            >
              ${iconMarkup()}
            </button>
          </header>
          ${buildQuotaMarkup(profile.quota, `status-${profile.status}`)}
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
