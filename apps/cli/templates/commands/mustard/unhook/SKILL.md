---
name: mustard-unhook
description: Use when the user runs /unhook or asks to disable the harness, kill the hooks, turn off Mustard, or "uninstall the hooks temporarily". Delegates to mustard-rt run unhook and prints output verbatim.
source: manual
---
<!-- mustard:generated -->
# /unhook — Harness Kill-Switch

## Trigger

`/mustard:unhook [--scope this|monorepo|all] [--confirm]`

## Description

Disables the Claude Code hook layer by renaming `.claude/settings.json` to `settings.json.disabled-<timestamp>` and wiping volatile harness state (`.agent-state/`, `.cluster-cache.json`, `.worktrees/`). Reversible with `/mustard:rehook`.

Use when:

- The harness is misbehaving and you need a clean baseline to confirm whether a bug is yours or Mustard's.
- You want to hand the project to someone who does not have `mustard-rt` installed.
- You need to run Claude Code without any hook overhead for a sensitive operation.

## Action

```bash
rtk mustard-rt run unhook --scope this
```

Print the output verbatim. The JSON `revert_with` field tells the user exactly how to restore.

## Scope flags

| Scope      | What it touches                                                                    |
|------------|------------------------------------------------------------------------------------|
| `this`     | Only `<repo>/.claude/settings.json` (default)                                       |
| `monorepo` | `<repo>/.claude/` + every `apps/*/.claude/` + `packages/*/.claude/`                  |
| `all`      | `monorepo` **plus** the user-global `~/.claude/settings.json` (requires `--confirm`) |

Without `--confirm`, an `all`-scope sweep reports the global target as `state: "skipped"` and leaves it alone.

## Rules

- Always delegate to `mustard-rt run unhook` — never `mv` or rename `settings.json` directly. The binary owns the timestamp format that `rehook` reads back.
- Report each entry's `state` field (`disabled` / `missing` / `skipped` / `error`) so the user knows what actually happened.
- After an `unhook`, suggest `/mustard:rehook --scope <same>` as the reversal.
- Never pass `--confirm` automatically. The user must type it themselves when they really mean `--scope all` — `confirm` is the safety latch on a destructive global operation.
