---
name: cut-worktree-from-inside-apps-call-wave3
description: To cut a worktree from inside apps/rt, call `work_unit_open::hook_create`, not `open_at`: only `hook_create` runs `init_submodules` + `carry_environment`, so `open_at` (the manual CLI face) hands back a worktree with no `.env` and no `node_modules`.
spec: worktree-isolation-becomes-usable-it
wave: 3
role: general-purpose
session: 2901f053-4baa-40c3-a158-dc19821a8d73
recorded: 2026-08-12T07:48:58.285Z
source: wave-close
---

To cut a worktree from inside apps/rt, call `work_unit_open::hook_create`, not `open_at`: only `hook_create` runs `init_submodules` + `carry_environment`, so `open_at` (the manual CLI face) hands back a worktree with no `.env` and no `node_modules`.
