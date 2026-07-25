---
description: Use when the user runs /rehook or asks to re-enable the harness, restore the hooks, turn Mustard back on, or undo the unhook. Delegates to mustard-rt run rehook.
disable-model-invocation: true
source: manual
---
<!-- mustard:generated -->
# /rehook — Restore the Harness

## Trigger

`/mustard:rehook [--scope this|monorepo|all] [--confirm]`

## Description

Reverses `/mustard:unhook`. For each `.claude/` in scope, removes the `"disableAllHooks"` key from the live `settings.json`. When there is no live file, the legacy path still applies: the most recent `settings.json.disabled*` snapshot is renamed back, so a project unhooked by an older build still recovers. Volatile state directories that `unhook` wiped are **not** recreated — the runtime regenerates them on next run.

## Action

```bash
rtk mustard-rt run rehook --scope this
```

Print stdout verbatim. Scope same as `/mustard:unhook`: `this` (default), `monorepo`, `all` (requires `--confirm`).

## Per-entry states

| State | Meaning |
|-------|---------|
| `restored` | `disableAllHooks` removed from the live `settings.json`, or a legacy `settings.json.disabled-<ts>` renamed back |
| `already-active` | Live `settings.json` carries no `disableAllHooks` — hooks were never off |
| `no-snapshot` | `.claude/` exists, no live `settings.json` and no `settings.json.disabled*` |
| `missing` | `.claude/` does not exist |
| `skipped` | `--scope all` without `--confirm` (global target left alone) |
| `error` | Settings unreadable/unparseable, or the rename failed — OS message in report; the file is left untouched |

## INVIOLABLE RULES

- Always delegate to `mustard-rt run rehook` — never edit `settings.json` or rename `settings.json.disabled*` files by hand.
- Report each entry's `state` field.
- If every entry is `already-active`, surface that — user may have meant `/mustard:unhook`.
