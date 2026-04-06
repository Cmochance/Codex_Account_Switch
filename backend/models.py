from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator


class QuotaWindow(BaseModel):
    remaining_percent: int | None = None
    refresh_at: str | None = None

    @field_validator("remaining_percent")
    @classmethod
    def validate_percent(cls, value: int | None) -> int | None:
        if value is None:
            return None
        if not 0 <= value <= 100:
            raise ValueError("remaining_percent must be between 0 and 100")
        return value


class QuotaSummary(BaseModel):
    five_hour: QuotaWindow = Field(default_factory=QuotaWindow)
    weekly: QuotaWindow = Field(default_factory=QuotaWindow)


class ProfileMetadata(BaseModel):
    model_config = ConfigDict(extra="ignore")

    folder_name: str | None = None
    account_label: str | None = None
    plan_name: str | None = None
    subscription_expires_at: str | None = None
    quota: QuotaSummary = Field(default_factory=QuotaSummary)


class ProfileCard(BaseModel):
    folder_name: str
    display_title: str
    status: Literal["current", "available", "missing_auth"]
    auth_present: bool
    plan_name: str | None = None
    subscription_days_left: int | None = None
    quota: QuotaSummary = Field(default_factory=QuotaSummary)


class CurrentCard(BaseModel):
    folder_name: str
    display_title: str
    plan_name: str | None = None
    subscription_days_left: int | None = None
    profile_folder_path: str


class PagingInfo(BaseModel):
    page: int
    page_size: int
    total_profiles: int
    total_pages: int
    has_previous: bool
    has_next: bool


class RuntimeSummary(BaseModel):
    codex_running: bool
    last_autosave_at: str | None = None


class DashboardResponse(BaseModel):
    paging: PagingInfo
    profiles: list[ProfileCard]
    current_card: CurrentCard | None = None
    current_quota_card: QuotaSummary | None = None
    runtime: RuntimeSummary


class SwitchRequest(BaseModel):
    profile: str


class ProfileActionRequest(BaseModel):
    profile: str


class AddProfileRequest(BaseModel):
    folder_name: str
    account_label: str | None = None


class SwitchResponse(BaseModel):
    ok: bool = True
    profile: str
    message: str
    warnings: list[str] = Field(default_factory=list)


class ActionResponse(BaseModel):
    ok: bool = True
    message: str
    path: str | None = None
