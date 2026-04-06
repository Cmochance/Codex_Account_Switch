from __future__ import annotations

import io
from contextlib import contextmanager
from pathlib import Path

from windows import codex_switch

from .config import get_switch_lock_path, validate_profile_name
from .errors import BackendError
from .models import SwitchResponse


@contextmanager
def acquire_switch_lock(codex_home: Path | None = None):
    lock_path = get_switch_lock_path(codex_home)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        handle = lock_path.open("x", encoding="utf-8")
    except FileExistsError as exc:
        raise BackendError(
            "SWITCH_IN_PROGRESS",
            "A profile switch is already in progress.",
            status_code=409,
        ) from exc
    else:
        handle.write("switching\n")
        handle.close()

    try:
        yield
    finally:
        if lock_path.exists():
            lock_path.unlink()


def _map_switch_error(stderr_text: str) -> tuple[str, int]:
    lowered = stderr_text.lower()
    if "missing auth file" in lowered:
        return ("PROFILE_AUTH_MISSING", 400)
    if "profile not found" in lowered:
        return ("PROFILE_NOT_FOUND", 404)
    if "backup folder not found" in lowered:
        return ("BACKUP_ROOT_MISSING", 500)
    if "did not exit cleanly" in lowered:
        return ("APP_EXIT_FAILED", 409)
    return ("SWITCH_FAILED", 400)


def switch_profile(profile_name: str, *, codex_home: Path | None = None) -> SwitchResponse:
    profile_name = validate_profile_name(profile_name)
    stdout = io.StringIO()
    stderr = io.StringIO()

    with acquire_switch_lock(codex_home):
        exit_code = codex_switch.run_switch_command(
            [profile_name],
            stdout=stdout,
            stderr=stderr,
            codex_home=codex_home,
        )

    if exit_code != 0:
        stderr_text = stderr.getvalue().strip()
        error_code, status_code = _map_switch_error(stderr_text)
        raise BackendError(error_code, stderr_text or "Profile switch failed.", status_code=status_code)

    stdout_lines = [line for line in stdout.getvalue().splitlines() if line.strip()]
    stderr_lines = [line for line in stderr.getvalue().splitlines() if line.strip()]
    message = stdout_lines[0] if stdout_lines else f"Switched to profile: {profile_name}"
    return SwitchResponse(
        profile=profile_name,
        message=message,
        warnings=stderr_lines,
    )
