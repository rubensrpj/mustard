---
name: stats
description: Use when the user runs /stats or asks for pipeline metrics, token savings, performance stats, or DORA-style PR metrics. Single command for all metrics views.
source: manual
---
<!-- mustard:generated -->
# /stats - Pipeline Metrics

`/stats [--hooks] [--since <ISO>] [--event <type>] [--compare <from> <to>] [--pr] [--days <n>]`

## Action

Default (no flags): `mustard-rt run metrics collect` — full superset (pipelines + hooks + RTK).

| Flag | Backend |
|------|---------|
| `--hooks` | `mustard-rt run metrics report $ARGS` — hook-level aggregation. Pass `--since` / `--event` / `--compare`. |
| `--pr` (alias `--view pr-metrics`) | `mustard-rt run event-projections --view pr-metrics --wave {days}` — DORA-style. `--days` window (default 30). |

Print stdout verbatim. If no metrics, suggest running a pipeline first.

## Default sections

Summary (5-8 lines, pipelines/orphans/Pass@1/RTK/top alert) → Active+Orphaned per spec → Completed → Last 7 Days + delta → Enforcement Events (hooks) → RTK Token Economy (`rtk gain`).

## DORA event sources (auto-emitted)

`pr.opened` / `pr.merged` from `gh pr create` / `gh pr merge` (via `bash_command_gate` PostToolUse(Bash) observer). `review.start` / `review.complete` from `/mustard:review` (inline emit). Pairing by `payload.spec` (preferred) or `payload.branch` within the window.

## Examples

`/stats` (superset); `/stats --hooks --since 2026-04-09`; `/stats --hooks --event budget-check`; `/stats --hooks --compare v3.1.21 v3.1.22`; `/stats --pr --days 7`.

## INVIOLABLE RULES

- Default always runs `metrics collect` — do not parse `.metrics/` manually.
- `--hooks` → `metrics report`; `--pr` → `event-projections --view pr-metrics`.
- Failures fail-open (suggest running a pipeline first).

Metrics in `.claude/.metrics/*.jsonl` (gitignored, runtime, auto-rotate at 10MB). Override mode: `CONTEXT_BUDGET_MODE` env (`strict`|`warn`|`observe`, default `strict`). `rtk-rewrite` events show counts only — real savings from `rtk gain`.
