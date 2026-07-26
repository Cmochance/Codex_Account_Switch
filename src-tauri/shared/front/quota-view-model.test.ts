import { describe, expect, it } from "vitest";
import { buildRateLimitResetCreditsPresentation } from "./quota-view-model";

describe("rate-limit reset credit presentation", () => {
  it("keeps the available count and every card timestamp", () => {
    const result = buildRateLimitResetCreditsPresentation({
      five_hour: { remaining_percent: 80, refresh_at: null, reset_at_timestamp: null },
      weekly: { remaining_percent: 90, refresh_at: null, reset_at_timestamp: null },
      rate_limit_reset_credits: {
        available_count: 2,
        credits: [
          { granted_at: 1_783_890_765, expires_at: 1_786_482_765 },
          { granted_at: 1_783_900_000, expires_at: 1_786_500_000 },
        ],
      },
    });

    expect(result).toEqual({
      state: "ready",
      availableCount: 2,
      cards: [
        { grantedAt: 1_783_890_765, expiresAt: 1_786_482_765 },
        { grantedAt: 1_783_900_000, expiresAt: 1_786_500_000 },
      ],
    });
  });

  it("does not invent a count when the API did not report one", () => {
    expect(
      buildRateLimitResetCreditsPresentation({
        five_hour: { remaining_percent: null, refresh_at: null, reset_at_timestamp: null },
        weekly: { remaining_percent: null, refresh_at: null, reset_at_timestamp: null },
      }),
    ).toEqual({ state: "unknown", availableCount: null });
  });
});
