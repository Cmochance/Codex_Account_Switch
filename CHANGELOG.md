# Changelog

## 1.5.6 - 2026-05-09

- Fixed the mojibake login toast on Windows: the previous `cmd /C codex login` fallback printed cmd.exe's GBK "command not found" message, which `String::from_utf8_lossy` mangled into U+FFFD replacement characters. `run_codex_login` now resolves the real codex path up-front and surfaces a typed `REAL_CODEX_NOT_FOUND` error; the front-end auto-opens a "Codex CLI path" dialog on this error.
- Added a manual override for the codex CLI path. The dialog lists common install locations (click to autofill), accepts a free-form path, and persists it as `user_codex_path` in `install_state.json` with priority over auto-discovery. If the override file disappears later, the resolver silently falls back to auto-discovery so users are never permanently wedged.
- Added a Settings → "Codex CLI path" row that always shows the resolved path plus its source label (manual override / cached / auto-discovered) and a Change button. The dialog is now reachable proactively, not only after a failure.
- Made the login button double as a cancel button while a login is in flight. Clicking the spinning button (now labelled "Cancel" in red) sends SIGTERM (Unix) / `taskkill /F /T` (Windows) to the codex login process, so closing the OAuth tab no longer leaves the app spinning forever.
- Fixed plan detection on downgrade so the cached quota is cleared when the API returns no quota, instead of showing stale numbers next to the new plan.

## 1.5.5 - 2026-05-08

- Added a plan-freshness tooltip on each profile card ("Plan tier confirmed N min ago"); cards stale by more than a local day are marked and trigger a bulk refresh at the next day rollover.
- Added an `unknown_paid` plan state so unrecognised paid tiers surface an explicit hint instead of a blank.
- Plan rotation is now driven by daily bulk refresh + force-rotation hooks, so accounts that aren't actively used still keep an up-to-date plan and `last_plan_check_ms`.
- Plan name now prefers the API `plan_type` field with a JWT fallback; stale "0 days" displays are dropped when the plan tier changes.
- Fixed the "switch already in progress" toast that occasionally got stuck after a switch (stale `.switch.lock` cleanup + double-click guard).
- Fixed the 5h / weekly quota windows occasionally cross-routing — windows are now selected by `window_minutes`, not slot position.
- Internal: split the plan-sync and quota-sync code paths (D1) and dropped the deprecated D2 endpoint plan.

## 1.5.4 - 2026-05-07

- Added per-card login: a profile can now be logged in directly from its card, without first switching to it. The login runs against a sandboxed `CODEX_HOME` and atomically copies the resulting `auth.json` into the target profile folder.
- Cherry-picked the direct ChatGPT-API quota refresh path from 1.6.x onto the 1.5 line so quota stays current without depending on a periodic codex CLI run.
- Fixed real-codex resolution to anchor on the live `~/.codex` instead of the sandboxed `CODEX_HOME`, so the managed-shim filter and install-state cache work correctly.

## 1.5.3 - 2026-04-27

- Added real update checks against the configured GitHub latest-release JSON endpoint.
- Added automatic new-version prompting when the latest release is newer than the running app.
- Fixed update version parsing so historical two-part tags such as `1.5` compare as `1.5.0` instead of failing.
- Added macOS `.pkg` packaging alongside `.app` and `.dmg`.
- Standardized future release records on full three-part semantic versions.

## 1.5.2 - 2026-04-21

- Windows installer uploaded as `codex_switch_1.5.2_x64-setup.exe`.
- Historical note: this asset was uploaded under the non-standard GitHub Release tag `1.5`.

## 1.5.1 - 2026-04-21

- Windows installer uploaded as `codex_switch_1.5.1_x64-setup.exe`.
- Historical note: this asset was uploaded under the non-standard GitHub Release tag `1.5`.

## 1.5.0 - 2026-04-20

- Added normalized installation and version-control release.
- Uploaded macOS DMG and Windows installer assets.
- Historical note: the GitHub Release tag was created as `1.5`; future release tags should use full semantic versions such as `1.5.3`.

## 1.4.2 - 2026-04-16

- Added the new local release after GitHub tag `1.4.1`.
- Added macOS drag-install DMG packaging and kept the generated `.dmg` beside the `.app`.
- Fixed macOS profile enumeration so the runtime `macos/` directory no longer appears as an empty account card.
- Improved macOS real `codex` CLI discovery for GUI refresh/login flows by falling back to the bundled `Codex.app` CLI when shell resolution is unavailable.
- Removed leftover preview mock account names from the shared frontend bridge.

## 1.4.1 - 2026-04-15

- Resolve merge conflicts.

## 1.4.0 - 2026-04-15

- Resolve merge conflicts.

## 1.3.2 - 2026-04-08

- 更新额度刷新流程和优化逻辑。
- 添加卡片 Base URL。
- 精简代码。

## 1.3.1 - 2026-04-08

- 更新额度刷新流程和优化逻辑。
- 添加卡片 Base URL。
- 精简代码。

## 1.2.4 - 2026-04-08

- 代码清理。

## 1.2.0 - 2026-04-08

- 移除 workflow。

## 1.1.5 - 2026-04-07

- 逻辑完善与界面美化。

## 1.1.4 - 2026-04-07

- 添加自动挂载 release。

## 1.1.0 - 2026-04-07

- Windows 优化完善。

## 0.1.1 - 2026-03-30

- Synced the repository script with the locally used production script.
- Removed the old auto-create profile behavior from the repository project.
- Kept the repository installer generic: non-switch commands continue to use the user's existing CLI.
- Updated README and implementation notes to require pre-created profile folders and `auth.json`.
- Removed local smoke-test instructions and the repository test script from the public project surface.

## 0.1.0 - 2026-03-04

- Initial standalone project extraction.
