---
name: mustard-knowledge
description: Use when the user runs /knowledge or asks about the project knowledge base, patterns, conventions, memory audit, or progress reports.
source: manual
---
<!-- mustard:generated -->
# /knowledge - Knowledge Management

## Trigger

`/knowledge <action> [args]`

| Action | Backend | Purpose |
|--------|---------|---------|
| `list` | `mustard-rt run memory list --grouped --format table` | Entries grouped by type |
| `search <term>` | `mustard-rt run memory search` | Case-insensitive match on name/description/tags |
| `notes [target]` | Edit `{subproject}/.claude/commands/notes.md` | Persistent observations injected into agent context — NEVER overwritten by `/scan` |
| `audit` | Compare auto-memory vs CLAUDE.md/skills | Report-only — never auto-edits |
| `report <period>` / `evolve` / `export` / `import <file>` | → `../../../refs/knowledge/evolve-report.md` | Reporting + sharing |

Per action: run the backend command and print stdout verbatim.

## INVIOLABLE RULES

- NEVER add `<!-- mustard:generated -->` to `notes.md` (user files).
- Always show entry count in list/search output.
