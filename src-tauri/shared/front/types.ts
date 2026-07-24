export interface QuotaWindow {
  remaining_percent: number | null;
  refresh_at: string | null;
  reset_at_timestamp: number | null;
}

export interface RateLimitResetCredit {
  granted_at: number | null;
  expires_at: number | null;
}

export interface RateLimitResetCredits {
  available_count: number | null;
  credits: RateLimitResetCredit[];
}

export interface QuotaSummary {
  five_hour: QuotaWindow;
  weekly: QuotaWindow;
  rate_limit_reset_credits?: RateLimitResetCredits | null;
}

export interface ProfileCard {
  folder_name: string;
  display_title: string;
  account_label: string | null;
  status: "current" | "available" | "missing_auth";
  auth_present: boolean;
  has_account_identity: boolean;
  plan_name: string | null;
  subscription_days_left: number | null;
  openai_base_url: string | null;
  quota: QuotaSummary;
  last_plan_check_ms: number | null;
}

export interface CurrentCard {
  folder_name: string;
  display_title: string;
  account_label: string | null;
  has_account_identity: boolean;
  plan_name: string | null;
  subscription_days_left: number | null;
  profile_folder_path: string;
  last_plan_check_ms: number | null;
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
  /** Label of the live `~/.codex` account when it belongs to no saved card
   *  (drift to an unmanaged account); `null` in the normal case. */
  unmanaged_live_account: string | null;
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

export interface UpdateCheckResponse {
  ok: boolean;
  current_version: string;
  latest_version: string | null;
  has_update: boolean;
  release_url: string | null;
  notes: string | null;
  checked_url: string;
}

export interface CommandError {
  error_code?: string;
  message?: string;
}

export type CodexCliSource = "user_override" | "install_state" | "discovery" | "none";

export interface CodexCliStatus {
  resolved_path: string | null;
  source: CodexCliSource;
  suggested_paths: string[];
}

export interface CodexCliCandidate {
  /** Absolute path to the verified-runnable codex binary. */
  path: string;
  /** `codex --version` line (e.g. "codex-cli 0.133.0"), or null if it ran but printed nothing parseable. */
  version: string | null;
}

export interface CodexCliRedetectResult {
  /** Candidates verified runnable by the forced scan, deduped, best-first. */
  candidates: CodexCliCandidate[];
  /** Refreshed status snapshot so the Settings row can update in step. */
  status: CodexCliStatus;
}

/** 当前生效的代理配置。`proxy_url` 为 null 表示直连。
 *  仅 macOS 启用应用层代理；Windows / Linux 永远返回 null。 */
export interface ProxyConfig {
  proxy_url: string | null;
}

/** `set_proxy_config` 的请求 payload。空字符串视为清空。 */
export interface SetProxyConfigPayload {
  proxy_url: string;
}

export type ShellRoute = "dashboard" | "profiles" | "settings" | "guide";
