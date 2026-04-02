from __future__ import annotations

from pathlib import Path


def test_macos_install_points_to_new_script_location():
    install_script = Path(__file__).resolve().parents[1] / "macOS" / "install.sh"

    contents = install_script.read_text(encoding="utf-8")

    assert 'SOURCE_SCRIPT="$PROJECT_ROOT/macOS/codex-switch.sh"' in contents
