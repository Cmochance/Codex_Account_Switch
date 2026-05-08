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
        assert_eq!(slot_from_window_minutes(None, QuotaSlot::FiveHour), QuotaSlot::FiveHour);
        assert_eq!(slot_from_window_minutes(None, QuotaSlot::Weekly), QuotaSlot::Weekly);
        // 60 (1h) is not one of the known windows; trust the position
        // hint rather than silently dropping the data.
        assert_eq!(slot_from_window_minutes(Some(60), QuotaSlot::FiveHour), QuotaSlot::FiveHour);
        assert_eq!(slot_from_window_minutes(Some(60), QuotaSlot::Weekly), QuotaSlot::Weekly);
    }
}
