const state = {
  page: 1,
  loading: false,
  currentProfile: null,
};

const profilesGrid = document.getElementById("profiles-grid");
const currentTitle = document.getElementById("current-title");
const currentPlan = document.getElementById("current-plan");
const currentQuotaPanel = document.getElementById("current-quota-panel");
const runtimeStatus = document.getElementById("runtime-status");
const runtimeAutosave = document.getElementById("runtime-autosave");
const previousPageButton = document.getElementById("previous-page-button");
const nextPageButton = document.getElementById("next-page-button");
const openCurrentFolderButton = document.getElementById("open-current-folder-button");
const openCodexButton = document.getElementById("open-codex-button");
const contactButton = document.getElementById("contact-button");
const addProfilesButton = document.getElementById("add-profiles-button");
const dialog = document.getElementById("add-profile-dialog");
const addProfileForm = document.getElementById("add-profile-form");
const cancelAddProfileButton = document.getElementById("cancel-add-profile-button");
const folderNameInput = document.getElementById("folder-name-input");
const dialogError = document.getElementById("dialog-error");
const toast = document.getElementById("toast");

function iconMarkup() {
  return `
    <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
      <path d="M7 17L17 7"></path>
      <path d="M9 7h8v8"></path>
    </svg>
  `;
}

function formatPercent(value) {
  return value == null ? "--" : `${value}%`;
}

function formatRefresh(value) {
  return value || "--";
}

function planLine(planName, daysLeft) {
  if (!planName && daysLeft == null) {
    return "Profile metadata not configured";
  }

  if (planName && daysLeft != null) {
    return `${planName} • ${daysLeft} days left`;
  }

  return planName || `Subscription • ${daysLeft} days left`;
}

function buildQuotaMarkup(quota, statusClass = "") {
  const windows = [
    { key: "five_hour", label: "5h allowance" },
    { key: "weekly", label: "Weekly allowance" },
  ];

  return windows
    .map(({ key, label }) => {
      const entry = quota?.[key] || {};
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

function showToast(message, isError = false) {
  toast.hidden = false;
  toast.textContent = message;
  toast.style.borderColor = isError ? "rgba(181, 82, 91, 0.55)" : "rgba(85, 112, 137, 0.42)";
  toast.style.color = isError ? "#ffd7d8" : "#f4efe8";
  window.clearTimeout(showToast.timeoutId);
  showToast.timeoutId = window.setTimeout(() => {
    toast.hidden = true;
  }, 3200);
}

async function request(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });

  const contentType = response.headers.get("content-type") || "";
  const payload = contentType.includes("application/json") ? await response.json() : null;

  if (!response.ok) {
    const message = payload?.message || `Request failed: ${response.status}`;
    throw new Error(message);
  }

  return payload;
}

function renderCurrentCard(dashboard) {
  const current = dashboard.current_card;
  if (!current) {
    currentTitle.textContent = "No active profile";
    currentPlan.textContent = "Switch a profile to start";
    openCurrentFolderButton.disabled = true;
    state.currentProfile = null;
    currentQuotaPanel.innerHTML = `<div class="empty-state">Quota data will appear after a profile is configured.</div>`;
    return;
  }

  state.currentProfile = current.folder_name;
  currentTitle.textContent = current.display_title;
  currentPlan.textContent = planLine(current.plan_name, current.subscription_days_left);
  openCurrentFolderButton.disabled = false;
  currentQuotaPanel.innerHTML = buildQuotaMarkup(dashboard.current_quota_card);
}

function renderRuntime(runtime) {
  runtimeStatus.textContent = runtime.codex_running ? "Codex running" : "Codex not running";
  runtimeStatus.classList.toggle("is-running", runtime.codex_running);
  runtimeStatus.classList.toggle("is-stopped", !runtime.codex_running);
  runtimeAutosave.textContent = runtime.last_autosave_at
    ? `Autosave ${runtime.last_autosave_at}`
    : "Autosave unavailable";
}

function renderProfiles(dashboard) {
  if (!dashboard.profiles.length) {
    profilesGrid.innerHTML = `<div class="empty-state">No profiles found. Use Add Profiles to create the first backup folder.</div>`;
    return;
  }

  profilesGrid.innerHTML = dashboard.profiles
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

  for (const button of profilesGrid.querySelectorAll("[data-switch-profile]")) {
    button.addEventListener("click", async () => {
      await switchProfile(button.dataset.switchProfile);
    });
  }
}

function renderPaging(paging) {
  previousPageButton.disabled = state.loading || !paging.has_previous;
  nextPageButton.disabled = state.loading || !paging.has_next;
}

async function loadDashboard(page = state.page) {
  state.loading = true;
  renderPaging({ has_previous: false, has_next: false });
  try {
    const dashboard = await request(`/api/dashboard?page=${page}`);
    state.loading = false;
    state.page = dashboard.paging.page;
    renderRuntime(dashboard.runtime);
    renderProfiles(dashboard);
    renderCurrentCard(dashboard);
    renderPaging(dashboard.paging);
  } catch (error) {
    state.loading = false;
    showToast(error.message, true);
    renderPaging({ has_previous: false, has_next: false });
  } finally {
    openCurrentFolderButton.disabled = !state.currentProfile;
  }
}

async function switchProfile(profile) {
  try {
    state.loading = true;
    await request("/api/profiles/switch", {
      method: "POST",
      body: JSON.stringify({ profile }),
    });
    showToast(`Switched to ${profile}`);
    await loadDashboard(state.page);
  } catch (error) {
    showToast(error.message, true);
  } finally {
    state.loading = false;
  }
}

async function openCurrentFolder() {
  if (!state.currentProfile) {
    return;
  }

  try {
    await request("/api/profiles/open-folder", {
      method: "POST",
      body: JSON.stringify({ profile: state.currentProfile }),
    });
    showToast("Opened profile folder");
  } catch (error) {
    showToast(error.message, true);
  }
}

async function openCodex() {
  try {
    await request("/api/app/open-codex", { method: "POST" });
    showToast("Opened Codex");
  } catch (error) {
    showToast(error.message, true);
  }
}

async function openContact() {
  try {
    await request("/api/contact/open", { method: "POST" });
    showToast("Opened repository");
  } catch (error) {
    showToast(error.message, true);
  }
}

function openAddProfileDialog() {
  dialogError.hidden = true;
  dialogError.textContent = "";
  addProfileForm.reset();
  dialog.showModal();
  folderNameInput.focus();
}

async function submitAddProfile(event) {
  event.preventDefault();
  dialogError.hidden = true;
  dialogError.textContent = "";

  const folderName = folderNameInput.value.trim();
  if (!folderName) {
    dialogError.hidden = false;
    dialogError.textContent = "Folder name is required.";
    return;
  }

  try {
    await request("/api/profiles/add", {
      method: "POST",
      body: JSON.stringify({ folder_name: folderName }),
    });
    dialog.close();
    showToast(`Created profile ${folderName}`);
    await loadDashboard(state.page);
  } catch (error) {
    dialogError.hidden = false;
    dialogError.textContent = error.message;
  }
}

previousPageButton.addEventListener("click", () => loadDashboard(state.page - 1));
nextPageButton.addEventListener("click", () => loadDashboard(state.page + 1));
openCurrentFolderButton.addEventListener("click", openCurrentFolder);
openCodexButton.addEventListener("click", openCodex);
contactButton.addEventListener("click", openContact);
addProfilesButton.addEventListener("click", openAddProfileDialog);
cancelAddProfileButton.addEventListener("click", () => dialog.close());
addProfileForm.addEventListener("submit", submitAddProfile);

loadDashboard();
