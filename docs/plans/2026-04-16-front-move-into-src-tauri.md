# Frontend Move Into src-tauri Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move the desktop frontend shells under `src-tauri` so frontend source layout matches the Rust runtime split by platform.

**Architecture:** Put Windows shell files under `src-tauri/win/front/`, macOS shell files under `src-tauri/mac/front/`, and shared frontend bridge code under `src-tauri/shared/front/`. Keep the Vite build output contract unchanged so Tauri can still load `dist/` from the repository root.

**Tech Stack:** TypeScript, Vite, Tauri 2

---

### Task 1: Create the target frontend layout

**Files:**
- Create: `src-tauri/win/front/`
- Create: `src-tauri/mac/front/`
- Create: `src-tauri/shared/front/`

**Step 1: Split platform shells from shared frontend code**

- Move Windows shell markup, styles, and window controls into `src-tauri/win/front/`
- Move macOS shell markup, styles, and window controls into `src-tauri/mac/front/`
- Move shared frontend actions, view-model, rendering, state, Tauri bridge, types, and fonts into `src-tauri/shared/front/`

### Task 2: Rewire frontend imports and build entrypoints

**Files:**
- Modify: `vite.config.ts`
- Modify: `tsconfig.json`
- Modify: `src-tauri/win/front/main.ts`
- Modify: `src-tauri/mac/front/main.ts`
- Modify: `src-tauri/win/front/styles.css`
- Modify: `src-tauri/mac/front/styles.css`

**Step 1: Point Vite to the new roots**

- Select `src-tauri/win/front` for Windows
- Select `src-tauri/mac/front` for macOS
- Keep `dist/` at the repository root

**Step 2: Point both platform shells at shared frontend modules**

- Import shared actions from `src-tauri/shared/front/`
- Point shared font URLs at `src-tauri/shared/front/fonts/`

### Task 3: Update docs and validate

**Files:**
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `docs/IMPLEMENTATION.md`

**Step 1: Document the new frontend layout**

- Explain that frontend shells now live under `src-tauri/win/front/` and `src-tauri/mac/front/`
- Explain that shared frontend bridge code now lives under `src-tauri/shared/front/`

**Step 2: Validate**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: Existing Rust tests remain green because the frontend move should not change runtime behavior
