---
description: "Show pipeline metrics, token savings, and performance stats — use when user asks for stats, metrics, performance, or token usage"
---
<!-- mustard:generated -->
# /stats - Pipeline Metrics (superset view)

## Trigger

`/stats`

## Description

Superset view of pipeline state + enforcement hooks + RTK token economy. This is the primary command; `/mustard:metrics` is a focused view for hook-only events and `--compare` windows.

## Action

1. Run `node .claude/scripts/metrics-collect.js` to collect all metrics
2. Present the output to the user
3. If no metrics found, inform user to run a pipeline first

## Sections emitted

- **Summary** — 5–8 lines with ✓/⚠/→ prefixes (pipelines tracked, orphans, Pass@1, RTK savings, top alert)
- **Active / Orphaned (per spec)** — duration, API calls, retries, top 3 tools, retries by phase, gate saves, wave reentries, skill hits, Pass@1 by agent (heuristic)
- **Completed Pipelines** — archived runs from `.claude/metrics/`
- **Last 7 Days** — events per day + current week vs prior week delta
- **Enforcement Events (hooks)** — table of events from `.claude/.metrics/*.jsonl`
- **RTK Token Economy** — totals from `rtk gain`

## When to Use

- To check pipeline performance
- To compare token usage across pipelines
- To see RTK savings and Pass@1 success rate
- To inspect gate saves (spec churn after /approve) and wave reentries (EXECUTE → PLAN)
- To see skill hit rate (how often recommended skills were actually read by agents)
- After completing a pipeline

## Rules

- Always run `metrics-collect.js` — do not attempt to read state files manually
- Present output as-is from the script (it is already formatted markdown)
- If the script fails, inform the user gracefully and suggest running a pipeline first

ULTRATHINK
