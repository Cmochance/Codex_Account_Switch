import type { ProfilesSnapshotResponse, QuotaSummary, ShellRoute } from "@front-shared/types";
import type { Locale } from "@front-shared/i18n";
import type { ThemeId } from "@front-shared/theme";

export const state = {
  page: 1,
  loading: false,
  refreshQueue: [] as string[],
  refreshActiveProfile: null as string | null,
  refreshWorkerActive: false,
  currentProfile: null as string | null,
  route: "dashboard" as ShellRoute,
  locale: "en" as Locale,
  theme: "classic" as ThemeId,
  pageSize: 4,
  snapshot: null as ProfilesSnapshotResponse | null,
  currentQuota: null as QuotaSummary | null,
};
