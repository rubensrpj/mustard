# /status - Consolidated Status

## Trigger

`/status [--harness]`

## Description

Shows consolidated project status. With `--harness`, shows the enforcement layer instead: which hooks are wired, what each enforces, and the current mode of each — so the harness is legible without reading `settings.json` by hand.

## Action

1. **Git Status** — run `rtk git status` and `rtk git log -1 --format=%H %s` for current branch, modified files, and last commit.
2. **Pipeline** — run `bun .claude/scripts/metrics.js collect` and surface the `## Active:` and any `## Orphaned:` sections. Do NOT attempt to read `.claude/pipeline-state.json` (legacy singular path, no longer written). The canonical source is `.claude/.pipeline-states/*.json`, read via the script above.
3. **Build** — if `.claude/.last-build.json` exists, report timestamp and pass/fail; otherwise note "no build state persisted".
4. **Entity Registry** — read `.claude/entity-registry.json` and report `_meta.version`, `_meta.generatedAt`, and total entity count (length of `entities` or equivalent top-level collection).

## Flags

- `--harness` — show the enforcement layer (wired hooks grouped by lifecycle event, what each enforces, and current mode). Read-only: never edits `settings.json`. See **Harness View** below.

## Harness View (`/status --harness`)

When invoked with `--harness`, ignore steps 1–4 above and produce the harness view instead.

This view is **READ-ONLY**. Read `.claude/settings.json` to enumerate the wired hooks; never write or edit it.

1. Read `.claude/settings.json` and parse the `hooks` object.
2. Group the registered hooks by lifecycle event, in this order: `PreToolUse`, `PostToolUse`, `SessionStart`, `PreCompact`, `SessionEnd`, `SubagentStart`, `SubagentStop`, `UserPromptSubmit`.
3. For each hook entry, report four columns:
   - **Hook** — the hook file name (e.g. `close-gate.js`).
   - **Matcher** — the matcher of its containing block (e.g. `Write|Edit`, `Bash`, `.*`, `startup`). Use `(any)` when no matcher key is present.
   - **Enforces** — one line describing what it does (use the Mode reference table below, or `pipeline-config.md` for hooks not listed).
   - **Mode** — resolve from the override env var when one exists, falling back to the default; otherwise `n/a`.
4. Resolve **Mode** like this:
   - If the hook has a mode env var (see table), read it from `settings.json` `env` block first; if absent there, use the documented default.
   - Show it as `{value} (env: {VAR_NAME})` so the reader knows which variable to flip.
   - Hooks with no mode env (counters, trackers, formatters, memory/session hooks) show `n/a`.
   - A hook whose name appears in `MUSTARD_DISABLED_HOOKS` (CSV) shows `disabled`.
5. Keep the output compact and scannable — a grouped table per lifecycle event. Do NOT dump raw `settings.json`.

### Mode env reference

Used to resolve the **Mode** column. Read the current value from `settings.json` `env`; fall back to the default when the key is absent.

| Hook | Mode env var | Default |
|------|-------------|---------|
| `close_gate` | `MUSTARD_CLOSE_GATE_MODE` | strict |
| `close_gate` (Wave 10 QA) | `MUSTARD_QA_GATE_MODE` | strict |
| `post_edit` checklist gate | `MUSTARD_CHECKLIST_GATE_MODE` | strict |
| debt gate | `MUSTARD_DEBT_GATE_MODE` | (see module) |
| `budget` (context budget) | `CONTEXT_BUDGET_MODE` | strict |
| `size_gate` (spec size) | `MUSTARD_SPEC_SIZE_MODE` | warn |
| `bash_guard` (commit gate) | `MUSTARD_COMMIT_GATE_MODE` | warn |
| `bash_guard` (native redirect) | `MUSTARD_BASH_REDIRECT_MODE` | strict |
| `model_routing` | `MUSTARD_MODEL_GATE_MODE` | strict |
| (global kill-switch) | `MUSTARD_DISABLED_HOOKS` | (empty) |

All enforcement runs as the single Rust binary `mustard-rt`; `settings.json`
wires one `mustard-rt on <event>` entry per lifecycle event.

Hooks not in this table have no mode env — report their **Mode** as `n/a`. Do not invent env var names; if a hook's mode is unclear, consult `pipeline-config.md`.

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

- Always delegate pipeline state reading to `metrics.js collect` — never parse `.pipeline-states/` directly. This keeps `/status` and `/stats` consistent.
- If `metrics.js collect` reports `## Orphaned:` pipelines, include them under the Pipeline section and suggest the user run `/mustard:close {spec-name}` for each, or `/mustard:maint` for bulk cleanup.
- If no `.claude/` directory exists, inform user that project is not initialized and suggest `mustard init`.
- `--harness` is strictly read-only: it reads `.claude/settings.json` and never edits it. To change a mode, the user edits the `env` block themselves (or via `/mustard:maint`).

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
