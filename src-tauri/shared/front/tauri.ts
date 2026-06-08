import { invoke } from "@tauri-apps/api/core";

import type {
  ActionResponse,
  CodexCliRedetectResult,
  CodexCliStatus,
  CommandError,
  CurrentCard,
  CurrentQuotaResponse,
  ProfileCard,
  ProfilesSnapshotResponse,
  QuotaSummary,
  SwitchResponse,
  UpdateCheckResponse,
} from "@front-shared/types";

type NativeCommandError = Error & {
  code?: string;
};

type RuntimeWindow = typeof globalThis & {
  __TAURI_INTERNALS__?: unknown;
  __TAURI__?: unknown;
};

const hasTauriRuntime = Boolean(
  (globalThis as RuntimeWindow).__TAURI_INTERNALS__ || (globalThis as RuntimeWindow).__TAURI__,
);

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function quota(
  fiveHourPercent: number | null,
  fiveHourRefresh: string | null,
  weeklyPercent: number | null,
  weeklyRefresh: string | null,
): QuotaSummary {
  return {
    five_hour: {
      remaining_percent: fiveHourPercent,
      refresh_at: fiveHourRefresh,
      reset_at_timestamp: null,
    },
    weekly: {
      remaining_percent: weeklyPercent,
      refresh_at: weeklyRefresh,
      reset_at_timestamp: null,
    },
  };
}

const previewProfiles: ProfileCard[] = [
  {
    folder_name: "workspace-alpha",
    display_title: "Workspace Alpha",
    account_label: "Workspace Alpha",
    status: "available",
    auth_present: true,
    has_account_identity: true,
    plan_name: "Pro plan",
    subscription_days_left: 18,
    openai_base_url: null,
    quota: quota(84, "3小时后刷新", 61, "2天4小时后刷新"),
    last_plan_check_ms: Date.now() - 30 * 60 * 1000,
  },
  {
    folder_name: "workspace-beta",
    display_title: "Workspace Beta",
    account_label: "Workspace Beta",
    status: "available",
    auth_present: true,
    has_account_identity: true,
    plan_name: "Plus plan",
    subscription_days_left: 12,
    openai_base_url: null,
    quota: quota(58, "1小时后刷新", 42, "4天6小时后刷新"),
    last_plan_check_ms: Date.now() - 4 * 60 * 60 * 1000,
  },
  {
    folder_name: "workspace-gamma",
    display_title: "Metadata Not Configured",
    account_label: null,
    status: "missing_auth",
    auth_present: false,
    has_account_identity: false,
    plan_name: null,
    subscription_days_left: null,
    openai_base_url: null,
    quota: quota(null, "6小时后刷新", null, "7天2小时后刷新"),
    last_plan_check_ms: null,
  },
  {
    folder_name: "workspace-delta",
    display_title: "Workspace Delta",
    account_label: "Workspace Delta",
    status: "available",
    auth_present: true,
    has_account_identity: true,
    plan_name: "Custom endpoint",
    subscription_days_left: 31,
    openai_base_url: "https://example.com/v1",
    quota: quota(73, "2小时后刷新", 88, "1天9小时后刷新"),
    last_plan_check_ms: Date.now() - 8 * 24 * 60 * 60 * 1000,
  },
  {
    folder_name: "workspace-epsilon",
    display_title: "Workspace Epsilon",
    account_label: "Workspace Epsilon",
    status: "available",
    auth_present: true,
    has_account_identity: true,
    plan_name: "Pro plan",
    subscription_days_left: 18,
    openai_base_url: null,
    quota: quota(84, "3小时后刷新", 61, "2天4小时后刷新"),
    last_plan_check_ms: Date.now() - 2 * 24 * 60 * 60 * 1000,
  },
  {
    folder_name: "workspace-zeta",
    display_title: "Workspace Zeta",
    account_label: "Workspace Zeta",
    status: "available",
    auth_present: true,
    has_account_identity: true,
    plan_name: "Custom endpoint",
    subscription_days_left: 31,
    openai_base_url: "https://example.com/v1",
    quota: quota(73, "2小时后刷新", 88, "1天9小时后刷新"),
    last_plan_check_ms: null,
  },
];

let previewCurrentCard: CurrentCard = {
  folder_name: "workspace-alpha",
  display_title: "Workspace Alpha",
  account_label: "Workspace Alpha",
  has_account_identity: true,
  plan_name: "Pro plan",
  subscription_days_left: 18,
  profile_folder_path: "/mock/workspace-alpha",
  last_plan_check_ms: Date.now() - 30 * 60 * 1000,
};

let previewCurrentQuota: QuotaSummary = quota(84, "3小时后刷新", 61, "2天4小时后刷新");

let previewSnapshot: ProfilesSnapshotResponse = {
  page_size: 8,
  profiles: clone(previewProfiles),
  current_card: clone(previewCurrentCard),
  current_quota_card: clone(previewCurrentQuota),
  unmanaged_live_account: null,
};

function mockAction(message: string, path: string | null = null): Promise<ActionResponse> {
  return Promise.resolve({
    ok: true,
    message,
    path,
  });
}

function refreshPreviewSnapshot(): void {
  previewSnapshot = {
    page_size: 8,
    profiles: clone(previewSnapshot.profiles),
    current_card: clone(previewCurrentCard),
    current_quota_card: clone(previewCurrentQuota),
    unmanaged_live_account: null,
  };
}

function toError(error: unknown): Error {
  if (typeof error === "string") {
    return new Error(error);
  }

  if (error && typeof error === "object") {
    const payload = error as CommandError;
    if (payload.message || payload.error_code) {
      const nextError = new Error(payload.message || "Unknown native command error.") as NativeCommandError;
      nextError.code = payload.error_code;
      return nextError;
    }
  }

  return new Error("Unknown native command error.");
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!hasTauriRuntime) {
    switch (command) {
      case "get_profiles_snapshot":
        return clone(previewSnapshot) as T;
      case "get_current_live_quota":
      case "refresh_active_profile_quota_silent":
        return {
          profile: previewCurrentCard.folder_name,
          quota: clone(previewCurrentQuota),
        } as T;
      case "refresh_all_oauth_profile_plans_silent":
        return 0 as T;
      case "switch_profile": {
        const profile = (args?.payload as { profile?: string } | undefined)?.profile ?? previewCurrentCard.folder_name;
        const next = previewSnapshot.profiles.find((entry) => entry.folder_name === profile);
        if (next) {
          previewCurrentCard = {
            folder_name: next.folder_name,
            display_title: next.display_title,
            account_label: next.account_label,
            has_account_identity: next.has_account_identity,
            plan_name: next.plan_name,
            subscription_days_left: next.subscription_days_left,
            profile_folder_path: `C:/mock/${next.folder_name}`,
            last_plan_check_ms: next.last_plan_check_ms,
          };
          previewCurrentQuota = clone(next.quota);
          refreshPreviewSnapshot();
        }
        return {
          ok: true,
          profile,
          message: "Switched in preview mode",
          warnings: [],
        } as T;
      }
      case "rename_profile": {
        const payload = args?.payload as { profile?: string; new_folder_name?: string } | undefined;
        if (payload?.profile && payload.new_folder_name) {
          const sourceProfile = payload.profile;
          const nextFolderName = payload.new_folder_name;
          previewSnapshot.profiles = previewSnapshot.profiles.map((entry) =>
            entry.folder_name === sourceProfile
              ? {
                  ...entry,
                  folder_name: nextFolderName,
                  display_title: nextFolderName,
                  account_label: nextFolderName,
                }
              : entry,
          );
          if (previewCurrentCard.folder_name === sourceProfile) {
            previewCurrentCard = {
              ...previewCurrentCard,
              folder_name: nextFolderName,
              display_title: nextFolderName,
              account_label: nextFolderName,
            };
          }
          refreshPreviewSnapshot();
        }
        return mockAction("Renamed in preview mode") as Promise<T>;
      }
      case "add_profile": {
        const payload = args?.payload as { folder_name?: string; openai_base_url?: string | null } | undefined;
        if (payload?.folder_name) {
          previewSnapshot.profiles.push({
            folder_name: payload.folder_name,
            display_title: payload.folder_name,
            account_label: payload.folder_name,
            status: "available",
            auth_present: true,
            has_account_identity: true,
            plan_name: "Pro plan",
            subscription_days_left: 30,
            openai_base_url: payload.openai_base_url ?? null,
            quota: quota(52, "5小时后刷新", 67, "3天后刷新"),
            last_plan_check_ms: Date.now(),
          });
          refreshPreviewSnapshot();
        }
        return mockAction("Added in preview mode") as Promise<T>;
      }
      case "update_profile_base_url": {
        const payload = args?.payload as { profile?: string; openai_base_url?: string } | undefined;
        if (payload?.profile) {
          previewSnapshot.profiles = previewSnapshot.profiles.map((entry) =>
            entry.folder_name === payload.profile
              ? { ...entry, openai_base_url: payload.openai_base_url ?? null }
              : entry,
          );
          refreshPreviewSnapshot();
        }
        return mockAction("Base URL updated in preview mode") as Promise<T>;
      }
      case "delete_profile": {
        const payload = args?.payload as { profile?: string } | undefined;
        if (payload?.profile) {
          previewSnapshot.profiles = previewSnapshot.profiles.filter(
            (entry) => entry.folder_name !== payload.profile,
          );
          refreshPreviewSnapshot();
        }
        return mockAction("Deleted in preview mode") as Promise<T>;
      }
      case "clear_profile_account": {
        const payload = args?.payload as { profile?: string } | undefined;
        if (payload?.profile) {
          previewSnapshot.profiles = previewSnapshot.profiles.map((entry) =>
            entry.folder_name === payload.profile
              ? {
                  ...entry,
                  account_label: null,
                  display_title: entry.folder_name,
                  status: "missing_auth",
                  auth_present: false,
                  has_account_identity: false,
                  plan_name: null,
                  subscription_days_left: null,
                  openai_base_url: null,
                  quota: quota(null, null, null, null),
                }
              : entry,
          );
          refreshPreviewSnapshot();
        }
        return mockAction("Cleared in preview mode") as Promise<T>;
      }
      case "check_update":
        return Promise.resolve({
          ok: true,
          current_version: "1.5.0",
          latest_version: "1.5.0",
          has_update: false,
          release_url: "https://github.com/Cmochance/Codex_Account_Switch/releases",
          notes: null,
          checked_url: "preview",
        }) as Promise<T>;
      case "open_url":
        return mockAction("Opened URL in preview mode", "preview:url") as Promise<T>;
      case "get_codex_cli_status":
      case "set_codex_cli_path":
      case "clear_codex_cli_path":
        return Promise.resolve({
          resolved_path: "/preview/codex",
          source: command === "set_codex_cli_path" ? "user_override" : "discovery",
          suggested_paths: ["/preview/codex", "/preview/usr/local/bin/codex"],
        }) as Promise<T>;
      case "redetect_codex_cli_path":
        return Promise.resolve({
          candidates: [{ path: "/preview/codex", version: "codex-cli 0.133.0" }],
          status: {
            resolved_path: "/preview/codex",
            source: "user_override",
            suggested_paths: ["/preview/codex", "/preview/usr/local/bin/codex"],
          },
        }) as Promise<T>;
      case "cancel_codex_login":
        return Promise.resolve(true) as Promise<T>;
      case "open_profile_folder":
      case "open_codex":
      case "login_current_profile":
      case "login_profile":
      case "refresh_profile":
      case "open_releases":
      case "open_contact":
      case "open_xiaohongshu":
        return mockAction(`${command} completed in preview mode`) as Promise<T>;
      default:
        return Promise.reject(new Error(`Unsupported preview command: ${command}`));
    }
  }

  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toError(error);
  }
}

export function getProfilesSnapshot(): Promise<ProfilesSnapshotResponse> {
  return invokeCommand<ProfilesSnapshotResponse>("get_profiles_snapshot");
}

export function getCurrentLiveQuota(): Promise<CurrentQuotaResponse> {
  return invokeCommand<CurrentQuotaResponse>("get_current_live_quota");
}

export function refreshActiveProfileQuotaSilent(): Promise<CurrentQuotaResponse> {
  return invokeCommand<CurrentQuotaResponse>("refresh_active_profile_quota_silent");
}

export function refreshAllOauthProfilePlansSilent(): Promise<number> {
  return invokeCommand<number>("refresh_all_oauth_profile_plans_silent");
}

export function switchProfile(profile: string): Promise<SwitchResponse> {
  return invokeCommand<SwitchResponse>("switch_profile", { payload: { profile } });
}

export function openProfileFolder(profile: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_profile_folder", { payload: { profile } });
}

export function addProfile(folderName: string, openaiBaseUrl: string | null): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("add_profile", {
    payload: {
      folder_name: folderName,
      openai_base_url: openaiBaseUrl,
    },
  });
}

export function openCodex(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_codex");
}

export function loginCurrentProfile(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("login_current_profile");
}

export function loginProfile(profile: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("login_profile", { payload: { profile } });
}

export function refreshProfile(profile: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("refresh_profile", { payload: { profile } });
}

export function renameProfile(profile: string, newFolderName: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("rename_profile", {
    payload: { profile, new_folder_name: newFolderName },
  });
}

export function updateProfileBaseUrl(profile: string, openaiBaseUrl: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("update_profile_base_url", {
    payload: { profile, openai_base_url: openaiBaseUrl },
  });
}

export function deleteProfile(profile: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("delete_profile", { payload: { profile } });
}

export function clearProfileAccount(profile: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("clear_profile_account", { payload: { profile } });
}

export function openContact(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_contact");
}

export function openReleases(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_releases");
}

export function openUrl(url: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_url", {
    payload: { url },
  });
}

export function checkUpdate(updateUrl: string): Promise<UpdateCheckResponse> {
  return invokeCommand<UpdateCheckResponse>("check_update", {
    payload: { update_url: updateUrl },
  });
}

export function openXiaohongshu(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_xiaohongshu");
}

export function getCodexCliStatus(): Promise<CodexCliStatus> {
  return invokeCommand<CodexCliStatus>("get_codex_cli_status");
}

export function setCodexCliPath(path: string): Promise<CodexCliStatus> {
  return invokeCommand<CodexCliStatus>("set_codex_cli_path", {
    payload: { path },
  });
}

export function clearCodexCliPath(): Promise<CodexCliStatus> {
  return invokeCommand<CodexCliStatus>("clear_codex_cli_path");
}

export function redetectCodexCliPath(): Promise<CodexCliRedetectResult> {
  return invokeCommand<CodexCliRedetectResult>("redetect_codex_cli_path");
}

export function cancelCodexLogin(): Promise<boolean> {
  return invokeCommand<boolean>("cancel_codex_login");
}
