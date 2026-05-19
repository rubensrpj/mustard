---
description: "Show pipeline metrics, token savings, and performance stats — use when user asks for stats, metrics, performance, or token usage"
---
<!-- mustard:generated -->
# /stats - Pipeline Metrics

## Trigger

`/stats [--hooks] [--since <ISO>] [--event <type>] [--compare <from> <to>] [--pr] [--days <n>]`

## Description

Single command for all metrics: pipeline state, enforcement hooks, RTK token economy, compare-window deltas, and DORA-style PR metrics. The default `/stats` is the superset view; flags narrow it to a focused slice.

## Action

1. **Default (`/stats`, no flags)** — run `mustard-rt run metrics collect` for the full superset view.
2. **`--hooks`** — run `mustard-rt run metrics report $ARGS` for hook-level event aggregation only. Pass through `--since`/`--event` flags.
3. **`--pr`** (alias `--view pr-metrics`) — run `mustard-rt run event-projections --view pr-metrics --wave {N}` (the `--wave` arg is the day window for this view) and pretty-print the JSON. Default window: 30 days.
4. Present the output to the user.
5. If no metrics found, inform user to run a pipeline first.

## Flags

- `--hooks` — hook-level event aggregation (events from `.claude/.metrics/*.jsonl`) via `mustard-rt run metrics report`
- `--since <ISO>` — filter events after this date (use with `--hooks`)
- `--event <type>` — filter to one event type, e.g. `budget-check` (use with `--hooks`)
- `--compare <from> <to>` — delta between two windows; each is a git tag `vX.Y.Z` or an ISO date (use with `--hooks`)
- `--pr` (alias `--view pr-metrics`) — DORA-style PR metrics (lead time, review time, PR size, opened/merged per day) over the last N days
- `--days <n>` — window for `--pr` (default 30)

## Sections emitted (default `/stats`)

- **Summary** — 5–8 lines with ✓/⚠/→ prefixes (pipelines tracked, orphans, Pass@1, RTK savings, top alert)
- **Active / Orphaned (per spec)** — duration, API calls, retries, top 3 tools, retries by phase, gate saves, wave reentries, skill hits, Pass@1 by agent (heuristic)
- **Completed Pipelines** — archived runs from `.claude/metrics/`
- **Last 7 Days** — events per day + current week vs prior week delta
- **Enforcement Events (hooks)** — table of events from `.claude/.metrics/*.jsonl`
- **RTK Token Economy** — totals from `rtk gain`

## DORA event sources (auto-emitted)

| Event | Trigger | Where |
|---|---|---|
| `pr.opened` | `gh pr create ...` | `mustard-rt` `bash_guard` PostToolUse(Bash) observer |
| `pr.merged` | `gh pr merge ...` | `mustard-rt` `bash_guard` PostToolUse(Bash) observer |
| `review.start` | `/mustard:review` invoked | inline node call in command |
| `review.complete` | `/mustard:review` returns | inline node call in command |

Pairing strategy: events match by `payload.spec` (preferred) or `payload.branch` within the window. Unmatched events count in totals only.

## Examples

- `/stats` — full superset view (pipelines + hooks + RTK)
- `/stats --hooks` — hook event aggregation since beginning
- `/stats --hooks --since 2026-04-09` — only recent hook events
- `/stats --hooks --event budget-check` — only budget-check events
- `/stats --hooks --compare v3.1.21 v3.1.22` — delta between two releases
- `/stats --hooks --compare 2026-04-09 2026-04-20` — delta between two dates
- `/stats --pr` — DORA-style PR metrics last 30 days
- `/stats --pr --days 7` — last 7 days

## When to Use

- To check pipeline performance
- To compare token usage across pipelines
- To see RTK savings and Pass@1 success rate
- To inspect gate saves (spec churn after /approve) and wave reentries (EXECUTE → PLAN)
- To see skill hit rate (how often recommended skills were actually read by agents)
- To inspect enforcement hook events and compare-window deltas (`--hooks`)
- After completing a pipeline

## Rules

- Default `/stats` always runs `mustard-rt run metrics collect` — do not attempt to read state files manually
- `--hooks` routes to `mustard-rt run metrics report`; `--pr` routes to `mustard-rt run event-projections --view pr-metrics`
- Present the output JSON as-is from the command
- If the script fails, inform the user gracefully and suggest running a pipeline first

## Notes

- Metrics live in `.claude/.metrics/*.jsonl` (gitignored, runtime state)
- Logs auto-rotate at 10MB
- To reset: delete files in `.claude/.metrics/` manually
- Advanced: override mode via `CONTEXT_BUDGET_MODE` env var (`strict`|`warn`|`observe`). Default is `strict`.
- `rtk-rewrite` events deliberately show only counts (no `tokens_saved` column) — real RTK numbers come from `rtk gain`, surfaced in the "RTK Token Savings" block.

ULTRATHINK
