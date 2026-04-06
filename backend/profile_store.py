from __future__ import annotations

from datetime import date, datetime
from math import ceil
from pathlib import Path

from windows import codex_switch
from windows.common import get_auto_save_root, list_profile_dirs

from .config import DEFAULT_PAGE_SIZE, get_backup_root
from .metadata_store import load_profile_metadata
from .models import CurrentCard, DashboardResponse, PagingInfo, ProfileCard, QuotaSummary, RuntimeSummary


def _build_display_title(profile_name: str, account_label: str | None) -> str:
    folder_title = profile_name.upper() if len(profile_name) == 1 else profile_name
    if account_label:
        return f"{folder_title} / {account_label}"
    return folder_title


def _compute_subscription_days_left(subscription_expires_at: str | None) -> int | None:
    if not subscription_expires_at:
        return None

    parsed: date | None = None
    for parser in (date.fromisoformat, datetime.fromisoformat):
        try:
            value = parser(subscription_expires_at)
        except ValueError:
            continue
        if isinstance(value, datetime):
            parsed = value.date()
        else:
            parsed = value
        break

    if parsed is None:
        return None

    return max((parsed - date.today()).days, 0)


def _latest_autosave_timestamp(codex_home: Path | None = None) -> str | None:
    auto_save_root = get_auto_save_root(codex_home)
    if not auto_save_root.is_dir():
        return None

    candidates = [path.name for path in auto_save_root.iterdir() if path.is_dir()]
    if not candidates:
        return None

    latest = max(candidates)
    try:
        return datetime.strptime(latest, "%Y%m%d-%H%M%S").isoformat()
    except ValueError:
        return latest


def _build_profile_card(profile_dir: Path, *, current_profile: str, codex_home: Path | None = None) -> ProfileCard:
    metadata = load_profile_metadata(profile_dir.name, codex_home)
    auth_present = (profile_dir / "auth.json").is_file()
    status = "current" if profile_dir.name == current_profile else "available"
    if not auth_present:
        status = "missing_auth"

    return ProfileCard(
        folder_name=profile_dir.name,
        display_title=_build_display_title(profile_dir.name, metadata.account_label),
        status=status,
        auth_present=auth_present,
        plan_name=metadata.plan_name,
        subscription_days_left=_compute_subscription_days_left(metadata.subscription_expires_at),
        quota=metadata.quota,
    )


def build_dashboard(*, page: int = 1, codex_home: Path | None = None, page_size: int = DEFAULT_PAGE_SIZE) -> DashboardResponse:
    backup_root = get_backup_root(codex_home)
    all_profile_dirs = list_profile_dirs(backup_root)
    current_profile = codex_switch.resolve_current_profile(backup_root)
    all_cards = [
        _build_profile_card(profile_dir, current_profile=current_profile, codex_home=codex_home)
        for profile_dir in all_profile_dirs
    ]

    total_profiles = len(all_cards)
    total_pages = max(ceil(total_profiles / page_size), 1)
    page = min(max(page, 1), total_pages)
    start = (page - 1) * page_size
    end = start + page_size

    current_card = None
    current_quota_card = None
    if current_profile:
        current_profile_dir = backup_root / current_profile
        if current_profile_dir.is_dir():
            current_metadata = load_profile_metadata(current_profile, codex_home)
            current_card = CurrentCard(
                folder_name=current_profile,
                display_title=_build_display_title(current_profile, current_metadata.account_label),
                plan_name=current_metadata.plan_name,
                subscription_days_left=_compute_subscription_days_left(current_metadata.subscription_expires_at),
                profile_folder_path=str(current_profile_dir),
            )
            current_quota_card = current_metadata.quota

    runtime = RuntimeSummary(
        codex_running=codex_switch.is_codex_app_running(),
        last_autosave_at=_latest_autosave_timestamp(codex_home),
    )

    return DashboardResponse(
        paging=PagingInfo(
            page=page,
            page_size=page_size,
            total_profiles=total_profiles,
            total_pages=total_pages,
            has_previous=page > 1,
            has_next=page < total_pages,
        ),
        profiles=all_cards[start:end],
        current_card=current_card,
        current_quota_card=current_quota_card,
        runtime=runtime,
    )

