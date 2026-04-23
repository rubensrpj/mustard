---
name: mustard:metrics
description: Focused view of enforcement hook events and compare-window deltas. For the superset (pipelines + hooks + RTK), use /mustard:stats.
---

# /mustard:metrics - Hook Events & Compare

## Trigger
`/mustard:metrics [--since <ISO date>] [--event <type>] [--compare <from> <to>]`

## What it does
Focused on two use cases:

1. **Hook-level aggregation** (default) — runs `.claude/scripts/metrics-report.js` and emits a table of events from `.claude/.metrics/*.jsonl`, plus RTK token savings.
2. **Compare window** (`--compare`) — delta between two git tags or ISO dates (reference window computed automatically from the delta).

For the superset view that also includes per-pipeline metrics, orphans, Pass@1 and Last 7 Days, use **`/mustard:stats`** (cross-reference).

## Action
1. Run `rtk node .claude/scripts/metrics-report.js $ARGS` (pass through any flags)
2. Display output verbatim

## Flags
- `--since <ISO date>` — filter events after this date
- `--event <type>` — filter to one event type (e.g. `budget-check`)
- `--compare <from> <to>` — delta between two windows (git tag `vX.Y.Z` or ISO date)

## Examples
- `/mustard:metrics` — hook event aggregation since beginning
- `/mustard:metrics --since 2026-04-09` — only recent events
- `/mustard:metrics --event budget-check` — only budget-check events
- `/mustard:metrics --compare v3.1.21 v3.1.22` — delta between two releases
- `/mustard:metrics --compare 2026-04-09 2026-04-20` — delta between two dates

## Notes
- Metrics live in `.claude/.metrics/*.jsonl` (gitignored, runtime state)
- Logs auto-rotate at 10MB
- To reset: delete files in `.claude/.metrics/` manually
- Advanced: override mode via `CONTEXT_BUDGET_MODE` env var (`strict`|`warn`|`observe`). Default is `strict`.
- `rtk-rewrite` events deliberately show only counts (no `tokens_saved` column) — real RTK numbers come from `rtk gain`, surfaced in the "RTK Token Savings" block.
