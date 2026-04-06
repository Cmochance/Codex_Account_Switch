from __future__ import annotations

import json
from pathlib import Path

from fastapi.testclient import TestClient

from backend.api import app


def test_dashboard_endpoint_returns_page(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    profile_dir = backup_root / "a"
    profile_dir.mkdir(parents=True, exist_ok=True)
    (profile_dir / "auth.json").write_text("auth\n", encoding="utf-8")
    (profile_dir / "profile.json").write_text(
        json.dumps(
            {
                "account_label": "Work",
                "plan_name": "ChatGPT Pro",
                "subscription_expires_at": "2099-05-01",
                "quota": {
                    "five_hour": {"remaining_percent": 74, "refresh_at": "2026-04-06T15:20:00+08:00"},
                    "weekly": {"remaining_percent": 41, "refresh_at": "2026-04-08T09:00:00+08:00"},
                },
            }
        ),
        encoding="utf-8",
    )
    (backup_root / ".current_profile").write_text("a\n", encoding="utf-8")

    monkeypatch.setenv("CODEX_HOME", str(codex_home))
    monkeypatch.setattr("backend.profile_store.codex_switch.is_codex_app_running", lambda: False)

    client = TestClient(app)
    response = client.get("/api/dashboard?page=1")

    assert response.status_code == 200
    payload = response.json()
    assert payload["profiles"][0]["folder_name"] == "a"
    assert payload["current_card"]["display_title"] == "A / Work"
    assert payload["current_quota_card"]["five_hour"]["remaining_percent"] == 74


def test_switch_endpoint_returns_backend_error(monkeypatch):
    client = TestClient(app)
    response = client.post("/api/profiles/switch", json={"profile": "../bad"})

    assert response.status_code == 400
    assert response.json()["error_code"] == "INVALID_PROFILE_NAME"


def test_open_codex_endpoint_returns_path(monkeypatch):
    client = TestClient(app)
    monkeypatch.setattr("backend.api.open_codex_app", lambda: "C:/Codex/Codex.exe")

    response = client.post("/api/app/open-codex")

    assert response.status_code == 200
    assert response.json()["path"] == "C:/Codex/Codex.exe"


def test_add_profile_endpoint_creates_template_files(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    monkeypatch.setenv("CODEX_HOME", str(codex_home))
    client = TestClient(app)

    response = client.post(
        "/api/profiles/add",
        json={"folder_name": "e", "account_label": "Client 2"},
    )

    assert response.status_code == 200
    created_path = Path(response.json()["path"])
    assert created_path.name == "e"
    assert (created_path / "auth.json").is_file()
    assert (created_path / "profile.json").is_file()
    metadata = json.loads((created_path / "profile.json").read_text(encoding="utf-8"))
    assert metadata["folder_name"] == "e"
    assert metadata["account_label"] == "Client 2"


def test_contact_endpoint_opens_fixed_url(monkeypatch):
    client = TestClient(app)
    monkeypatch.setattr(
        "backend.api.open_contact_url",
        lambda: "https://github.com/Cmochance/Codex_Account_Switch",
    )

    response = client.post("/api/contact/open")

    assert response.status_code == 200
    assert response.json()["path"] == "https://github.com/Cmochance/Codex_Account_Switch"
