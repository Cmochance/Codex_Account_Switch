from __future__ import annotations

import subprocess
import sys
import time
from pathlib import Path
from typing import Sequence, TextIO

if __package__ in {None, ""}:
    from common import (  # type: ignore[no-redef]
        ACTIVE_MARKER_FILE,
        APP_NAME,
        APP_PROCESS_NAME,
        autosave_timestamp,
        copy_entry,
        detect_codex_app_path,
        get_auto_save_root,
        get_backup_root,
        get_codex_home,
        get_current_profile_file,
        list_profile_dirs,
        list_profile_names,
        load_install_state,
        overlay_directory_contents,
        read_text_stripped,
        remove_path,
        resolve_windows_invokable_path,
        save_install_state,
        utc_timestamp,
    )
else:
    from .common import (
        ACTIVE_MARKER_FILE,
        APP_NAME,
        APP_PROCESS_NAME,
        autosave_timestamp,
        copy_entry,
        detect_codex_app_path,
        get_auto_save_root,
        get_backup_root,
        get_codex_home,
        get_current_profile_file,
        list_profile_dirs,
        list_profile_names,
        load_install_state,
        overlay_directory_contents,
        read_text_stripped,
        remove_path,
        resolve_windows_invokable_path,
        save_install_state,
        utc_timestamp,
    )


USAGE = """Usage:
  codex switch <profile>
  codex switch list"""


def resolve_current_profile(backup_root: Path) -> str:
    current_profile_file = get_current_profile_file(backup_root.parent)
    profile = read_text_stripped(current_profile_file)
    if profile and (backup_root / profile).is_dir():
        return profile

    for profile_dir in list_profile_dirs(backup_root):
        if (profile_dir / ACTIVE_MARKER_FILE).is_file():
            return profile_dir.name
    return ""


def list_profiles(backup_root: Path, stdout: TextIO) -> None:
    for name in list_profile_names(backup_root):
        print(name, file=stdout)


def backup_root_state_to_profile(profile: str, codex_home: Path, backup_root: Path) -> None:
    profile_dir = backup_root / profile
    if not profile_dir.is_dir():
        return

    managed_names = {"auth.json"}
    for entry in profile_dir.iterdir():
        if entry.name in {".DS_Store", ACTIVE_MARKER_FILE}:
            continue
        managed_names.add(entry.name)

    for name in sorted(managed_names):
        src = codex_home / name
        dst = profile_dir / name
        if src.is_dir():
            copy_entry(src, dst)
        elif src.is_file():
            dst.parent.mkdir(parents=True, exist_ok=True)
            copy_entry(src, dst)
        else:
            remove_path(dst)


def set_active_marker(profile: str, backup_root: Path) -> None:
    for profile_dir in list_profile_dirs(backup_root):
        remove_path(profile_dir / ACTIVE_MARKER_FILE)

    marker = backup_root / profile / ACTIVE_MARKER_FILE
    marker.write_text(f"activated_at={utc_timestamp()}\n", encoding="utf-8")
    get_current_profile_file(backup_root.parent).write_text(f"{profile}\n", encoding="utf-8")


def is_codex_app_running(run=subprocess.run) -> bool:
    try:
        result = run(
            ["tasklist", "/FI", f"IMAGENAME eq {APP_PROCESS_NAME}", "/FO", "CSV", "/NH"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
    except OSError:
        return False
    return APP_PROCESS_NAME.lower() in result.stdout.lower()


def quit_codex_app_if_running(run=subprocess.run, sleep=time.sleep) -> bool:
    if not is_codex_app_running(run=run):
        return False

    run(
        ["taskkill", "/IM", APP_PROCESS_NAME],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    for _ in range(20):
        if not is_codex_app_running(run=run):
            return True
        sleep(0.2)

    run(
        ["taskkill", "/F", "/IM", APP_PROCESS_NAME],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    for _ in range(10):
        if not is_codex_app_running(run=run):
            return True
        sleep(0.2)

    raise RuntimeError(f"Error: {APP_NAME} did not exit cleanly. Close it manually and retry.")


def reopen_codex_app_if_needed(
    app_was_running: bool,
    app_path: str | None = None,
    popen=subprocess.Popen,
    stderr: TextIO | None = None,
) -> None:
    if not app_was_running:
        return

    stderr = sys.stderr if stderr is None else stderr
    resolved_path = Path(app_path) if app_path else detect_codex_app_path()
    if resolved_path is None or not resolved_path.is_file():
        print(f"Warning: could not relaunch {APP_NAME}. Start it manually if needed.", file=stderr)
        return

    try:
        popen([str(resolved_path)])
    except OSError as exc:
        print(f"Warning: failed to relaunch {APP_NAME}: {exc}", file=stderr)


def autosave_auth(codex_home: Path) -> None:
    auth_file = codex_home / "auth.json"
    if not auth_file.is_file():
        return
    snapshot_dir = get_auto_save_root(codex_home) / autosave_timestamp()
    snapshot_dir.mkdir(parents=True, exist_ok=True)
    copy_entry(auth_file, snapshot_dir / "auth.json")


def overlay_profile_to_codex_home(profile_dir: Path, codex_home: Path) -> None:
    overlay_directory_contents(profile_dir, codex_home)


def forward_to_real_codex(argv: Sequence[str], codex_home: Path, stderr: TextIO) -> int:
    state = load_install_state(codex_home)
    target = state.get("real_codex_path")
    if not target:
        print("Error: real Codex CLI path not found. Re-run python windows/install.py.", file=stderr)
        return 1
    resolved_target = resolve_windows_invokable_path(str(target))
    if resolved_target is None:
        print(
            f"Error: real Codex CLI path is not invokable on Windows: {target}. Re-run python windows/install.py.",
            file=stderr,
        )
        return 1
    if str(resolved_target) != str(target):
        state["real_codex_path"] = str(resolved_target)
        save_install_state(state, codex_home=codex_home)

    try:
        return subprocess.call([str(resolved_target), *argv])
    except OSError as exc:
        print(f"Error: failed to launch Codex CLI at {resolved_target}: {exc}", file=stderr)
        return 1


def run_switch_command(
    args: Sequence[str],
    *,
    stdout: TextIO,
    stderr: TextIO,
    codex_home: Path | None = None,
    run=subprocess.run,
    sleep=time.sleep,
    popen=subprocess.Popen,
) -> int:
    codex_home = get_codex_home() if codex_home is None else Path(codex_home)
    backup_root = get_backup_root(codex_home)

    if not backup_root.is_dir():
        print(f"Error: backup folder not found: {backup_root}", file=stderr)
        return 1

    if not args:
        print(USAGE, file=stderr)
        return 1

    command = args[0]
    if command in {"list", "--list", "-l"}:
        list_profiles(backup_root, stdout)
        current_profile = resolve_current_profile(backup_root)
        if current_profile:
            print(f"current: {current_profile}", file=stdout)
        return 0

    profile = command
    profile_dir = backup_root / profile
    if not profile_dir.is_dir():
        print(f"Error: profile not found: {profile}", file=stderr)
        print("Available profiles:", file=stderr)
        list_profiles(backup_root, stderr)
        return 1

    auth_file = profile_dir / "auth.json"
    if not auth_file.is_file():
        print(f"Error: missing auth file: {auth_file}", file=stderr)
        return 1

    app_was_running = False
    if is_codex_app_running(run=run):
        app_was_running = True
        try:
            quit_codex_app_if_running(run=run, sleep=sleep)
        except RuntimeError as exc:
            print(str(exc), file=stderr)
            return 1

    current_profile = resolve_current_profile(backup_root)
    if current_profile:
        backup_root_state_to_profile(current_profile, codex_home, backup_root)

    autosave_auth(codex_home)
    overlay_profile_to_codex_home(profile_dir, codex_home)
    set_active_marker(profile, backup_root)

    state = load_install_state(codex_home)
    reopen_codex_app_if_needed(
        app_was_running,
        app_path=str(state.get("app_path")) if state.get("app_path") else None,
        popen=popen,
        stderr=stderr,
    )

    print(f"Switched to profile: {profile}", file=stdout)
    if current_profile:
        print(f"Backed up current root state to profile: {current_profile}", file=stdout)
    print(f"Auth file replaced: {codex_home / 'auth.json'}", file=stdout)
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    codex_home = get_codex_home()

    if argv and argv[0] == "switch":
        return run_switch_command(argv[1:], stdout=sys.stdout, stderr=sys.stderr, codex_home=codex_home)
    return forward_to_real_codex(argv, codex_home=codex_home, stderr=sys.stderr)


if __name__ == "__main__":
    raise SystemExit(main())
