from __future__ import annotations

from pathlib import Path

from windows import install


def test_install_creates_profiles_seeds_auth_and_writes_shim(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    codex_home.mkdir()
    (codex_home / "auth.json").write_text("seed-auth\n", encoding="utf-8")
    monkeypatch.setenv("CODEX_HOME", str(codex_home))
    monkeypatch.setattr(install, "resolve_real_codex_path", lambda managed_shim_path: Path("C:/Codex/bin/codex.exe"))
    monkeypatch.setattr(install, "ensure_dir_on_user_path", lambda path: True)
    monkeypatch.setattr(install, "detect_codex_app_path", lambda: Path("C:/Program Files/Codex/Codex.exe"))
    monkeypatch.setattr(install, "utc_timestamp", lambda: "2026-04-05T00:00:00Z")

    exit_code = install.main()

    assert exit_code == 0
    for profile in ("a", "b", "c", "d"):
        assert (codex_home / "account_backup" / profile).is_dir()
    assert (codex_home / "account_backup" / "a" / "auth.json").read_text(encoding="utf-8") == "seed-auth\n"
    placeholder = install.placeholder_auth_template_path().read_text(encoding="utf-8")
    for profile in ("b", "c", "d"):
        assert (codex_home / "account_backup" / profile / "auth.json").read_text(encoding="utf-8") == placeholder
    assert (codex_home / "account_backup" / ".current_profile").read_text(encoding="utf-8") == "a\n"
    assert (
        (codex_home / "account_backup" / "a" / ".active_profile").read_text(encoding="utf-8")
        == "activated_at=2026-04-05T00:00:00Z\n"
    )
    shim_contents = (codex_home / "bin" / "codex.cmd").read_text(encoding="utf-8")
    assert "codex_switch.py" in shim_contents
    state_contents = (codex_home / "account_backup" / "windows" / "install_state.json").read_text(encoding="utf-8")
    assert '"real_codex_path": "C:/Codex/bin/codex.exe"' in state_contents
    assert '"path_added_by_installer": true' in state_contents


def test_install_without_root_auth_creates_placeholders_but_no_active_profile(monkeypatch, tmp_path, capsys):
    codex_home = tmp_path / ".codex"
    codex_home.mkdir()
    monkeypatch.setenv("CODEX_HOME", str(codex_home))
    monkeypatch.setattr(install, "resolve_real_codex_path", lambda managed_shim_path: Path("C:/Codex/bin/codex.exe"))
    monkeypatch.setattr(install, "ensure_dir_on_user_path", lambda path: True)
    monkeypatch.setattr(install, "detect_codex_app_path", lambda: None)

    exit_code = install.main()

    captured = capsys.readouterr()
    placeholder = install.placeholder_auth_template_path().read_text(encoding="utf-8")
    assert exit_code == 0
    for profile in ("a", "b", "c", "d"):
        assert (codex_home / "account_backup" / profile / "auth.json").read_text(encoding="utf-8") == placeholder
    assert not (codex_home / "account_backup" / ".current_profile").exists()
    assert "left profile auth files as placeholders" in captured.err


def test_install_preserves_existing_active_profile(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    backup_root = codex_home / "account_backup"
    codex_home.mkdir()
    (codex_home / "auth.json").write_text("seed-auth\n", encoding="utf-8")
    (backup_root / "b").mkdir(parents=True, exist_ok=True)
    (backup_root / ".current_profile").write_text("b\n", encoding="utf-8")
    (backup_root / "b" / ".active_profile").write_text("activated_at=old\n", encoding="utf-8")
    monkeypatch.setenv("CODEX_HOME", str(codex_home))
    monkeypatch.setattr(install, "resolve_real_codex_path", lambda managed_shim_path: Path("C:/Codex/bin/codex.exe"))
    monkeypatch.setattr(install, "ensure_dir_on_user_path", lambda path: True)
    monkeypatch.setattr(install, "detect_codex_app_path", lambda: None)

    exit_code = install.main()

    assert exit_code == 0
    assert (backup_root / ".current_profile").read_text(encoding="utf-8") == "b\n"
    assert (backup_root / "b" / ".active_profile").read_text(encoding="utf-8") == "activated_at=old\n"


def test_install_fails_when_real_codex_cannot_be_resolved(monkeypatch, tmp_path, capsys):
    codex_home = tmp_path / ".codex"
    codex_home.mkdir()
    monkeypatch.setenv("CODEX_HOME", str(codex_home))
    monkeypatch.setattr(
        install,
        "resolve_real_codex_path",
        lambda managed_shim_path: (_ for _ in ()).throw(RuntimeError("Error: unable to resolve the real Codex CLI.")),
    )

    exit_code = install.main()

    captured = capsys.readouterr()
    assert exit_code == 1
    assert "unable to resolve the real Codex CLI" in captured.err
    assert not (codex_home / "bin" / "codex.cmd").exists()


def test_resolve_real_codex_path_prefers_windows_shim_over_extensionless_script(monkeypatch, tmp_path):
    managed_shim_path = tmp_path / ".codex" / "bin" / "codex.cmd"
    managed_shim_path.parent.mkdir(parents=True)
    managed_shim_path.write_text("@echo off\r\n", encoding="utf-8")

    npm_dir = tmp_path / "Roaming" / "npm"
    npm_dir.mkdir(parents=True)
    (npm_dir / "codex").write_text("#!/bin/sh\n", encoding="utf-8")
    cmd_shim = npm_dir / "codex.cmd"
    cmd_shim.write_text("@echo off\r\n", encoding="utf-8")

    class Result:
        stdout = f"{managed_shim_path}\n{npm_dir / 'codex'}\n{cmd_shim}\n"
        stderr = ""

    monkeypatch.setattr(install, "load_install_state", lambda codex_home=None: {})
    monkeypatch.setattr(install.subprocess, "run", lambda *args, **kwargs: Result())
    monkeypatch.setattr(install.shutil, "which", lambda name: None)

    assert install.resolve_real_codex_path(managed_shim_path) == cmd_shim
