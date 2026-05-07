// One-off live test of the v1.5.4 quota path against the user's real
// ~/.codex on this machine. Exercises the same lib functions the Tauri
// commands wire up, so any breakage caused by Codex CLI / ChatGPT
// backend drift surfaces here without needing to drive the UI.
//
// Run with:
//   cargo run --manifest-path src-tauri/Cargo.toml \
//     --example live_quota_check -- <profile_folder_name>
//
// Reads but never prints OAuth tokens — only structural / numeric fields
// that already appear in the dashboard UI.

use std::env;

#[cfg(target_os = "macos")]
use codex_switch_lib::macos::profiles_index;
#[cfg(not(target_os = "macos"))]
use codex_switch_lib::windows::profiles_index;

use codex_switch_lib::shared::chatgpt_api;
use codex_switch_lib::shared::paths::get_codex_home;

fn main() {
    let target_profile = env::args().nth(1);
    // Use the same resolution the app's runtime uses: honor CODEX_HOME first,
    // fall back to $HOME/.codex on POSIX or %USERPROFILE%\.codex on Windows.
    // Bypassing this helper (e.g. reading HOME directly) panics on Windows
    // shells that don't set HOME and ignores CODEX_HOME overrides.
    let codex_home = get_codex_home();

    println!("== codex_home: {} ==\n", codex_home.display());

    println!("--- load_profiles_snapshot ---");
    match profiles_index::load_profiles_snapshot(Some(&codex_home)) {
        Ok(snap) => {
            println!(
                "current card: {:?}",
                snap.current_card.as_ref().map(|c| &c.folder_name)
            );
            println!("profile count: {}", snap.profiles.len());
            for p in &snap.profiles {
                println!(
                    "  - {:<8} label={:?} plan={:?} base_url={:?}",
                    p.folder_name, p.account_label, p.plan_name, p.openai_base_url
                );
            }
        }
        Err(error) => {
            println!("FAIL: {error:?}");
        }
    }

    println!("\n--- load_current_live_quota (active profile, JSONL path) ---");
    match profiles_index::load_current_live_quota(Some(&codex_home)) {
        Ok(quota) => {
            println!("profile: {:?}", quota.profile);
            if let Some(q) = &quota.quota {
                println!(
                    "5h window:  remaining={:?}% refresh_at={:?}",
                    q.five_hour.remaining_percent, q.five_hour.refresh_at
                );
                println!(
                    "7d window:  remaining={:?}% refresh_at={:?}",
                    q.weekly.remaining_percent, q.weekly.refresh_at
                );
            } else {
                println!("(no JSONL quota snapshot)");
            }
        }
        Err(error) => {
            println!("FAIL: {error:?}");
        }
    }

    if let Some(profile) = target_profile {
        println!(
            "\n--- chatgpt_api::profile_supports_api_refresh({profile}) ---"
        );
        let backup_root = codex_home.join("account_backup").join(&profile);
        let supports = chatgpt_api::profile_supports_api_refresh(&backup_root);
        println!("supports api refresh: {supports}");

        if supports {
            println!("\n--- chatgpt_api::refresh_profile_via_api({profile}) ---");
            match chatgpt_api::refresh_profile_via_api(&profile, &codex_home) {
                Ok(snapshot) => {
                    println!("ok:");
                    if let Some(q) = &snapshot.quota {
                        println!(
                            "  5h window: remaining={:?}% refresh_at={:?}",
                            q.five_hour.remaining_percent, q.five_hour.refresh_at
                        );
                        println!(
                            "  7d window: remaining={:?}% refresh_at={:?}",
                            q.weekly.remaining_percent, q.weekly.refresh_at
                        );
                    } else {
                        println!("  (no quota in API response)");
                    }
                    if let Some(plan) = &snapshot.plan_type {
                        println!("  plan: {plan}");
                    }
                }
                Err(error) => {
                    println!("FAIL: code={} message={}", error.error_code, error.message);
                }
            }
        }
    } else {
        println!(
            "\n(skip chatgpt_api refresh: pass a profile folder name as argv[1] to test)"
        );
    }
}
