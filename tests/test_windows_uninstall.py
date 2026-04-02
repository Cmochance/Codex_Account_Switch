from __future__ import annotations

from pathlib import Path

from windows import uninstall


def test_uninstall_default_removes_shim_and_state_but_keeps_runtime(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    runtime_dir = codex_home / "account_backup" / "windows"
    bin_dir = codex_home / "bin"
    runtime_dir.mkdir(parents=True)
    bin_dir.mkdir(parents=True)
    (runtime_dir / "install_state.json").write_text(
        '{"path_added_by_installer": true, "managed_bin_dir": "X"}\n',
        encoding="utf-8",
    )
    (runtime_dir / "codex_switch.py").write_text("x\n", encoding="utf-8")
    (runtime_dir / "common.py").write_text("x\n", encoding="utf-8")
    (bin_dir / "codex.cmd").write_text("shim\n", encoding="utf-8")
    monkeypatch.setenv("CODEX_HOME", str(codex_home))

    removed_paths: list[Path] = []
    monkeypatch.setattr(
        uninstall,
        "remove_dir_from_user_path",
        lambda path: removed_paths.append(Path(path)) or True,
    )

    exit_code = uninstall.main([])

    assert exit_code == 0
    assert not (bin_dir / "codex.cmd").exists()
    assert not (runtime_dir / "install_state.json").exists()
    assert (runtime_dir / "codex_switch.py").exists()
    assert removed_paths == [bin_dir]


def test_uninstall_remove_script_removes_runtime(monkeypatch, tmp_path):
    codex_home = tmp_path / ".codex"
    runtime_dir = codex_home / "account_backup" / "windows"
    bin_dir = codex_home / "bin"
    runtime_dir.mkdir(parents=True)
    bin_dir.mkdir(parents=True)
    (runtime_dir / "install_state.json").write_text(
        '{"path_added_by_installer": false, "managed_bin_dir": "X"}\n',
        encoding="utf-8",
    )
    (runtime_dir / "codex_switch.py").write_text("x\n", encoding="utf-8")
    (runtime_dir / "common.py").write_text("x\n", encoding="utf-8")
    (bin_dir / "codex.cmd").write_text("shim\n", encoding="utf-8")
    monkeypatch.setenv("CODEX_HOME", str(codex_home))

    exit_code = uninstall.main(["--remove-script"])

    assert exit_code == 0
    assert not runtime_dir.exists()
