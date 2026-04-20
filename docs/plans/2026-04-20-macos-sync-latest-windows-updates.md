# macOS Sync of Latest Windows Updates Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bring the macOS desktop shell to parity with the April 20, 2026 Windows-side updates that added per-profile Base Url editing, card deletion and account clearing, while preserving separate Windows and macOS ownership wherever practical.

**Architecture:** Keep Windows and macOS runtime behavior owned by `src-tauri/win/**` and `src-tauri/mac/**` rather than extracting more platform behavior into `src-tauri/shared/**`. Add macOS-native action and refresh modules by adapting the Windows behavior into the macOS runtime, then use compile-time dispatch at the shared Tauri command boundary. Keep shared code only where one source of truth is actually required, such as payload types, profile metadata/index schema, and version/build plumbing.

**Tech Stack:** Rust, Tauri 2, TypeScript, Vite, platform shells under `src-tauri/win/front/` and `src-tauri/mac/front/`

## Execution Adjustments

- The macOS refresh flow needed a platform-local `get_refresh_runtime_dir(&codex_home)` helper under `src-tauri/mac/runtime/cli_shim.rs`. Reusing the existing shared helper would have kept macOS refresh runtime ownership tied to the Windows-oriented runtime path layout, which violated the repository's platform-isolation rule.
- `src-tauri/shared/commands/switch.rs` also needed the same compile-time platform dispatch pattern as `actions.rs` and `dashboard.rs`. The written file list was narrower than the real shared command boundary, and leaving `switch.rs` unchanged would still have routed macOS switching through `crate::windows`.
- The Task 1 Rust verification command was widened to `cargo test --manifest-path src-tauri/Cargo.toml`. The filtered example in this plan is not accepted by Cargo as written because Cargo only takes a single name filter; the full suite was the smallest verified command that exercised both new macOS modules without inventing extra wrappers.
- `npm run tauri:build:macos-dmg` fails inside the Codex sandbox at `hdiutil create` with `设备未配置`. Final `.dmg` verification therefore requires rerunning the same command outside the sandbox. This is an execution-environment adjustment, not a repository code change.

---

## Update Breakdown

- `3b463e8` on April 20, 2026 introduced the real feature delta:
  - per-profile Base Url editing for every card
  - delete-card and clear-account actions
  - richer profile metadata and root `config.toml` Base Url sync
- `b05b9f0` on April 20, 2026 added release plumbing:
  - `scripts/version-sync.mjs`
  - `version:sync*` package scripts
  - Windows-only NSIS bundling in `src-tauri/tauri.windows.conf.json`
- The current macOS gap is mostly ownership and wiring:
  - shared Tauri commands still route through `crate::windows`
  - the macOS shell is missing the delete and clear-account dialog markup
  - the shared renderer still hides delete on macOS and still disables Base Url editing for auth-less macOS cards

### Task 1: Add macOS-owned runtime action modules

**Files:**
- Create: `src-tauri/mac/runtime/profile_actions.rs`
- Create: `src-tauri/mac/runtime/refresh_runtime.rs`
- Modify: `src-tauri/mac/runtime/mod.rs`
- Modify: `src-tauri/shared/commands/actions.rs`
- Modify: `src-tauri/shared/commands/dashboard.rs`
- Test: `src-tauri/mac/runtime/profile_actions.rs`
- Test: `src-tauri/mac/runtime/refresh_runtime.rs`

**Step 1: Mirror the Windows action surface under the macOS runtime**

Implement macOS-owned versions of the action functions currently living under `src-tauri/win/runtime/profile_actions.rs`. Keep the same command payloads and response shapes, but make `src-tauri/mac/runtime/profile_actions.rs` the owner for macOS behavior.

Required function surface:

```rust
pub fn open_codex_app() -> AppResult<String>;
pub fn login_current_profile() -> AppResult<String>;
pub fn refresh_profile(profile_name: &str) -> AppResult<String>;
pub fn rename_profile(profile_name: &str, new_folder_name: &str) -> AppResult<String>;
pub fn delete_profile(profile_name: &str) -> AppResult<String>;
pub fn clear_profile_account(profile_name: &str) -> AppResult<String>;
pub fn update_profile_base_url(profile_name: &str, openai_base_url: &str) -> AppResult<String>;
pub fn open_profile_folder(app: &tauri::AppHandle, profile_name: &str) -> AppResult<String>;
pub fn add_profile(folder_name: &str, openai_base_url: Option<&str>) -> AppResult<String>;
pub fn open_contact(app: &tauri::AppHandle) -> AppResult<String>;
pub fn open_releases(app: &tauri::AppHandle) -> AppResult<String>;
pub fn open_xiaohongshu(app: &tauri::AppHandle) -> AppResult<String>;
```

**Step 2: Keep only unavoidable contracts shared**

Do not create new shared action services. Reuse only the existing neutral helpers that already define shared behavior:

- profile metadata schema
- profile index schema
- root `config.toml` Base Url sync
- Tauri payload and response structs

If a helper is platform-specific in practice, keep its owner under `src-tauri/mac/runtime/`.

**Step 3: Give the Tauri command boundary compile-time dispatch**

Update `src-tauri/shared/commands/actions.rs` and `src-tauri/shared/commands/dashboard.rs` so they dispatch to `crate::macos` on macOS and `crate::windows` elsewhere, instead of hard-coding `crate::windows`.

The target shape is:

```rust
#[cfg(target_os = "macos")]
use crate::macos as platform_runtime;

#[cfg(not(target_os = "macos"))]
use crate::windows as platform_runtime;
```

Then route commands through `platform_runtime`.

**Step 4: Add macOS-native tests instead of moving ownership into shared**

Mirror the behavior checks under the new macOS modules:

- current profile cannot be deleted, cleared, or renamed
- Base Url save and clear update profile metadata correctly
- add profile writes template files plus optional Base Url
- refresh runtime prep keeps only the intended files

**Step 5: Run focused Rust verification**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml macos::profile_actions macos::refresh_runtime
```

Expected: PASS. Do not commit unless the user explicitly asks.

### Task 2: Expose the missing macOS UI with the smallest necessary shared touch

**Files:**
- Modify: `src-tauri/mac/front/index.html`
- Modify: `src-tauri/shared/front/render.ts`
- Modify: `src-tauri/shared/front/actions.ts`
- Modify: `src-tauri/shared/front/i18n.ts`

**Step 1: Add the delete dialog markup to the macOS shell**

Bring the missing dialog structure into `src-tauri/mac/front/index.html` so the macOS shell owns the actual DOM surface for:

```html
<dialog id="delete-profile-dialog" class="dialog">
  <button id="delete-profile-button" type="button">...</button>
  <button id="clear-profile-account-button" type="button">...</button>
  <button id="cancel-delete-profile-button" type="button">...</button>
</dialog>
```

**Step 2: Stop treating delete as Windows-only in the shared renderer**

Touch `src-tauri/shared/front/render.ts` only because that file already owns the shared card markup. Replace the Windows-only delete gating with DOM-capability or platform-capability logic that works for both desktop shells once both shells provide the dialog elements.

The goal is to avoid a broad frontend extraction, not to ban every shared-line edit.

**Step 3: Make Base Url editable on every macOS card**

Remove the current macOS-only auth guard so macOS matches the intended `1.4.3` behavior.

Target behavior:

```ts
const baseDisabled = state.loading || refreshPending;
```

This applies to both:

- the per-card “Base” action
- the “Add Profile” dialog’s optional Base Url field

**Step 4: Keep copy differences explicit instead of hiding them in ownership changes**

If Windows still needs a stronger warning for non-API logins, keep that as a text choice in `i18n.ts`. Do not use that copy difference as a reason to move more behavior into shared runtime code.

**Step 5: Run a macOS shell smoke pass**

Verify:

1. A card with `auth.json` can open and save Base Url.
2. A card without `auth.json` can still open and save Base Url.
3. A non-current card can be deleted.
4. A non-current card can have its bound account cleared without deleting the folder.
5. The current card still cannot be renamed, deleted, or cleared.

### Task 3: Keep shared data contracts stable while avoiding new shared behavior

**Files:**
- Verify: `src-tauri/shared/runtime/config.rs`
- Verify: `src-tauri/shared/runtime/metadata.rs`
- Verify: `src-tauri/shared/runtime/profiles_index.rs`
- Add tests where needed in macOS-owned modules

**Step 1: Reuse existing shared schema helpers, do not widen them casually**

These shared files are acceptable to keep because they define neutral contracts already used by both platforms:

- `src-tauri/shared/runtime/config.rs`
- `src-tauri/shared/runtime/metadata.rs`
- `src-tauri/shared/runtime/profiles_index.rs`
- `src-tauri/shared/runtime/models.rs`

Do not move more platform flow into them.

**Step 2: Verify Base Url persistence through the existing shared schema layer**

Keep assertions that prove the on-disk contract still works:

```rust
assert!(config.contains("openai_base_url = \"https://example.com/v1\""));
assert!(!config.contains("openai_base_url"));
```

**Step 3: Verify delete-versus-clear semantics through macOS-owned actions**

Confirm:

```rust
assert!(!profile_dir.exists());
assert!(cleared_profile_dir.is_dir());
assert!(!cleared_profile_dir.join("auth.json").exists());
```

**Step 4: Verify snapshot/index output after macOS mutations**

After add, rename, delete, clear, and Base Url updates, confirm the snapshot still exposes the correct:

- `auth_present`
- `has_account_identity`
- `openai_base_url`
- `status`

### Task 4: Keep `1.5.0` scoped to the right layer

**Files:**
- Verify: `package.json`
- Verify: `scripts/version-sync.mjs`
- Verify: `src-tauri/tauri.conf.json`
- Verify: `src-tauri/tauri.macos.conf.json`
- Modify if needed: `README.md`
- Modify if needed: `README.zh-CN.md`

**Step 1: Keep version sync shared because it is truly neutral plumbing**

The shared version flow from `1.5.0` is appropriate to reuse across platforms:

```bash
npm run version:sync
npm run version:sync:release
```

This is one of the few areas that should stay shared.

**Step 2: Keep Windows installer behavior Windows-only**

Do not port NSIS behavior from `src-tauri/tauri.windows.conf.json` into macOS logic. For macOS, the correct release artifact remains:

- `.app`
- `.dmg`

**Step 3: Verify the macOS release path still works with the shared version source**

Run:

```bash
npm run version:sync
npm run tauri:build:macos-dmg
```

Expected outputs:

- `src-tauri/target/release/bundle/macos/codex_switch.app`
- `src-tauri/target/release/bundle/macos/codex_switch_*.dmg`

## Exit Criteria

- macOS owns its action and refresh orchestration under `src-tauri/mac/runtime/`
- shared Tauri commands dispatch by target platform instead of hard-coding `crate::windows`
- macOS can edit Base Url on every profile card, including cards without `auth.json`
- macOS can delete a non-current card and clear a non-current card's bound account
- no new broad shared platform-behavior modules are introduced for this sync
- version sync stays shared, while Windows NSIS packaging remains Windows-only
