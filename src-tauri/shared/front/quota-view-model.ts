import type { QuotaSummary } from "@front-shared/types";

export type RateLimitResetCreditsPresentation =
  | { state: "unknown"; availableCount: null }
  | {
      state: "ready";
      availableCount: number;
      cards: Array<{ grantedAt: number | null; expiresAt: number | null }>;
    };

export function buildRateLimitResetCreditsPresentation(
  quota: QuotaSummary | null | undefined,
): RateLimitResetCreditsPresentation {
  const availableCount = quota?.rate_limit_reset_credits?.available_count;
  if (availableCount == null) {
    return { state: "unknown", availableCount: null };
  }

  return {
    state: "ready",
    availableCount,
    cards: (quota?.rate_limit_reset_credits?.credits ?? []).map((credit) => ({
      grantedAt: credit.granted_at,
      expiresAt: credit.expires_at,
    })),
  };
}
