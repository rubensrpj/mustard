# Pipeline Config

> Static orchestrator reference, pulled via `§ section` pointers — never injected whole. Rationale: `docs/TEMPLATE-RATIONALE.md`.

## Pipeline Phases

Canonical: `ANALYZE→PLAN→EXECUTE→REVIEW→QA→CLOSE` (+`COORDINATE`). Source: `refs/canonical-phases.md`.

### Spec Layout — Flat `spec/{name}/`

- Flat under `.claude/spec/{name}/` — no bucket subdirs, dir never moves; archival = the `pipeline.status: completed` emit.
- Lifecycle (`stage`+`outcome`+`flags`) lives in `meta.json` + event projection.
- `spec.md` is pure narrative — never lifecycle headers (`### Stage:`-style: Stage/Outcome/Flags/Phase/Scope/Lang).
- Wave plans add `wave-plan.md` + `wave-N-{role}/spec.md` subdirs (each with own `meta.json`).

### Close — Deterministic Gate Chain

`/close` → `close-orchestrate`: gates = `verify-pipeline` (build+test; lint only when `stack.md [scripts]` declares), `qa-run`, `review-spans`, `docs-stale-check`, + advisory `pipeline-summary`. `overall=pass` auto-finalizes in-process, emitting `closed-followup` + `pipeline.complete`; terminal `completed` is a separate stage. Unchecked-`- [ ]` abort: `/close` SKILL precondition, not a gate.

### Spec Artifact — Two Layers

One `spec.md`, two `##` layers — `## PRD` (what/why) + `## Plan`/`## Plano` (how); headings per `Lang`: `refs/feature/spec-language.md`. PLAN writes both; approve covers both; EXECUTE consumes Plan; QA runs the ACs; Light keeps the shape, lean.

## Agents

Read by `verify-pipeline` only when grain discovery is empty (per-subproject `Build`/`Validate` overrides); leave empty unless overriding.

| Subproject | Build | Validate |
|------------|-------|----------|

## Role Rules

Roles emerge from detection (manifests+folders) — no canonical list. Delivery contracts are code-rendered (`build_role_block`/`build_guards_role_block`) as `{role_block}` — never hand-written into tables or subproject `CLAUDE.md`; in the project's `CLAUDE.md` `/scan` authors only the `@.claude/scan-map.md` import line + the `## Guards` seed (the machine map lives in `scan-map.md`).

## Skill Discovery Heuristic

Deterministic aggregation = `mustard-rt`; human decisions = LLM. Pattern: ONE `rtk mustard-rt run <cmd> --format table`, print verbatim, static blocks as literals, parse the reply.

## Tactical Fix Discovery

- A REVIEW/QA finding becomes a linked sub-spec (`meta.json#parent` + `spec.link`; fails open on missing parent) — never a silent follow-up or mid-EXECUTE wave; the parent freezes at approve. → `commands/mustard/tactical-fix/SKILL.md`.
- Agents list candidates under `## Tactical Fix Candidates`; orchestrator suggests `/mustard:tactical-fix <parent> "<desc>"` — advisory, never blocks.
- Qualification (ALL): ≤100 LOC; no public contract change; no pending design decision; no new dependency. Outside → follow-up or fresh spec.

## Diff Context Interpolation

Two artifacts — never conflate:

- Per-wave `diff.md`: single writer `wave-done` caches `git diff HEAD~1 HEAD --stat` (atomic LF write); `agent-prompt-render` reads it next round; skip when empty.
- `run diff-context`: git-state summary on stdout (cap 3000 chars) for review dispatch; never writes `diff.md`.

## Diagnostic Failure Routing

| Class | Meaning | Examples |
|-------|---------|---------|
| Transient | recoverable without new info | flaky test, race, timeout |
| Resolvable | targeted patch fixes it | type mismatch, missing import |
| Structural | approach wrong — re-analyze | wrong layer, false spec assumption |
| Internal | crash / no parseable output | context overflow, API error |

Internal → re-dispatch SEQUENTIALLY, same prompt (max 1). Transient → retry once. Resolvable → patch + retry (counts as retry 1). Structural → re-analyze 1-2 files, update spec, re-dispatch (outside the 2-retry cap).

## Parallel Rules

- Wave order = `wave-plan.md`'s `Depends on` column — no rigid default.
- A `(parallel-safe)` task consuming no upstream-generated types dispatches in the SAME message; missing artifacts → demote to the next wave.
- Review agents always dispatch in one message — independent + read-only.

## Model

Agents inherit the session model — no routing table. Pinned: Haiku for the spec-memory relevance gate (`skills/pipeline-execution/SKILL.md`). The digest has no judge layer; RT stays LLM-free.

## Context Loading

| Context | Source | Loading |
|---------|--------|---------|
| Guards | `{subproject}/CLAUDE.md` § Guards | always (when present) |
| Repo model | `grain.model.json` | queried via `run feature`/`scan digest --query` — never whole |
| Anchors | files the digest points to | the ~12 files read — never the repo |
| Shared language | `CONTEXT.md` | relevance-sliced via `context-slice` as `{context_md}` |

`CONTEXT.md` is never injected whole: sliced by the spec's entities/files/tokens, snapshotted per wave. No glossary → blank slice by design; a named-but-missing `--context` path reports on stderr.

## Token Budget per Agent

Keyed on `subagent_type`; `qa` rides `mustard-review`, `guards` `mustard-guards`.

| Agent | Max Context | Max Tool Uses | Max Return |
|-------|-------------|---------------|------------|
| `impl` (general-purpose) | ≤30K | — | 40 lines |
| `explore` | ≤10K | ≤20 | 30 lines |
| `review` / `qa` | ≤12K | — | 60 lines |
| `plan` | — | — | 80 lines |

Explorer: prefer Grep; max 3 full reads; return once the pattern is clear.

## Agent Return Format

Files modified (`path:line`, omit when none) · non-obvious decisions (1-3 bullets or `none`) · blockers (if any). Never: identity restatement, checklist re-list, files-read list, step narrative.

## Escalation Statuses

| Status | Meaning | Action |
|--------|---------|--------|
| `CONCERN` | risk flagged, work done | record in `## Concerns`; continue; surface at CLOSE |
| `BLOCKED` | missing dep / unclear / unsafe | stop the wave; AskUserQuestion |
| `PARTIAL` | some steps done | resume from last completed |
| `DEFERRED` | skipped with justification | note; no retry; confirm if load-bearing |

≥2 `CONCERN` in a wave → surface together; `BLOCKED` never triggers retry.

## Enforcement Hooks

One binary (`mustard-rt`); `settings.json` wires one `on <event>` per event; a hook bug fails open.

| Module | Matcher | Mode env | Blocks on |
|--------|---------|----------|-----------|
| `close_gate` | CLOSE emit | `MUSTARD_CLOSE_GATE_MODE` (strict) | build/test fail |
| `close_gate` (QA) | same | `MUSTARD_QA_GATE_MODE` (strict) | no `qa.result`, `fail`, or spec/wave-plan edited after it (stale → re-run `/mustard:qa`) |
| `close_gate` (checklist) | same | `MUSTARD_CHECKLIST_GATE_MODE` (strict) | unchecked `- [ ]` |
| `close_gate` (debt) | same | `MUSTARD_DEBT_GATE_MODE` (strict) | unresolved tracked debt |
| `approve-spec` (approval) | `approve-spec` run | `MUSTARD_APPROVAL_MODE` (strict) | no `<spec>/.approved-by-user` marker — approval must come from the USER: plan-mode accept (`ExitPlanMode` → `plan_approval_observer`) or the approval `AskUserQuestion` (`approval_marker_observer`); strict refuses (exit≠0), warn nudges, off disables; the orchestrator cannot self-approve |
| `bash_command_gate` (rtk) | Bash | `MUSTARD_RTK_GATE_MODE` (warn) | unprefixed → auto-rewrite to `rtk`; `strict` denies; builtins pass |
| `bash_command_gate` (commit) | `git commit` | `MUSTARD_COMMIT_GATE_MODE` (warn) | secrets staged / build broken |
| `bash_command_gate` (native-redirect) | Bash | always-on | `grep`/`ls`/`cat`/`head`/`tail`/`find` → suggests Grep/Glob/Read |
| `scope_guard` | Write/Edit/Task | fail-open | production change outside an approved spec |
| `tool_use_counter` | Subagent* | hard | Explore at 15 tool uses (warn 12) |
| `skills_advisory` | Task | advisory | skills >10 |

### Destructive-ops Law (BG01–BG13)

Two redundant layers: `permissions.deny` holds every canonical spelling (start-anchored, survives `/unhook`); `safety.rs` keeps the full BG01–BG13 table with substring semantics — wrapper-prefix insensitive (`rtk`/`sudo` spellings escape start-anchored globs) + the shapes globs cannot express (flag clusters, reordering, `format <letter>:`); `--force-with-lease` stays allowed. Secret files: same design — 24 deny globs + `secret_files.rs` (case-insensitive full-path substring).

## Shared Memory Architecture

Truth: per-spec append-only NDJSON under `.claude/spec/{name}/.events/` (and per-wave) — no SQLite, no central `events.jsonl`.

Decisions/Lessons: `decision`/`lesson` events in the per-spec NDJSON (emitted at CLOSE via `run emit-event`; queried by `run event-projections` + MCP `search_knowledge`). Durable prose knowledge lives in Claude Code native auto-memory — no markdown knowledge store.

Views (`run event-projections --view <name>`): `agent-visibility`/`pipeline-state`/`active-pipelines`/`session-summary`/`spec-tree`/`epic-summary`/`pr-metrics`. SessionStart injection is terrain-only (census); views serve history.
