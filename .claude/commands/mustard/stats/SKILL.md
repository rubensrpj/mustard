---
description: "Show pipeline metrics, token savings, and performance stats — use when user asks for stats, metrics, performance, or token usage"
---
<!-- mustard:generated -->
# /stats - Pipeline Metrics

## Trigger

`/stats`

## Description

Displays pipeline metrics including duration, API calls, retries, Pass@1 success rate, tool breakdown, RTK token savings, gate saves, wave reentries, and skill hit rate per agent.

## Action

1. Run `node .claude/scripts/metrics-collect.js` to collect all metrics
2. Present the output to the user
3. If no metrics found, inform user to run a pipeline first

## Pass@1 Metrics

`metrics-collect.js` emits a `## Pass@1 Metrics` section at the end of completed-pipeline output:

- **Pass@1**: percentage of pipelines completed without any retries (retries === 0)
- **Avg retries**: mean retry count across all completed pipelines

Example output:
```
## Pass@1 Metrics
- Pass@1: 80% (4/5 completed without retries)
- Avg retries per pipeline: 0.4
```

This section is omitted automatically when no completed pipelines exist yet.

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
