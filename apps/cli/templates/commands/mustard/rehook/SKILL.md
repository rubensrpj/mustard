---
name: mustard-rehook
description: Use when the user runs /rehook or asks to re-enable the harness, restore the hooks, turn Mustard back on, or "undo the unhook". Delegates to mustard-rt run rehook and prints output verbatim.
source: manual
---
<!-- mustard:generated -->
# /rehook — Restore the Harness

## Trigger

`/mustard:rehook [--scope this|monorepo|all] [--confirm]`

## Description

Reverses `/mustard:unhook`. For each `.claude/` directory in scope, finds the most recent `settings.json.disabled*` snapshot and renames it back to `settings.json`. Volatile state directories that `unhook` wiped (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`) are **not** recreated — the runtime regenerates them on the next run.

## Action

```bash
rtk mustard-rt run rehook --scope this
```

Print the output verbatim.

## Scope flags

Same as `/mustard:unhook`: `this` (default), `monorepo`, `all` (requires `--confirm` to touch `~/.claude/settings.json`).

## Per-entry states

- `restored` — `settings.json.disabled-<ts>` was renamed back to `settings.json`.
- `already-active` — a live `settings.json` is in place; nothing to do.
- `no-snapshot` — the `.claude/` exists but holds no `settings.json.disabled*` to restore from.
- `missing` — the `.claude/` directory does not exist.
- `skipped` — `--scope all` was passed without `--confirm` (global target left alone).
- `error` — the rename failed (path locked, permissions denied); the report includes the OS message.

## Rules

- Always delegate to `mustard-rt run rehook` — never rename `settings.json.disabled*` files by hand.
- Report each entry's `state` field so the user sees exactly which `.claude/`s came back online.
- If every entry is `already-active`, surface that the harness was already on — the user may be looking for `/mustard:unhook` instead.
