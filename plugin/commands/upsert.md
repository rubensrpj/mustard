---
description: Use when the user runs /upsert, asks to install, set up, or update Mustard in the current project, or when any /mustard:* command is blocked because Mustard is not installed (no mustard.json at the project root). The bootstrap door — delegates to mustard-rt run upsert.
source: manual
---
<!-- mustard:generated -->
# /upsert — Install or update Mustard in this project

## Trigger

`/mustard:upsert`

## Description

Installs (or updates) Mustard in the current project: seeds `.claude/settings.json`, the injectable instruction files under `.claude/mustard/`, `.claude/.gitignore`, and the project-root `mustard.json`. Idempotent and merge-only — a file you already have is preserved; only what is missing is created. A legacy Mustard-planted `.claude/CLAUDE.md` (and the old import/breadcrumb lines in the root `CLAUDE.md`) is migrated away in the same pass. Until this has run, every other `/mustard:*` command is disabled.

## Action

```bash
mustard-rt run upsert
```

Print nothing raw — read the JSON report and relay it in clear language:

1. `installedBefore: false` → this was a **first install**; `true` → an update over an existing installation.
2. Walk the four lists — `created`, `updated`, `preserved`, `migrated` — and say plainly what each file got (e.g. "your customised orchestrator.md was kept untouched").
3. After a **first install**, add: the defaults work out of the box; `git.flow` (the branch promotion map) and `specLang` can be adjusted anytime by editing `mustard.json` at the project root.
4. Next step: run `/mustard:scan` to analyze the codebase and build the repo model.

## INVIOLABLE RULES

- Never create or edit `.claude/settings.json`, `.claude/mustard/*.md`, `.claude/.gitignore` or `mustard.json` by hand — the binary is the only writer.
- Relay every list from the report; if the JSON carries an `error` field, surface it verbatim — never mask it.
