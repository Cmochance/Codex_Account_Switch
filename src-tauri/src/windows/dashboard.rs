use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate};

use crate::errors::AppResult;
use crate::models::{
    CurrentCard, DashboardResponse, PagingInfo, ProfileCard, ProfileMetadata, QuotaSummary,
};

use super::fs_ops::read_text_stripped;
use super::metadata::{load_profile_metadata, load_root_auth_metadata};
use super::paths::{
    get_backup_root, get_current_profile_file, list_profile_dirs, ACTIVE_MARKER_FILE,
    DEFAULT_PAGE_SIZE,
};
use super::session_usage::{load_latest_local_quota, normalize_quota_summary};

fn build_display_title(profile_name: &str, account_label: Option<&str>) -> String {
    let account_label = account_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("--");

    format!("{profile_name} / {account_label}")
}

fn compute_subscription_days_left(subscription_expires_at: Option<&str>) -> Option<i64> {
    let value = subscription_expires_at?;
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|datetime| datetime.with_timezone(&Local).date_naive())
        })?;

    let today = Local::now().date_naive();
    Some((parsed - today).num_days().max(0))
}

fn build_paging(total_profiles: u32, requested_page: u32) -> PagingInfo {
    let total_pages = ((total_profiles + DEFAULT_PAGE_SIZE - 1) / DEFAULT_PAGE_SIZE).max(1);
    let page = requested_page.clamp(1, total_pages);

    PagingInfo {
        page,
        page_size: DEFAULT_PAGE_SIZE,
        total_profiles,
        total_pages,
        has_previous: page > 1,
        has_next: page < total_pages,
    }
}

fn page_bounds(paging: &PagingInfo, total_items: usize) -> (usize, usize) {
    let start = ((paging.page - 1) * paging.page_size) as usize;
    let end = (start + paging.page_size as usize).min(total_items);
    (start, end)
}

fn build_profile_card(
    profile_dir: &Path,
    current_profile: Option<&str>,
    codex_home: Option<&Path>,
    live_quota: Option<&QuotaSummary>,
) -> ProfileCard {
    let metadata = load_profile_metadata(
        profile_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
        codex_home,
    );
    let has_account_identity = metadata
        .account_label
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let auth_present = profile_dir.join("auth.json").is_file();
    let folder_name = profile_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();

    let mut status = if current_profile == Some(folder_name.as_str()) {
        "current"
    } else {
        "available"
    }
    .to_string();

    if !auth_present {
        status = "missing_auth".to_string();
    }

    let raw_quota = if current_profile == Some(folder_name.as_str()) {
        live_quota
            .cloned()
            .unwrap_or_else(|| metadata.quota.clone())
    } else {
        metadata.quota.clone()
    };
    let quota = normalize_quota_summary(
        Some(raw_quota),
        metadata.plan_name.as_deref(),
        has_account_identity,
    );

    ProfileCard {
        folder_name: folder_name.clone(),
        display_title: build_display_title(&folder_name, metadata.account_label.as_deref()),
        status,
        auth_present,
        has_account_identity,
        plan_name: metadata.plan_name.clone(),
        subscription_days_left: compute_subscription_days_left(
            metadata.subscription_expires_at.as_deref(),
        ),
        quota,
    }
}

pub fn resolve_current_profile(backup_root: &Path) -> Option<String> {
    let current_profile_file = get_current_profile_file(backup_root.parent());
    let profile = read_text_stripped(&current_profile_file);
    if !profile.is_empty() && backup_root.join(&profile).is_dir() {
        return Some(profile);
    }

    for profile_dir in list_profile_dirs(backup_root) {
        if profile_dir.join(ACTIVE_MARKER_FILE).is_file() {
            if let Some(name) = profile_dir.file_name().and_then(|value| value.to_str()) {
                return Some(name.to_string());
            }
        }
    }

    None
}

pub fn build_dashboard(page: u32, codex_home: Option<&Path>) -> AppResult<DashboardResponse> {
    let codex_home = codex_home
        .map(PathBuf::from)
        .unwrap_or_else(super::paths::get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let all_profile_dirs = list_profile_dirs(&backup_root);
    let current_profile = resolve_current_profile(&backup_root);
    let live_quota = load_latest_local_quota(Some(&codex_home));
    let paging = build_paging(all_profile_dirs.len() as u32, page);
    let (start, end) = page_bounds(&paging, all_profile_dirs.len());
    let profiles = all_profile_dirs[start..end]
        .iter()
        .map(|profile_dir| {
            build_profile_card(
                profile_dir,
                current_profile.as_deref(),
                Some(&codex_home),
                live_quota.as_ref(),
            )
        })
        .collect::<Vec<_>>();

    let (current_card, current_quota_card) = match current_profile {
        Some(ref current_profile) if backup_root.join(current_profile).is_dir() => {
            let mut metadata: ProfileMetadata =
                load_profile_metadata(current_profile, Some(&codex_home));
            let current_profile_dir = backup_root.join(current_profile);
            if let Some(root_auth_metadata) = load_root_auth_metadata(Some(&codex_home)) {
                if let Some(account_label) = root_auth_metadata.account_label {
                    metadata.account_label = Some(account_label);
                }
                if root_auth_metadata.has_plan_claims {
                    metadata.plan_name = root_auth_metadata.plan_name;
                    metadata.subscription_expires_at = root_auth_metadata.subscription_expires_at;
                }
            }
            let has_account_identity = metadata
                .account_label
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());
            (
                Some(CurrentCard {
                    folder_name: current_profile.clone(),
                    display_title: build_display_title(
                        current_profile,
                        metadata.account_label.as_deref(),
                    ),
                    has_account_identity,
                    plan_name: metadata.plan_name.clone(),
                    subscription_days_left: compute_subscription_days_left(
                        metadata.subscription_expires_at.as_deref(),
                    ),
                    profile_folder_path: current_profile_dir.to_string_lossy().into_owned(),
                }),
                Some(normalize_quota_summary(
                    Some(live_quota.clone().unwrap_or(metadata.quota)),
                    metadata.plan_name.as_deref(),
                    has_account_identity,
                )),
            )
        }
        _ => (None, None),
    };

    Ok(DashboardResponse {
        paging,
        profiles,
        current_card,
        current_quota_card,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_dashboard, build_paging, build_profile_card, page_bounds};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::models::QuotaSummary;
    use crate::windows::paths::get_current_profile_file;

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-dashboard-{name}-{unique}"))
    }

    fn write_profile(codex_home: &PathBuf, profile_name: &str) {
        let profile_dir = codex_home.join("account_backup").join(profile_name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"account_id":"acct_{profile_name}"}}}}"#),
        )
        .unwrap();
    }

    #[test]
    fn build_paging_clamps_requested_page_and_bounds() {
        let paging = build_paging(5, 9);
        let bounds = page_bounds(&paging, 5);

        assert_eq!(paging.page, 2);
        assert_eq!(paging.total_pages, 2);
        assert_eq!(bounds, (4, 5));
    }

    #[test]
    fn build_dashboard_returns_only_requested_page_profiles() {
        let codex_home = temp_codex_home("paged-dashboard");
        for profile_name in ["a", "b", "c", "d", "e"] {
            write_profile(&codex_home, profile_name);
        }
        fs::write(get_current_profile_file(Some(&codex_home)), "a").unwrap();

        let dashboard = build_dashboard(2, Some(&codex_home)).unwrap();

        assert_eq!(dashboard.paging.page, 2);
        assert_eq!(dashboard.paging.total_pages, 2);
        assert_eq!(dashboard.profiles.len(), 1);
        assert_eq!(dashboard.profiles[0].folder_name, "e");
        assert_eq!(
            dashboard
                .current_card
                .as_ref()
                .map(|card| card.folder_name.as_str()),
            Some("a")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn build_profile_card_prefers_live_quota_for_current_profile() {
        let codex_home = temp_codex_home("live-quota");
        let profile_dir = codex_home.join("account_backup").join("a");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"account_id":"acct_a"}}"#,
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{
                "folder_name":"a",
                "quota":{
                    "five_hour":{"remaining_percent":12,"refresh_at":"old-5h"},
                    "weekly":{"remaining_percent":34,"refresh_at":"old-week"}
                }
            }"#,
        )
        .unwrap();

        let live_quota = QuotaSummary {
            five_hour: crate::models::QuotaWindow {
                remaining_percent: Some(88),
                refresh_at: Some("2099-04-07 12:00".to_string()),
            },
            weekly: crate::models::QuotaWindow {
                remaining_percent: Some(77),
                refresh_at: Some("2099-04-10 12:00".to_string()),
            },
        };

        let card = build_profile_card(
            &profile_dir,
            Some("a"),
            Some(&codex_home),
            Some(&live_quota),
        );

        assert_eq!(card.quota.five_hour.remaining_percent, Some(88));
        assert_eq!(card.quota.weekly.remaining_percent, Some(77));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn build_profile_card_keeps_stored_quota_for_non_current_profile() {
        let codex_home = temp_codex_home("stored-quota");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"account_id":"acct_b"}}"#,
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{
                "folder_name":"b",
                "quota":{
                    "five_hour":{"remaining_percent":21,"refresh_at":"stored-5h"},
                    "weekly":{"remaining_percent":43,"refresh_at":"stored-week"}
                }
            }"#,
        )
        .unwrap();

        let live_quota = QuotaSummary {
            five_hour: crate::models::QuotaWindow {
                remaining_percent: Some(88),
                refresh_at: Some("2026-04-07 12:00".to_string()),
            },
            weekly: crate::models::QuotaWindow {
                remaining_percent: Some(77),
                refresh_at: Some("2026-04-10 12:00".to_string()),
            },
        };

        let card = build_profile_card(
            &profile_dir,
            Some("a"),
            Some(&codex_home),
            Some(&live_quota),
        );

        assert_eq!(card.quota.five_hour.remaining_percent, Some(21));
        assert_eq!(card.quota.weekly.remaining_percent, Some(43));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn build_profile_card_defaults_missing_quota_to_full_allowance() {
        let codex_home = temp_codex_home("default-quota");
        let profile_dir = codex_home.join("account_backup").join("a");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"account_id":"acct_a"}}"#,
        )
        .unwrap();

        let card = build_profile_card(&profile_dir, Some("a"), Some(&codex_home), None);

        assert_eq!(card.quota.five_hour.remaining_percent, Some(100));
        assert_eq!(card.quota.weekly.remaining_percent, Some(100));
        assert!(card.quota.five_hour.refresh_at.is_some());
        assert!(card.quota.weekly.refresh_at.is_some());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn build_profile_card_disables_quota_when_account_identity_is_missing() {
        let codex_home = temp_codex_home("missing-identity");
        let profile_dir = codex_home.join("account_backup").join("x");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"id_token":"replace-me","account_id":"replace-me"}}"#,
        )
        .unwrap();

        let card = build_profile_card(&profile_dir, Some("x"), Some(&codex_home), None);

        assert!(!card.has_account_identity);
        assert_eq!(card.quota.five_hour.remaining_percent, None);
        assert_eq!(card.quota.weekly.remaining_percent, None);
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn build_profile_card_disables_five_hour_quota_for_free_plan() {
        let codex_home = temp_codex_home("free-plan-quota");
        let profile_dir = codex_home.join("account_backup").join("f");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            r#"{"tokens":{"account_id":"acct_f"}}"#,
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{
                "folder_name":"f",
                "plan_name":"free",
                "quota":{
                    "five_hour":{"remaining_percent":55,"refresh_at":"2099-01-01 00:00"},
                    "weekly":{"remaining_percent":81,"refresh_at":"2099-01-08 00:00"}
                }
            }"#,
        )
        .unwrap();

        let card = build_profile_card(&profile_dir, Some("f"), Some(&codex_home), None);

        assert_eq!(card.quota.five_hour.remaining_percent, None);
        assert_eq!(card.quota.five_hour.refresh_at, None);
        assert_eq!(card.quota.weekly.remaining_percent, Some(81));
        assert!(card.quota.weekly.refresh_at.is_some());
        let _ = fs::remove_dir_all(&codex_home);
    }
}
