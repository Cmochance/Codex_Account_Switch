//! Persistent cache for JSONL → `QuotaSummary` parses.
//!
//! Background: `load_latest_local_quota_snapshot_since` is on the
//! 15-second dashboard hot path. Without a cache, every tick walks
//! `~/.codex/sessions/**.jsonl`, sorts by path, and reads up to 32 of
//! the most recent files line-by-line through `serde_json::from_str`
//! looking for a `token_count` event. On a real corpus (1.1 GB / 232
//! files / ~5 MB average on the maintainer's machine), even a single
//! parse pass costs hundreds of milliseconds; the worst case is several
//! seconds blocking the Tauri command thread.
//!
//! This cache short-circuits the common case where neither the file
//! roster nor the latest file's `(mtime, size)` signature has changed
//! since the last successful parse: the on-disk snapshot is returned
//! directly without re-reading any JSONL. Per-file entries also let
//! the slow-path scan skip files that haven't moved.
//!
//! On-disk schema (`<runtime_dir>/quota_cache.json`):
//!
//! ```jsonc
//! {
//!   "schema_version": 1,
//!   // The path/signature/quota we returned the last time we hit this
//!   // function — the dashboard's 15s ticker hits this >99% of the
//!   // time during idle and gets to skip the parse entirely.
//!   "last_snapshot": {
//!     "path": "<absolute>",
//!     "mtime_ms": 1770000000000,
//!     "size": 1234567,
//!     "quota": { "five_hour": {...}, "weekly": {...} },
//!     "source_mtime_ms": 1770000000000
//!   },
//!   // Per-file parse results. Keyed by absolute path. Bounded to
//!   // `MAX_ENTRIES` to keep the file small over many sessions.
//!   "entries": {
//!     "<path>": { "mtime_ms": ..., "size": ..., "quota": ... | null }
//!   }
//! }
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::models::QuotaSummary;

use super::paths::get_quota_cache_path;

const SCHEMA_VERSION: u32 = 1;
/// Entries beyond this count get pruned on save (oldest by mtime
/// first). Caps cache size on a corpus that grows indefinitely without
/// risking a multi-megabyte cache file.
const MAX_ENTRIES: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSnapshot {
    pub path: PathBuf,
    pub mtime_ms: u64,
    pub size: u64,
    pub quota: QuotaSummary,
    /// Mirrors `LocalQuotaSnapshot::source_mtime_ms` — the value
    /// callers compare against `stored_quota_updated_at_ms` when
    /// deciding which side wins.
    #[serde(default)]
    pub source_mtime_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedEntry {
    pub mtime_ms: u64,
    pub size: u64,
    /// `None` means "we parsed this file and confirmed it has no
    /// `token_count` event" — lets the slow-path skip it on the next
    /// scan without re-reading. `Some(...)` is the latest quota
    /// observed in the file.
    pub quota: Option<QuotaSummary>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QuotaCache {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub last_snapshot: Option<CachedSnapshot>,
    #[serde(default)]
    pub entries: HashMap<PathBuf, CachedEntry>,
}

impl QuotaCache {
    pub fn load(codex_home: Option<&Path>) -> Self {
        let path = get_quota_cache_path(codex_home);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let parsed: QuotaCache = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(error) => {
                // Corrupt cache — partial atomic-write fallout, manual
                // edit, or filesystem damage. Surface a single line on
                // stderr so a "dashboard froze for 5 s once" report has
                // something to correlate with, instead of a silent
                // rebuild that overwrites the evidence on next save.
                eprintln!(
                    "quota_cache: failed to parse {} ({error}); rebuilding from scratch",
                    path.display()
                );
                return Self::default();
            }
        };
        if parsed.schema_version != SCHEMA_VERSION {
            // Schema bumps are expected on upgrade — log distinctly from
            // a corrupt-file rebuild so the two cases stay
            // distinguishable in any future bug report.
            eprintln!(
                "quota_cache: schema {} != current {SCHEMA_VERSION}, rebuilding",
                parsed.schema_version
            );
            return Self::default();
        }
        parsed
    }

    pub fn save(&mut self, codex_home: Option<&Path>) {
        self.schema_version = SCHEMA_VERSION;
        self.prune();
        let path = get_quota_cache_path(codex_home);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(error) = fs::create_dir_all(parent) {
                    eprintln!(
                        "quota_cache: failed to create {} ({error})",
                        parent.display()
                    );
                    return;
                }
            }
        }
        let serialized = match serde_json::to_vec_pretty(self) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("quota_cache: failed to serialize cache ({error})");
                return;
            }
        };
        // Atomic write: stage to a sibling tmp file with a nanosecond
        // suffix, then rename. The nanosecond suffix mirrors
        // `chatgpt_api::atomic_write_bytes` and keeps two concurrent
        // writers (e.g. release app + `tauri:dev`, or the GUI racing
        // a CLI example binary) from overwriting each other's tmp
        // file mid-write — without it both processes would fight for
        // a fixed `quota_cache.json.tmp` and the loser's transactional
        // intent would be discarded.
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let mut temp_name = path
            .file_name()
            .map(|name| name.to_os_string())
            .unwrap_or_default();
        temp_name.push(format!(".{suffix}.tmp"));
        let temp_path = path.with_file_name(temp_name);
        if let Err(error) = fs::write(&temp_path, &serialized) {
            let _ = fs::remove_file(&temp_path);
            eprintln!(
                "quota_cache: failed to stage write to {} ({error})",
                temp_path.display()
            );
            return;
        }
        if let Err(error) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            eprintln!(
                "quota_cache: failed to publish write to {} ({error})",
                path.display()
            );
        }
    }

    /// Get the cached entry for `path` if its `(mtime, size)` matches
    /// `signature`. A mismatch (or missing entry) means the file
    /// changed and the caller must re-parse.
    pub fn lookup(&self, path: &Path, signature: (u64, u64)) -> Option<&CachedEntry> {
        let entry = self.entries.get(path)?;
        if entry.mtime_ms == signature.0 && entry.size == signature.1 {
            Some(entry)
        } else {
            None
        }
    }

    pub fn upsert_entry(&mut self, path: PathBuf, entry: CachedEntry) {
        self.entries.insert(path, entry);
    }

    pub fn set_last_snapshot(&mut self, snapshot: CachedSnapshot) {
        self.last_snapshot = Some(snapshot);
    }

    pub fn clear_last_snapshot(&mut self) {
        self.last_snapshot = None;
    }

    fn prune(&mut self) {
        if self.entries.len() <= MAX_ENTRIES {
            return;
        }
        let mut by_mtime: Vec<(PathBuf, u64)> = self
            .entries
            .iter()
            .map(|(path, entry)| (path.clone(), entry.mtime_ms))
            .collect();
        // Newest first; we keep the head and drop the tail.
        by_mtime.sort_by(|left, right| right.1.cmp(&left.1));
        let keep: std::collections::HashSet<PathBuf> = by_mtime
            .into_iter()
            .take(MAX_ENTRIES)
            .map(|(path, _)| path)
            .collect();
        self.entries.retain(|path, _| keep.contains(path));
    }
}

/// Helper: compute `(mtime_ms, size)` from a file path. Returns
/// `(0, 0)` on stat failure so the comparison falls through to a
/// fresh parse.
pub fn file_signature(path: &Path) -> (u64, u64) {
    let Ok(metadata) = fs::metadata(path) else {
        return (0, 0);
    };
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .unwrap_or(0);
    (mtime_ms, metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{QuotaSummary, QuotaWindow};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_codex_home(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-switch-quota-cache-{name}-{unique}"))
    }

    fn sample_quota() -> QuotaSummary {
        QuotaSummary {
            five_hour: QuotaWindow {
                remaining_percent: Some(80),
                refresh_at: Some("2026-05-10 10:00".to_string()),
                ..QuotaWindow::default()
            },
            weekly: QuotaWindow {
                remaining_percent: Some(50),
                refresh_at: None,
                ..QuotaWindow::default()
            },
        }
    }

    #[test]
    fn save_then_load_round_trips_last_snapshot_and_entries() {
        let codex_home = temp_codex_home("round-trip");
        // Mirror the runtime layout the real path helper expects.
        fs::create_dir_all(codex_home.join("account_backup").join("windows")).unwrap();
        fs::create_dir_all(codex_home.join("account_backup").join("macos")).unwrap();

        let mut cache = QuotaCache::default();
        cache.set_last_snapshot(CachedSnapshot {
            path: PathBuf::from("/tmp/sample.jsonl"),
            mtime_ms: 1_770_000_000_000,
            size: 4096,
            quota: sample_quota(),
            source_mtime_ms: Some(1_770_000_000_000),
        });
        cache.upsert_entry(
            PathBuf::from("/tmp/sample.jsonl"),
            CachedEntry {
                mtime_ms: 1_770_000_000_000,
                size: 4096,
                quota: Some(sample_quota()),
            },
        );
        cache.save(Some(&codex_home));

        let reloaded = QuotaCache::load(Some(&codex_home));
        assert_eq!(reloaded.schema_version, SCHEMA_VERSION);
        let last = reloaded.last_snapshot.expect("last_snapshot");
        assert_eq!(last.path, PathBuf::from("/tmp/sample.jsonl"));
        assert_eq!(last.size, 4096);
        assert_eq!(last.quota.five_hour.remaining_percent, Some(80));
        let entry = reloaded
            .entries
            .get(&PathBuf::from("/tmp/sample.jsonl"))
            .expect("entry");
        assert_eq!(entry.size, 4096);
        assert!(entry.quota.is_some());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn lookup_returns_none_when_signature_changes() {
        let mut cache = QuotaCache::default();
        cache.upsert_entry(
            PathBuf::from("/tmp/x.jsonl"),
            CachedEntry {
                mtime_ms: 100,
                size: 200,
                quota: None,
            },
        );

        assert!(cache.lookup(&PathBuf::from("/tmp/x.jsonl"), (100, 200)).is_some());
        // mtime drift: cache miss
        assert!(cache.lookup(&PathBuf::from("/tmp/x.jsonl"), (101, 200)).is_none());
        // size drift: cache miss
        assert!(cache.lookup(&PathBuf::from("/tmp/x.jsonl"), (100, 201)).is_none());
        // unknown path: cache miss
        assert!(cache.lookup(&PathBuf::from("/tmp/y.jsonl"), (100, 200)).is_none());
    }

    #[test]
    fn schema_version_mismatch_yields_empty_cache() {
        let codex_home = temp_codex_home("schema-mismatch");
        fs::create_dir_all(codex_home.join("account_backup").join("windows")).unwrap();
        fs::create_dir_all(codex_home.join("account_backup").join("macos")).unwrap();

        let cache_path = get_quota_cache_path(Some(&codex_home));
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        fs::write(
            &cache_path,
            r#"{ "schema_version": 99, "last_snapshot": null, "entries": {} }"#,
        )
        .unwrap();

        let cache = QuotaCache::load(Some(&codex_home));
        // Future schema → wipe and rebuild.
        assert_eq!(cache.schema_version, 0);
        assert!(cache.last_snapshot.is_none());
        assert!(cache.entries.is_empty());
        let _ = fs::remove_dir_all(&codex_home);
    }

    #[test]
    fn prune_drops_oldest_entries_above_cap() {
        let mut cache = QuotaCache::default();
        // Insert 80 entries; expect prune to retain the newest 64.
        for index in 0..80 {
            cache.upsert_entry(
                PathBuf::from(format!("/tmp/{index:03}.jsonl")),
                CachedEntry {
                    mtime_ms: index as u64,
                    size: 0,
                    quota: None,
                },
            );
        }
        cache.prune();
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        // Newest mtime kept (79); oldest dropped (0).
        assert!(cache.entries.contains_key(&PathBuf::from("/tmp/079.jsonl")));
        assert!(!cache.entries.contains_key(&PathBuf::from("/tmp/000.jsonl")));
    }
}
