---
name: rehook
description: Use when the user runs /rehook or asks to re-enable the harness, restore the hooks, turn Mustard back on, or undo the unhook. Delegates to mustard-rt run rehook.
source: manual
---
<!-- mustard:generated -->
# /rehook — Restore the Harness

## Trigger

`/mustard:rehook [--scope this|monorepo|all] [--confirm]`

## Description

Reverses `/mustard:unhook`. For each `.claude/` in scope, finds the most recent `settings.json.disabled*` snapshot and renames it back to `settings.json`. Volatile state directories that `unhook` wiped are **not** recreated — the runtime regenerates them on next run.

## Action

```bash
rtk mustard-rt run rehook --scope this
```

Print stdout verbatim. Scope same as `/mustard:unhook`: `this` (default), `monorepo`, `all` (requires `--confirm`).

## Per-entry states

| State | Meaning |
|-------|---------|
| `restored` | `settings.json.disabled-<ts>` renamed back to `settings.json` |
| `already-active` | Live `settings.json` already in place |
| `no-snapshot` | `.claude/` exists but has no `settings.json.disabled*` |
| `missing` | `.claude/` does not exist |
| `skipped` | `--scope all` without `--confirm` (global target left alone) |
| `error` | Rename failed (path locked, permissions denied) — OS message in report |

## INVIOLABLE RULES

- Always delegate to `mustard-rt run rehook` — never rename `settings.json.disabled*` files by hand.
- Report each entry's `state` field.
- If every entry is `already-active`, surface that — user may have meant `/mustard:unhook`.
