from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

if __package__ in {None, ""}:
    from common import (  # type: ignore[no-redef]
        DEFAULT_PROFILES,
        detect_codex_app_path,
        ensure_dir_on_user_path,
        get_backup_root,
        get_codex_home,
        get_runtime_dir,
        load_install_state,
        save_install_state,
    )
else:
    from .common import (
        DEFAULT_PROFILES,
        detect_codex_app_path,
        ensure_dir_on_user_path,
        get_backup_root,
        get_codex_home,
        get_runtime_dir,
        load_install_state,
        save_install_state,
    )


SOURCE_FILENAMES = ("codex_switch.py", "common.py")


def source_dir() -> Path:
    return Path(__file__).resolve().parent


def write_codex_shim(shim_path: Path, python_executable: str) -> None:
    shim_path.parent.mkdir(parents=True, exist_ok=True)
    shim_contents = f"""@echo off
setlocal
if not defined CODEX_HOME set "CODEX_HOME=%USERPROFILE%\\.codex"
"{python_executable}" "%CODEX_HOME%\\account_backup\\windows\\codex_switch.py" %*
exit /b %ERRORLEVEL%
"""
    shim_path.write_text(shim_contents, encoding="utf-8")


def copy_runtime_sources(runtime_dir: Path) -> None:
    runtime_dir.mkdir(parents=True, exist_ok=True)
    for filename in SOURCE_FILENAMES:
        shutil.copy2(source_dir() / filename, runtime_dir / filename)


def resolve_real_codex_path(managed_shim_path: Path) -> Path:
    candidates: list[str] = []
    existing_state = load_install_state(get_codex_home())
    existing_path = existing_state.get("real_codex_path")
    if existing_path:
        candidates.append(str(existing_path))

    try:
        result = subprocess.run(
            ["where", "codex"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
    except OSError:
        result = None

    if result and result.stdout:
        candidates.extend(line.strip() for line in result.stdout.splitlines() if line.strip())

    fallback = shutil.which("codex")
    if fallback:
        candidates.append(fallback)

    managed_norm = str(managed_shim_path.resolve()).casefold() if managed_shim_path.exists() else str(managed_shim_path).casefold()
    seen: set[str] = set()
    for candidate in candidates:
        candidate_path = Path(candidate)
        normalized = str(candidate_path).casefold()
        if normalized == managed_norm or normalized in seen:
            continue
        seen.add(normalized)
        if candidate_path.exists():
            return candidate_path

    raise RuntimeError(
        "Error: unable to resolve the real Codex CLI. Make sure `codex` is installed before running windows/install.py."
    )


def seed_default_profile(codex_home: Path, backup_root: Path) -> str | None:
    root_auth_file = codex_home / "auth.json"
    default_profile_auth_file = backup_root / "a" / "auth.json"
    if not root_auth_file.is_file():
        return None
    default_profile_auth_file.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(root_auth_file, default_profile_auth_file)
    return str(default_profile_auth_file)


def main() -> int:
    codex_home = get_codex_home()
    backup_root = get_backup_root(codex_home)
    runtime_dir = get_runtime_dir(codex_home)
    managed_bin_dir = codex_home / "bin"
    managed_shim_path = managed_bin_dir / "codex.cmd"

    try:
        real_codex_path = resolve_real_codex_path(managed_shim_path)
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    backup_root.mkdir(parents=True, exist_ok=True)
    for profile in DEFAULT_PROFILES:
        (backup_root / profile).mkdir(parents=True, exist_ok=True)

    copy_runtime_sources(runtime_dir)
    seeded_auth = seed_default_profile(codex_home, backup_root)
    write_codex_shim(managed_shim_path, sys.executable)

    existing_state = load_install_state(codex_home)
    added_to_path = ensure_dir_on_user_path(managed_bin_dir)
    path_added_by_installer = added_to_path or bool(existing_state.get("path_added_by_installer"))

    app_path = detect_codex_app_path()
    save_install_state(
        {
            "app_path": str(app_path) if app_path else "",
            "managed_bin_dir": str(managed_bin_dir),
            "path_added_by_installer": path_added_by_installer,
            "real_codex_path": str(real_codex_path),
        },
        codex_home=codex_home,
    )

    if seeded_auth:
        print(f"Backed up current login to: {seeded_auth}")
    else:
        print(f"Warning: current auth.json not found at {codex_home / 'auth.json'}; skipped seeding profile a.", file=sys.stderr)

    print(f"Installed Windows runtime to: {runtime_dir}")
    print(f"Installed command shim to: {managed_shim_path}")
    if added_to_path:
        print(f"Ensured command shim directory is first in user PATH: {managed_bin_dir}")
    else:
        print(f"Command shim directory already first in user PATH: {managed_bin_dir}")
    print("Reopen your terminal to refresh PATH.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
