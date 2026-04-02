from __future__ import annotations

import sys
from pathlib import Path

if __package__ in {None, ""}:
    from common import (  # type: ignore[no-redef]
        get_codex_home,
        get_install_state_file,
        get_runtime_dir,
        load_install_state,
        remove_dir_from_user_path,
        remove_path,
    )
else:
    from .common import (
        get_codex_home,
        get_install_state_file,
        get_runtime_dir,
        load_install_state,
        remove_dir_from_user_path,
        remove_path,
    )


def is_directory_empty(path: Path) -> bool:
    return path.is_dir() and not any(path.iterdir())


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    remove_script = "--remove-script" in argv

    codex_home = get_codex_home()
    runtime_dir = get_runtime_dir(codex_home)
    state_file = get_install_state_file(codex_home)
    install_state = load_install_state(codex_home)
    bin_dir = codex_home / "bin"
    managed_shim_path = bin_dir / "codex.cmd"

    if managed_shim_path.exists():
        managed_shim_path.unlink()
        print(f"Removed command shim: {managed_shim_path}")
    else:
        print(f"No managed command shim found at: {managed_shim_path}")

    if state_file.exists():
        state_file.unlink()
        print(f"Removed install state: {state_file}")
    else:
        print(f"No install state found at: {state_file}")

    if remove_script:
        remove_path(runtime_dir / "codex_switch.py")
        remove_path(runtime_dir / "common.py")
        remove_path(runtime_dir / "__pycache__")
        print(f"Removed Windows runtime scripts from: {runtime_dir}")
    else:
        print(f"Windows runtime kept at: {runtime_dir}")

    if bool(install_state.get("path_added_by_installer")) and is_directory_empty(bin_dir):
        removed = remove_dir_from_user_path(bin_dir)
        if removed:
            print(f"Removed PATH entry: {bin_dir}")
        else:
            print(f"PATH entry already absent: {bin_dir}")

    if is_directory_empty(bin_dir):
        bin_dir.rmdir()
    if is_directory_empty(runtime_dir):
        runtime_dir.rmdir()

    print("Reopen your terminal to refresh PATH.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
