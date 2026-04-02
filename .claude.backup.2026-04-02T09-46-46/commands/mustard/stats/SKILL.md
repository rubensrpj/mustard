---
description: "Show pipeline metrics, token savings, and performance stats — use when user asks for stats, metrics, performance, or token usage"
---
<!-- mustard:generated -->
# /stats - Pipeline Metrics

## Trigger

`/stats`

## Description

Displays pipeline metrics including duration, API calls, retries, tool breakdown, and RTK token savings.

## Action

1. Run `node .claude/scripts/metrics-collect.js` to collect all metrics
2. Present the output to the user
3. If no metrics found, inform user to run a pipeline first

## When to Use

- To check pipeline performance
- To compare token usage across pipelines
- To see RTK savings
- After completing a pipeline

## Rules

- Always run `metrics-collect.js` — do not attempt to read state files manually
- Present output as-is from the script (it is already formatted markdown)
- If the script fails, inform the user gracefully and suggest running a pipeline first

ULTRATHINK
