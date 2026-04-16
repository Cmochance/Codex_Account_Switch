# src-tauri Layout Reorganization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reorganize `src-tauri` so Windows-specific, macOS-specific, and shared Rust runtime files are grouped under clear top-level folders without changing application behavior.

**Architecture:** Keep Cargo and Tauri convention-bound root files in place, but move runtime source files into `src-tauri/win/`, `src-tauri/mac/`, and `src-tauri/shared/`. Leave generated, capability, icon, and build output folders in their existing toolchain locations unless a convention-safe move is required.

**Tech Stack:** Rust, Cargo, Tauri 2

---

### Task 1: Create the target source layout

**Files:**
- Create: `src-tauri/win/runtime/`
- Create: `src-tauri/mac/runtime/`
- Create: `src-tauri/shared/runtime/`
- Create: `src-tauri/shared/platform/`
- Create: `src-tauri/shared/commands/`

**Step 1: Define the source grouping**

- Put Windows-only runtime modules under `src-tauri/win/runtime/`
- Put macOS-only runtime modules under `src-tauri/mac/runtime/`
- Put shared runtime, command, and platform adapter code under `src-tauri/shared/`
- Keep `src-tauri/src/lib.rs` and `src-tauri/src/main.rs` as crate entrypoints only

### Task 2: Move platform and shared Rust modules

**Files:**
- Move: `src-tauri/src/windows/*.rs`
- Move: `src-tauri/src/windowing.rs`
- Move: `src-tauri/src/macos/*.rs`
- Move: `src-tauri/src/shared/*.rs`
- Move: `src-tauri/src/platform/*.rs`
- Move: `src-tauri/src/commands/*.rs`
- Move: `src-tauri/src/cli.rs`
- Move: `src-tauri/src/errors.rs`
- Move: `src-tauri/src/models.rs`

**Step 1: Relocate Windows modules**

- Move `src-tauri/src/windows/` to `src-tauri/win/runtime/`
- Move `src-tauri/src/windowing.rs` to `src-tauri/win/runtime/windowing.rs`

**Step 2: Relocate macOS modules**

- Move `src-tauri/src/macos/` to `src-tauri/mac/runtime/`

**Step 3: Relocate shared modules**

- Move shared runtime files to `src-tauri/shared/runtime/`
- Move platform adapter files to `src-tauri/shared/platform/`
- Move Tauri command handlers to `src-tauri/shared/commands/`
- Move `cli.rs`, `errors.rs`, and `models.rs` beside shared runtime code

### Task 3: Rewire crate entrypoints to the new layout

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/main.rs` if needed

**Step 1: Replace implicit sibling-module loading**

- Use `#[path = "..."]` module declarations from `src-tauri/src/lib.rs` so the crate can load modules from `../win/`, `../mac/`, and `../shared/`

**Step 2: Preserve public module names**

- Keep module names `windows`, `macos`, `shared`, `platform`, `commands`, `errors`, and `models` so call sites remain stable

### Task 4: Update documentation to the new layout

**Files:**
- Modify: `docs/IMPLEMENTATION.md`
- Modify: `README.md`
- Modify: `README.zh-CN.md`

**Step 1: Document the new `src-tauri` source split**

- Explain that source code is now grouped under `win/`, `mac/`, and `shared/`
- Explicitly call out that Cargo/Tauri root files remain at `src-tauri/` because the toolchain expects them there

### Task 5: Validate the reorganization

**Files:**
- Test: `src-tauri/**`

**Step 1: Run Rust tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: Existing Rust tests stay green after the module path changes
