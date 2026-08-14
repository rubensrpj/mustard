---
name: cut-worktree-from-inside-apps-call-wave3
description: To cut a worktree from inside apps/rt, call `work_unit_open::hook_create`, not `open_at`: only `hook_create` runs `init_submodules`, so `open_at` (the manual CLI face) hands back a worktree whose submodules were never initialised.
spec: worktree-isolation-becomes-usable-it
wave: 3
role: general-purpose
session: 2901f053-4baa-40c3-a158-dc19821a8d73
recorded: 2026-08-12T07:48:58.285Z
corrected: 2026-08-13
source: wave-close
---

To cut a worktree from inside apps/rt, call `work_unit_open::hook_create`, not `open_at`: only `hook_create` runs `init_submodules`, so `open_at` (the manual CLI face) hands back a worktree whose submodules were never initialised.

CORRECTED 2026-08-13, same unit. As first recorded, this named `carry_environment` alongside `init_submodules` as the reason. That function no longer exists — this very unit deleted it, along with the whole `carry`/`link` environment-population design, after review proved the Windows junction it planted was followed by `git worktree remove` into the MAIN checkout. `init_submodules` is the only surviving difference between the two faces, and the memory was being injected into live sessions teaching a symbol nobody can call.