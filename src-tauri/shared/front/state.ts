import type { ProfilesSnapshotResponse, QuotaSummary } from "@front-shared/types";
import type { Locale } from "@front-shared/i18n";

export type AppPage = "dashboard" | "settings" | "guide" | "proxy";

export const state = {
  page: 1,
  loading: false,
  refreshQueue: [] as string[],
  refreshActiveProfile: null as string | null,
  refreshWorkerActive: false,
  currentProfile: null as string | null,
  locale: "en" as Locale,
  pageSize: 4,
  snapshot: null as ProfilesSnapshotResponse | null,
  currentQuota: null as QuotaSummary | null,
  currentPage: "dashboard" as AppPage,
  proxyPort: 18080,
  proxyRunning: false,
  proxyLogs: [] as string[],
  theme: "light" as "light" | "dark" | "system",
};
