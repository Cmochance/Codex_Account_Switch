use std::path::Path;

use chrono::{DateTime, Local, NaiveDate};

use super::fs_ops::read_text_stripped;
use super::metadata::{auth_is_empty_placeholder, load_account_identity_from_path, AccountIdentity};
use super::paths::{get_current_profile_file, list_profile_dirs, ACTIVE_MARKER_FILE};

pub fn build_display_title(profile_name: &str, account_label: Option<&str>) -> String {
    let account_label = account_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("--");

    format!("{profile_name} / {account_label}")
}

pub fn compute_subscription_days_left(subscription_expires_at: Option<&str>) -> Option<i64> {
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

/// Decide which profile slot the live `~/.codex` state should be written back
/// into, verified against the *actual account identity* in `auth.json` rather
/// than blindly trusting the `.current_profile` marker.
///
/// The historical bug ("串号" / cross-contamination): switch and the
/// launch-time bootstrap copied the live root state into whatever profile
/// `resolve_current_profile` named, with no check that the account currently
/// in `~/.codex/auth.json` is the one that profile actually holds. If the live
/// account drifted — a manual `codex login`, the official Codex app re-authing,
/// a multi-account user editing `~/.codex` directly — the marker went stale and
/// the next write-back silently overwrote an unrelated profile's credentials
/// with the wrong account.
///
/// Resolution order:
/// 1. Root identity unknown (placeholder / apikey / missing auth) → fall back
///    to the marker. We can't prove a mismatch, so we must not block a normal
///    apikey / placeholder refresh.
/// 2. The marker slot's identity matches root → write back there (happy path).
/// 3. Root identity matches a *different* managed profile → return that one, so
///    refreshed tokens land in their real owner and never a stranger's slot.
/// 4. The marker slot has no identity (empty placeholder) and root is a
///    brand-new account owned by no profile → seat it into the marker slot.
/// 5. Otherwise (root is an unmanaged account and the marker slot holds a
///    different, identified account) → `None`: refuse the write-back rather
///    than contaminate a slot.
pub fn resolve_backup_target(backup_root: &Path, codex_home: &Path) -> Option<String> {
    let marked = resolve_current_profile(backup_root);

    // (1) Can't identify the live account → preserve legacy behavior.
    let Some(root_identity) = load_account_identity_from_path(&codex_home.join("auth.json")) else {
        return marked;
    };

    let slot_identity = |profile: &str| {
        load_account_identity_from_path(&backup_root.join(profile).join("auth.json"))
    };

    // (2) Marker already points at the live account → keep it.
    if let Some(marked_profile) = marked.as_deref() {
        if slot_identity(marked_profile).is_some_and(|slot| slot.same_account(&root_identity)) {
            return marked;
        }
    }

    // (3) Live account drifted to a different *managed* profile → route there.
    //     The marker wins ties (handled by the early return in (2)).
    if let Some(owner) = find_profile_owning_identity(backup_root, &root_identity) {
        return Some(owner);
    }

    // (4) Marker slot is a *genuine* empty placeholder (no creds of any kind)
    //     and the live account belongs to no profile yet → seat the fresh login
    //     into the marked card. An API-key / malformed / unreadable slot is NOT
    //     a placeholder (its `None` identity means "no OAuth identity", not
    //     "empty") and must never be overwritten, or an API-key card would lose
    //     its credentials.
    if let Some(marked_profile) = marked.as_deref() {
        if auth_is_empty_placeholder(&backup_root.join(marked_profile).join("auth.json")) {
            return marked;
        }
    }

    // (5) Live account is unmanaged and the marker slot holds a different,
    //     identified account → refuse to overwrite it.
    None
}

/// First managed profile whose stored `auth.json` is the *same account* as
/// `identity` (shared account_id or email), or `None` if no profile owns it.
fn find_profile_owning_identity(backup_root: &Path, identity: &AccountIdentity) -> Option<String> {
    for profile_dir in list_profile_dirs(backup_root) {
        let Some(name) = profile_dir.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if load_account_identity_from_path(&profile_dir.join("auth.json"))
            .is_some_and(|slot| slot.same_account(identity))
        {
            return Some(name.to_string());
        }
    }
    None
}

/// Detect the "drifted to an unmanaged account" condition: the live `~/.codex`
/// account has a resolvable identity, but no managed profile owns it. Returns a
/// human-facing label (email when available, else the account id) for the
/// dashboard prompt, or `None` when the live account is unidentifiable
/// (apikey / placeholder / missing) or already owned by a profile.
pub fn detect_unmanaged_live_account(backup_root: &Path, codex_home: &Path) -> Option<String> {
    let root_identity = load_account_identity_from_path(&codex_home.join("auth.json"))?;
    if find_profile_owning_identity(backup_root, &root_identity).is_some() {
        return None;
    }
    root_identity.label()
}

#[cfg(test)]
mod tests {
    use super::{detect_unmanaged_live_account, resolve_backup_target};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-resolve-backup-{name}-{unique}"))
    }

    /// Minimal real auth.json whose stable identity is `acct:<account_id>`.
    fn auth_with_account(account_id: &str) -> String {
        format!("{{\"tokens\":{{\"account_id\":{}}}}}", serde_json::Value::String(account_id.to_string()))
    }

    fn write_profile(backup_root: &Path, profile: &str, auth_body: &str) {
        let dir = backup_root.join(profile);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("auth.json"), auth_body).unwrap();
    }

    fn set_marker(backup_root: &Path, profile: &str) {
        fs::write(backup_root.join(".current_profile"), format!("{profile}\n")).unwrap();
    }

    fn setup(name: &str) -> (PathBuf, PathBuf) {
        let codex_home = temp_codex_home(name);
        let backup_root = codex_home.join("account_backup");
        fs::create_dir_all(&backup_root).unwrap();
        (codex_home, backup_root)
    }

    // (2) Marker matches the live account → write back to the marked slot.
    #[test]
    fn returns_marked_when_root_identity_matches_marker_slot() {
        let (codex_home, backup_root) = setup("happy-path");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        write_profile(&backup_root, "b", &auth_with_account("acct_Y"));
        set_marker(&backup_root, "a");
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_X")).unwrap();

        assert_eq!(
            resolve_backup_target(&backup_root, &codex_home).as_deref(),
            Some("a")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    // (5) The core 串号 guard: live account (Z) differs from the marker slot's
    // account (X) and matches no profile → refuse the write-back.
    #[test]
    fn returns_none_when_root_drifted_to_unmanaged_account() {
        let (codex_home, backup_root) = setup("drift-unmanaged");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        write_profile(&backup_root, "b", &auth_with_account("acct_Y"));
        set_marker(&backup_root, "a");
        // Live root drifted to a brand-new account Z (e.g. a manual codex login).
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_Z")).unwrap();

        assert_eq!(resolve_backup_target(&backup_root, &codex_home), None);
        let _ = fs::remove_dir_all(&codex_home);
    }

    // (3) Live account drifted to a *different managed* profile → route the
    // write-back to that profile, not the stale marker.
    #[test]
    fn reassigns_to_profile_that_actually_owns_the_live_account() {
        let (codex_home, backup_root) = setup("reassign");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        write_profile(&backup_root, "b", &auth_with_account("acct_Y"));
        set_marker(&backup_root, "a");
        // Marker says "a" but the live account is actually "b"'s.
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_Y")).unwrap();

        assert_eq!(
            resolve_backup_target(&backup_root, &codex_home).as_deref(),
            Some("b")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    // (4) Marker slot is an empty placeholder and the live account is new →
    // seat it into the marked card.
    #[test]
    fn seats_new_account_into_placeholder_marker_slot() {
        let (codex_home, backup_root) = setup("placeholder-seat");
        // Placeholder card has no resolvable identity.
        write_profile(&backup_root, "a", r#"{"tokens":{"account_id":"replace-me"}}"#);
        set_marker(&backup_root, "a");
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_NEW")).unwrap();

        assert_eq!(
            resolve_backup_target(&backup_root, &codex_home).as_deref(),
            Some("a")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    // (1) apikey / unidentifiable live auth → preserve legacy behavior (write
    // back to the marker) so non-OAuth cards keep working.
    #[test]
    fn falls_back_to_marker_when_root_identity_unknown() {
        let (codex_home, backup_root) = setup("apikey-fallback");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        set_marker(&backup_root, "a");
        // apikey-mode auth carries no tokens → no resolvable identity.
        fs::write(codex_home.join("auth.json"), r#"{"auth_mode":"apikey"}"#).unwrap();

        assert_eq!(
            resolve_backup_target(&backup_root, &codex_home).as_deref(),
            Some("a")
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    // (4 guard) Marker points at an API-key card (no OAuth identity, but real
    // credentials). A drifted OAuth root must NOT be seated on top of it.
    #[test]
    fn does_not_seat_drifted_oauth_account_onto_apikey_marker_slot() {
        let (codex_home, backup_root) = setup("apikey-not-seatable");
        let apikey = r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-real-key"}"#;
        write_profile(&backup_root, "a", apikey);
        set_marker(&backup_root, "a");
        // Live root drifted to an OAuth account owned by no card.
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_OAUTH")).unwrap();

        // Must refuse (case 5), not seat into the API-key card (case 4)…
        assert_eq!(resolve_backup_target(&backup_root, &codex_home), None);
        // …and the API-key card's credentials stay intact.
        assert_eq!(
            fs::read_to_string(backup_root.join("a").join("auth.json")).unwrap(),
            apikey
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    // A legacy email-only slot must still match the same account after a later
    // refresh adds account_id — matched via the shared email, not refused.
    #[test]
    fn matches_email_only_slot_against_account_id_identity() {
        use base64::{engine::general_purpose, Engine as _};
        let id_token = |email: &str| {
            let payload =
                general_purpose::URL_SAFE_NO_PAD.encode(format!("{{\"email\":\"{email}\"}}"));
            format!("h.{payload}.s")
        };
        let (codex_home, backup_root) = setup("email-then-account-id");
        // Card "a" was created from an email-only auth (no account_id).
        write_profile(
            &backup_root,
            "a",
            &format!(
                "{{\"tokens\":{{\"id_token\":\"{}\"}}}}",
                id_token("user@example.com")
            ),
        );
        set_marker(&backup_root, "a");
        // Live root for the same account now also carries account_id.
        fs::write(
            codex_home.join("auth.json"),
            format!(
                "{{\"tokens\":{{\"account_id\":\"acct_new\",\"id_token\":\"{}\"}}}}",
                id_token("user@example.com")
            ),
        )
        .unwrap();

        // Same account (matched by email) → write back to "a", not refused…
        assert_eq!(
            resolve_backup_target(&backup_root, &codex_home).as_deref(),
            Some("a")
        );
        // …and not flagged unmanaged.
        assert_eq!(detect_unmanaged_live_account(&backup_root, &codex_home), None);
        let _ = fs::remove_dir_all(&codex_home);
    }

    // detect_unmanaged_live_account: live account owned by no profile → Some(label).
    #[test]
    fn detect_flags_unmanaged_live_account_with_label() {
        let (codex_home, backup_root) = setup("detect-unmanaged");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_Z")).unwrap();

        assert_eq!(
            detect_unmanaged_live_account(&backup_root, &codex_home).as_deref(),
            Some("acct_Z"),
            "an identified live account owned by no profile must be flagged"
        );
        let _ = fs::remove_dir_all(&codex_home);
    }

    // detect_unmanaged_live_account: live account owned by a profile → None.
    #[test]
    fn detect_returns_none_when_live_account_is_managed() {
        let (codex_home, backup_root) = setup("detect-managed");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        fs::write(codex_home.join("auth.json"), auth_with_account("acct_X")).unwrap();

        assert_eq!(detect_unmanaged_live_account(&backup_root, &codex_home), None);
        let _ = fs::remove_dir_all(&codex_home);
    }

    // detect_unmanaged_live_account: unidentifiable live auth (apikey) → None.
    #[test]
    fn detect_returns_none_when_live_account_unidentifiable() {
        let (codex_home, backup_root) = setup("detect-apikey");
        write_profile(&backup_root, "a", &auth_with_account("acct_X"));
        fs::write(codex_home.join("auth.json"), r#"{"auth_mode":"apikey"}"#).unwrap();

        assert_eq!(detect_unmanaged_live_account(&backup_root, &codex_home), None);
        let _ = fs::remove_dir_all(&codex_home);
    }
}
