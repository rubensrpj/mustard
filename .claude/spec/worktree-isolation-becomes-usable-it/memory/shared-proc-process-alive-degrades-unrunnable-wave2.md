---
name: shared-proc-process-alive-degrades-unrunnable-wave2
description: `shared::proc::is_process_alive` degrades an unrunnable probe to `false`, so it must never authorise a destructive action — use `process_liveness` and treat `None` as "not measured, not allowed".
spec: worktree-isolation-becomes-usable-it
wave: 2
role: general-purpose
session: 2901f053-4baa-40c3-a158-dc19821a8d73
recorded: 2026-08-12T07:24:07.476Z
source: wave-close
---

`shared::proc::is_process_alive` degrades an unrunnable probe to `false`, so it must never authorise a destructive action — use `process_liveness` and treat `None` as "not measured, not allowed".
