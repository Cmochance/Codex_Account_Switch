use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Instant, UNIX_EPOCH};

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
use super::session_usage::{load_latest_local_quota_snapshot, normalize_quota_summary};

const PROFILES_INDEX_SCHEMA_VERSION: u32 = 3;

/// How long a freshly-rebuilt index lives in the in-process cache
/// before the next `load_profiles_index` call re-reads the disk.
/// Tuned to dedupe the back-to-back IPC pair issued by the front-end
/// `refreshAllData` call (`get_profiles_snapshot` + `get_current_live_quota`
/// fire concurrently via `Promise.all` and each used to do its own
/// reconcile + write of `profiles.json`) without holding stale data
/// across user-visible state changes.
const PROFILES_INDEX_CACHE_TTL_MS: u128 = 250;

struct CachedProfilesIndex {
    fetched_at: Instant,
    index: ProfilesIndex,
}

fn cache_slot() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, CachedProfilesIndex>>
{
    static SLOT: OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, CachedProfilesIndex>>> =
        OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn try_load_cached_index(codex_home: &Path) -> Option<ProfilesIndex> {
    // Tests bypass the cache so per-test fs setup is observable on
    // the next `load_profiles_index` call without explicit
    // invalidation. Production has a single `codex_home` and benefits
    // from the dedup; tests run with throw-away temp dirs and do
    // their own state setup between calls.
    if cfg!(test) {
        return None;
    }
    let slot = cache_slot().lock().ok()?;
    let entry = slot.get(codex_home)?;
    if entry.fetched_at.elapsed().as_millis() >= PROFILES_INDEX_CACHE_TTL_MS {
        return None;
    }
    Some(entry.index.clone())
}

fn store_cached_index(codex_home: &Path, index: &ProfilesIndex) {
    if cfg!(test) {
        return;
    }
    let Ok(mut slot) = cache_slot().lock() else {
        return;
    };
    slot.insert(
        codex_home.to_path_buf(),
        CachedProfilesIndex {
            fetched_at: Instant::now(),
            index: index.clone(),
        },
    );
}

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
        openai_base_url: metadata.openai_base_url,
        auth_present: auth_path.is_file(),
        stored_quota: metadata.quota,
        stored_quota_updated_at_ms: metadata.quota_updated_at_ms,
        last_plan_check_ms: metadata.last_plan_check_ms,
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
    if let Some(cached) = try_load_cached_index(&codex_home) {
        return Ok(cached);
    }
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

    store_cached_index(&codex_home, &index);
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
        account_label: entry.account_label.clone(),
        status,
        auth_present: entry.auth_present,
        has_account_identity: entry.has_account_identity,
        plan_name: entry.plan_name.clone(),
        subscription_days_left: compute_subscription_days_left(
            entry.subscription_expires_at.as_deref(),
        ),
        openai_base_url: entry.openai_base_url.clone(),
        quota: normalize_quota_summary(
            Some(entry.stored_quota.clone()),
            entry.plan_name.as_deref(),
            entry.has_account_identity,
        ),
        last_plan_check_ms: entry.last_plan_check_ms,
    }
}

fn build_current_card(entry: &ProfileIndexEntry, codex_home: &Path) -> CurrentCard {
    let profile_dir = get_backup_root(Some(codex_home)).join(&entry.folder_name);

    CurrentCard {
        folder_name: entry.folder_name.clone(),
        display_title: build_display_title(&entry.folder_name, entry.account_label.as_deref()),
        account_label: entry.account_label.clone(),
        has_account_identity: entry.has_account_identity,
        plan_name: entry.plan_name.clone(),
        subscription_days_left: compute_subscription_days_left(
            entry.subscription_expires_at.as_deref(),
        ),
        profile_folder_path: profile_dir.to_string_lossy().into_owned(),
        last_plan_check_ms: entry.last_plan_check_ms,
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn quota_summary_has_data(quota: &crate::models::QuotaSummary) -> bool {
    quota.five_hour.remaining_percent.is_some()
        || quota.five_hour.refresh_at.is_some()
        || quota.weekly.remaining_percent.is_some()
        || quota.weekly.refresh_at.is_some()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn select_current_quota(
    entry: &ProfileIndexEntry,
    live_snapshot: Option<&super::session_usage::LocalQuotaSnapshot>,
) -> crate::models::QuotaSummary {
    let stored_is_populated = quota_summary_has_data(&entry.stored_quota);
    let stored_updated_at_ms = entry.stored_quota_updated_at_ms.unwrap_or(0);

    match live_snapshot {
        Some(snapshot)
            if snapshot.source_mtime_ms.unwrap_or(0) > stored_updated_at_ms
                || !stored_is_populated =>
        {
            snapshot.quota.clone()
        }
        _ => entry.stored_quota.clone(),
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

    let live_snapshot = load_latest_local_quota_snapshot(Some(&codex_home));
    let quota = normalize_quota_summary(
        Some(select_current_quota(entry, live_snapshot.as_ref())),
        entry.plan_name.as_deref(),
        entry.has_account_identity,
    );

    Ok(CurrentQuotaResponse {
        profile: Some(entry.folder_name.clone()),
        quota: Some(quota),
    })
}
