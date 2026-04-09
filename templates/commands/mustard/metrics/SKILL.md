---
name: mustard:metrics
description: Show enforcement metrics report — hook hit rates, budget distributions, gate activity. Metrics are recorded automatically; just run this to see them.
---

# /mustard:metrics - Show Metrics Report

## Trigger
`/mustard:metrics [--since <ISO date>] [--event <type>]`

## What it does
Runs `.claude/scripts/metrics-report.js` and shows the aggregated report.

Metrics are recorded **automatically** by enforcement hooks on every Task dispatch — no activation needed. Just run this command whenever you want to see the current state.

## Action
1. Run `rtk node .claude/scripts/metrics-report.js $ARGS` (pass through any flags)
2. Display output verbatim

## Optional flags
- `--since <ISO date>` — filter events after this date
- `--event <type>` — filter to one event type (e.g. `budget-check`)

## Examples
- `/mustard:metrics` — full report since beginning
- `/mustard:metrics --since 2026-04-09` — only recent events
- `/mustard:metrics --event budget-check` — only budget-check events

## Notes
- Metrics live in `.claude/.metrics/*.jsonl` (gitignored, runtime state)
- Logs auto-rotate at 10MB
- To reset: delete files in `.claude/.metrics/` manually
- Advanced: override mode via `CONTEXT_BUDGET_MODE` env var (`strict`|`warn`|`observe`). Default is `strict`.
