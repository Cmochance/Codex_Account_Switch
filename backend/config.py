from __future__ import annotations

import re
from pathlib import Path

from windows.common import get_backup_root as get_windows_backup_root
from windows.common import get_codex_home as get_windows_codex_home

from .errors import BackendError


DEFAULT_PAGE_SIZE = 4
PROFILE_METADATA_FILENAME = "profile.json"
SWITCH_LOCK_FILENAME = ".switch.lock"
PROFILE_NAME_PATTERN = re.compile(r"^[A-Za-z0-9_-]+$")
CONTACT_URL = "https://github.com/Cmochance/Codex_Account_Switch"


def get_codex_home() -> Path:
    return get_windows_codex_home()


def get_backup_root(codex_home: Path | None = None) -> Path:
    return get_windows_backup_root(codex_home)


def get_profile_metadata_path(profile_name: str, codex_home: Path | None = None) -> Path:
    return get_backup_root(codex_home) / profile_name / PROFILE_METADATA_FILENAME


def get_switch_lock_path(codex_home: Path | None = None) -> Path:
    return get_backup_root(codex_home) / SWITCH_LOCK_FILENAME


def get_repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def get_auth_template_path() -> Path:
    return get_repo_root() / "examples" / "account_backup" / "demo" / "auth.json.example"


def validate_profile_name(profile_name: str) -> str:
    if not PROFILE_NAME_PATTERN.fullmatch(profile_name):
        raise BackendError(
            "INVALID_PROFILE_NAME",
            f"Invalid profile name: {profile_name!r}",
            status_code=400,
        )
    return profile_name
