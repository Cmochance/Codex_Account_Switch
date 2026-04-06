from __future__ import annotations

import json
from pathlib import Path

from pydantic import ValidationError

from .config import get_profile_metadata_path, validate_profile_name
from .models import ProfileMetadata


def load_profile_metadata(profile_name: str, codex_home: Path | None = None) -> ProfileMetadata:
    profile_name = validate_profile_name(profile_name)
    metadata_path = get_profile_metadata_path(profile_name, codex_home)
    if not metadata_path.is_file():
        return ProfileMetadata(folder_name=profile_name)

    try:
        payload = json.loads(metadata_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return ProfileMetadata(folder_name=profile_name)

    if not isinstance(payload, dict):
        return ProfileMetadata(folder_name=profile_name)

    payload.setdefault("folder_name", profile_name)

    try:
        return ProfileMetadata.model_validate(payload)
    except ValidationError:
        return ProfileMetadata(folder_name=profile_name)

