from __future__ import annotations

import io
from pathlib import Path

from windows import codex_switch


def test_list_excludes_special_directories_and_sorts(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    for name in ("b", "_autosave", "a", "windows"):
        (backup_root / name).mkdir(parents=True, exist_ok=True)
    (backup_root / ".current_profile").write_text("b\n", encoding="utf-8")

    stdout = io.StringIO()
    stderr = io.StringIO()

    exit_code = codex_switch.run_switch_command(
        ["list"],
        stdout=stdout,
        stderr=stderr,
        codex_home=codex_home,
    )

    assert exit_code == 0
    assert stdout.getvalue().splitlines() == ["a", "b", "current: b"]
    assert stderr.getvalue() == ""


def test_current_profile_prefers_pointer_file(tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    for name in ("a", "b"):
        (backup_root / name).mkdir(parents=True, exist_ok=True)
    (backup_root / ".current_profile").write_text("b\n", encoding="utf-8")
    (backup_root / "a" / ".active_profile").write_text("activated_at=old\n", encoding="utf-8")

    assert codex_switch.resolve_current_profile(backup_root) == "b"


def test_backup_root_state_copies_files_directories_and_removes_missing(tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    profile_dir = backup_root / "a"
    profile_dir.mkdir(parents=True)

    (codex_home / "auth.json").write_text('{"token":"new"}\n', encoding="utf-8")
    (codex_home / "config").mkdir()
    (codex_home / "config" / "state.txt").write_text("current\n", encoding="utf-8")
    (profile_dir / "config").mkdir()
    (profile_dir / "config" / "stale.txt").write_text("remove-me\n", encoding="utf-8")
    (profile_dir / "obsolete.txt").write_text("remove-me\n", encoding="utf-8")

    codex_switch.backup_root_state_to_profile("a", codex_home, backup_root)

    assert (profile_dir / "auth.json").read_text(encoding="utf-8") == '{"token":"new"}\n'
    assert (profile_dir / "config" / "state.txt").read_text(encoding="utf-8") == "current\n"
    assert not (profile_dir / "config" / "stale.txt").exists()
    assert not (profile_dir / "obsolete.txt").exists()


def test_switch_creates_autosave_updates_markers_and_overlays_profile(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    current_profile_dir = backup_root / "a"
    target_profile_dir = backup_root / "b"
    current_profile_dir.mkdir(parents=True)
    target_profile_dir.mkdir(parents=True)
    (backup_root / ".current_profile").write_text("a\n", encoding="utf-8")
    (current_profile_dir / ".active_profile").write_text("activated_at=old\n", encoding="utf-8")

    (codex_home / "auth.json").write_text("root-auth\n", encoding="utf-8")
    (codex_home / "settings.json").write_text("root-settings\n", encoding="utf-8")
    (codex_home / "keep.txt").write_text("keep-me\n", encoding="utf-8")
    (current_profile_dir / "settings.json").write_text("stale-profile-settings\n", encoding="utf-8")
    (target_profile_dir / "auth.json").write_text("target-auth\n", encoding="utf-8")
    (target_profile_dir / "settings.json").write_text("target-settings\n", encoding="utf-8")

    monkeypatch.setattr(codex_switch, "is_codex_app_running", lambda run=None: False)
    monkeypatch.setattr(codex_switch, "autosave_timestamp", lambda: "20260101-010101")

    stdout = io.StringIO()
    stderr = io.StringIO()
    exit_code = codex_switch.run_switch_command(
        ["b"],
        stdout=stdout,
        stderr=stderr,
        codex_home=codex_home,
    )

    assert exit_code == 0
    assert (backup_root / "_autosave" / "20260101-010101" / "auth.json").read_text(encoding="utf-8") == "root-auth\n"
    assert (codex_home / "auth.json").read_text(encoding="utf-8") == "target-auth\n"
    assert (codex_home / "settings.json").read_text(encoding="utf-8") == "target-settings\n"
    assert (codex_home / "keep.txt").read_text(encoding="utf-8") == "keep-me\n"
    assert (current_profile_dir / "auth.json").read_text(encoding="utf-8") == "root-auth\n"
    assert (current_profile_dir / "settings.json").read_text(encoding="utf-8") == "root-settings\n"
    assert (backup_root / ".current_profile").read_text(encoding="utf-8") == "b\n"
    assert (target_profile_dir / ".active_profile").is_file()
    assert not (current_profile_dir / ".active_profile").exists()
    assert "Switched to profile: b" in stdout.getvalue()
    assert stderr.getvalue() == ""


def test_switch_handles_running_app_and_reopens(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    target_profile_dir = backup_root / "b"
    target_profile_dir.mkdir(parents=True)
    (target_profile_dir / "auth.json").write_text("target-auth\n", encoding="utf-8")
    (codex_home / "auth.json").write_text("root-auth\n", encoding="utf-8")

    events: list[str] = []

    monkeypatch.setattr(codex_switch, "is_codex_app_running", lambda run=None: True)
    monkeypatch.setattr(
        codex_switch,
        "quit_codex_app_if_running",
        lambda run=None, sleep=None: events.append("quit") or True,
    )
    monkeypatch.setattr(
        codex_switch,
        "reopen_codex_app_if_needed",
        lambda app_was_running, app_path=None, popen=None, stderr=None: events.append(f"reopen:{app_was_running}:{app_path}"),
    )
    monkeypatch.setattr(codex_switch, "load_install_state", lambda codex_home=None: {"app_path": "C:/Codex/Codex.exe"})

    stdout = io.StringIO()
    stderr = io.StringIO()
    exit_code = codex_switch.run_switch_command(
        ["b"],
        stdout=stdout,
        stderr=stderr,
        codex_home=codex_home,
    )

    assert exit_code == 0
    assert events == ["quit", "reopen:True:C:/Codex/Codex.exe"]


def test_main_passthrough_uses_real_codex(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    runtime_dir = codex_home / "account_backup" / "windows"
    runtime_dir.mkdir(parents=True)
    (runtime_dir / "install_state.json").write_text(
        '{"real_codex_path": "C:/Codex/bin/codex.exe"}\n',
        encoding="utf-8",
    )
    monkeypatch.setenv("CODEX_HOME", str(codex_home))

    called: list[list[str]] = []
    monkeypatch.setattr(codex_switch.subprocess, "call", lambda args: called.append(args) or 0)

    exit_code = codex_switch.main(["login", "--device"])

    assert exit_code == 0
    assert called == [["C:/Codex/bin/codex.exe", "login", "--device"]]


def test_main_passthrough_recovers_legacy_extensionless_real_codex_path(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    runtime_dir = codex_home / "account_backup" / "windows"
    npm_dir = tmp_path / "Roaming" / "npm"
    runtime_dir.mkdir(parents=True)
    npm_dir.mkdir(parents=True)
    (npm_dir / "codex").write_text("#!/bin/sh\n", encoding="utf-8")
    cmd_shim = npm_dir / "codex.cmd"
    cmd_shim.write_text("@echo off\r\n", encoding="utf-8")
    (runtime_dir / "install_state.json").write_text(
        f'{{"real_codex_path": "{npm_dir / "codex"}"}}\n',
        encoding="utf-8",
    )
    monkeypatch.setenv("CODEX_HOME", str(codex_home))

    called: list[list[str]] = []
    monkeypatch.setattr(codex_switch.subprocess, "call", lambda args: called.append(args) or 0)

    exit_code = codex_switch.main(["--version"])

    assert exit_code == 0
    assert called == [[str(cmd_shim), "--version"]]
    assert f'"real_codex_path": "{cmd_shim}"' in (runtime_dir / "install_state.json").read_text(encoding="utf-8")
