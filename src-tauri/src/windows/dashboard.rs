use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};

use crate::errors::AppResult;
use crate::models::{CurrentCard, DashboardResponse, PagingInfo, ProfileCard, ProfileMetadata, RuntimeSummary};

use super::fs_ops::read_text_stripped;
use super::metadata::load_profile_metadata;
use super::paths::{
    get_auto_save_root, get_backup_root, get_current_profile_file, list_profile_dirs, ACTIVE_MARKER_FILE,
    DEFAULT_PAGE_SIZE,
};
use super::process;

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

fn latest_autosave_timestamp(codex_home: Option<&Path>) -> Option<String> {
    let auto_save_root = get_auto_save_root(codex_home);
    if !auto_save_root.is_dir() {
        return None;
    }

    let latest = auto_save_root
        .read_dir()
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()).map(str::to_string))
        .max()?;

    NaiveDateTime::parse_from_str(&latest, "%Y%m%d-%H%M%S")
        .map(|datetime| datetime.format("%Y-%m-%dT%H:%M:%S").to_string())
        .ok()
        .or(Some(latest))
}

fn build_profile_card(profile_dir: &Path, current_profile: Option<&str>, codex_home: Option<&Path>) -> ProfileCard {
    let metadata = load_profile_metadata(profile_dir.file_name().and_then(|name| name.to_str()).unwrap_or_default(), codex_home);
    let auth_present = profile_dir.join("auth.json").is_file();
    let folder_name = profile_dir.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string();

    let mut status = if current_profile == Some(folder_name.as_str()) {
        "current"
    } else {
        "available"
    }
    .to_string();

    if !auth_present {
        status = "missing_auth".to_string();
    }

    ProfileCard {
        folder_name: folder_name.clone(),
        display_title: build_display_title(&folder_name, metadata.account_label.as_deref()),
        status,
        auth_present,
        plan_name: metadata.plan_name.clone(),
        subscription_days_left: compute_subscription_days_left(metadata.subscription_expires_at.as_deref()),
        quota: metadata.quota.clone(),
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
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(super::paths::get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let all_profile_dirs = list_profile_dirs(&backup_root);
    let current_profile = resolve_current_profile(&backup_root);
    let all_cards = all_profile_dirs
        .iter()
        .map(|profile_dir| build_profile_card(profile_dir, current_profile.as_deref(), Some(&codex_home)))
        .collect::<Vec<_>>();

    let total_profiles = all_cards.len() as u32;
    let total_pages = ((total_profiles + DEFAULT_PAGE_SIZE - 1) / DEFAULT_PAGE_SIZE).max(1);
    let page = page.clamp(1, total_pages);
    let start = ((page - 1) * DEFAULT_PAGE_SIZE) as usize;
    let end = (start + DEFAULT_PAGE_SIZE as usize).min(all_cards.len());

    let (current_card, current_quota_card) = match current_profile {
        Some(ref current_profile) if backup_root.join(current_profile).is_dir() => {
            let metadata: ProfileMetadata = load_profile_metadata(current_profile, Some(&codex_home));
            let current_profile_dir = backup_root.join(current_profile);
            (
                Some(CurrentCard {
                    folder_name: current_profile.clone(),
                    display_title: build_display_title(current_profile, metadata.account_label.as_deref()),
                    plan_name: metadata.plan_name.clone(),
                    subscription_days_left: compute_subscription_days_left(metadata.subscription_expires_at.as_deref()),
                    profile_folder_path: current_profile_dir.to_string_lossy().into_owned(),
                }),
                Some(metadata.quota),
            )
        }
        _ => (None, None),
    };

    Ok(DashboardResponse {
        paging: PagingInfo {
            page,
            page_size: DEFAULT_PAGE_SIZE,
            total_profiles,
            total_pages,
            has_previous: page > 1,
            has_next: page < total_pages,
        },
        profiles: all_cards[start..end].to_vec(),
        current_card,
        current_quota_card,
        runtime: RuntimeSummary {
            codex_running: process::is_codex_app_running(),
            last_autosave_at: latest_autosave_timestamp(Some(&codex_home)),
        },
    })
}
