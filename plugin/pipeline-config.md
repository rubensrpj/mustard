# Pipeline Config

> Static orchestrator reference, pulled via `§ section` pointers — never injected whole. Rationale: `docs/TEMPLATE-RATIONALE.md`; maintainer-only sections moved to `docs/PIPELINE-CONFIG-REFERENCE.md`.

## Pipeline Phases

Canonical: `ANALYZE→PLAN→EXECUTE→REVIEW→QA→CLOSE` (+`COORDINATE`). This section is the single source of the phase vocabulary — every consumer (hooks, docs, dashboard, metrics) uses these names; the pipeline-phase hook records descriptively and does not reject unknown values. Light scope skips PLAN: `ANALYZE→EXECUTE→REVIEW→QA→CLOSE`.

| Phase | Represents | Entry trigger |
|-------|------------|---------------|
| `ANALYZE` | research the codebase: locate entities, read anchors, map the change surface | pipeline starts (`/feature`, `/bugfix`); `spec-draft` backfills the marker |
| `PLAN` | write the spec: scope, waves, Acceptance Criteria (Full only) | plan materialised — `emit-phase --to Plan` |
| `EXECUTE` | implement the change across delegated agents | `/mustard:spec` approval accepted, or Light straight after ANALYZE |
| `REVIEW` | inspect produced code for correctness/conventions before QA | review agents dispatched — emits `review.*` events |
| `QA` | run the spec's Acceptance Criteria commands, record pass/fail | `/mustard:qa` — emits `qa.result` |
| `CLOSE` | finalize: gates, mark completed, commit — archival is event-only | `close_gate` hook gates the emit |
| `COORDINATE` | parent-level orchestration of a multi-spec roadmap | a spec with children enters coordination |

### Spec Layout — Flat `spec/{name}/`

- Flat under `.claude/spec/{name}/` — no bucket subdirs, dir never moves; archival = the `pipeline.status: completed` emit.
- Lifecycle (`stage`+`outcome`+`flags`) lives in `meta.json` + event projection.
- `spec.md` is pure narrative — never lifecycle headers (`### Stage:`-style: Stage/Outcome/Flags/Phase/Scope/Lang).
- Wave plans add `wave-plan.md` + `wave-N-{role}/spec.md` subdirs (each with own `meta.json`).
- The scaffold is materialised only by `spec-draft` (auto-downgrade + `--force-scope`: `refs/feature/full-plan.md`).

### Close — Deterministic Gate Chain

`/close` → `close-orchestrate`: gates = `verify-pipeline` (build+test; lint only when `stack.md [scripts]` declares), `qa-run`, `review-spans`, `docs-stale-check`, + advisory `pipeline-summary`. `overall=pass` auto-finalizes in-process, emitting `closed-followup` + `pipeline.complete`; terminal `completed` is a separate stage. Unchecked-`- [ ]` abort: `/close` SKILL precondition, not a gate.

### Spec Artifact — Two Layers

One `spec.md`, two `##` layers — `## PRD` (what/why) + `## Plan`/`## Plano` (how); headings per `Lang`: `refs/feature/spec-language.md`. PLAN writes both; approve covers both; EXECUTE consumes Plan; QA runs the ACs; Light keeps the shape, lean.

## Tactical Fix Discovery

- A REVIEW/QA finding becomes a linked sub-spec (`meta.json#parent` + `spec.link`; fails open on missing parent) — never a silent follow-up or mid-EXECUTE wave; the parent freezes at approve. → `commands/tactical-fix.md`.
- Agents list candidates under `## Tactical Fix Candidates`; orchestrator suggests `/mustard:tactical-fix <parent> "<desc>"` — advisory, never blocks.
- Qualification (ALL): ≤100 LOC; no public contract change; no pending design decision; no new dependency. Outside → follow-up or fresh spec.

## Diagnostic Failure Routing

| Class | Meaning | Examples |
|-------|---------|---------|
| Transient | recoverable without new info | flaky test, race, timeout |
| Resolvable | targeted patch fixes it | type mismatch, missing import |
| Structural | approach wrong — re-analyze | wrong layer, false spec assumption |
| Internal | crash / no parseable output | context overflow, API error |

Internal → re-dispatch SEQUENTIALLY, same prompt (max 1). Transient → retry once. Resolvable → patch + retry (counts as retry 1). Structural → re-analyze 1-2 files, update spec, re-dispatch (outside the 2-retry cap).

## Token Budget per Agent

Keyed on `subagent_type`; `qa` rides `mustard-review`, `guards` `mustard-guards`.

| Agent | Max Context | Max Tool Uses | Max Return |
|-------|-------------|---------------|------------|
| `impl` (general-purpose) | ≤30K | — | 40 lines |
| `explore` | ≤10K | ≤15 (warn 12) | 30 lines |
| `review` / `qa` | ≤12K | — | 60 lines |
| `plan` | — | — | 80 lines |

Explorer: prefer Grep; max 3 full reads; return once the pattern is clear.

## Escalation Statuses

Definitions only — the operational handling table lives in `refs/spec/resume-loop.md § Escalation`.

- `CONCERN` — a risk was flagged; the work itself was completed.
- `BLOCKED` — cannot proceed: missing dependency, unclear requirement, or unsafe change.
- `PARTIAL` — some steps done, some not.
- `DEFERRED` — a step intentionally skipped, with justification.

## Enforcement Hooks

One binary (`mustard-rt`); `settings.json` wires one `on <event>` per event; a hook bug fails open.

| Module | Matcher | Mode env | Blocks on |
|--------|---------|----------|-----------|
| `close_gate` | CLOSE emit | `MUSTARD_CLOSE_GATE_MODE` (strict) | build/test fail |
| `close_gate` (QA) | same | `MUSTARD_QA_GATE_MODE` (strict) | no `qa.result`, `fail`, or spec/wave-plan edited after it (stale → re-run `/mustard:qa`) |
| `close_gate` (checklist) | same | `MUSTARD_CHECKLIST_GATE_MODE` (strict) | unchecked `- [ ]` |
| `close_gate` (debt) | same | `MUSTARD_DEBT_GATE_MODE` (strict) | unresolved tracked debt |
| `approve-spec` (approval) | `approve-spec` run | `MUSTARD_APPROVAL_MODE` (strict) | no `<spec>/.approved-by-user` marker — approval must come from the USER: plan-mode accept (`ExitPlanMode` → `plan_approval_observer`) or the approval `AskUserQuestion` (`approval_marker_observer`); strict refuses (exit≠0), warn nudges, off disables |
| `bash_command_gate` (rtk) | Bash | `MUSTARD_RTK_GATE_MODE` (warn) | unprefixed → auto-rewrite to `rtk`; `strict` denies; builtins pass |
| `bash_command_gate` (commit) | `git commit` | `MUSTARD_COMMIT_GATE_MODE` (warn) | secrets staged / build broken |
| `bash_command_gate` (native-redirect) | Bash | always-on | `grep`/`ls`/`cat`/`head`/`tail`/`find` → suggests Grep/Glob/Read |
| `scope_guard` | Write/Edit/Task | fail-open | production change outside an approved spec |
| `tool_use_counter` | Subagent* | hard | Explore at 15 tool uses (warn 12) |
| `skills_advisory` | Task | advisory | skills >10 |

### Destructive-ops Law (BG01–BG13)

Two redundant layers: `permissions.deny` holds every canonical spelling (start-anchored, survives `/unhook`); `safety.rs` keeps the full BG01–BG13 table with substring semantics — wrapper-prefix insensitive (`rtk`/`sudo` spellings escape start-anchored globs) + the shapes globs cannot express (flag clusters, reordering, `format <letter>:`); `--force-with-lease` stays allowed. Secret files: same design — 24 deny globs + `secret_files.rs` (case-insensitive full-path substring).
