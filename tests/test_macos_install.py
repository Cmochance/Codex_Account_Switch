from __future__ import annotations

import os
import subprocess
from pathlib import Path


def test_macos_install_points_to_new_script_location():
    install_script = Path(__file__).resolve().parents[1] / "macOS" / "install.sh"

    contents = install_script.read_text(encoding="utf-8")

    assert 'SOURCE_SCRIPT="$PROJECT_ROOT/macOS/codex-switch.sh"' in contents


def run_macos_install(tmp_path: Path, *, with_root_auth: bool) -> tuple[subprocess.CompletedProcess[str], Path]:
    repo_root = Path(__file__).resolve().parents[1]
    home_dir = tmp_path / "home"
    codex_home = tmp_path / ".codex"
    home_dir.mkdir()
    codex_home.mkdir()

    if with_root_auth:
        (codex_home / "auth.json").write_text("seed-auth\n", encoding="utf-8")

    env = os.environ.copy()
    env["HOME"] = str(home_dir)
    env["CODEX_HOME"] = str(codex_home)

    result = subprocess.run(
        ["bash", str(repo_root / "macOS" / "install.sh"), "--no-shell"],
        cwd=repo_root,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    return result, codex_home


def test_macos_install_creates_placeholders_without_root_auth(tmp_path):
    result, codex_home = run_macos_install(tmp_path, with_root_auth=False)

    assert result.returncode == 0
    placeholder = (
        Path(__file__).resolve().parents[1] / "examples" / "account_backup" / "demo" / "auth.json.example"
    ).read_text(encoding="utf-8")
    for profile in ("a", "b", "c", "d"):
        assert (codex_home / "account_backup" / profile / "auth.json").read_text(encoding="utf-8") == placeholder
    assert not (codex_home / "account_backup" / ".current_profile").exists()
    assert "left profile auth files as placeholders" in result.stderr


def test_macos_install_seeds_a_and_initializes_active_profile(tmp_path):
    result, codex_home = run_macos_install(tmp_path, with_root_auth=True)

    assert result.returncode == 0
    placeholder = (
        Path(__file__).resolve().parents[1] / "examples" / "account_backup" / "demo" / "auth.json.example"
    ).read_text(encoding="utf-8")
    assert (codex_home / "account_backup" / "a" / "auth.json").read_text(encoding="utf-8") == "seed-auth\n"
    for profile in ("b", "c", "d"):
        assert (codex_home / "account_backup" / profile / "auth.json").read_text(encoding="utf-8") == placeholder
    assert (codex_home / "account_backup" / ".current_profile").read_text(encoding="utf-8") == "a\n"
    assert "activated_at=" in (codex_home / "account_backup" / "a" / ".active_profile").read_text(encoding="utf-8")
