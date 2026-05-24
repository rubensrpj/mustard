---
name: mustard-status
description: Use when the user runs /status or asks about project status, pipeline state, entity registry, or harness enforcement layer. Delegates to mustard-rt run status and prints output verbatim.
source: manual
---
<!-- mustard:generated -->
# /status - Consolidated Status

## Trigger

`/status [--harness]`

## Description

Shows consolidated project status by delegating to `mustard-rt run status`. With `--harness`, shows the enforcement layer instead: which hooks are wired, what each enforces, and the current mode of each — read directly from `settings.json` by the binary. Both modes delegate entirely to the binary; the SKILL prints output verbatim.

## Action

```bash
rtk mustard-rt run status --format table
```

Print the output verbatim. The binary covers: git branch/modified files/last commit, active and orphaned pipeline specs (from the SQLite event log), last build state, and entity registry summary.

## Flags

- `--harness` — show the enforcement layer (wired hooks grouped by lifecycle event, what each enforces, and current mode). Read-only: never edits `settings.json`. See **Harness View** below.

## Harness View (`/status --harness`)

```bash
rtk mustard-rt run status --harness --format table
```

Print the output verbatim. The binary reads `.claude/settings.json`, groups hooks by lifecycle event, resolves each mode from the `env` block (falling back to the documented default), and renders the grouped table. Mode env reference is embedded in the binary — consult `pipeline-config.md` § "Mode env reference" for the full table or read the live output directly.

This view is **READ-ONLY**. The binary never writes `settings.json`. To change a mode, the user edits the `env` block themselves (or via `/mustard:maint`).

### Output layout (`--harness`)

```
Harness — enforcement layer (read-only view of settings.json)

PreToolUse  (mustard-rt on PreToolUse)
  Module        Matcher      Enforces                              Mode
  close_gate    Write|Edit   blocks CLOSE if build/QA fail          strict (env: MUSTARD_CLOSE_GATE_MODE)
  bash_guard    Bash         redirects grep/ls/cat to native tools  strict (env: MUSTARD_BASH_REDIRECT_MODE)
  ...

PostToolUse
  ...

(one block per lifecycle event that has hooks)
```

## Rules

- Always delegate status reading to `mustard-rt run status` — never parse `.pipeline-states/` or `.last-build.json` directly. This keeps `/status` and `/stats` consistent.
- If the binary reports orphaned pipelines, suggest the user run `/mustard:close {spec-name}` for each, or `/mustard:maint` for bulk cleanup.
- If no `.claude/` directory exists, inform user that project is not initialized and suggest `mustard init`.
- `--harness` is strictly read-only.

## Information Layout

```
1. Git
   - Branch, modified files, last commit

2. Pipeline
   - Active: {name} (phase: {phase})  ← if any
   - Orphaned: {name} (spec no longer active)  ← if any
   - (or "none" if both empty)

3. Build
   - Last validation: {timestamp} — {pass|fail}
   - (or "no build state persisted")

4. Entity Registry
   - Version, generated at, total entities
```
