use std::path::{Path, PathBuf};

use crate::errors::AppError;

#[cfg(target_os = "macos")]
use crate::macos as native;
#[cfg(not(target_os = "macos"))]
use crate::windows as native;

const USAGE: &str = "Usage:\n  codex_switch_cli install\n  codex_switch_cli uninstall [--remove-script]\n  codex_switch_cli shim switch <profile>\n  codex_switch_cli shim switch list\n  codex_switch_cli shim <codex args...>";

fn print_usage() {
    eprintln!("{USAGE}");
}

fn print_install_summary(summary: &native::install::InstallSummary) {
    let default_profile_auth = summary
        .runtime_cli_path
        .parent()
        .and_then(|runtime_dir| runtime_dir.parent())
        .map(|backup_root| backup_root.join("a").join("auth.json"))
        .unwrap_or_else(|| PathBuf::from("a").join("auth.json"));

    if summary.seeded_auth {
        println!(
            "Backed up current login to: {}",
            default_profile_auth.display()
        );
    } else {
        eprintln!("Warning: current auth.json not found; left profile auth files as placeholders.");
    }

    if !summary.placeholder_auth_files.is_empty() {
        println!("Created placeholder auth templates:");
        for auth_file in &summary.placeholder_auth_files {
            println!("- {}", auth_file.display());
        }
    }

    if summary.initialized_default_profile {
        println!("Initialized default active profile: a");
    }

    println!(
        "Installed runtime CLI to: {}",
        summary.runtime_cli_path.display()
    );
    println!(
        "Installed command shim to: {}",
        summary.managed_shim_path.display()
    );
    println!(
        "Resolved real Codex CLI to: {}",
        summary.real_codex_path.display()
    );
    if summary.path_changed {
        println!(
            "Ensured command shim directory is first in PATH: {}",
            summary
                .managed_shim_path
                .parent()
                .unwrap_or(Path::new(""))
                .display()
        );
        println!("Reopen your terminal to refresh PATH.");
    }
}

fn print_uninstall_summary(summary: &native::install::UninstallSummary, codex_home: &Path) {
    if summary.removed_shim {
        println!(
            "Removed command shim: {}",
            summary.managed_shim_path.display()
        );
    } else {
        println!(
            "No managed command shim found at: {}",
            summary.managed_shim_path.display()
        );
    }

    if summary.removed_install_state {
        println!(
            "Removed install state: {}",
            summary.install_state_path.display()
        );
    } else {
        println!(
            "No install state found at: {}",
            summary.install_state_path.display()
        );
    }

    if summary.removed_runtime_cli {
        println!(
            "Removed runtime CLI: {}",
            summary.runtime_cli_path.display()
        );
    } else {
        println!(
            "Runtime CLI kept at: {}",
            summary.runtime_cli_path.display()
        );
    }

    if summary.removed_path_entry {
        println!("Removed PATH entry: {}", codex_home.join("bin").display());
        println!("Reopen your terminal to refresh PATH.");
    }
}

fn ensure_shim_ready(codex_home: &Path) -> Result<(), AppError> {
    let backup_root = native::paths::get_backup_root(Some(codex_home));
    if backup_root.is_dir() {
        native::install::refresh_install_state(codex_home)?;
        return Ok(());
    }

    let current_exe = std::env::current_exe().map_err(|error| {
        AppError::new(
            "CLI_PATH_UNAVAILABLE",
            format!("Failed to resolve current CLI path: {error}"),
        )
    })?;
    let summary = native::install::install_from(&current_exe, Some(codex_home))?;
    print_install_summary(&summary);
    Ok(())
}

fn run_install(codex_home: Option<PathBuf>) -> Result<i32, AppError> {
    let summary = native::install::install_current_exe(codex_home.as_deref())?;
    print_install_summary(&summary);
    Ok(0)
}

fn run_uninstall(args: &[String], codex_home: Option<PathBuf>) -> Result<i32, AppError> {
    let remove_script = args.iter().any(|arg| arg == "--remove-script");
    let codex_home = codex_home.unwrap_or_else(native::paths::get_codex_home);
    let summary = native::install::uninstall(remove_script, Some(&codex_home))?;
    print_uninstall_summary(&summary, &codex_home);
    Ok(0)
}

fn run_shim(args: &[String], codex_home: Option<PathBuf>) -> Result<i32, AppError> {
    let codex_home = codex_home.unwrap_or_else(native::paths::get_codex_home);
    ensure_shim_ready(&codex_home)?;

    if matches!(args.first().map(String::as_str), Some("switch")) {
        let switch_args = &args[1..];
        if switch_args.is_empty() {
            print_usage();
            return Ok(1);
        }

        let command = &switch_args[0];
        if matches!(command.as_str(), "list" | "--list" | "-l") {
            let backup_root = native::paths::get_backup_root(Some(&codex_home));
            for profile_dir in native::paths::list_profile_dirs(&backup_root) {
                if let Some(name) = profile_dir.file_name().and_then(|value| value.to_str()) {
                    println!("{name}");
                }
            }
            if let Some(current_profile) = native::profiles::resolve_current_profile(&backup_root) {
                println!("current: {current_profile}");
            }
            return Ok(0);
        }

        let response = native::switch::switch_profile_with_home(command, Some(&codex_home))?;
        println!("{}", response.message);
        return Ok(0);
    }

    native::process::forward_to_real_codex(args, Some(&codex_home))
}

pub fn run(args: &[String], codex_home: Option<PathBuf>) -> i32 {
    let result = match args.first().map(String::as_str) {
        Some("install") => run_install(codex_home),
        Some("uninstall") => run_uninstall(&args[1..], codex_home),
        Some("shim") => run_shim(&args[1..], codex_home),
        _ => {
            print_usage();
            Ok(1)
        }
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.message);
            1
        }
    }
}
