import { invoke } from "@tauri-apps/api/core";

import type { ActionResponse, CommandError, DashboardResponse, SwitchResponse } from "./types";

function toError(error: unknown): Error {
  if (typeof error === "string") {
    return new Error(error);
  }

  if (error && typeof error === "object") {
    const payload = error as CommandError;
    if (payload.message) {
      return new Error(payload.message);
    }
  }

  return new Error("Unknown native command error.");
}

async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toError(error);
  }
}

export function getDashboard(page: number): Promise<DashboardResponse> {
  return invokeCommand<DashboardResponse>("get_dashboard", { page });
}

export function switchProfile(profile: string): Promise<SwitchResponse> {
  return invokeCommand<SwitchResponse>("switch_profile", { payload: { profile } });
}

export function openProfileFolder(profile: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_profile_folder", { payload: { profile } });
}

export function addProfile(folderName: string): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("add_profile", { payload: { folder_name: folderName } });
}

export function openCodex(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_codex");
}

export function openContact(): Promise<ActionResponse> {
  return invokeCommand<ActionResponse>("open_contact");
}
