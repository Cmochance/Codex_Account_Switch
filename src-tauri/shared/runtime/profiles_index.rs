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
    build_display_title, compute_subscription_days_left, detect_unmanaged_live_account,
    profile_activated_at_ms, resolve_current_profile,
};
use super::session_usage::{load_latest_local_quota_snapshot_since, normalize_quota_summary};

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
    static SLOT: OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, CachedProfilesIndex>>,
    > = OnceLock::new();
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

/// Invariant for callers mutating per-profile metadata (`auth.json`,
/// `profile.json`, the `.current_profile` marker, or anything that
/// shows up in `ProfileIndexEntry`): the mutation site MUST follow up
/// with a `load_profiles_index` call so the cache repopulates from
/// the post-write disk state. Today every action handler in
/// `shared/commands/actions.rs`, both refresh paths in
/// `mac/win/runtime/refresh_runtime.rs`, the bulk refresh in
/// `dashboard.rs`, and `login_runtime` / `switch_core` all comply —
/// breaking this invariant would silently leak stale data through
/// the 250 ms cache window for the next concurrent IPC pair.
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

    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    let _guard = LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    // Double check cache after acquiring the lock to avoid redundant disk reads/writes
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

pub(crate) fn quota_summary_has_data(quota: &crate::models::QuotaSummary) -> bool {
    quota.five_hour.remaining_percent.is_some()
        || quota.five_hour.refresh_at.is_some()
        || quota.weekly.remaining_percent.is_some()
        || quota.weekly.refresh_at.is_some()
        || quota.rate_limit_reset_credits.is_some()
}

fn select_current_quota(
    entry: &ProfileIndexEntry,
    live_snapshot: Option<&super::session_usage::LocalQuotaSnapshot>,
) -> crate::models::QuotaSummary {
    let stored_updated_at_ms = entry.stored_quota_updated_at_ms.unwrap_or(0);

    // Strictly newer-wins, even when the stored summary is empty: an
    // empty stored card with a *newer* timestamp is the deliberate
    // downgrade-to-free clear written by the API refresh paths, and an
    // older live session must not resurrect the pre-downgrade numbers
    // next to a Free label. A never-refreshed card has no timestamp
    // (0), so any live session still wins there.
    match live_snapshot {
        Some(snapshot) if snapshot.source_mtime_ms.unwrap_or(0) > stored_updated_at_ms => {
            let mut quota = snapshot.quota.clone();
            if quota.rate_limit_reset_credits.is_none() {
                quota.rate_limit_reset_credits =
                    entry.stored_quota.rate_limit_reset_credits.clone();
            }
            quota
        }
        _ => entry.stored_quota.clone(),
    }
}

pub fn load_profiles_snapshot(codex_home: Option<&Path>) -> AppResult<ProfilesSnapshotResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let index = load_profiles_index(Some(&codex_home))?;

    // Surface "the live ~/.codex account isn't saved to any card" so the
    // dashboard can prompt the user. Recomputed every snapshot so it reflects
    // reality even between launches (e.g. an external `codex login` mid-session).
    let unmanaged_live_account =
        detect_unmanaged_live_account(&get_backup_root(Some(&codex_home)), &codex_home);

    // When the live account is unmanaged, the `.current_profile` marker is stale
    // (it names a card the live account doesn't belong to). Bootstrap clears it
    // on launch, but for mid-session drift it hasn't run yet — so suppress the
    // current card here too, or this snapshot would both flag the live account
    // as unmanaged AND show an old card as "current".
    let current_profile = if unmanaged_live_account.is_some() {
        None
    } else {
        index.current_profile.as_deref()
    };
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
        unmanaged_live_account,
    })
}

pub fn load_current_live_quota(codex_home: Option<&Path>) -> AppResult<CurrentQuotaResponse> {
    let codex_home = codex_home.map(PathBuf::from).unwrap_or_else(get_codex_home);
    let backup_root = get_backup_root(Some(&codex_home));
    let index = load_profiles_index(Some(&codex_home))?;

    // Mirror load_profiles_snapshot: when the live account is unmanaged the
    // `.current_profile` marker is stale, so report no current quota rather than
    // the drifted-away card's numbers.
    if detect_unmanaged_live_account(&backup_root, &codex_home).is_some() {
        return Ok(CurrentQuotaResponse {
            profile: None,
            quota: None,
        });
    }

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

    // Scope the live JSONL to sessions written since this profile became
    // active: `~/.codex/sessions` is not swapped on switch, so newer
    // entries from the previously-live account would otherwise win the
    // freshness race and show the *previous* account's usage as this
    // profile's quota for the first minutes after a switch.
    let activated_at_ms = profile_activated_at_ms(&current_profile, &backup_root);
    let live_snapshot = load_latest_local_quota_snapshot_since(Some(&codex_home), activated_at_ms);
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

#[cfg(test)]
mod tests {
    use super::{load_current_live_quota, load_profiles_snapshot};
    use crate::shared::paths::{get_current_profile_file, ACTIVE_MARKER_FILE};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-snapshot-{name}-{unique}"))
    }

    fn auth_with_account(account_id: &str) -> String {
        format!(
            "{{\"tokens\":{{\"account_id\":{}}}}}",
            serde_json::Value::String(account_id.to_string())
        )
    }

    /// codex_home with one managed card "a" (account X) marked current.
    fn seed(name: &str) -> PathBuf {
        let codex_home = temp_codex_home(name);
        let profile_a = codex_home.join("account_backup").join("a");
        fs::create_dir_all(&profile_a).unwrap();
        fs::write(profile_a.join("auth.json"), auth_with_account("acct_X")).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "a\n").unwrap();
        codex_home
    }

    #[test]
    fn snapshot_shows_current_card_when_live_account_matches_marker() {
        let codex_home = seed("snapshot-managed");
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_X")).unwrap();

        let snapshot = load_profiles_snapshot(Some(&codex_home)).unwrap();

        assert_eq!(snapshot.unmanaged_live_account, None);
        assert_eq!(
            snapshot
                .current_card
                .as_ref()
                .map(|card| card.folder_name.as_str()),
            Some("a")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn snapshot_suppresses_current_card_when_live_account_unmanaged() {
        let codex_home = seed("snapshot-unmanaged");
        // Live root drifted to an account no card owns (mid-session external login).
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_Z")).unwrap();

        let snapshot = load_profiles_snapshot(Some(&codex_home)).unwrap();

        assert!(snapshot.unmanaged_live_account.is_some());
        assert!(
            snapshot.current_card.is_none(),
            "stale current card must be suppressed when the live account is unmanaged"
        );
        assert!(snapshot.current_quota_card.is_none());
        assert!(
            snapshot
                .profiles
                .iter()
                .all(|card| card.status != "current"),
            "no card in the list should be flagged current"
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    fn write_quota_session(path: &Path) {
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":11.0,"resets_at":1730000000,"window_minutes":300},"secondary":{"used_percent":12.0,"resets_at":1730600000,"window_minutes":10080}}}}"#;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, format!("{line}\n")).unwrap();
    }

    fn write_active_marker(profile_dir: &Path, activated_at: &str) {
        fs::write(
            profile_dir.join(ACTIVE_MARKER_FILE),
            format!("activated_at={activated_at}\n"),
        )
        .unwrap();
    }

    fn utc_string(offset_seconds: i64) -> String {
        (chrono::Utc::now() + chrono::Duration::seconds(offset_seconds))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }

    // Session JSONL written by the previously-live account (mtime before
    // this profile's activation) must not be attributed to the current
    // profile — the cross-account bleed regression.
    #[test]
    fn current_live_quota_ignores_foreign_live_session_older_than_activation() {
        let codex_home = temp_codex_home("live-quota-foreign-session");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "b\n").unwrap();
        fs::write(profile_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"b","account_label":"b@example.com","quota":{"five_hour":{},"weekly":{}},"quota_updated_at_ms":1}"#,
        )
        .unwrap();
        write_quota_session(&codex_home.join("sessions").join("session-001.jsonl"));
        // Activation stamped after the session file's mtime → the session
        // belongs to whichever account was live before the switch.
        write_active_marker(&profile_dir, &utc_string(5));

        let response = load_current_live_quota(Some(&codex_home)).unwrap();

        assert_eq!(response.profile.as_deref(), Some("b"));
        let quota = response.quota.expect("expected quota");
        assert_eq!(quota.five_hour.remaining_percent, None);
        assert_eq!(quota.weekly.remaining_percent, None);

        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn current_live_quota_uses_live_session_written_after_activation() {
        let codex_home = temp_codex_home("live-quota-own-session");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "b\n").unwrap();
        fs::write(profile_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"b","account_label":"b@example.com","quota":{"five_hour":{"remaining_percent":41},"weekly":{"remaining_percent":63},"rate_limit_reset_credits":{"available_count":1,"credits":[{"granted_at":1783890765,"expires_at":1786482765}]}},"quota_updated_at_ms":1}"#,
        )
        .unwrap();
        write_active_marker(&profile_dir, "2020-01-01T00:00:00Z");
        write_quota_session(&codex_home.join("sessions").join("session-001.jsonl"));

        let response = load_current_live_quota(Some(&codex_home)).unwrap();

        assert_eq!(response.profile.as_deref(), Some("b"));
        let quota = response.quota.expect("expected quota");
        assert_eq!(quota.five_hour.remaining_percent, Some(89));
        assert_eq!(quota.weekly.remaining_percent, Some(88));
        let reset_credits = quota
            .rate_limit_reset_credits
            .expect("stored reset credit details");
        assert_eq!(reset_credits.available_count, Some(1));
        assert_eq!(reset_credits.credits[0].expires_at, Some(1_786_482_765));

        let _ = fs::remove_dir_all(&codex_home);
    }

    // Pre-existing installs have markers without a parseable
    // activated_at (or none at all): fall back to the unfiltered
    // legacy freshness race.
    #[test]
    fn current_live_quota_falls_back_unfiltered_without_activation_marker() {
        let codex_home = temp_codex_home("live-quota-no-marker");
        let profile_dir = codex_home.join("account_backup").join("b");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(get_current_profile_file(Some(&codex_home)), "b\n").unwrap();
        fs::write(profile_dir.join("auth.json"), "profile-b-auth\n").unwrap();
        fs::write(
            profile_dir.join("profile.json"),
            r#"{"folder_name":"b","account_label":"b@example.com","quota":{"five_hour":{"remaining_percent":41},"weekly":{"remaining_percent":63}},"quota_updated_at_ms":1}"#,
        )
        .unwrap();
        write_quota_session(&codex_home.join("sessions").join("session-001.jsonl"));

        let response = load_current_live_quota(Some(&codex_home)).unwrap();

        assert_eq!(response.profile.as_deref(), Some("b"));
        let quota = response.quota.expect("expected quota");
        assert_eq!(quota.five_hour.remaining_percent, Some(89));
        assert_eq!(quota.weekly.remaining_percent, Some(88));

        let _ = fs::remove_dir_all(&codex_home);
    }
}
