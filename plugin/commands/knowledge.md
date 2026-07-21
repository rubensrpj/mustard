---
description: Use when the user runs /knowledge or asks about the project knowledge base, patterns, conventions, memory audit, or progress reports.
argument-hint: <action> [args]
source: manual
---
<!-- mustard:generated -->
# /knowledge - Knowledge Management

Knowledge = **native auto-memory** (Claude Code's own memory — durable prose, maintained by the model itself) + **decision/lesson events** (append-only per-spec NDJSON, emitted at CLOSE via `mustard-rt run emit-event`).

## Trigger

`/knowledge <action> [args]`

| Action | Backend | Purpose |
|--------|---------|---------|
| `list [spec]` | `mustard-rt run event-projections --view pipeline-state --spec {spec}` | The spec's recorded `decisions[]` / `lessons[]` (cross-spec view: `--view session-summary`) |
| `search <term>` | MCP tool `search_knowledge` (mustard-memory server) | Substring match over decision/lesson events (title + detail) |
| `add` | Interactive → `mustard-rt run emit-event --event decision --spec {spec} --payload "title=…" --payload "rationale=…"` (lesson: `--event lesson --payload "takeaway=…" --payload "trigger=…"`) | Record one decision or lesson event |
| `notes [target]` | Edit `{subproject}/.claude/commands/notes.md` | Persistent observations injected into agent context — NEVER overwritten by `/scan` |
| `audit` | Compare native auto-memory vs CLAUDE.md/skills | Report-only — never auto-edits |
| `report <period>` | → `${CLAUDE_PLUGIN_ROOT}/refs/knowledge/report.md` | Git-based progress reports |

Per action: run the backend command and print stdout verbatim.

## INVIOLABLE RULES

- Decisions/lessons live as EVENTS in the per-spec NDJSON log — append-only, never edited by hand; retention pruning is session-cleanup's job.
- `add` and the `/close` capture both emit the same shapes: `decision{title,rationale}`, `lesson{takeaway,trigger}`.
- NEVER add `<!-- mustard:generated -->` to `notes.md` (user files).
- Always show entry count in list/search output.
