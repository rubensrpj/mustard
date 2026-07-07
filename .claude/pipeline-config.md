# Pipeline Config

> Static orchestrator reference, hand-maintained; pulled on demand via `§ section` pointers — never injected whole. Rationale: `docs/TEMPLATE-RATIONALE.md`.

## Pipeline Phases

Canonical: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE` (+ `COORDINATE`). Single source: `refs/canonical-phases.md`.

### Spec Layout — Flat `spec/{name}/`

- Flat under `.claude/spec/{name}/` — no bucket subdirs; the dir never moves; archival is the `pipeline.status: completed` emit.
- Lifecycle (`stage`+`outcome`+`flags`) lives in the `meta.json` sidecar + event projection.
- `spec.md` is pure narrative — never lifecycle headers (`### Stage:`, `### Outcome:`, `### Flags:`, `### Phase:`, `### Scope:`, `### Lang:`, …).
- Wave plans add `wave-plan.md` + `wave-N-{role}/spec.md` subdirs (each with its own `meta.json`).

### Close — Deterministic Gate Chain

`/close` → `close-orchestrate`: gates = `verify-pipeline` (build+test; lint only when `stack.md [scripts]` declares), `qa-run`, `review-spans`, `docs-stale-check`, + advisory `pipeline-summary`. `overall=pass` auto-finalizes in-process (LLM relays), emitting `closed-followup` + `pipeline.complete`; terminal `completed` is a separate stage. Unchecked-`- [ ]` abort: `/close` SKILL precondition, not a gate.

### Spec Artifact — Two Layers

One `spec.md`, two `##` divider layers — `## PRD` (what/why) + `## Plan`/`## Plano` (how); subsection headings per `Lang`: table in `refs/feature/spec-language.md`. PLAN produces both; approve covers both; EXECUTE consumes Plan; QA runs the ACs; Light keeps the shape, lean.

## Agents

Read by `verify-pipeline` only when grain discovery is empty (`Build`/`Validate` overrides per subproject); leave empty unless overriding.

| Subproject | Build | Validate |
|------------|-------|----------|

## Role Rules

Roles emerge from detection (manifests + folders) — no canonical list. Per-role delivery contracts are code-rendered (`build_role_block`/`build_guards_role_block`) as `{role_block}` — never written into tables or subproject `CLAUDE.md`. `/scan` authors only orientation blocks + `## Guards`.

## Skill Discovery Heuristic

Deterministic filesystem aggregation = `mustard-rt` work; human decisions = LLM. Pattern: ONE `rtk mustard-rt run <cmd> --format table`, print verbatim, static blocks as literals, parse the reply.

## Tactical Fix Discovery

- A fix found in REVIEW/QA becomes a linked sub-spec (`meta.json#parent` + `spec.link`; fails open on a missing parent) — never a silent follow-up or mid-EXECUTE wave; the parent freezes at approve. → `commands/mustard/tactical-fix/SKILL.md`.
- Agents list candidates under `## Tactical Fix Candidates`; the orchestrator suggests `/mustard:tactical-fix <parent> "<desc>"` — advisory, never blocks.
- Qualification (ALL): ≤100 LOC; no public contract change; no pending design decision; no new dependency. Outside → follow-up or fresh spec.

## Diff Context Interpolation

Two artifacts — never conflate:

- Per-wave `diff.md`: single writer `wave-done` caches the wave's `git diff HEAD~1 HEAD --stat` (atomic LF write); `agent-prompt-render` reads it next round; skip when empty.
- `run diff-context`: git-state summary on stdout (cap 3000 chars) for review dispatch; never writes `diff.md`.

## Diagnostic Failure Routing

| Class | Meaning | Examples |
|-------|---------|---------|
| Transient | recoverable without new info | flaky test, race, timeout |
| Resolvable | targeted patch fixes it | type mismatch, missing import |
| Structural | approach wrong — re-analyze | wrong layer, false spec assumption |
| Internal | crash / no parseable output | context overflow, API error |

Internal → re-dispatch SEQUENTIALLY, same prompt (max 1). Transient → retry once. Resolvable → patch + retry (counts as retry 1). Structural → re-analyze 1-2 files, update spec, re-dispatch (doesn't count against the 2-retry cap).

## Parallel Rules

- Wave order = `wave-plan.md`'s `Depends on` column — no rigid default.
- A downstream task marked `(parallel-safe)` consuming no upstream-generated types dispatches in the SAME message; on missing artifacts, demote to the next wave.
- Review agents always dispatch in one message — independent + read-only.

## Model

Agents inherit the session model — no routing table. Pinned: Haiku for the spec-memory relevance gate (bounded mechanical relevance; `skills/pipeline-execution/SKILL.md`). The digest has no judge layer; RT stays LLM-free; pins live in orchestration.

## Context Loading

| Context | Source | Loading |
|---------|--------|---------|
| Guards | `{subproject}/CLAUDE.md` § Guards | always (when present) |
| Repo model | `grain.model.json` | queried via `run feature` / `scan digest --query` — never whole |
| Anchors | files the digest points to | the ~12 files read — never the repo |
| Shared language | `CONTEXT.md` | relevance-sliced via `context-slice` as `{context_md}` |

`CONTEXT.md` is never injected whole: sliced by the spec's entities/files/tokens, snapshotted per wave to `.pipeline-states/{spec}.context-md.md`. No glossary → blank slice by design; a named-but-missing `--context` path reports on stderr.

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

Files modified (`path:line`, omit when none) · non-obvious decisions (1-3 bullets or `none`) · blockers (only if any). Never: identity restatement, checklist re-list, files-read list, step narrative.

## Escalation Statuses

| Status | Meaning | Action |
|--------|---------|--------|
| `CONCERN` | risk flagged, work done | record in `## Concerns`; continue; surface at CLOSE |
| `BLOCKED` | missing dep / unclear / unsafe | stop the wave; AskUserQuestion |
| `PARTIAL` | some steps done | resume from last completed |
| `DEFERRED` | skipped with justification | note; no retry; confirm if load-bearing |

≥2 `CONCERN` in a wave → surface together. `BLOCKED` never triggers retry.

## Enforcement Hooks

One binary (`mustard-rt`); `settings.json` wires one `on <event>` per lifecycle event. A bug in the hook fails open.

| Module | Matcher | Mode env | Blocks on |
|--------|---------|----------|-----------|
| `close_gate` | CLOSE emit | `MUSTARD_CLOSE_GATE_MODE` (strict) | build/test fail |
| `close_gate` (QA) | same | `MUSTARD_QA_GATE_MODE` (strict) | no `qa.result`, `fail`, or spec/wave-plan edited after it (stale → re-run `/mustard:qa`) |
| `close_gate` (checklist) | same | `MUSTARD_CHECKLIST_GATE_MODE` (strict) | unchecked `- [ ]` |
| `close_gate` (debt) | same | `MUSTARD_DEBT_GATE_MODE` (strict) | unresolved tracked debt |
| `bash_command_gate` (rtk) | Bash | `MUSTARD_RTK_GATE_MODE` (warn) | unprefixed → auto-rewrite to `rtk`; `strict` denies; builtins pass |
| `bash_command_gate` (commit) | `git commit` | `MUSTARD_COMMIT_GATE_MODE` (warn) | secrets staged / build broken |
| `bash_command_gate` (native-redirect) | Bash | always-on | `grep`/`ls`/`cat`/`head`/`tail`/`find` → suggests Grep/Glob/Read |
| `scope_guard` | Write/Edit/Task | fail-open | production change outside an approved spec |
| `tool_use_counter` | Subagent* | hard | Explore at 15 tool uses (warn 12) |
| `skills_advisory` | Task | advisory | skills >10 |

### bash_command_gate Safety Rules (BG01–BG13)

IDs in the deny reason (`[bash-safety BGnn]`): BG01 `rm -rf`; BG02 force push (`--force-with-lease` ok); BG03 hard reset; BG04 `git clean -f`; BG05 `git checkout -- .`; BG06 `git restore .`; BG07 delete main/master; BG08 chmod 777; BG09 mkfs; BG10 `dd if=`; BG11 `format <letter>:`; BG12 shutdown; BG13 reboot.

## Shared Memory Architecture

Truth: per-spec append-only NDJSON under `.claude/spec/{name}/.events/` (and per-wave) — no SQLite append, no central `events.jsonl`.

Knowledge patterns: `.claude/knowledge/{slug}.md` (writer `session_knowledge_observer`). Decisions/Lessons: `.claude/memory/{decisions,lessons}/{slug}.md` (writer `run memory decision`).

Views (`run event-projections --view <name>`): `agent-visibility`, `pipeline-state`, `active-pipelines`, `session-summary`, `spec-tree`, `epic-summary`, `pr-metrics`. SessionStart injection is capped (`MEMORY_MAX_CHARS` 2000); the views serve deeper history.
