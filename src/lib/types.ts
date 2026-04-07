export interface QuotaWindow {
  remaining_percent: number | null;
  refresh_at: string | null;
}

export interface QuotaSummary {
  five_hour: QuotaWindow;
  weekly: QuotaWindow;
}

export interface ProfileCard {
  folder_name: string;
  display_title: string;
  status: "current" | "available" | "missing_auth";
  auth_present: boolean;
  has_account_identity: boolean;
  plan_name: string | null;
  subscription_days_left: number | null;
  quota: QuotaSummary;
}

export interface CurrentCard {
  folder_name: string;
  display_title: string;
  has_account_identity: boolean;
  plan_name: string | null;
  subscription_days_left: number | null;
  profile_folder_path: string;
}

export interface PagingInfo {
  page: number;
  page_size: number;
  total_profiles: number;
  total_pages: number;
  has_previous: boolean;
  has_next: boolean;
}

export interface DashboardViewModel {
  paging: PagingInfo;
  profiles: ProfileCard[];
  current_card: CurrentCard | null;
  current_quota_card: QuotaSummary | null;
}

export interface ProfilesSnapshotResponse {
  page_size: number;
  profiles: ProfileCard[];
  current_card: CurrentCard | null;
  current_quota_card: QuotaSummary | null;
}

export interface CurrentQuotaResponse {
  profile: string | null;
  quota: QuotaSummary | null;
}

export interface SwitchResponse {
  ok: boolean;
  profile: string;
  message: string;
  warnings: string[];
}

export interface ActionResponse {
  ok: boolean;
  message: string;
  path: string | null;
}

export interface CommandError {
  error_code?: string;
  message?: string;
}
