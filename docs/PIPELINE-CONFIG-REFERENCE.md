# Pipeline Config — Maintainer Reference

> Origin: sections moved out of `plugin/pipeline-config.md` (2026-07-18, orchestrator redesign). None of them had a `§ section` entry pointer from any flow, so they cost loaded context without ever being pulled. They are preserved here for maintainers; the loaded `pipeline-config.md` keeps only pointed-to sections. (The old `Agents` section — an empty per-subproject Build/Validate override table — was deleted outright; the `verify-pipeline` code fallback that read it was already commented out.)

## Role Rules

Roles emerge from detection (manifests+folders) — no canonical list. Delivery contracts are code-rendered (`build_role_block`/`build_guards_role_block`) as `{role_block}` — never hand-written into tables or subproject `CLAUDE.md`; there `/scan` authors only the `@.claude/scan-map.md` import + the `## Guards` seed.

## Skill Discovery Heuristic

Deterministic aggregation = `mustard-rt`; human decisions = LLM. Pattern: ONE `rtk mustard-rt run <cmd> --format table`, print verbatim, static blocks as literals, parse the reply.

## Diff Context Interpolation

Two artifacts — never conflate:

- Per-wave `diff.md`: single writer `wave-done` caches `git diff HEAD~1 HEAD --stat` (atomic LF write); `agent-prompt-render` reads it next round; skip when empty.
- `run diff-context`: git-state summary on stdout (cap 3000 chars) for review dispatch; never writes `diff.md`.

## Parallel Rules

- Wave order = `wave-plan.md`'s `Depends on` column — no rigid default.
- A `(parallel-safe)` task consuming no upstream-generated types dispatches in the SAME message; missing artifacts → demote to the next wave.
- Review agents always dispatch in one message — independent + read-only.

## Model

Agents inherit the session model — no routing table. The digest has no judge layer; RT stays LLM-free. (The former Haiku-pinned spec-memory relevance gate was retired together with the `pipeline-execution` skill: the deterministic recall matcher is the definitive selector. A `<spec>/.memory-approved` file, when present, is still honoured by `context_inject.rs` — but nothing in the pipeline authors it anymore.)

## Context Loading

| Context | Source | Loading |
|---------|--------|---------|
| Guards | `{subproject}/CLAUDE.md` § Guards | always (when present) |
| Repo model | `grain.model.json` | queried via `run feature`/`scan digest --query` — never whole |
| Anchors | files the digest points to | the ~12 files read — never the repo |
| Shared language | `CONTEXT.md` | relevance-sliced via `context-slice` as `{context_md}` |

`CONTEXT.md` is never injected whole: sliced by the spec's entities/files/tokens, snapshotted per wave. No glossary → blank slice by design; a named-but-missing `--context` path reports on stderr.

## Agent Return Format

Files modified (`path:line`, omit when none) · non-obvious decisions (1-3 bullets or `none`) · blockers (if any). Never: identity restatement, checklist re-list, files-read list, step narrative.

## Shared Memory Architecture

Truth: per-spec append-only NDJSON under `.claude/spec/{name}/.events/` (and per-wave) — no SQLite, no central `events.jsonl`.

Decisions/Lessons: `decision`/`lesson` events in the per-spec NDJSON (emitted at CLOSE via `run emit-event`; queried by `run event-projections` + MCP `search_knowledge`). Durable prose knowledge lives in Claude Code native auto-memory — no markdown knowledge store.

Views (`run event-projections --view <name>`): `agent-visibility`/`pipeline-state`/`active-pipelines`/`session-summary`/`spec-tree`/`epic-summary`/`pr-metrics`. SessionStart injection is terrain-only (census); views serve history.
