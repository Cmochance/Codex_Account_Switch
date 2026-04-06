from __future__ import annotations

import pytest

from backend.config import validate_profile_name
from backend.errors import BackendError
from backend.models import QuotaWindow


def test_quota_window_rejects_invalid_percent():
    with pytest.raises(ValueError):
        QuotaWindow(remaining_percent=101)


def test_validate_profile_name_rejects_path_traversal():
    with pytest.raises(BackendError) as exc:
        validate_profile_name("../bad")

    assert exc.value.error_code == "INVALID_PROFILE_NAME"

