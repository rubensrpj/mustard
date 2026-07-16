---
description: Use when the user runs /unhook or asks to disable the harness, kill the hooks, turn off Mustard, or uninstall the hooks temporarily. Delegates to mustard-rt run unhook.
disable-model-invocation: true
source: manual
---
<!-- mustard:generated -->
# /unhook — Harness Kill-Switch

## Trigger

`/mustard:unhook [--scope this|monorepo|all] [--confirm]`

## Description

Disables hooks by renaming `.claude/settings.json` to `settings.json.disabled-<timestamp>` and wiping volatile state (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`). Reversible via `/mustard:rehook`.

Use when: harness misbehavior + clean baseline; handing project to someone without `mustard-rt`; sensitive operation without hook overhead.

## Action

```bash
rtk mustard-rt run unhook --scope this
```

Print stdout verbatim. The JSON `revert_with` field tells the user exactly how to restore.

## Scope

| Scope | What it touches |
|-------|-----------------|
| `this` | Only `<repo>/.claude/settings.json` (default) |
| `monorepo` | `<repo>/.claude/` + every `apps/*/.claude/` + `packages/*/.claude/` |
| `all` | `monorepo` plus user-global `~/.claude/settings.json` (requires `--confirm`) |

Without `--confirm`, `all`-scope reports the global target as `state: "skipped"` and leaves it alone.

## INVIOLABLE RULES

- Always delegate to `mustard-rt run unhook` — never rename by hand. The binary owns the timestamp format `rehook` reads back.
- Report each entry's `state` field (`disabled`/`missing`/`skipped`/`error`).
- After `unhook`, suggest `/mustard:rehook --scope <same>` as the reversal.
- Never pass `--confirm` automatically — user types it for `--scope all`.
