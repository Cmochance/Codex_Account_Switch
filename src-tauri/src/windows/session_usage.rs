use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{DateTime, Duration, Local, LocalResult, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::models::{QuotaSummary, QuotaWindow};

use super::paths::get_codex_home;

const FIVE_HOUR_WINDOW_MINUTES: i64 = 300;
const WEEKLY_WINDOW_MINUTES: i64 = 10_080;

#[derive(Deserialize)]
struct SessionLine {
    #[serde(rename = "type")]
    line_type: String,
    payload: Option<SessionPayload>,
}

#[derive(Deserialize)]
struct SessionPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    rate_limits: Option<SessionRateLimits>,
}

#[derive(Deserialize)]
struct SessionRateLimits {
    primary: Option<SessionRateLimitWindow>,
    secondary: Option<SessionRateLimitWindow>,
}

#[derive(Deserialize)]
struct SessionRateLimitWindow {
    used_percent: Option<f64>,
    resets_at: Option<i64>,
    window_minutes: Option<i64>,
}

fn get_sessions_root(codex_home: Option<&Path>) -> PathBuf {
    codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(get_codex_home)
        .join("sessions")
}

fn collect_session_files(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_files(&path, files);
            continue;
        }

        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }
}

fn session_files_descending(codex_home: Option<&Path>) -> Vec<PathBuf> {
    let sessions_root = get_sessions_root(codex_home);
    if !sessions_root.is_dir() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_session_files(&sessions_root, &mut files);
    files.sort_by(|left, right| right.as_os_str().cmp(left.as_os_str()));
    files
}

fn format_reset_time(timestamp: i64) -> Option<String> {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M").to_string())
}

fn format_reset_datetime(datetime: DateTime<Local>) -> String {
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

fn parse_refresh_time(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|datetime| datetime.with_timezone(&Local))
        .or_else(|| {
            let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M").ok()?;
            match Local.from_local_datetime(&naive) {
                LocalResult::Single(datetime) => Some(datetime),
                LocalResult::Ambiguous(datetime, _) => Some(datetime),
                LocalResult::None => None,
            }
        })
}

fn quota_startup_anchor() -> DateTime<Local> {
    static STARTUP_TS: OnceLock<i64> = OnceLock::new();

    let timestamp = *STARTUP_TS.get_or_init(|| Utc::now().timestamp());
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .unwrap_or_else(Local::now)
}

fn next_refresh_after(mut refresh_at: DateTime<Local>, window_duration: Duration, now: DateTime<Local>) -> DateTime<Local> {
    while refresh_at <= now {
        refresh_at += window_duration;
    }

    refresh_at
}

fn normalize_quota_window(
    window: QuotaWindow,
    window_duration: Duration,
    default_refresh_at: DateTime<Local>,
) -> QuotaWindow {
    let now = Local::now();
    let mut remaining_percent = window.remaining_percent.map(|value| value.min(100));
    let mut refresh_at = window
        .refresh_at
        .as_deref()
        .and_then(parse_refresh_time)
        .unwrap_or(default_refresh_at);

    if refresh_at <= now {
        remaining_percent = Some(100);
        refresh_at = next_refresh_after(refresh_at, window_duration, now);
    }

    QuotaWindow {
        remaining_percent: Some(remaining_percent.unwrap_or(100)),
        refresh_at: Some(format_reset_datetime(refresh_at)),
    }
}

fn is_free_plan(plan_name: Option<&str>) -> bool {
    plan_name
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("free"))
}

pub fn normalize_quota_summary(
    quota: Option<QuotaSummary>,
    plan_name: Option<&str>,
    has_account_identity: bool,
) -> QuotaSummary {
    if !has_account_identity {
        return QuotaSummary::default();
    }

    let anchor = quota_startup_anchor();
    let quota = quota.unwrap_or_default();

    QuotaSummary {
        five_hour: if is_free_plan(plan_name) {
            QuotaWindow::default()
        } else {
            normalize_quota_window(
                quota.five_hour,
                Duration::minutes(FIVE_HOUR_WINDOW_MINUTES),
                anchor + Duration::minutes(FIVE_HOUR_WINDOW_MINUTES),
            )
        },
        weekly: normalize_quota_window(
            quota.weekly,
            Duration::minutes(WEEKLY_WINDOW_MINUTES),
            anchor + Duration::minutes(WEEKLY_WINDOW_MINUTES),
        ),
    }
}

fn quota_window_from_rate_limit(window: Option<SessionRateLimitWindow>) -> QuotaWindow {
    let Some(window) = window else {
        return QuotaWindow::default();
    };

    let remaining_percent = window
        .used_percent
        .map(|used_percent| (100.0 - used_percent).round().clamp(0.0, 100.0) as u8);
    let refresh_at = window.resets_at.and_then(format_reset_time);

    QuotaWindow {
        remaining_percent,
        refresh_at,
    }
}

#[derive(Clone, Copy)]
enum QuotaSlot {
    FiveHour,
    Weekly,
}

fn slot_from_window(window: &SessionRateLimitWindow, fallback: QuotaSlot) -> QuotaSlot {
    match window.window_minutes {
        Some(FIVE_HOUR_WINDOW_MINUTES) => QuotaSlot::FiveHour,
        Some(WEEKLY_WINDOW_MINUTES) => QuotaSlot::Weekly,
        _ => fallback,
    }
}

fn apply_rate_limit_window(summary: &mut QuotaSummary, window: Option<SessionRateLimitWindow>, fallback: QuotaSlot) {
    let Some(window) = window else {
        return;
    };

    let slot = slot_from_window(&window, fallback);
    let quota_window = quota_window_from_rate_limit(Some(window));

    match slot {
        QuotaSlot::FiveHour => summary.five_hour = quota_window,
        QuotaSlot::Weekly => summary.weekly = quota_window,
    }
}

fn quota_from_line(line: &str) -> Option<QuotaSummary> {
    let parsed = serde_json::from_str::<SessionLine>(line).ok()?;
    if parsed.line_type != "event_msg" {
        return None;
    }

    let payload = parsed.payload?;
    if payload.payload_type.as_deref() != Some("token_count") {
        return None;
    }

    let rate_limits = payload.rate_limits?;
    let mut quota = QuotaSummary::default();
    apply_rate_limit_window(&mut quota, rate_limits.primary, QuotaSlot::FiveHour);
    apply_rate_limit_window(&mut quota, rate_limits.secondary, QuotaSlot::Weekly);

    (quota.five_hour.remaining_percent.is_some()
        || quota.five_hour.refresh_at.is_some()
        || quota.weekly.remaining_percent.is_some()
        || quota.weekly.refresh_at.is_some())
    .then_some(quota)
}

fn load_latest_quota_from_file(path: &Path) -> Option<QuotaSummary> {
    let raw = fs::read_to_string(path).ok()?;
    let mut latest_quota = None;

    for line in raw.lines() {
        if let Some(quota) = quota_from_line(line) {
            latest_quota = Some(quota);
        }
    }

    latest_quota
}

pub fn load_latest_local_quota(codex_home: Option<&Path>) -> Option<QuotaSummary> {
    for path in session_files_descending(codex_home).into_iter().take(32) {
        if let Some(quota) = load_latest_quota_from_file(&path) {
            return Some(quota);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{load_latest_local_quota, normalize_quota_summary};
    use chrono::{DateTime, Duration, Local, NaiveDateTime, TimeZone};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::models::{QuotaSummary, QuotaWindow};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-session-usage-{name}-{unique}"))
    }

    #[test]
    fn load_latest_local_quota_uses_latest_token_count_event() {
        let codex_home = temp_codex_home("latest-token-count");
        let session_dir = codex_home.join("sessions").join("2026").join("04").join("07");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("rollout-2026-04-07T11-48-31.jsonl"),
            concat!(
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":",
                "{\"primary\":{\"used_percent\":5.0,\"resets_at\":1775551886,\"window_minutes\":300},",
                "\"secondary\":{\"used_percent\":27.0,\"resets_at\":1775970905,\"window_minutes\":10080}}}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":",
                "{\"primary\":{\"used_percent\":8.0,\"resets_at\":1775552886,\"window_minutes\":300},",
                "\"secondary\":{\"used_percent\":30.0,\"resets_at\":1775971905,\"window_minutes\":10080}}}}\n"
            ),
        )
        .unwrap();

        let quota = load_latest_local_quota(Some(&codex_home)).unwrap();

        assert_eq!(quota.five_hour.remaining_percent, Some(92));
        assert_eq!(quota.weekly.remaining_percent, Some(70));
        assert!(quota.five_hour.refresh_at.is_some());
        assert!(quota.weekly.refresh_at.is_some());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_latest_local_quota_prefers_lexically_latest_session_file() {
        let codex_home = temp_codex_home("latest-file");
        let old_dir = codex_home.join("sessions").join("2026").join("04").join("06");
        let new_dir = codex_home.join("sessions").join("2026").join("04").join("07");
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&new_dir).unwrap();
        fs::write(
            old_dir.join("rollout-2026-04-06T08-00-00.jsonl"),
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":50.0,\"resets_at\":1775000000,\"window_minutes\":300},\"secondary\":{\"used_percent\":40.0,\"resets_at\":1775600000,\"window_minutes\":10080}}}}\n",
        )
        .unwrap();
        fs::write(
            new_dir.join("rollout-2026-04-07T08-00-00.jsonl"),
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":10.0,\"resets_at\":1776000000,\"window_minutes\":300},\"secondary\":{\"used_percent\":20.0,\"resets_at\":1776600000,\"window_minutes\":10080}}}}\n",
        )
        .unwrap();

        let quota = load_latest_local_quota(Some(&codex_home)).unwrap();

        assert_eq!(quota.five_hour.remaining_percent, Some(90));
        assert_eq!(quota.weekly.remaining_percent, Some(80));
        let _ = fs::remove_dir_all(&codex_home);
    }

    fn parse_local_refresh(value: &str) -> DateTime<Local> {
        let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M").unwrap();
        match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(datetime) => datetime,
            chrono::LocalResult::Ambiguous(datetime, _) => datetime,
            chrono::LocalResult::None => panic!("invalid local datetime"),
        }
    }

    #[test]
    fn normalize_quota_summary_defaults_missing_windows_to_full_allowance() {
        let now = Local::now();
        let quota = normalize_quota_summary(None, Some("pro"), true);

        assert_eq!(quota.five_hour.remaining_percent, Some(100));
        assert_eq!(quota.weekly.remaining_percent, Some(100));

        let five_hour_refresh = parse_local_refresh(quota.five_hour.refresh_at.as_deref().unwrap());
        let weekly_refresh = parse_local_refresh(quota.weekly.refresh_at.as_deref().unwrap());

        assert!(five_hour_refresh > now + Duration::minutes(295));
        assert!(weekly_refresh > now + Duration::minutes(10_000));
    }

    #[test]
    fn normalize_quota_summary_restores_expired_windows_to_full_allowance() {
        let quota = normalize_quota_summary(Some(QuotaSummary {
            five_hour: QuotaWindow {
                remaining_percent: Some(12),
                refresh_at: Some("2000-01-01 00:00".to_string()),
            },
            weekly: QuotaWindow {
                remaining_percent: Some(34),
                refresh_at: Some("2000-01-02 00:00".to_string()),
            },
        }), Some("plus"), true);

        assert_eq!(quota.five_hour.remaining_percent, Some(100));
        assert_eq!(quota.weekly.remaining_percent, Some(100));

        let five_hour_refresh = parse_local_refresh(quota.five_hour.refresh_at.as_deref().unwrap());
        let weekly_refresh = parse_local_refresh(quota.weekly.refresh_at.as_deref().unwrap());
        let now = Local::now();

        assert!(five_hour_refresh > now);
        assert!(weekly_refresh > now);
    }

    #[test]
    fn normalize_quota_summary_disables_five_hour_window_for_free_plan() {
        let quota = normalize_quota_summary(
            Some(QuotaSummary {
                five_hour: QuotaWindow {
                    remaining_percent: Some(64),
                    refresh_at: Some("2099-01-01 00:00".to_string()),
                },
                weekly: QuotaWindow {
                    remaining_percent: Some(82),
                    refresh_at: Some("2099-01-08 00:00".to_string()),
                },
            }),
            Some("free"),
            true,
        );

        assert_eq!(quota.five_hour.remaining_percent, None);
        assert_eq!(quota.five_hour.refresh_at, None);
        assert_eq!(quota.weekly.remaining_percent, Some(82));
        assert!(quota.weekly.refresh_at.is_some());
    }

    #[test]
    fn normalize_quota_summary_disables_unknown_account_quota_defaults() {
        let quota = normalize_quota_summary(None, Some("pro"), false);

        assert_eq!(quota.five_hour.remaining_percent, None);
        assert_eq!(quota.five_hour.refresh_at, None);
        assert_eq!(quota.weekly.remaining_percent, None);
        assert_eq!(quota.weekly.refresh_at, None);
    }

    #[test]
    fn load_latest_local_quota_maps_weekly_only_free_limit_from_primary_slot() {
        let codex_home = temp_codex_home("weekly-only-primary");
        let session_dir = codex_home.join("sessions").join("2026").join("04").join("07");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("rollout-2026-04-07T16-00-00.jsonl"),
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":36.0,\"resets_at\":1776149706,\"window_minutes\":10080},\"secondary\":null}}}\n",
        )
        .unwrap();

        let quota = load_latest_local_quota(Some(&codex_home)).unwrap();

        assert_eq!(quota.five_hour.remaining_percent, None);
        assert_eq!(quota.weekly.remaining_percent, Some(64));
        assert!(quota.weekly.refresh_at.is_some());
        let _ = fs::remove_dir_all(&codex_home);
    }
}
