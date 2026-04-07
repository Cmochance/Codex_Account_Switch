from __future__ import annotations

from pathlib import Path

from windows import common


def test_candidate_app_paths_scans_openai_codex_subdirectories(monkeypatch, tmp_path):
    local_app_data = tmp_path / "LocalAppData"
    codex_exe = local_app_data / "Programs" / "OpenAI" / "Codex" / "Codex.exe"
    codex_exe.parent.mkdir(parents=True, exist_ok=True)
    codex_exe.write_text("", encoding="utf-8")

    monkeypatch.setenv("LOCALAPPDATA", str(local_app_data))
    monkeypatch.delenv("ProgramFiles", raising=False)
    monkeypatch.setattr(common, "winreg", None)

    candidates = common.candidate_app_paths()

    assert codex_exe in candidates
    assert common.detect_codex_app_path() == codex_exe


def test_candidate_app_paths_includes_registry_app_path(monkeypatch):
    class FakeKey:
        def __init__(self, value: str) -> None:
            self.value = value

        def __enter__(self) -> "FakeKey":
            return self

        def __exit__(self, exc_type, exc, tb) -> bool:
            return False

    class FakeWinReg:
        HKEY_CURRENT_USER = "hkcu"
        HKEY_LOCAL_MACHINE = "hklm"
        KEY_READ = 0

        def OpenKey(self, hive, path, reserved=0, access=0):  # noqa: N802
            if hive == self.HKEY_CURRENT_USER:
                return FakeKey(r"C:\Users\demo\AppData\Local\Programs\Codex\Codex.exe")
            raise OSError("missing")

        def QueryValueEx(self, key, name):  # noqa: N802
            return key.value, None

    monkeypatch.delenv("LOCALAPPDATA", raising=False)
    monkeypatch.delenv("ProgramFiles", raising=False)
    monkeypatch.setattr(common, "winreg", FakeWinReg())

    candidates = common.candidate_app_paths()

    assert Path(r"C:\Users\demo\AppData\Local\Programs\Codex\Codex.exe") in candidates


def test_resolve_windows_invokable_path_prefers_cmd_for_extensionless_candidate(tmp_path):
    npm_dir = tmp_path / "npm"
    npm_dir.mkdir()
    extensionless = npm_dir / "codex"
    cmd_shim = npm_dir / "codex.cmd"
    extensionless.write_text("#!/bin/sh\n", encoding="utf-8")
    cmd_shim.write_text("@echo off\r\n", encoding="utf-8")

    assert common.resolve_windows_invokable_path(extensionless) == cmd_shim
