from __future__ import annotations

import json
import subprocess
import webbrowser
from pathlib import Path

from windows.common import detect_codex_app_path, get_backup_root, load_install_state

from .config import CONTACT_URL, get_auth_template_path, get_profile_metadata_path, validate_profile_name
from .errors import BackendError
from .models import ProfileMetadata


def open_codex_app(*, codex_home: Path | None = None, popen=subprocess.Popen) -> str:
    state = load_install_state(codex_home)
    app_path = state.get("app_path")
    resolved_path = Path(str(app_path)) if app_path else detect_codex_app_path()
    if resolved_path is None or not resolved_path.is_file():
        raise BackendError("APP_NOT_FOUND", "Codex desktop app path could not be resolved.", status_code=404)

    try:
        popen([str(resolved_path)])
    except OSError as exc:
        raise BackendError("APP_OPEN_FAILED", f"Failed to open Codex: {exc}", status_code=500) from exc
    return str(resolved_path)


def open_profile_folder(profile_name: str, *, codex_home: Path | None = None, popen=subprocess.Popen) -> str:
    profile_name = validate_profile_name(profile_name)
    profile_dir = get_backup_root(codex_home) / profile_name
    if not profile_dir.is_dir():
        raise BackendError("PROFILE_NOT_FOUND", f"Profile not found: {profile_name}", status_code=404)

    try:
        popen(["explorer", str(profile_dir)])
    except OSError as exc:
        raise BackendError("PROFILE_FOLDER_OPEN_FAILED", f"Failed to open profile folder: {exc}", status_code=500) from exc
    return str(profile_dir)


def add_profile(
    folder_name: str,
    *,
    account_label: str | None = None,
    codex_home: Path | None = None,
) -> str:
    folder_name = validate_profile_name(folder_name)
    profile_dir = get_backup_root(codex_home) / folder_name
    if profile_dir.exists():
        raise BackendError(
            "PROFILE_ALREADY_EXISTS",
            f"Profile already exists: {folder_name}",
            status_code=409,
        )

    auth_template_path = get_auth_template_path()
    if not auth_template_path.is_file():
        raise BackendError(
            "AUTH_TEMPLATE_MISSING",
            f"Auth template not found: {auth_template_path}",
            status_code=500,
        )

    profile_dir.mkdir(parents=True, exist_ok=False)
    (profile_dir / "auth.json").write_text(auth_template_path.read_text(encoding="utf-8"), encoding="utf-8")

    metadata = ProfileMetadata(folder_name=folder_name, account_label=account_label)
    metadata_path = get_profile_metadata_path(folder_name, codex_home)
    metadata_path.write_text(
        json.dumps(metadata.model_dump(mode="json"), indent=2) + "\n",
        encoding="utf-8",
    )
    return str(profile_dir)


def open_contact_url(open_fn=webbrowser.open) -> str:
    try:
        opened = open_fn(CONTACT_URL)
    except Exception as exc:  # pragma: no cover - depends on runtime browser configuration
        raise BackendError("CONTACT_URL_OPEN_FAILED", f"Failed to open contact URL: {exc}", status_code=500) from exc

    if opened is False:
        raise BackendError("CONTACT_URL_OPEN_FAILED", "Failed to open contact URL.", status_code=500)
    return CONTACT_URL
