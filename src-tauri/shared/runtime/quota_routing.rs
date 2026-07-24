//! Shared quota-window routing.
//!
//! Both the `chatgpt_api` (live `/wham/usage` payload) and
//! `session_usage` (Codex CLI session JSONL `token_count` events) paths
//! receive a primary / secondary pair of rate-limit windows from the
//! same upstream source. OpenAI MOSTLY puts the 5h window in `primary`
//! and the weekly window in `secondary`, but real-world data shows
//! exceptions — at least one observed `token_count` event carried the
//! weekly window in the primary slot with secondary null. Position is
//! not authoritative; `window_minutes` is.
//!
//! Without routing by `window_minutes`, an account where the API
//! returns only a weekly window in primary slot (e.g. a Team plan with
//! no 5h budget enforcement) ends up with the weekly data labeled as
//! 5h on the dashboard and no weekly bar at all. Mapping by
//! `window_minutes` keeps the buckets aligned regardless of position.

/// Length in minutes of OpenAI's 5-hour rate-limit window.
pub const FIVE_HOUR_WINDOW_MINUTES: i64 = 300;

/// Length in minutes of OpenAI's weekly rate-limit window. 7 days *
/// 24h * 60min = 10_080.
pub const WEEKLY_WINDOW_MINUTES: i64 = 10_080;

/// Threshold (in seconds) used to classify a window by its `reset_at`
/// distance from now when `window_minutes` is missing. 6 hours
/// (21600s) cleanly separates a 5h window (resets within a few hours)
/// from a weekly window (resets in days).
pub const RESET_AT_CLASSIFY_THRESHOLD_SECONDS: i64 = 6 * 60 * 60;

/// Which slot of `QuotaSummary` a rate-limit window belongs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaSlot {
    FiveHour,
    Weekly,
}

/// Decide which `QuotaSummary` slot a rate-limit window belongs in,
/// based on its `window_minutes` field. Falls back to `fallback` (the
/// position-based guess — primary→FiveHour, secondary→Weekly) when the
/// upstream payload omits `window_minutes` or carries an unknown value.
pub fn slot_from_window_minutes(window_minutes: Option<i64>, fallback: QuotaSlot) -> QuotaSlot {
    match window_minutes {
        Some(FIVE_HOUR_WINDOW_MINUTES) => QuotaSlot::FiveHour,
        Some(WEEKLY_WINDOW_MINUTES) => QuotaSlot::Weekly,
        _ => fallback,
    }
}

/// Classify a window by the distance between its `reset_at` (Unix
/// seconds) and `now_secs`. Used when `window_minutes` is missing —
/// OpenAI's `/wham/usage` payload has been observed omitting
/// `window_minutes` entirely while still reporting a weekly `reset_at`
/// several days out. Returns `None` when `reset_at` is missing or in
/// the past (cannot classify reliably).
pub fn slot_from_reset_at(reset_at: Option<i64>, now_secs: i64) -> Option<QuotaSlot> {
    let reset_at = reset_at?;
    let delta = reset_at - now_secs;
    if delta <= 0 {
        return None;
    }
    if delta <= RESET_AT_CLASSIFY_THRESHOLD_SECONDS {
        Some(QuotaSlot::FiveHour)
    } else {
        Some(QuotaSlot::Weekly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_hour_window_routes_to_five_hour_regardless_of_fallback() {
        assert_eq!(
            slot_from_window_minutes(Some(FIVE_HOUR_WINDOW_MINUTES), QuotaSlot::Weekly),
            QuotaSlot::FiveHour
        );
        assert_eq!(
            slot_from_window_minutes(Some(FIVE_HOUR_WINDOW_MINUTES), QuotaSlot::FiveHour),
            QuotaSlot::FiveHour
        );
    }

    #[test]
    fn weekly_window_routes_to_weekly_regardless_of_fallback() {
        assert_eq!(
            slot_from_window_minutes(Some(WEEKLY_WINDOW_MINUTES), QuotaSlot::FiveHour),
            QuotaSlot::Weekly
        );
        assert_eq!(
            slot_from_window_minutes(Some(WEEKLY_WINDOW_MINUTES), QuotaSlot::Weekly),
            QuotaSlot::Weekly
        );
    }

    #[test]
    fn missing_or_unknown_window_minutes_falls_back_to_position() {
        assert_eq!(
            slot_from_window_minutes(None, QuotaSlot::FiveHour),
            QuotaSlot::FiveHour
        );
        assert_eq!(
            slot_from_window_minutes(None, QuotaSlot::Weekly),
            QuotaSlot::Weekly
        );
        // 60 (1h) is not one of the known windows; trust the position
        // hint rather than silently dropping the data.
        assert_eq!(
            slot_from_window_minutes(Some(60), QuotaSlot::FiveHour),
            QuotaSlot::FiveHour
        );
        assert_eq!(
            slot_from_window_minutes(Some(60), QuotaSlot::Weekly),
            QuotaSlot::Weekly
        );
    }

    #[test]
    fn slot_from_reset_at_classifies_by_distance() {
        // 1h ahead -> 5h window
        assert_eq!(slot_from_reset_at(Some(3600), 0), Some(QuotaSlot::FiveHour));
        // Exactly at the 6h threshold -> 5h
        assert_eq!(
            slot_from_reset_at(Some(RESET_AT_CLASSIFY_THRESHOLD_SECONDS), 0),
            Some(QuotaSlot::FiveHour)
        );
        // 1 day ahead -> weekly
        assert_eq!(slot_from_reset_at(Some(86_400), 0), Some(QuotaSlot::Weekly));
        // 5 days ahead -> weekly (the observed 2026-07-29 case)
        assert_eq!(
            slot_from_reset_at(Some(5 * 86_400), 0),
            Some(QuotaSlot::Weekly)
        );
    }

    #[test]
    fn slot_from_reset_at_returns_none_for_missing_or_past() {
        assert_eq!(slot_from_reset_at(None, 0), None);
        // Already reset (past) -> cannot classify
        assert_eq!(slot_from_reset_at(Some(-100), 0), None);
        // Reset exactly now -> ambiguous, treat as unclassifiable
        assert_eq!(slot_from_reset_at(Some(0), 0), None);
    }
}
