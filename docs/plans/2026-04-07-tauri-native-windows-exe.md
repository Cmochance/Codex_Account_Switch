# Tauri Native Windows EXE Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use implementation task-by-task and keep parity with the current control panel behavior.

**Goal:** Replace the current browser-hosted Windows control panel runtime with a native Tauri 2 desktop application that produces a portable Windows executable named `codex_switch.exe` and preserves all existing control panel behavior.

**Architecture:** Reuse the current UI structure from `backend/static/` but move it into a Tauri 2 frontend built with Vite + TypeScript. Reimplement the current Python backend and Windows switching logic in Rust commands so the final runtime has no local HTTP server and no Python sidecar dependency.

**Tech Stack:** Tauri 2, Rust, Vite, TypeScript, Serde, Chrono, Tauri opener plugin, GitHub Actions (Windows runner).

---

### Scope and non-goals

- Keep Windows as the only supported runtime target for this migration.
- Ship the desktop app as a portable executable, not an installer.
- Preserve the current product behavior exactly unless the user gives a new instruction.
- Keep the current `backend/` Python code as migration reference only until Rust parity is complete.
- Do not introduce a Python sidecar, local FastAPI server, or browser-launching wrapper.
- Do not redesign the current UI during the migration.

### Current behavior that must be preserved

- Left-side profile cards are paged in groups of 4.
- The top-right icon on each left-side card switches to that profile.
- `Add Profiles` opens a small form and only asks for `folder_name`.
- `Add Profiles` creates a new backup directory and writes template `auth.json` and `profile.json`.
- `Open Codex` launches the Codex desktop application.
- `Contact Us` opens `https://github.com/Cmochance/Codex_Account_Switch`.
- The right-side `Current` card is status-only and does not gain new actions.
- The right-side `Current quota` card mirrors the currently active profile metadata.
- Switching must keep the existing safety flow: stop Codex if needed, back up root state, autosave `auth.json`, overlay target profile, mark active profile, relaunch Codex if it was running.

### Final target repository shape

**Create:**
- `src/index.html`
- `src/main.ts`
- `src/styles.css`
- `src/lib/tauri.ts`
- `src/lib/types.ts`
- `src/lib/render.ts`
- `src/lib/state.ts`
- `src/lib/actions.ts`
- `package.json`
- `package-lock.json` or `pnpm-lock.yaml`
- `tsconfig.json`
- `vite.config.ts`
- `src-tauri/build.rs`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/capabilities/default.json`
- `src-tauri/icons/*`
- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/errors.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/commands/dashboard.rs`
- `src-tauri/src/commands/actions.rs`
- `src-tauri/src/commands/switch.rs`
- `src-tauri/src/windows/mod.rs`
- `src-tauri/src/windows/paths.rs`
- `src-tauri/src/windows/metadata.rs`
- `src-tauri/src/windows/dashboard.rs`
- `src-tauri/src/windows/actions.rs`
- `src-tauri/src/windows/process.rs`
- `src-tauri/src/windows/fs_ops.rs`
- `src-tauri/src/windows/switch.rs`
- `.github/workflows/windows-tauri-build.yml`

**Delete after parity is proven:**
- `backend/__main__.py`
- `backend/api.py`
- `backend/static/index.html`
- `backend/static/app.js`
- `backend/static/styles.css`
- `backend/profile_store.py`
- `backend/switch_service.py`
- `backend/actions.py`
- `backend/models.py`
- `backend/config.py`
- `backend/errors.py`
- `backend/metadata_store.py`
- `tests/test_backend_api.py`
- `tests/test_backend_frontend.py`
- `tests/test_backend_models.py`
- `tests/test_backend_profile_store.py`
- `tests/test_backend_switch_service.py`

**Keep:**
- `windows/common.py`
- `windows/codex_switch.py`
- `windows/install.py`
- `windows/uninstall.py`
- current Python tests for Windows CLI behavior until equivalent Rust behavior is verified

### Target command surface

Rust commands must mirror the current backend interface:

- `get_dashboard(page: u32) -> DashboardResponse`
- `switch_profile(profile: String) -> SwitchResponse`
- `add_profile(folder_name: String) -> ActionResponse`
- `open_profile_folder(profile: String) -> ActionResponse`
- `open_codex() -> ActionResponse`
- `open_contact() -> ActionResponse`

These commands replace the current HTTP endpoints in `backend/api.py`.

### Packaging target

- Final artifact name: `codex_switch.exe`
- Distribution shape: portable executable
- CI artifact should upload the raw `target/release/codex_switch.exe` binary

### Target data model parity

The Rust serde models must preserve the current payload contract from `backend/models.py`:

- `QuotaWindow`
- `QuotaSummary`
- `ProfileMetadata`
- `ProfileCard`
- `CurrentCard`
- `PagingInfo`
- `RuntimeSummary`
- `DashboardResponse`
- `SwitchResponse`
- `ActionResponse`

The frontend should continue to consume this shape so the migration does not require a product-level redesign.

### Execution plan

### Task 1: Scaffold the Tauri 2 application shell

**Files:**
- Create: `package.json`
- Create: `tsconfig.json`
- Create: `vite.config.ts`
- Create: `src/index.html`
- Create: `src/main.ts`
- Create: `src/styles.css`
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/capabilities/default.json`

**Step 1:** Initialize a Tauri 2 app with Vite and TypeScript in the existing repo root.

**Step 2:** Configure a single main window with a fixed title such as `Codex Switch`, normal resizability, and no browser auto-open behavior.

**Step 3:** Configure Tauri capabilities so only the default window can invoke the app commands.

**Step 4:** Add `tauri-plugin-opener` to support opening folders and URLs from the native app.

**Step 5:** Verify the shell starts with `npm run tauri dev` on Windows and renders a blank window.

**Test command:**
- `npm run tauri dev`

**Expected result:**
- A native Tauri window opens, not the system browser.

### Task 2: Migrate the current static UI into the Tauri frontend

**Files:**
- Create: `src/lib/state.ts`
- Create: `src/lib/render.ts`
- Create: `src/lib/actions.ts`
- Create: `src/lib/tauri.ts`
- Create: `src/lib/types.ts`
- Modify: `src/index.html`
- Modify: `src/main.ts`
- Modify: `src/styles.css`
- Reference: `backend/static/index.html`
- Reference: `backend/static/app.js`
- Reference: `backend/static/styles.css`

**Step 1:** Copy the current HTML structure from `backend/static/index.html` into the Vite frontend entry.

**Step 2:** Copy the current CSS from `backend/static/styles.css` with only path-related adjustments.

**Step 3:** Split `backend/static/app.js` into typed modules:
- `state.ts` for dashboard state
- `render.ts` for DOM rendering
- `actions.ts` for button and dialog handlers
- `tauri.ts` for `invoke()` wrappers
- `types.ts` for frontend payload types matching Rust serde models

**Step 4:** Replace all `fetch('/api/...')` calls with `invoke()` wrappers.

**Step 5:** Keep the existing interaction model unchanged:
- switch icon triggers profile switch
- dialog only asks for `folder_name`
- pager buttons change the left-side card page
- current card remains status-only

**Test command:**
- `npm run build`

**Expected result:**
- The frontend builds cleanly with no HTTP endpoint references left.

### Task 3: Define the shared Rust model and error layer

**Files:**
- Create: `src-tauri/src/models.rs`
- Create: `src-tauri/src/errors.rs`
- Modify: `src-tauri/src/lib.rs`
- Reference: `backend/models.py`
- Reference: `backend/errors.py`

**Step 1:** Recreate the current backend response types in Rust using `serde::Serialize` and `serde::Deserialize`.

**Step 2:** Add error types with explicit error codes matching current backend semantics, for example:
- `INVALID_PROFILE_NAME`
- `PROFILE_NOT_FOUND`
- `PROFILE_AUTH_MISSING`
- `PROFILE_ALREADY_EXISTS`
- `APP_NOT_FOUND`
- `APP_OPEN_FAILED`
- `SWITCH_IN_PROGRESS`
- `SWITCH_FAILED`

**Step 3:** Add a helper that maps internal Rust errors to user-facing Tauri command errors in a consistent JSON-compatible shape.

**Step 4:** Keep field names aligned with the current frontend payload expectations so the UI can stay mostly unchanged.

**Test command:**
- `cargo test models`

**Expected result:**
- Serialization and validation tests pass.

### Task 4: Port Windows path and metadata logic to Rust

**Files:**
- Create: `src-tauri/src/windows/paths.rs`
- Create: `src-tauri/src/windows/metadata.rs`
- Modify: `src-tauri/src/windows/mod.rs`
- Reference: `backend/config.py`
- Reference: `windows/common.py`
- Reference: `backend/metadata_store.py`

**Step 1:** Implement Windows path helpers for:
- `CODEX_HOME`
- backup root
- autosave root
- current profile marker
- runtime paths
- auth template path

**Step 2:** Port profile name validation with the current allowed pattern `[A-Za-z0-9_-]+`.

**Step 3:** Implement `profile.json` read and write helpers.

**Step 4:** Keep default values identical to current behavior when metadata is missing.

**Step 5:** Add tests for path resolution, metadata parsing, and invalid profile names.

**Test command:**
- `cargo test windows::metadata`
- `cargo test windows::paths`

**Expected result:**
- Metadata and path helpers behave like the Python implementation.

### Task 5: Port dashboard assembly to Rust

**Files:**
- Create: `src-tauri/src/windows/dashboard.rs`
- Create: `src-tauri/src/commands/dashboard.rs`
- Modify: `src-tauri/src/lib.rs`
- Reference: `backend/profile_store.py`
- Reference: `windows/common.py`
- Reference: `windows/codex_switch.py`

**Step 1:** Implement profile listing and current-profile resolution matching the current Python logic.

**Step 2:** Recreate subscription day calculations and latest autosave timestamp behavior.

**Step 3:** Implement the page size default of `4` and paging flags:
- `has_previous`
- `has_next`
- `total_pages`

**Step 4:** Build the `DashboardResponse` payload so the frontend can render without modification.

**Step 5:** Expose the Tauri command `get_dashboard(page)`.

**Test command:**
- `cargo test windows::dashboard`

**Expected result:**
- Dashboard pagination and current-card behavior match the Python backend.

### Task 6: Port non-switch actions to Rust

**Files:**
- Create: `src-tauri/src/windows/actions.rs`
- Create: `src-tauri/src/commands/actions.rs`
- Modify: `src-tauri/src/lib.rs`
- Reference: `backend/actions.py`
- Reference: `windows/common.py`

**Step 1:** Implement `open_codex()` using the Windows install state and app detection logic.

**Step 2:** Implement `open_profile_folder(profile)` using `tauri-plugin-opener` reveal/open support.

**Step 3:** Implement `open_contact()` using `tauri-plugin-opener` URL support with the fixed repository URL.

**Step 4:** Implement `add_profile(folder_name)` to:
- validate the folder name
- create the profile directory
- copy the auth template to `auth.json`
- create `profile.json`

**Step 5:** Preserve current add-profile behavior exactly. Do not infer labels, do not auto-number folders, and do not ask for any additional form fields.

**Test command:**
- `cargo test windows::actions`

**Expected result:**
- New profile creation, folder opening, app opening, and contact opening all succeed or return the expected error codes.

### Task 7: Port the switching engine to Rust

**Files:**
- Create: `src-tauri/src/windows/process.rs`
- Create: `src-tauri/src/windows/fs_ops.rs`
- Create: `src-tauri/src/windows/switch.rs`
- Create: `src-tauri/src/commands/switch.rs`
- Modify: `src-tauri/src/lib.rs`
- Reference: `backend/switch_service.py`
- Reference: `windows/codex_switch.py`
- Reference: `windows/common.py`

**Step 1:** Port process detection and shutdown logic for `Codex.exe` using `tasklist` and `taskkill`.

**Step 2:** Port file operations:
- profile backup from root into the active profile
- autosave snapshot of `auth.json`
- overlay profile contents back into `CODEX_HOME`
- active-profile marker update

**Step 3:** Preserve the lock-file behavior using the current `.switch.lock` semantics.

**Step 4:** Run the actual switch operation inside `spawn_blocking` so Tauri command handling stays responsive.

**Step 5:** Preserve current error mapping and message behavior.

**Step 6:** Expose `switch_profile(profile)` as a Tauri command and keep the response shape aligned with the current frontend.

**Test command:**
- `cargo test windows::switch`

**Expected result:**
- Switching behavior matches the current Python logic, including conflict locking and missing-auth handling.

### Task 8: Wire the frontend to the Tauri commands

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/actions.ts`
- Modify: `src/lib/render.ts`
- Modify: `src/main.ts`
- Reference: `backend/static/app.js`

**Step 1:** Add a typed invoke layer for every Rust command.

**Step 2:** Ensure left-card switch icons call `switch_profile` and reload the dashboard on success.

**Step 3:** Ensure `Add Profiles` submits only `folder_name` and refreshes the dashboard after success.

**Step 4:** Ensure `Open Codex`, `Open folder`, and `Contact Us` call the matching native commands.

**Step 5:** Preserve the current toast/error handling model and loading-state behavior.

**Test command:**
- `npm run build`
- `npm run tauri dev`

**Expected result:**
- All current UI actions work inside the native Tauri window with no local HTTP server.

### Task 9: Add test coverage for the new native path

**Files:**
- Create: `src-tauri/src/windows/tests.rs` or inline module tests
- Create: `src/lib/tauri.test.ts` if frontend invoke wrapper tests are needed
- Modify: `package.json`
- Reference: existing `tests/test_backend_*.py`
- Reference: existing `tests/test_windows_*.py`

**Step 1:** Move backend-behavior assertions into Rust unit tests for the new implementation.

**Step 2:** Keep the current Python Windows tests as behavioral references during migration.

**Step 3:** Add a minimal frontend smoke test only if the TypeScript invoke layer contains enough logic to justify it.

**Step 4:** Define canonical validation commands:
- `cargo test`
- `npm run build`

**Expected result:**
- The native implementation has first-party tests before the Python reference code is removed.

### Task 10: Remove the Python control panel runtime after parity

**Files:**
- Delete: `backend/__main__.py`
- Delete: `backend/api.py`
- Delete: `backend/profile_store.py`
- Delete: `backend/switch_service.py`
- Delete: `backend/actions.py`
- Delete: `backend/models.py`
- Delete: `backend/config.py`
- Delete: `backend/errors.py`
- Delete: `backend/metadata_store.py`
- Delete: `backend/static/index.html`
- Delete: `backend/static/app.js`
- Delete: `backend/static/styles.css`
- Delete: `tests/test_backend_api.py`
- Delete: `tests/test_backend_frontend.py`
- Delete: `tests/test_backend_models.py`
- Delete: `tests/test_backend_profile_store.py`
- Delete: `tests/test_backend_switch_service.py`
- Modify: `README.md`
- Modify: `docs/IMPLEMENTATION.md`

**Step 1:** Remove the Python runtime files only after the Tauri implementation passes parity tests.

**Step 2:** Update docs so the product is described as a native Tauri desktop app, not a local web control panel.

**Step 3:** Keep `windows/*.py` CLI/install/uninstall support unless the user later requests a full Rust replacement of the CLI toolchain too.

**Test command:**
- `cargo test`
- `npm run build`
- `npm run tauri build` on Windows

**Expected result:**
- The repo no longer contains two runtime control panel implementations.

### Task 11: Add Windows packaging and CI for native EXE delivery

**Files:**
- Create: `.github/workflows/windows-tauri-build.yml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `README.md`

**Step 1:** Configure Tauri for portable Windows executable output instead of installer bundling.

**Step 2:** Build on a Windows runner in GitHub Actions.

**Step 3:** Publish the portable `codex_switch.exe` artifact from CI for manual download and testing.

**Step 4:** Document the local Windows build command and CI artifact location.

**Step 5:** Keep the build target as Windows-native CI. Do not rely on macOS cross-packaging as the primary release path.

**Recommended Windows build commands:**
- `npm ci`
- `npm run tauri build --no-bundle`

**Expected result:**
- A portable `codex_switch.exe` artifact is produced from CI.

### Task 12: Final verification checklist

**Manual checks on Windows:**
- App opens as a native window, not in the browser.
- Main dashboard loads with no network server running.
- Left-side profile cards page correctly in groups of 4.
- Card switch icon switches profile and refreshes the dashboard.
- `Add Profiles` prompts only for `folder_name`.
- New profile creation writes both `auth.json` and `profile.json`.
- `Open Codex` launches Codex.
- `Contact Us` opens the repository URL.
- `Open folder` reveals the active profile folder.
- `Current` card stays informational only.
- Switch lock prevents double-trigger switching.
- Missing `auth.json` cards remain non-switchable.
- Portable `codex_switch.exe` launches correctly after copying to a clean Windows machine.

### Notes for implementation

- Keep the current Python files as executable reference until the Rust version reaches parity. Do not try to maintain Python and Rust as long-term dual runtimes.
- Prefer parity over cleanup in the first implementation pass. Once the Tauri app is correct, then remove the Python control panel runtime.
- Keep the UI stable. This migration is a runtime and packaging migration, not a product redesign.
- Use `npm` as the package manager for this repo unless the user later requests a different tool.
