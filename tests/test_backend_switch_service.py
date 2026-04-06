from __future__ import annotations

import pytest

from backend.errors import BackendError
from backend.switch_service import switch_profile


def test_switch_profile_returns_structured_success(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"

    def fake_run_switch_command(args, *, stdout, stderr, codex_home=None, **_kwargs):
        assert args == ["b"]
        stdout.write("Switched to profile: b\n")
        return 0

    monkeypatch.setattr("backend.switch_service.codex_switch.run_switch_command", fake_run_switch_command)

    response = switch_profile("b", codex_home=codex_home)

    assert response.profile == "b"
    assert response.message == "Switched to profile: b"
    assert response.warnings == []


def test_switch_profile_maps_missing_auth_error(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"

    def fake_run_switch_command(_args, *, stdout, stderr, codex_home=None, **_kwargs):
        stderr.write("Error: missing auth file: C:/Users/test/.codex/account_backup/b/auth.json\n")
        return 1

    monkeypatch.setattr("backend.switch_service.codex_switch.run_switch_command", fake_run_switch_command)

    with pytest.raises(BackendError) as exc:
        switch_profile("b", codex_home=codex_home)

    assert exc.value.error_code == "PROFILE_AUTH_MISSING"


def test_switch_profile_rejects_concurrent_switch(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    lock_path = codex_home / "account_backup" / ".switch.lock"
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    lock_path.write_text("busy\n", encoding="utf-8")

    monkeypatch.setattr("backend.switch_service.codex_switch.run_switch_command", lambda *args, **kwargs: 0)

    with pytest.raises(BackendError) as exc:
        switch_profile("a", codex_home=codex_home)

    assert exc.value.error_code == "SWITCH_IN_PROGRESS"

