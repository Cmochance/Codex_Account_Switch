import type { DashboardResponse } from "./types";
import type { Locale } from "./i18n";

export const state = {
  page: 1,
  loading: false,
  currentProfile: null as string | null,
  locale: "en" as Locale,
  dashboard: null as DashboardResponse | null,
};
