from __future__ import annotations

import json

from backend.profile_store import build_dashboard


def test_build_dashboard_returns_paged_profiles(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    autosave_root = backup_root / "_autosave" / "20260406-134200"
    autosave_root.mkdir(parents=True, exist_ok=True)

    for name in ("a", "b", "c", "d", "e"):
        profile_dir = backup_root / name
        profile_dir.mkdir(parents=True, exist_ok=True)
        (profile_dir / "auth.json").write_text("auth\n", encoding="utf-8")
        (profile_dir / "profile.json").write_text(
            json.dumps(
                {
                    "account_label": f"Label {name}",
                    "plan_name": "ChatGPT Pro",
                    "subscription_expires_at": "2099-05-01",
                    "quota": {
                        "five_hour": {"remaining_percent": 70, "refresh_at": "2026-04-06T15:20:00+08:00"},
                        "weekly": {"remaining_percent": 40, "refresh_at": "2026-04-08T09:00:00+08:00"},
                    },
                }
            ),
            encoding="utf-8",
        )

    (backup_root / ".current_profile").write_text("b\n", encoding="utf-8")

    monkeypatch.setattr("backend.profile_store.codex_switch.is_codex_app_running", lambda: True)

    dashboard = build_dashboard(page=2, codex_home=codex_home)

    assert dashboard.paging.page == 2
    assert dashboard.paging.total_profiles == 5
    assert dashboard.paging.total_pages == 2
    assert dashboard.paging.has_previous is True
    assert dashboard.paging.has_next is False
    assert [profile.folder_name for profile in dashboard.profiles] == ["e"]
    assert dashboard.current_card is not None
    assert dashboard.current_card.folder_name == "b"
    assert dashboard.current_quota_card is not None
    assert dashboard.runtime.codex_running is True
    assert dashboard.runtime.last_autosave_at == "2026-04-06T13:42:00"


def test_build_dashboard_falls_back_without_metadata(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    profile_dir = backup_root / "a"
    profile_dir.mkdir(parents=True, exist_ok=True)
    (profile_dir / "auth.json").write_text("auth\n", encoding="utf-8")
    (backup_root / ".current_profile").write_text("a\n", encoding="utf-8")

    monkeypatch.setattr("backend.profile_store.codex_switch.is_codex_app_running", lambda: False)

    dashboard = build_dashboard(page=1, codex_home=codex_home)

    assert dashboard.profiles[0].display_title == "A"
    assert dashboard.profiles[0].status == "current"
    assert dashboard.current_card is not None
    assert dashboard.current_card.display_title == "A"

