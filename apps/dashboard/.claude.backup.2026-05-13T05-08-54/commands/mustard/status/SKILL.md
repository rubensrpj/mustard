# /status - Consolidated Status

## Trigger

`/status`

## Description

Shows consolidated project status.

## Action

1. **Git Status** — run `rtk git status` and `rtk git log -1 --format=%H %s` for current branch, modified files, and last commit.
2. **Pipeline** — run `node .claude/scripts/metrics-collect.js` and surface the `## Active:` and any `## Orphaned:` sections. Do NOT attempt to read `.claude/pipeline-state.json` (legacy singular path, no longer written). The canonical source is `.claude/.pipeline-states/*.json`, read via the script above.
3. **Build** — if `.claude/.last-build.json` exists, report timestamp and pass/fail; otherwise note "no build state persisted".
4. **Entity Registry** — read `.claude/entity-registry.json` and report `_meta.version`, `_meta.generatedAt`, and total entity count (length of `entities` or equivalent top-level collection).

## Rules

- Always delegate pipeline state reading to `metrics-collect.js` — never parse `.pipeline-states/` directly. This keeps `/status` and `/stats` consistent.
- If `metrics-collect.js` reports `## Orphaned:` pipelines, include them under the Pipeline section and suggest the user run `/mustard:complete {spec-name}` for each, or `/mustard:maint` for bulk cleanup.
- If no `.claude/` directory exists, inform user that project is not initialized and suggest `mustard init`.

## Information Layout

```
1. Git
   - Branch, modified files, last commit

2. Pipeline
   - Active: {name} (phase: {phase})  ← if any
   - Orphaned: {name} (spec no longer in active/)  ← if any
   - (or "none" if both empty)

3. Build
   - Last validation: {timestamp} — {pass|fail}
   - (or "no build state persisted")

4. Entity Registry
   - Version, generated at, total entities
```
