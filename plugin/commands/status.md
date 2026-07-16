---
description: Use when the user runs /status or asks about project status, pipeline state, entity registry, or the harness enforcement layer. Delegates to mustard-rt run status and prints output verbatim.
source: manual
---
<!-- mustard:generated -->
# /status - Consolidated Status

## Trigger

`/status [--harness]`

## Action

```bash
rtk mustard-rt run status --format table
```

Print stdout verbatim. The binary covers: git (branch, modified files, last commit), active and orphaned pipeline specs (NDJSON event log — per-spec `.events/`), last build state, entity registry summary.

## --harness View

```bash
rtk mustard-rt run status --harness --format table
```

Print verbatim. The binary reads `.claude/settings.json`, groups hooks by lifecycle event, resolves each mode from the `env` block (falling back to the documented default), and renders the grouped table. **READ-ONLY** — never writes `settings.json`. To change a mode, the user edits the `env` block (or via `/mustard:maint`).

Layout: `PreToolUse (mustard-rt on PreToolUse)` followed by per-module rows (Module / Matcher / Enforces / Mode). One block per lifecycle event.

## INVIOLABLE RULES

- Always delegate to `mustard-rt run status` — never parse the per-spec `.events/` NDJSON or state files by hand. Keeps `/status` and `/stats` consistent.
- Orphaned pipelines → suggest `/mustard:close {spec}` or `/mustard:maint` for bulk cleanup.
- No `.claude/` → suggest `mustard init`.
- `--harness` is strictly read-only.
