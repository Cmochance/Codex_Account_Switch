from __future__ import annotations

import json
import os
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable

try:
    import winreg
except ImportError:  # pragma: no cover - exercised via monkeypatch on non-Windows
    winreg = None  # type: ignore[assignment]


ACTIVE_MARKER_FILE = ".active_profile"
APP_NAME = "Codex"
APP_PROCESS_NAME = "Codex.exe"
CURRENT_PROFILE_FILENAME = ".current_profile"
DEFAULT_PROFILES = ("a", "b", "c", "d")
IGNORED_ENTRY_NAMES = {".DS_Store", ACTIVE_MARKER_FILE}
INSTALL_STATE_FILENAME = "install_state.json"
SPECIAL_PROFILE_DIRS = {"_autosave", "windows"}
WINDOWS_RUNTIME_DIRNAME = "windows"


def get_codex_home() -> Path:
    configured = os.environ.get("CODEX_HOME")
    if configured:
        return Path(configured).expanduser()
    return Path.home() / ".codex"


def get_backup_root(codex_home: Path | None = None) -> Path:
    home = Path(codex_home) if codex_home is not None else get_codex_home()
    return home / "account_backup"


def get_auto_save_root(codex_home: Path | None = None) -> Path:
    return get_backup_root(codex_home) / "_autosave"


def get_current_profile_file(codex_home: Path | None = None) -> Path:
    return get_backup_root(codex_home) / CURRENT_PROFILE_FILENAME


def get_runtime_dir(codex_home: Path | None = None) -> Path:
    return get_backup_root(codex_home) / WINDOWS_RUNTIME_DIRNAME


def get_install_state_file(codex_home: Path | None = None) -> Path:
    return get_runtime_dir(codex_home) / INSTALL_STATE_FILENAME


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def autosave_timestamp() -> str:
    return datetime.now().strftime("%Y%m%d-%H%M%S")


def read_text_stripped(path: Path) -> str:
    if not path.is_file():
        return ""
    return path.read_text(encoding="utf-8").strip()


def is_profile_dir(path: Path) -> bool:
    return path.is_dir() and path.name not in SPECIAL_PROFILE_DIRS


def list_profile_dirs(backup_root: Path) -> list[Path]:
    if not backup_root.is_dir():
        return []
    return sorted((path for path in backup_root.iterdir() if is_profile_dir(path)), key=lambda path: path.name)


def list_profile_names(backup_root: Path) -> list[str]:
    return [path.name for path in list_profile_dirs(backup_root)]


def remove_path(path: Path) -> None:
    if not path.exists() and not path.is_symlink():
        return
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
        return
    path.unlink()


def replace_tree(src: Path, dst: Path) -> None:
    if dst.exists() or dst.is_symlink():
        remove_path(dst)
    shutil.copytree(src, dst, copy_function=shutil.copy2)


def copy_entry(src: Path, dst: Path) -> None:
    if src.is_dir():
        replace_tree(src, dst)
        return
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)


def iter_profile_payload_entries(directory: Path) -> Iterable[Path]:
    for entry in directory.iterdir():
        if entry.name in IGNORED_ENTRY_NAMES:
            continue
        yield entry


def overlay_directory_contents(source_dir: Path, target_dir: Path) -> None:
    target_dir.mkdir(parents=True, exist_ok=True)
    for entry in iter_profile_payload_entries(source_dir):
        copy_entry(entry, target_dir / entry.name)


def load_install_state(codex_home: Path | None = None) -> dict[str, object]:
    state_file = get_install_state_file(codex_home)
    if not state_file.is_file():
        return {}
    try:
        return json.loads(state_file.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def save_install_state(state: dict[str, object], codex_home: Path | None = None) -> Path:
    state_file = get_install_state_file(codex_home)
    state_file.parent.mkdir(parents=True, exist_ok=True)
    state_file.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return state_file


def candidate_app_paths() -> list[Path]:
    candidates: list[Path] = []
    local_app_data = os.environ.get("LOCALAPPDATA")
    program_files = os.environ.get("ProgramFiles")
    if local_app_data:
        candidates.append(Path(local_app_data) / "Programs" / APP_NAME / APP_PROCESS_NAME)
    if program_files:
        candidates.append(Path(program_files) / APP_NAME / APP_PROCESS_NAME)
    return candidates


def detect_codex_app_path() -> Path | None:
    for path in candidate_app_paths():
        if path.is_file():
            return path
    return None


def split_windows_path(value: str) -> list[str]:
    return [entry for entry in value.split(";") if entry]


def _normalize_windows_path_entry(entry: str | Path) -> str:
    return os.path.normcase(os.path.abspath(str(entry)))


def _path_entries_contain(entries: Iterable[str], target: str | Path) -> bool:
    normalized_target = _normalize_windows_path_entry(target)
    return any(_normalize_windows_path_entry(entry) == normalized_target for entry in entries)


def _require_winreg(registry_module):
    if registry_module is None:
        raise RuntimeError("winreg is unavailable on this platform.")
    return registry_module


def read_user_path_value(registry_module=None) -> str:
    registry_module = _require_winreg(winreg if registry_module is None else registry_module)
    with registry_module.OpenKey(
        registry_module.HKEY_CURRENT_USER,
        "Environment",
        0,
        registry_module.KEY_READ | registry_module.KEY_WRITE,
    ) as key:
        try:
            value, _ = registry_module.QueryValueEx(key, "Path")
        except FileNotFoundError:
            return ""
    return value


def write_user_path_value(value: str, registry_module=None) -> None:
    registry_module = _require_winreg(winreg if registry_module is None else registry_module)
    with registry_module.OpenKey(
        registry_module.HKEY_CURRENT_USER,
        "Environment",
        0,
        registry_module.KEY_READ | registry_module.KEY_WRITE,
    ) as key:
        registry_module.SetValueEx(key, "Path", 0, registry_module.REG_EXPAND_SZ, value)


def ensure_dir_on_user_path(path: str | Path, registry_module=None) -> bool:
    current = read_user_path_value(registry_module=registry_module)
    entries = split_windows_path(current)
    if _path_entries_contain(entries, path):
        return False
    new_entries = entries + [str(path)]
    write_user_path_value(";".join(new_entries), registry_module=registry_module)
    return True


def remove_dir_from_user_path(path: str | Path, registry_module=None) -> bool:
    current = read_user_path_value(registry_module=registry_module)
    entries = split_windows_path(current)
    normalized_target = _normalize_windows_path_entry(path)
    kept_entries = [entry for entry in entries if _normalize_windows_path_entry(entry) != normalized_target]
    if kept_entries == entries:
        return False
    write_user_path_value(";".join(kept_entries), registry_module=registry_module)
    return True
