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

    exit_code = install.main()

    assert exit_code == 0
    for profile in ("a", "b", "c", "d"):
        assert (codex_home / "account_backup" / profile).is_dir()
    assert (codex_home / "account_backup" / "a" / "auth.json").read_text(encoding="utf-8") == "seed-auth\n"
    shim_contents = (codex_home / "bin" / "codex.cmd").read_text(encoding="utf-8")
    assert "codex_switch.py" in shim_contents
    state_contents = (codex_home / "account_backup" / "windows" / "install_state.json").read_text(encoding="utf-8")
    assert '"real_codex_path": "C:/Codex/bin/codex.exe"' in state_contents
    assert '"path_added_by_installer": true' in state_contents


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
