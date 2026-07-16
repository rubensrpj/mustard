---
name: skills
description: Use when the user runs /skills or asks about installing, creating, listing, removing, optimizing, or evaluating skills. Handles install/create/list/remove/optimize/eval/update actions.
source: manual
---
<!-- mustard:generated -->
# /skills — Skill Manager

`/skills <action> [args]`

| Action | Usage | Backend |
|--------|-------|---------|
| `install` | `/skills install <name-or-path>` | MANUAL — copy the skill folder into `.claude/skills/<name>/` (bundled skills already ship with the plugin; there is no built-in fetch) |
| `create` | `/skills create <name>` | `skill-creator` (see note) |
| `list` | `/skills list` | list `.claude/skills/*/SKILL.md` + read each frontmatter (no dedicated command) |
| `remove` | `/skills remove <name>` | delete `.claude/skills/{name}/` (warn if `source: scan`) |
| `optimize` | `/skills optimize <name>` | `skill-creator` description-optimization (see note) |
| `eval` | `/skills eval <name>` | `skill-creator` eval methodology (see note) |
| `update` | `/skills update` | bundled skills ship with the plugin — update the plugin through its marketplace (or re-run `mustard init`, idempotent); manual skills you copied in are yours to refresh |

> **`skill-creator` is NOT bundled** (a ~250 KB Python authoring tool — the project is shell-native, no Python). `create`/`optimize`/`eval` depend on it and are **inert until you install it manually**: clone the `skills/skill-creator` subdir of `github.com/anthropics/skills` into `.claude/skills/skill-creator/` (needs Python 3 + the `claude` CLI). There is **no built-in fetch**.

> **Why `skills.md`, not `skill.md`**: on case-insensitive filesystems (Windows/macOS) a command file named `skill.md` matches the `SKILL.md` skill-folder marker, so the plugin loader treats the whole `commands/` folder as ONE skill and every command vanishes. Never name a command file `skill.md`.

## install — manual only (no fetch backend)

Bundled skills ship inside the plugin. Anything else you install by copying the skill folder into `.claude/skills/<name>/` yourself (clone or sparse-checkout, then copy). After copying, validate the frontmatter (rules below) and confirm the skill loads.

## Inviolable

- NEVER delete `source: manual` skills without user confirmation.
- `source:` is territorial: `/scan` writes `source: scan` ONLY; `/skills install|create` writes `source: manual` ONLY. Missing `source:` → treat as `manual` (conservative).
- ALWAYS validate SKILL.md frontmatter on install (kebab-case `name`, description 50-600 chars with a trigger word, `source: scan|manual`).
- `create`/`optimize`/`eval` need `skill-creator` installed manually (not bundled, no built-in fetch) plus Python 3 + the `claude` CLI; they are inert otherwise.
