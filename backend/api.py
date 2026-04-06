from __future__ import annotations

from pathlib import Path

from fastapi import FastAPI, Query
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles

from .actions import add_profile, open_codex_app, open_contact_url, open_profile_folder
from .errors import BackendError
from .models import AddProfileRequest, ActionResponse, DashboardResponse, ProfileActionRequest, SwitchRequest, SwitchResponse
from .profile_store import build_dashboard
from .switch_service import switch_profile


app = FastAPI(title="Codex Control Panel Backend", version="0.1.0")
STATIC_ROOT = Path(__file__).parent / "static"
app.mount("/static", StaticFiles(directory=STATIC_ROOT), name="static")


@app.exception_handler(BackendError)
async def handle_backend_error(_request, exc: BackendError):
    return JSONResponse(
        status_code=exc.status_code,
        content={
            "ok": False,
            "error_code": exc.error_code,
            "message": exc.message,
        },
    )


@app.get("/api/dashboard", response_model=DashboardResponse)
def get_dashboard(page: int = Query(default=1, ge=1)) -> DashboardResponse:
    return build_dashboard(page=page)


@app.get("/", response_class=FileResponse)
def get_control_panel() -> FileResponse:
    return FileResponse(STATIC_ROOT / "index.html")


@app.post("/api/profiles/switch", response_model=SwitchResponse)
def post_switch(request: SwitchRequest) -> SwitchResponse:
    return switch_profile(request.profile)


@app.post("/api/profiles/open-folder", response_model=ActionResponse)
def post_open_profile_folder(request: ProfileActionRequest) -> ActionResponse:
    opened_path = open_profile_folder(request.profile)
    return ActionResponse(message="Opened profile folder.", path=opened_path)


@app.post("/api/profiles/add", response_model=ActionResponse)
def post_add_profile(request: AddProfileRequest) -> ActionResponse:
    created_path = add_profile(request.folder_name, account_label=request.account_label)
    return ActionResponse(message="Created profile template.", path=created_path)


@app.post("/api/app/open-codex", response_model=ActionResponse)
def post_open_codex() -> ActionResponse:
    opened_path = open_codex_app()
    return ActionResponse(message="Opened Codex.", path=opened_path)


@app.post("/api/contact/open", response_model=ActionResponse)
def post_open_contact() -> ActionResponse:
    opened_url = open_contact_url()
    return ActionResponse(message="Opened contact URL.", path=opened_url)
