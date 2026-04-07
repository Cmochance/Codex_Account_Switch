import type { ProfilesSnapshotResponse, QuotaSummary, RuntimeSummary } from "./types";
import type { Locale } from "./i18n";

export const state = {
  page: 1,
  loading: false,
  currentProfile: null as string | null,
  locale: "en" as Locale,
  pageSize: 4,
  snapshot: null as ProfilesSnapshotResponse | null,
  runtime: null as RuntimeSummary | null,
  currentQuota: null as QuotaSummary | null,
};
