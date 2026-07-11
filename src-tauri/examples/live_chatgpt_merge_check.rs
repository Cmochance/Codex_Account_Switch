// Live e2e probe for the Codex → ChatGPT.app merge adaptation.
//
// Run while `npm run tauri:dev` is up (or independently):
//   cargo run --manifest-path src-tauri/Cargo.toml --example live_chatgpt_merge_check
//
// Does NOT quit/relaunch the desktop host and does not print tokens.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
use codex_switch_lib::macos::process::{
    is_codex_app_running, redetect_runnable_codex_cli_paths, suggested_codex_cli_paths,
    MACOS_CODEX_PATH_RESOLVER,
};
#[cfg(target_os = "macos")]
use codex_switch_lib::shared::codex_cli_path::{build_codex_cli_status, redetect_codex_cli_path};
#[cfg(target_os = "macos")]
use codex_switch_lib::shared::paths::get_codex_home;

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("live_chatgpt_merge_check is macOS-only");
        std::process::exit(2);
    }

    #[cfg(target_os = "macos")]
    {
        let mut failures = 0usize;
        let mut skipped = 0usize;
        let codex_home = get_codex_home();
        println!("== live ChatGPT merge e2e ==");
        println!("codex_home: {}", codex_home.display());

        // 1) Desktop host process detection (ChatGPT host, not Codex process name).
        // Keep spawn failures distinct from "not running": collapsing them
        // into `false` would silently disarm the cross-check below and let
        // the probe report green while it verified nothing.
        let probe_pgrep = |name: &str| -> Result<bool, String> {
            Command::new("pgrep")
                .args(["-x", name])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .map_err(|error| error.to_string())
        };
        let running = is_codex_app_running();
        let pgrep_chatgpt = probe_pgrep("ChatGPT");
        let pgrep_codex = probe_pgrep("Codex");
        println!(
            "\n[1] is_codex_app_running={running} (pgrep ChatGPT={pgrep_chatgpt:?}, pgrep Codex={pgrep_codex:?})"
        );
        match (&pgrep_chatgpt, running) {
            (Err(error), _) => {
                eprintln!(
                    "SKIP: pgrep unavailable ({error}) — process-table cross-check is blind this run"
                );
                skipped += 1;
            }
            (Ok(true), false) => {
                eprintln!("FAIL: ChatGPT process is up but is_codex_app_running returned false");
                failures += 1;
            }
            (Ok(_), true) => println!("PASS: desktop host detected"),
            (Ok(false), false) => {
                println!("WARN: ChatGPT not running — detection path not fully exercised");
            }
        }

        // 2) Suggested paths must include the ChatGPT-bundled CLI when present.
        let suggestions = suggested_codex_cli_paths(Some(&codex_home));
        println!("\n[2] suggested_codex_cli_paths ({}):", suggestions.len());
        for path in &suggestions {
            println!("  - {}", path.display());
        }
        let chatgpt_cli = PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex");
        if chatgpt_cli.is_file() {
            if suggestions.iter().any(|p| p == &chatgpt_cli) {
                println!("PASS: ChatGPT.app CLI is in suggestions");
            } else {
                eprintln!("FAIL: ChatGPT.app CLI exists but is missing from suggestions");
                failures += 1;
            }
        } else {
            println!("WARN: /Applications/ChatGPT.app CLI not on disk");
        }

        // 3) Redetect must verify the ChatGPT CLI is runnable and capture a version.
        let redetect = redetect_runnable_codex_cli_paths(Some(&codex_home));
        println!(
            "\n[3] redetect_runnable_codex_cli_paths ({}):",
            redetect.len()
        );
        for candidate in &redetect {
            println!(
                "  - {} ({})",
                candidate.path,
                candidate.version.as_deref().unwrap_or("<no version>")
            );
        }
        if chatgpt_cli.is_file() {
            match redetect
                .iter()
                .find(|c| c.path == chatgpt_cli.to_string_lossy())
            {
                Some(hit) if hit.version.as_deref().unwrap_or("").contains("codex") => {
                    println!("PASS: ChatGPT CLI probed runnable: {:?}", hit.version);
                }
                Some(hit) => {
                    eprintln!(
                        "FAIL: ChatGPT CLI found but version look wrong: {:?}",
                        hit.version
                    );
                    failures += 1;
                }
                None => {
                    eprintln!("FAIL: ChatGPT CLI not returned by redetect probe");
                    failures += 1;
                }
            }
        }

        // 4) Stale user override (old Codex.app path) must fall through to discovery.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let probe_home = std::env::temp_dir().join(format!("codex-switch-e2e-merge-{unique}"));
        let runtime_dir = probe_home.join("account_backup").join("macos");
        fs::create_dir_all(&runtime_dir).expect("create probe runtime dir");
        let install_state = runtime_dir.join("install_state.json");
        fs::write(
            &install_state,
            r#"{
  "real_codex_path": null,
  "path_added_by_installer": false,
  "user_codex_path": "/Applications/Codex.app/Contents/Resources/codex"
}
"#,
        )
        .expect("write stale install_state");

        let status = build_codex_cli_status(&MACOS_CODEX_PATH_RESOLVER, &probe_home);
        println!("\n[4] status with stale Codex.app user_override:");
        println!("  resolved_path = {:?}", status.resolved_path);
        println!("  source        = {}", status.source);
        println!("  suggested     = {:?}", status.suggested_paths);

        if chatgpt_cli.is_file() {
            match status.resolved_path.as_deref() {
                Some(path) if path == chatgpt_cli.to_string_lossy() => {
                    println!("PASS: stale override fell through to ChatGPT.app CLI");
                }
                Some(path) if path.contains("ChatGPT.app") => {
                    println!("PASS: stale override fell through to a ChatGPT host CLI ({path})");
                }
                Some(path) => {
                    // Discovery may still prefer a PATH/npm hit if present; require runnable.
                    if PathBuf::from(path).is_file() {
                        println!("PASS: stale override fell through to runnable path {path}");
                    } else {
                        eprintln!("FAIL: resolved non-file path after stale override: {path}");
                        failures += 1;
                    }
                }
                None => {
                    eprintln!("FAIL: resolver returned None despite ChatGPT.app CLI on disk");
                    failures += 1;
                }
            }
            if status.source == "user_override" {
                eprintln!("FAIL: resolver still claims user_override for a missing file");
                failures += 1;
            }
        }

        // 5) Shared redetect wrapper (what Settings auto-detect button calls).
        let redetect_result = redetect_codex_cli_path(&MACOS_CODEX_PATH_RESOLVER, &probe_home);
        println!(
            "\n[5] redetect_codex_cli_path candidates={} status.source={}",
            redetect_result.candidates.len(),
            redetect_result.status.source
        );
        if redetect_result.candidates.is_empty() && chatgpt_cli.is_file() {
            eprintln!("FAIL: Settings redetect returned zero candidates with ChatGPT CLI present");
            failures += 1;
        } else if !redetect_result.candidates.is_empty() {
            println!(
                "PASS: Settings redetect first candidate = {} ({:?})",
                redetect_result.candidates[0].path, redetect_result.candidates[0].version
            );
        }

        // 6) Confirm the real CLI answers --version (same binary login/app-server use).
        if let Some(resolved) = status.resolved_path.as_ref().map(PathBuf::from) {
            let output = Command::new(&resolved).arg("--version").output();
            match output {
                Ok(out) if out.status.success() => {
                    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    println!("\n[6] `{resolved:?} --version` => {version}");
                    if version.is_empty() {
                        eprintln!("FAIL: empty version string");
                        failures += 1;
                    } else {
                        println!("PASS: resolved CLI is executable");
                    }
                }
                Ok(out) => {
                    eprintln!(
                        "FAIL: --version exited {:?}: {}",
                        out.status.code(),
                        String::from_utf8_lossy(&out.stderr)
                    );
                    failures += 1;
                }
                Err(error) => {
                    eprintln!("FAIL: failed to spawn resolved CLI: {error}");
                    failures += 1;
                }
            }
        }

        if let Err(error) = fs::remove_dir_all(&probe_home) {
            eprintln!(
                "WARN: failed to clean probe dir {}: {error}",
                probe_home.display()
            );
        }

        println!("\n== summary: {failures} failure(s), {skipped} skipped check(s) ==");
        if failures > 0 {
            std::process::exit(1);
        }
    }
}
