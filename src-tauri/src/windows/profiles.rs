use std::path::Path;

use chrono::{DateTime, Local, NaiveDate};

use super::fs_ops::read_text_stripped;
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::resolve_current_profile;

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-profiles-{name}-{unique}"))
    }

    #[test]
    fn resolve_current_profile_prefers_pointer_file() {
        let codex_home = temp_codex_home("pointer");
        let backup_root = codex_home.join("account_backup");
        fs::create_dir_all(backup_root.join("a")).unwrap();
        fs::create_dir_all(backup_root.join("b")).unwrap();
        fs::write(backup_root.join(".current_profile"), "b").unwrap();
        fs::write(backup_root.join("a").join(".active_profile"), "activated_at=old").unwrap();

        assert_eq!(resolve_current_profile(&backup_root), Some("b".to_string()));
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn resolve_current_profile_falls_back_to_active_marker() {
        let codex_home = temp_codex_home("marker");
        let backup_root = codex_home.join("account_backup");
        fs::create_dir_all(backup_root.join("a")).unwrap();
        fs::write(
            backup_root.join("a").join(".active_profile"),
            "activated_at=now",
        )
        .unwrap();

        assert_eq!(resolve_current_profile(&backup_root), Some("a".to_string()));
        let _ = fs::remove_dir_all(&codex_home);
    }
}
