---
name: project_workspace_no_src_tauri
description: apps/dashboard/src-tauri was purged in commit 189a414; Cargo.toml workspace must list only apps/cli, apps/rt, packages/core
metadata:
  type: project
---

`apps/dashboard/src-tauri` was removed wholesale in commit 189a414 (feat W5/per-spec-event-log + dashboard purge, 2026-05-25). The root `Cargo.toml` workspace previously listed it as a member.

Fix (2026-05-25): removed `"apps/dashboard/src-tauri"` from `[workspace] members`.

**Why:** Without this fix, every `cargo build -p mustard-rt` fails with "failed to read Cargo.toml" — the build is completely blocked.

**How to apply:** If cargo builds fail with the src-tauri path error, remove it from workspace members. The dashboard is now a pure Vite/React app with no Rust backend.
