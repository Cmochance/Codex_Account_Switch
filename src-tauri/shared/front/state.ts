import type { ProfilesSnapshotResponse, QuotaSummary } from "@front-shared/types";
import type { Locale } from "@front-shared/i18n";

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
};
