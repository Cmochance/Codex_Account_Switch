use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::errors::{AppError, AppResult};
use crate::models::{
    CurrentCard, CurrentQuotaResponse, ProfileCard, ProfileIndexEntry, ProfilesIndex,
    ProfilesSnapshotResponse,
};

use super::metadata::load_profile_metadata;
use super::paths::{
    get_backup_root, get_codex_home, get_profiles_index_path, list_profile_dirs, utc_timestamp,
    DEFAULT_PAGE_SIZE,
};
use super::profiles::{
    build_display_title, compute_subscription_days_left, resolve_current_profile,
};
use super::session_usage::{load_latest_local_quota, normalize_quota_summary};

const PROFILES_INDEX_SCHEMA_VERSION: u32 = 1;

fn file_signature(path: &Path) -> (Option<u64>, Option<u64>) {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(_) => return (None, None),
    };

    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|value| u64::try_from(value.as_millis()).ok());

    (modified, Some(metadata.len()))
}

fn build_profile_index_entry(profile_name: &str, codex_home: &Path) -> ProfileIndexEntry {
    let profile_dir = get_backup_root(Some(codex_home)).join(profile_name);
    let metadata = load_profile_metadata(profile_name, Some(codex_home));
    let account_label = metadata
        .account_label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let has_account_identity = account_label.is_some();
    let auth_path = profile_dir.join("auth.json");
    let metadata_path = profile_dir.join("profile.json");
    let (auth_mtime_ms, auth_size) = file_signature(&auth_path);
    let (profile_mtime_ms, profile_size) = file_signature(&metadata_path);

    ProfileIndexEntry {
        folder_name: profile_name.to_string(),
        account_label,
        has_account_identity,
        plan_name: metadata.plan_name,
        subscription_expires_at: metadata.subscription_expires_at,
        auth_present: auth_path.is_file(),
        stored_quota: metadata.quota,
        auth_mtime_ms,
        auth_size,
        profile_mtime_ms,
        profile_size,
        updated_at: utc_timestamp(),
    }
}

fn save_profiles_index(index: &ProfilesIndex, codex_home: &Path) -> AppResult<()> {
    let index_path = get_profiles_index_path(Some(codex_home));
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AppError::new(
                "FS_CREATE_FAILED",
                format!(
                    "Failed to create index directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }

    let serialized = serde_json::to_string_pretty(index).map_err(|error| {
        AppError::new(
            "PROFILES_INDEX_INVALID",
            format!("Failed to serialize profiles index: {error}"),
        )
    })?;

    let temp_path = index_path.with_extension("json.tmp");
    fs::write(&temp_path, format!("{serialized}\n")).map_err(|error| {
        AppError::new(
            "PROFILES_INDEX_WRITE_FAILED",
            format!(
                "Failed to write temp profiles index {}: {error}",
                temp_path.display()
            ),
        )
    })?;

    if index_path.exists() {
        fs::remove_file(&index_path).map_err(|error| {
            AppError::new(
                "PROFILES_INDEX_WRITE_FAILED",
                format!(
                    "Failed to replace existing profiles index {}: {error}",
                    index_path.display()
                ),
            )
        })?;
    }

    fs::rename(&temp_path, &index_path).map_err(|error| {
        AppError::new(
            "PROFILES_INDEX_WRITE_FAILED",
            format!(
                "Failed to move temp profiles index {} -> {}: {error}",
                temp_path.display(),
                index_path.display()
            ),
        )
    })
}

fn rebuild_profiles_index(codex_home: &Path) -> ProfilesIndex {
    let backup_root = get_backup_root(Some(codex_home));
    let current_profile = resolve_current_profile(&backup_root);
    let profiles = list_profile_dirs(&backup_root)
        .iter()
        .filter_map(|profile_dir| profile_dir.file_name().and_then(|name| name.to_str()))
        .map(|profile_name| build_profile_index_entry(profile_name, codex_home))
        .collect::<Vec<_>>();

    ProfilesIndex {
        schema_version: PROFILES_INDEX_SCHEMA_VERSION,
        updated_at: utc_timestamp(),
        current_profile,
        profiles,
    }
}

fn load_profiles_index_file(codex_home: &Path) -> Option<ProfilesIndex> {
    let raw = fs::read_to_string(get_profiles_index_path(Some(codex_home))).ok()?;
    let index = serde_json::from_str::<ProfilesIndex>(&raw).ok()?;
    (index.schema_version == PROFILES_INDEX_SCHEMA_VERSION).then_some(index)
}

fn index_entry_matches_disk(entry: &ProfileIndexEntry, profile_dir: &Path) -> bool {
    let auth_path = profile_dir.join("auth.json");
    let metadata_path = profile_dir.join("profile.json");
    let (auth_mtime_ms, auth_size) = file_signature(&auth_path);
    let (profile_mtime_ms, profile_size) = file_signature(&metadata_path);

    entry.auth_present == auth_path.is_file()
        && entry.auth_mtime_ms == auth_mtime_ms
        && entry.auth_size == auth_size
        && entry.profile_mtime_ms == profile_mtime_ms
        && entry.profile_size == profile_size
}

pub fn load_profiles_index(codex_home: Option<&Path>) -> AppResult<ProfilesIndex> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let (mut index, mut changed) = match load_profiles_index_file(&codex_home) {
        Some(index) => (index, false),
        None => (rebuild_profiles_index(&codex_home), true),
    };
    let current_profile = resolve_current_profile(&backup_root);
    changed = changed
        || index.schema_version != PROFILES_INDEX_SCHEMA_VERSION
        || index.current_profile != current_profile;

    let mut reconciled_profiles = Vec::new();
    for profile_dir in list_profile_dirs(&backup_root) {
        let Some(profile_name) = profile_dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let next_entry = match index
            .profiles
            .iter()
            .find(|entry| entry.folder_name == profile_name)
        {
            Some(entry) if index_entry_matches_disk(entry, &profile_dir) => entry.clone(),
            _ => {
                changed = true;
                build_profile_index_entry(profile_name, &codex_home)
            }
        };

        reconciled_profiles.push(next_entry);
    }

    if reconciled_profiles.len() != index.profiles.len() {
        changed = true;
    }

    index.schema_version = PROFILES_INDEX_SCHEMA_VERSION;
    index.current_profile = current_profile;
    index.profiles = reconciled_profiles;

    if changed {
        index.updated_at = utc_timestamp();
        save_profiles_index(&index, &codex_home)?;
    }

    Ok(index)
}

fn build_profile_card(entry: &ProfileIndexEntry, current_profile: Option<&str>) -> ProfileCard {
    let status = if !entry.auth_present {
        "missing_auth"
    } else if current_profile == Some(entry.folder_name.as_str()) {
        "current"
    } else {
        "available"
    }
    .to_string();

    ProfileCard {
        folder_name: entry.folder_name.clone(),
        display_title: build_display_title(&entry.folder_name, entry.account_label.as_deref()),
        status,
        auth_present: entry.auth_present,
        has_account_identity: entry.has_account_identity,
        plan_name: entry.plan_name.clone(),
        subscription_days_left: compute_subscription_days_left(
            entry.subscription_expires_at.as_deref(),
        ),
        quota: normalize_quota_summary(
            Some(entry.stored_quota.clone()),
            entry.plan_name.as_deref(),
            entry.has_account_identity,
        ),
    }
}

fn build_current_card(entry: &ProfileIndexEntry, codex_home: &Path) -> CurrentCard {
    let profile_dir = get_backup_root(Some(codex_home)).join(&entry.folder_name);

    CurrentCard {
        folder_name: entry.folder_name.clone(),
        display_title: build_display_title(&entry.folder_name, entry.account_label.as_deref()),
        has_account_identity: entry.has_account_identity,
        plan_name: entry.plan_name.clone(),
        subscription_days_left: compute_subscription_days_left(
            entry.subscription_expires_at.as_deref(),
        ),
        profile_folder_path: profile_dir.to_string_lossy().into_owned(),
    }
}

pub fn load_profiles_snapshot(codex_home: Option<&Path>) -> AppResult<ProfilesSnapshotResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let index = load_profiles_index(Some(&codex_home))?;
    let current_profile = index.current_profile.as_deref();
    let current_entry = current_profile.and_then(|profile_name| {
        index
            .profiles
            .iter()
            .find(|entry| entry.folder_name == profile_name)
    });

    Ok(ProfilesSnapshotResponse {
        page_size: DEFAULT_PAGE_SIZE,
        profiles: index
            .profiles
            .iter()
            .map(|entry| build_profile_card(entry, current_profile))
            .collect(),
        current_card: current_entry.map(|entry| build_current_card(entry, &codex_home)),
        current_quota_card: current_entry.map(|entry| {
            normalize_quota_summary(
                Some(entry.stored_quota.clone()),
                entry.plan_name.as_deref(),
                entry.has_account_identity,
            )
        }),
    })
}

pub fn load_current_live_quota(codex_home: Option<&Path>) -> AppResult<CurrentQuotaResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let index = load_profiles_index(Some(&codex_home))?;
    let Some(current_profile) = index.current_profile.clone() else {
        return Ok(CurrentQuotaResponse {
            profile: None,
            quota: None,
        });
    };
    let Some(entry) = index
        .profiles
        .iter()
        .find(|profile| profile.folder_name == current_profile)
    else {
        return Ok(CurrentQuotaResponse {
            profile: Some(current_profile),
            quota: None,
        });
    };

    let quota = normalize_quota_summary(
        Some(
            load_latest_local_quota(Some(&codex_home))
                .unwrap_or_else(|| entry.stored_quota.clone()),
        ),
        entry.plan_name.as_deref(),
        entry.has_account_identity,
    );

    Ok(CurrentQuotaResponse {
        profile: Some(entry.folder_name.clone()),
        quota: Some(quota),
    })
}

#[cfg(test)]
mod tests {
    use super::{load_current_live_quota, load_profiles_index, load_profiles_snapshot};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::windows::paths::{get_current_profile_file, get_profiles_index_path};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-profiles-index-{name}-{unique}"))
    }

    fn write_profile(codex_home: &PathBuf, profile_name: &str, account_label: &str) {
        let profile_dir = codex_home.join("account_backup").join(profile_name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("auth.json"),
            format!(r#"{{"tokens":{{"account_id":"acct_{profile_name}"}}}}"#),
        )
        .unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            format!(
                r#"{{
                    "folder_name":"{profile_name}",
                    "account_label":"{account_label}",
                    "plan_name":"pro"
                }}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn load_profiles_index_creates_index_file() {
        let codex_home = temp_codex_home("create");
        write_profile(&codex_home, "a", "a@example.com");
        fs::write(get_current_profile_file(Some(&codex_home)), "a").unwrap();

        let index = load_profiles_index(Some(&codex_home)).unwrap();

        assert_eq!(index.profiles.len(), 1);
        assert!(get_profiles_index_path(Some(&codex_home)).is_file());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_profiles_snapshot_returns_all_profiles() {
        let codex_home = temp_codex_home("snapshot");
        write_profile(&codex_home, "a", "a@example.com");
        write_profile(&codex_home, "b", "b@example.com");
        fs::write(get_current_profile_file(Some(&codex_home)), "b").unwrap();

        let snapshot = load_profiles_snapshot(Some(&codex_home)).unwrap();

        assert_eq!(snapshot.profiles.len(), 2);
        assert_eq!(
            snapshot
                .current_card
                .as_ref()
                .map(|card| card.folder_name.as_str()),
            Some("b")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn load_current_live_quota_falls_back_to_stored_quota() {
        let codex_home = temp_codex_home("quota");
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
                "account_label":"a@example.com",
                "plan_name":"pro",
                "quota":{
                    "five_hour":{"remaining_percent":60,"refresh_at":"2099-01-01 00:00"},
                    "weekly":{"remaining_percent":80,"refresh_at":"2099-01-08 00:00"}
                }
            }"#,
        )
        .unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a").unwrap();

        let response = load_current_live_quota(Some(&codex_home)).unwrap();

        assert_eq!(response.profile.as_deref(), Some("a"));
        assert_eq!(
            response
                .quota
                .as_ref()
                .and_then(|quota| quota.five_hour.remaining_percent),
            Some(60)
        );
        let _ = fs::remove_dir_all(&codex_home);
    }
}
