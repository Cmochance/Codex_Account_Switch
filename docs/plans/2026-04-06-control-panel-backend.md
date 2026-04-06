# Control Panel Backend Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a local backend that drives the new account-switch control panel, exposes profile data and fixed right-side card data to the frontend, and executes account switching safely on macOS and Windows.

**Architecture:** Add a local-only Python backend using FastAPI. Keep the UI thin: it reads paged profile data, current account summary, and quota metadata from the backend, and sends explicit actions such as switch, add profile, open folder, and open Codex back to the backend. Reuse the existing switching logic where possible, but move future-facing orchestration into shared Python services so the frontend does not depend on shell output or platform-specific scripts.

**Tech Stack:** Python 3.11+, FastAPI, Uvicorn, Pydantic, pytest, existing repo logic under `windows/` plus a new macOS Python adapter.

---

## Product Constraints

- The **right-side two cards are fixed** and always visible.
- The **left-side profile grid is paged**, showing four profiles at a time.
- The backend is **local only** and must not bind to non-loopback interfaces.
- Existing profile switching safety rules remain intact:
  - Target profile folder must exist.
  - Target profile must contain `auth.json`.
  - Root state must be backed up before switch.
  - `auth.json` autosave must be created before switch.
- The current repository can reliably read:
  - profile folders
  - current profile markers
  - `auth.json` existence
  - autosave timestamps
  - app running state
- The current repository **cannot reliably derive** plan type, subscription time left, or quota usage from `auth.json`. Those fields must come from a sidecar metadata file or a future provider adapter.

## Proposed Backend Layout

```text
backend/
├── __init__.py
├── api.py
├── config.py
├── models.py
├── profile_store.py
├── switch_service.py
├── metadata_store.py
├── actions.py
└── platforms/
    ├── __init__.py
    ├── base.py
    ├── macos.py
    └── windows.py
tests/
├── test_backend_api.py
├── test_backend_metadata_store.py
├── test_backend_profile_store.py
└── test_backend_switch_service.py
```

## Data Model

### 1. Runtime profile source

Still comes from `~/.codex/account_backup/<profile>` and existing marker files:

- `.current_profile`
- `.active_profile`
- `auth.json`
- `_autosave/<timestamp>/auth.json`

### 2. New UI metadata sidecar

Add one metadata file per profile:

`~/.codex/account_backup/<profile>/profile.json`

Example:

```json
{
  "folder_name": "a",
  "account_label": "Work",
  "plan_name": "ChatGPT Pro",
  "subscription_expires_at": "2026-05-05",
  "quota": {
    "five_hour": {
      "remaining_percent": 74,
      "refresh_at": "2026-04-06T15:20:00+08:00"
    },
    "weekly": {
      "remaining_percent": 41,
      "refresh_at": "2026-04-08T09:00:00+08:00"
    }
  }
}
```

### 3. Global UI config

Add one global config file:

`~/.codex/account_backup/ui_config.json`

Example:

```json
{
  "profiles_per_page": 4,
  "contact_url": "https://example.com/support",
  "contact_label": "Contact Us"
}
```

## API Contract

### `GET /api/dashboard`

Purpose: bootstrap the entire screen in one request.

Response shape:

```json
{
  "paging": {
    "page": 1,
    "page_size": 4,
    "total_profiles": 7,
    "total_pages": 2,
    "has_previous": false,
    "has_next": true
  },
  "toolbar": {
    "contact_label": "Contact Us",
    "contact_url": "https://example.com/support"
  },
  "profiles": [
    {
      "folder_name": "a",
      "display_title": "A / Work",
      "status": "current",
      "auth_present": true,
      "plan_name": "ChatGPT Pro",
      "subscription_days_left": 29,
      "quota": {
        "five_hour": {
          "remaining_percent": 74,
          "refresh_at": "2026-04-06T15:20:00+08:00"
        },
        "weekly": {
          "remaining_percent": 41,
          "refresh_at": "2026-04-08T09:00:00+08:00"
        }
      }
    }
  ],
  "current_card": {
    "folder_name": "a",
    "display_title": "A / Work",
    "plan_name": "ChatGPT Pro",
    "subscription_days_left": 29,
    "profile_folder_path": "/Users/example/.codex/account_backup/a"
  },
  "current_quota_card": {
    "five_hour": {
      "remaining_percent": 74,
      "refresh_at": "2026-04-06T15:20:00+08:00"
    },
    "weekly": {
      "remaining_percent": 41,
      "refresh_at": "2026-04-08T09:00:00+08:00"
    }
  },
  "runtime": {
    "codex_running": true,
    "last_autosave_at": "2026-04-06T13:42:00+08:00"
  }
}
```

### `POST /api/profiles/switch`

Request:

```json
{
  "profile": "b"
}
```

Response:

```json
{
  "ok": true,
  "profile": "b",
  "message": "Switched to profile: b",
  "warnings": []
}
```

### `POST /api/profiles/add`

Request:

```json
{
  "folder_name": "e",
  "account_label": "Client 2",
  "seed_mode": "template"
}
```

Behavior:

- creates `~/.codex/account_backup/e`
- writes `profile.json`
- optionally writes template `auth.json`
- does **not** switch automatically

### `POST /api/profiles/open-folder`

Request:

```json
{
  "profile": "a"
}
```

Behavior:

- opens that profile folder in Finder/Explorer

### `POST /api/app/open-codex`

Behavior:

- opens Codex desktop app if installed

### `GET /api/contact`

Response:

```json
{
  "label": "Contact Us",
  "url": "https://example.com/support"
}
```

## Task 1: Add backend package, config, and response models

**Files:**
- Create: `backend/__init__.py`
- Create: `backend/config.py`
- Create: `backend/models.py`
- Test: `tests/test_backend_models.py`

**Implementation notes:**
- Add Pydantic models for:
  - `QuotaWindow`
  - `ProfileMetadata`
  - `ProfileCard`
  - `DashboardResponse`
  - `SwitchRequest`
  - `AddProfileRequest`
- Centralize paths in `backend/config.py`:
  - `get_codex_home()`
  - `get_backup_root()`
  - `get_ui_config_path()`
  - `get_profile_metadata_path(profile_name)`
- Reuse the same path rules already implemented in [windows/common.py](/Users/alysechen/alysechen/github/Codex_Account_Switch/windows/common.py).

**Tests:**
- model validation rejects invalid profile names such as `../x`
- missing quota fields are allowed and serialized as `null`
- subscription days left is derived from `subscription_expires_at`

## Task 2: Implement profile and metadata loading

**Files:**
- Create: `backend/profile_store.py`
- Create: `backend/metadata_store.py`
- Test: `tests/test_backend_profile_store.py`
- Test: `tests/test_backend_metadata_store.py`

**Implementation notes:**
- `profile_store.py` loads actual profile folders from disk using the same exclusion rules as current code:
  - ignore `_autosave`
  - ignore `windows`
- It must identify:
  - current profile
  - auth presence
  - last autosave timestamp
- `metadata_store.py` reads `profile.json` and `ui_config.json`.
- Merge filesystem truth with metadata into the frontend shape:
  - real switching status must come from filesystem
  - plan/quota/subscription fields come from metadata
- Pagination must be backend-owned:
  - page size defaults to `4`
  - `page` query param selects which four profiles are returned

**Tests:**
- `GET` logic returns four profiles on page 1 and remaining profiles on page 2
- profiles without metadata still render with fallback title such as `A / a`
- current profile resolution prefers `.current_profile`
- autosave timestamp returns newest `_autosave` folder

## Task 3: Implement switch orchestration and platform actions

**Files:**
- Create: `backend/switch_service.py`
- Create: `backend/actions.py`
- Create: `backend/platforms/base.py`
- Create: `backend/platforms/windows.py`
- Create: `backend/platforms/macos.py`
- Modify: `windows/codex_switch.py`
- Test: `tests/test_backend_switch_service.py`

**Implementation notes:**
- `switch_service.py` should expose a single service entry point:

```python
result = switch_profile(profile_name="b")
```

- Use a lock file, e.g. `~/.codex/account_backup/.switch.lock`, so concurrent UI clicks cannot run multiple switches.
- Reuse existing Windows logic rather than duplicating it blindly:
  - extract reusable functions from [windows/codex_switch.py](/Users/alysechen/alysechen/github/Codex_Account_Switch/windows/codex_switch.py)
  - keep CLI behavior unchanged
- Port the macOS shell behavior from [macOS/codex-switch.sh](/Users/alysechen/alysechen/github/Codex_Account_Switch/macOS/codex-switch.sh) into `backend/platforms/macos.py`.
- `actions.py` handles:
  - open profile folder
  - open Codex app
  - open contact URL

**Tests:**
- switching rejects missing profile folder
- switching rejects profile with missing `auth.json`
- switching creates autosave before overlay
- switching updates current marker
- switch lock prevents concurrent execution

## Task 4: Add FastAPI routes for the control panel

**Files:**
- Create: `backend/api.py`
- Create: `tests/test_backend_api.py`
- Modify: `requirements-dev.txt`

**Implementation notes:**
- Add FastAPI app with local-only boot command:

```bash
uvicorn backend.api:app --host 127.0.0.1 --port 8765
```

- Implement routes:
  - `GET /api/dashboard`
  - `POST /api/profiles/switch`
  - `POST /api/profiles/add`
  - `POST /api/profiles/open-folder`
  - `POST /api/app/open-codex`
  - `GET /api/contact`
- Bind only to `127.0.0.1`.
- Add strict profile-name validation:
  - allow letters, digits, `_`, `-`
  - reject path separators and traversal
- Return structured errors:

```json
{
  "ok": false,
  "error_code": "PROFILE_AUTH_MISSING",
  "message": "Profile e is missing auth.json"
}
```

**Tests:**
- dashboard route returns paged profiles
- switch route returns `400` for invalid profile name
- add-profile route creates folder and metadata
- open-folder and open-codex routes delegate to platform actions

## Task 5: Define frontend integration contract

**Files:**
- Create: `docs/CONTROL_PANEL_API.md`
- Modify: `README.md`

**Implementation notes:**
- Document these frontend rules:
  - toolbar buttons use backend actions only
  - left profile grid reads from `profiles`
  - right current card reads from `current_card`
  - right quota card reads from `current_quota_card`
  - `Previous` / `Next` buttons read `paging.has_previous` and `paging.has_next`
  - disabled cards are those with `auth_present = false`
- Define expected frontend flows:
  - initial bootstrap: call `/api/dashboard?page=1`
  - page change: call `/api/dashboard?page=n`
  - after successful switch: refetch `/api/dashboard?page=1` or preserve current page if target remains visible

## Task 6: Local developer workflow

**Files:**
- Create: `backend/__main__.py`
- Modify: `README.md`

**Implementation notes:**
- Provide a single local start command:

```bash
python -m backend
```

- `backend/__main__.py` should:
  - resolve host/port defaults
  - start Uvicorn
  - print the local panel URL
- Document that the backend remains local and unauthenticated because it is loopback-only.

## Recommended Delivery Order

1. Build models/config first.
2. Build filesystem + metadata loaders.
3. Build switch service and platform actions.
4. Add API routes.
5. Wire frontend to the API.
6. Add docs.

## Risks To Address Early

- **Data source risk:** plan type and quota are not derivable from current `auth.json`; metadata sidecar is required unless you later discover a stable provider API.
- **macOS parity risk:** backend must not call the shell script as its primary integration forever; that will make error handling brittle.
- **Concurrency risk:** double-clicking switch or paging during switch must not corrupt state.
- **Path safety risk:** add-profile and open-folder endpoints must validate names strictly.

## Minimal MVP Definition

Ship this first:

- local FastAPI backend
- paged `/api/dashboard`
- switch action
- add-profile action
- open-folder action
- open-Codex action
- manual `profile.json` metadata for plan/quota/subscription

Do **not** block MVP on:

- live quota scraping
- websockets
- multi-user auth
- remote deployment
