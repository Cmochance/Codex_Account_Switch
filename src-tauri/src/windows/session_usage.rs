use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};
use serde::Deserialize;

use crate::models::{QuotaSummary, QuotaWindow};

use super::paths::get_codex_home;
use super::session_files::{collect_jsonl_files, file_modified_ms};

const FIVE_HOUR_WINDOW_MINUTES: i64 = 300;
const WEEKLY_WINDOW_MINUTES: i64 = 10_080;

#[derive(Clone, Debug)]
pub struct LocalQuotaSnapshot {
    pub quota: QuotaSummary,
    pub source_mtime_ms: Option<u64>,
}

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

fn session_files_descending(codex_home: Option<&Path>) -> Vec<PathBuf> {
    let sessions_root = get_sessions_root(codex_home);
    if !sessions_root.is_dir() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_jsonl_files(&sessions_root, &mut files);
    files.sort_by(|left, right| right.as_os_str().cmp(left.as_os_str()));
    files
}

fn format_reset_time(timestamp: i64) -> Option<String> {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M").to_string())
}

fn normalize_quota_window(window: QuotaWindow) -> QuotaWindow {
    QuotaWindow {
        remaining_percent: window.remaining_percent.map(|value| value.min(100)),
        refresh_at: window.refresh_at,
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

    let quota = quota.unwrap_or_default();

    QuotaSummary {
        five_hour: if is_free_plan(plan_name) {
            QuotaWindow::default()
        } else {
            normalize_quota_window(quota.five_hour)
        },
        weekly: normalize_quota_window(quota.weekly),
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

fn apply_rate_limit_window(
    summary: &mut QuotaSummary,
    window: Option<SessionRateLimitWindow>,
    fallback: QuotaSlot,
) {
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

#[allow(dead_code)]
pub fn load_latest_local_quota(codex_home: Option<&Path>) -> Option<QuotaSummary> {
    load_latest_local_quota_snapshot(codex_home).map(|snapshot| snapshot.quota)
}

pub fn load_latest_local_quota_snapshot(codex_home: Option<&Path>) -> Option<LocalQuotaSnapshot> {
    load_latest_local_quota_snapshot_since(codex_home, None)
}

pub fn load_latest_local_quota_snapshot_since(
    codex_home: Option<&Path>,
    min_source_mtime_ms: Option<u64>,
) -> Option<LocalQuotaSnapshot> {
    for path in session_files_descending(codex_home).into_iter().take(32) {
        let source_mtime_ms = file_modified_ms(&path);
        if min_source_mtime_ms.is_some_and(|min_mtime| source_mtime_ms.unwrap_or(0) < min_mtime) {
            continue;
        }
        if let Some(quota) = load_latest_quota_from_file(&path) {
            return Some(LocalQuotaSnapshot {
                quota,
                source_mtime_ms,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        load_latest_local_quota, load_latest_local_quota_snapshot_since, normalize_quota_summary,
    };
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
        let session_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("07");
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
        let old_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("06");
        let new_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("07");
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

    #[test]
    fn load_latest_local_quota_snapshot_since_skips_older_sessions() {
        let codex_home = temp_codex_home("latest-since");
        let old_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("07");
        let new_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("08");
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&new_dir).unwrap();
        let old_path = old_dir.join("rollout-2026-04-07T08-00-00.jsonl");
        let new_path = new_dir.join("rollout-2026-04-08T08-00-00.jsonl");
        fs::write(
            &old_path,
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":50.0,\"resets_at\":1775000000,\"window_minutes\":300},\"secondary\":{\"used_percent\":40.0,\"resets_at\":1775600000,\"window_minutes\":10080}}}}\n",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let threshold = fs::metadata(&old_path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 1;
        fs::write(
            &new_path,
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":10.0,\"resets_at\":1776000000,\"window_minutes\":10080},\"secondary\":null}}}\n",
        )
        .unwrap();

        let quota = load_latest_local_quota_snapshot_since(Some(&codex_home), Some(threshold)).unwrap();

        assert_eq!(quota.quota.five_hour.remaining_percent, None);
        assert_eq!(quota.quota.weekly.remaining_percent, Some(90));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn normalize_quota_summary_keeps_missing_windows_empty() {
        let quota = normalize_quota_summary(None, Some("pro"), true);

        assert_eq!(quota.five_hour.remaining_percent, None);
        assert_eq!(quota.five_hour.refresh_at, None);
        assert_eq!(quota.weekly.remaining_percent, None);
        assert_eq!(quota.weekly.refresh_at, None);
    }

    #[test]
    fn normalize_quota_summary_preserves_existing_profile_values() {
        let quota = normalize_quota_summary(
            Some(QuotaSummary {
                five_hour: QuotaWindow {
                    remaining_percent: Some(12),
                    refresh_at: Some("2000-01-01 00:00".to_string()),
                },
                weekly: QuotaWindow {
                    remaining_percent: Some(34),
                    refresh_at: Some("2000-01-02 00:00".to_string()),
                },
            }),
            Some("plus"),
            true,
        );

        assert_eq!(quota.five_hour.remaining_percent, Some(12));
        assert_eq!(quota.five_hour.refresh_at.as_deref(), Some("2000-01-01 00:00"));
        assert_eq!(quota.weekly.remaining_percent, Some(34));
        assert_eq!(quota.weekly.refresh_at.as_deref(), Some("2000-01-02 00:00"));
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
        let session_dir = codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("07");
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
