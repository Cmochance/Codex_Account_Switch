import { state } from "./state";
import type {
  CurrentQuotaResponse,
  DashboardViewModel,
  PagingInfo,
  ProfilesSnapshotResponse,
} from "./types";

export function buildPaging(totalProfiles: number, pageSize: number, page: number): PagingInfo {
  const totalPages = Math.max(1, Math.ceil(totalProfiles / pageSize));
  const nextPage = Math.min(Math.max(1, page), totalPages);

  return {
    page: nextPage,
    page_size: pageSize,
    total_profiles: totalProfiles,
    total_pages: totalPages,
    has_previous: nextPage > 1,
    has_next: nextPage < totalPages,
  };
}

export function buildDashboardViewModel(): DashboardViewModel | null {
  if (!state.snapshot) {
    return null;
  }

  const paging = buildPaging(state.snapshot.profiles.length, state.pageSize, state.page);
  const start = (paging.page - 1) * paging.page_size;
  const end = start + paging.page_size;
  state.page = paging.page;

  return {
    paging,
    profiles: state.snapshot.profiles.slice(start, end),
    current_card: state.snapshot.current_card,
    current_quota_card: state.currentQuota ?? state.snapshot.current_quota_card,
  };
}

export function applySnapshot(snapshot: ProfilesSnapshotResponse): void {
  state.snapshot = snapshot;
  state.pageSize = snapshot.page_size;
  state.currentProfile = snapshot.current_card?.folder_name ?? null;
  state.currentQuota = snapshot.current_quota_card;
  state.page = buildPaging(snapshot.profiles.length, snapshot.page_size, state.page).page;
}

export function applyCurrentQuota(response: CurrentQuotaResponse): void {
  const currentProfile = state.snapshot?.current_card?.folder_name ?? null;

  if (!response.profile) {
    if (!currentProfile) {
      state.currentQuota = null;
    }
    return;
  }

  if (response.profile === currentProfile) {
    state.currentQuota = response.quota;
  }
}
