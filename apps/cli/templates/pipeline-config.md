# Pipeline Config

> Static orchestrator reference, hand-maintained (`/scan` does **not** generate it). Pulled on demand via `§ section` pointers and sliced per-role into dispatch prompts (`build_context_extras`) — never injected whole.

## Pipeline Phases

Canonical: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE` (+ `COORDINATE` for roadmaps). Single source of truth — phase names, descriptions, entry triggers: `refs/canonical-phases.md`.

### Spec Layout — Flat `spec/{name}/`

Specs live under a single flat directory: `.claude/spec/{name}/`. No `active/`/`completed/`/`superseded/` bucket subdirectories. Lifecycle state (`stage` + `outcome` + `flags`) is read from the `meta.json` sidecar beside each `spec.md` (the single source of truth) + the event-log projection; archival is semantic (event-only — `/close` emits `pipeline.status: completed`; the directory never moves). The `spec.md` is pure narrative — no `### Stage:` / `### Outcome:` / `### Flags:` / `### Phase:` / `### Scope:` / `### Lang:` / `### Checkpoint:` / `### Parent:` / `### Total waves:` header lines. Wave plans add `wave-plan.md` + `wave-N-{role}/spec.md` subdirectories (each with its own `meta.json`) in the same `{name}/`.

### Close — Deterministic Gate Chain

`/close` delegates to `close-orchestrate`, which runs a gate vector — `verify-pipeline` (build + test; lint only when declared in `stack.md [scripts]`), `qa-run`, `review-spans`, `docs-stale-check` — plus an advisory `pipeline-summary`. On `overall=pass` it AUTO-FINALIZES in-process (`complete_spec` followup; the LLM only relays), emitting `pipeline.status: closed-followup` + `pipeline.complete`; terminal archival (`pipeline.status: completed`) is a separate stage. The unchecked-checklist (`- [ ]`) abort is an orchestrator / `/close` SKILL precondition, not a close-orchestrate gate.

### Spec Artifact — Two Layers

A spec is a single `spec.md` organized in two layers, with body-heading names varying by `Lang`:

- `## PRD` — the *what & why* (intent): when `Lang=en-US`, body headings are `## Context`, `## Users/Stakeholders`, `## Success metric`, `## Non-Goals`, closing with `## Acceptance Criteria`. When `Lang=pt-BR`, they become `## Contexto`, `## Usuários/Stakeholders`, `## Métrica de sucesso`, `## Não-Objetivos`, `## Critérios de Aceitação`.
- `## Plano` (or `## Plan`) — the *how*: `## Entity Info`/`## Informações da Entidade`, `## Files`/`## Arquivos`, optional `## Component Contract`, `## Tasks`/`## Tarefas`, `## Dependencies`/`## Dependências`, `## Boundaries`/`## Limites`.

Both `## PRD` and `## Plan`/`## Plano` are `##`-level dividers; parsed subsections stay at `##`. PLAN produces both layers; the approve flow approves them together (no separate PRD gate); EXECUTE consumes the Plan layer; QA runs the Acceptance Criteria section. Light scope keeps the same shape but lean. Templates: `/feature § Full/Light Scope` + `refs/feature/spec-language.md`.

## Agents

Hand-maintained fallback read by `verify-pipeline` (`discover_via_config`) **only when grain discovery is empty** — the `Build`/`Validate` columns supply per-subproject commands. Primary discovery is `grain.model.json` (`discover_via_grain`); leave this table empty unless a subproject needs an override.

| Subproject | Build | Validate |
|------------|-------|----------|

## Role Rules

There is no canonical list of roles — a role is a runtime parameter that flows from detection (`/scan` analyses manifests like `Cargo.toml`, `package.json`, `.csproj`, `pyproject.toml` and folder structure) into the dispatch. Common labels (`general`, `ui`, `api`, `database`, `library`, …) emerge from detection, not a fixed taxonomy. Per-role delivery contracts (what to produce, how to deliver, return cap, read-only vs write) are **code-rendered** by `build_role_block` / `build_guards_role_block` and interpolated as `{role_block}` at render time — they are NOT written into the Agents table or subproject `CLAUDE.md`. `/scan` only authors each subproject's orientation block + its pending `## Guards`.

## Skill Discovery Heuristic

**Rule**: When a SKILL does glob + parse + aggregation of filesystem state and returns a deterministic table, that is `mustard-rt` work, not LLM work.

**Criterion**: Does the operation change with disk/event-log state (→ `mustard-rt run <X>` subcommand) or with a human decision (→ LLM)?

**Correct pattern**: (1) SKILL calls ONE `rtk mustard-rt run <subcommand> --format table`; (2) prints output verbatim; (3) embeds STATIC blocks (acronyms, modes, fixed instructions) as literals — no dynamic regeneration; (4) parses the USER reply and routes the action.

## Tactical Fix Discovery

A tactical fix discovered during REVIEW/QA CANNOT become a silent follow-up or a brand-new wave mid-EXECUTE — both break SDD purity. Mustard rule: a tactical fix becomes a sub-spec linked via the `meta.json#parent` field + a `spec.link` event. The parent spec is frozen after approve.

**Rule**: REVIEW and QA agents list candidates under `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`. The orchestrator suggests `/mustard:tactical-fix <parent> "<description>"` — **advisory only**, never blocks approve/close.

**Qualification** (ALL): ≤100 LOC; no public contract change (schema, API, exported types, CLI flags); no pending design decision; no new dependency. Outside those bounds → legitimate follow-up OR a fresh full-scope spec.

**Mechanics**: `/mustard:tactical-fix` creates `.claude/spec/<slug>/spec.md` (pure narrative — the parent is surfaced as a `[[<parent>]]` wikilink in the context note) plus a `meta.json` sidecar carrying `parent: <slug>`, and emits `spec.link parent→child`. Fails open when the parent slug is missing. → See `commands/mustard/tactical-fix/SKILL.md`.

## Diff Context Interpolation

Task dispatches inside an active pipeline are prefixed with current git state. Two **distinct** artifacts produce it — do not conflate them (the per-wave cache is NOT `diff-context`):

- **Per-wave `diff.md`** (`.claude/spec/{spec}/wave-N-{role}/diff.md`): the single writer is `mustard-rt run wave-done`, which caches the just-committed wave's `git diff HEAD~1 HEAD --stat` (an atomic LF write — no shell redirect). `agent-prompt-render` reads it back so the next round's agents see the prior wave's changes. Skip the prefix when the file is empty/missing.
- **`mustard-rt run diff-context`**: a richer git-state summary on **stdout** (`## Branch` / `## Staged Changes` / `## Unstaged Changes` / `## Untracked Files` / `## Commits since {parent}` / `### Changed files since divergence`, capped at 3000 chars), consumed by the review dispatch. It does **not** write the per-wave `diff.md`.

## Diagnostic Failure Routing

| Class | Description | Examples |
|-------|-------------|---------|
| **Transient** | Recoverable without new info — retry resolves it | Cache stale, flaky test, race, timeout |
| **Resolvable** | Fixable with targeted patch — root cause clear | Type mismatch, missing import, wrong arg |
| **Structural** | Requires re-analysis — current approach wrong | Wrong layer, entity relation mismatch, false spec assumption |
| **Internal** | Agent crashed / returned no parseable output | Context overflow, parallel race, API error, empty return |

Routing: Internal → re-dispatch SEQUENTIALLY (not parallel), same prompt (retry 1; max 1 per agent). Transient → retry once. Resolvable → patch + retry (counts as retry 1). Structural → re-analyze (1-2 key files), update spec, re-dispatch (does NOT count against the 2-retry cap).

## Parallel Rules

Wave order emerges from dependency analysis (the `Depends on` column of `wave-plan.md`) — there is no rigid default. Schema/data layers typically precede layers that consume them, but that is a deduction from the scan, not a fixed rule. **Parallel override** when a downstream task can run in parallel with its declared upstream: the downstream consumes no generated types/contracts from the upstream and the spec marks the task `(parallel-safe)`. Dispatch parallel-safe tasks in the SAME Task message; if a downstream task fails on missing artifacts, demote it to the next wave and retry. Review parallelism: all review agents dispatch in a SINGLE message; independent + read-only, so parallel is always safe.

## Model

Dispatched agents inherit the main session's model **by default** — there is no routing table, no per-scope sonnet/opus split, and no model-routing gate. A handful of steps **pin** a model in their dispatch prose; this is the authoritative list (each is also documented at its own call site).

**Pinned to Haiku — a bounded, mechanical judgement over a short list, where cost matters and quality is not the constraint:**
- spec-memory relevance gate — keeps only the principle files relevant to a spec (`skills/pipeline-execution/SKILL.md`).

The principle: pin **Haiku** only for bounded mechanical partition/relevance work. Everything not listed inherits the session model — retrieval (the digest) is pure deterministic with NO judge layer above it, and RT itself stays LLM-free and byte-stable, so every pin lives in the orchestration layer, never in the binary.

## Context Loading

| Context Type | Source | Loading |
|-------------|--------|---------|
| Guards | `{subproject}/CLAUDE.md` § Guards | Always loaded (when present) |
| Repo model | `.claude/grain.model.json` | Queried via `mustard-rt run feature` / `scan digest --query` (never read whole) |
| Anchors | files the `feature` insumos / `scan spec` point to | The ~12 real files the agent reads — never the repo |
| Shared language | `CONTEXT.md` (built by `grill-with-docs`) | Relevance-sliced via `context-slice`, injected as `{context_md}` |

`CONTEXT.md` is **never injected whole** — same anti-bloat rule as the `grain.model.json` (read only the anchors). Sliced by entities/file names/key-tokens of the active spec; snapshotted once per wave transition to `.claude/.pipeline-states/{specName}.context-md.md`. Relevance is the only filter — no line cap. No `CONTEXT.md` glossary authored (opt-in via `grill-with-docs`) → empty slice, `{context_md}` blank **by design** — dispatches never block. A `--context` path that is named but missing is reported on stderr (caller misconfiguration), distinct from this blank-by-design case.

## Token Budget per Agent

Budgets are keyed on the dispatched `subagent_type` (`Explore` / `mustard-review` / `general-purpose`); `qa` rides `mustard-review`, `guards` rides `mustard-guards` (read-only).

| Agent | Max Context | Max Tool Uses | Max Return |
|-------|-------------|---------------|------------|
| `impl` (general-purpose) | ≤30K tokens | — | 40 lines |
| `explore` | ≤10K tokens | **≤20** | 30 lines |
| `review` / `qa` | ≤12K tokens | — | 60 lines |
| `plan` | — | — | 80 lines |

Explorer rules: max 20 tool uses; prefer Grep over Read; max 3 full file reads; return findings as soon as root-cause/pattern is clear.

## Agent Return Format (Compact)

- **Files modified:** `path:line` (one per line). Omit if no files touched.
- **Non-obvious decisions:** 1-3 bullets, or `none`.
- **Blockers:** only if any.

DO NOT include identity restatement, full checklist re-listing, list of files read, narrative of steps, confirmation of understanding. The parent has context — return only what is actionable.

## Escalation Statuses

| Status | Meaning | Pipeline Action |
|--------|---------|-----------------|
| `CONCERN` | Quality/design risk flagged, work done | Record in `## Concerns`; continue; surface at CLOSE |
| `BLOCKED` | Missing dep, unclear requirement, unsafe change | Stop the wave; AskUserQuestion; do NOT advance |
| `PARTIAL` | Some steps done, not all | Resume from last completed (Granular Retry Protocol) |
| `DEFERRED` | Step skipped with justification | Note; do NOT retry; confirm with user if load-bearing |

Accumulation: ≥2 agents in same wave return `CONCERN` → surface all together before next wave. `BLOCKED` is NOT a retry trigger — requires user input.

## Enforcement Hooks

Enforcement runs as the single Rust binary `mustard-rt` (modules `bash_command_gate`, `scope_guard`, `tool_use_counter`, `skills_advisory`, `close_gate`, …). `settings.json` wires one `mustard-rt on <event>` entry per lifecycle event.

| Module | Matcher | Mode env | Blocks on |
|--------|---------|----------|-----------|
| `close_gate` | `emit-pipeline` phase=CLOSE | `MUSTARD_CLOSE_GATE_MODE` (default strict) | build/test fail |
| `close_gate` (QA) | same | `MUSTARD_QA_GATE_MODE` (default strict) | no `qa.result` or `qa.result=fail` |
| `close_gate` (QA stale) | same | `MUSTARD_QA_GATE_MODE` (default strict) | `spec.md`/`wave-plan.md` edited after the last `qa.result` → the pass was never re-verified, re-run /mustard:qa |
| `close_gate` (checklist) | same | `MUSTARD_CHECKLIST_GATE_MODE` (default strict) | unchecked `- [ ]` items remain |
| `close_gate` (debt) | same | `MUSTARD_DEBT_GATE_MODE` (default strict) | unresolved tracked debt |
| `bash_command_gate` (rtk gate) | Bash | `MUSTARD_RTK_GATE_MODE` (default **warn**) | unprefixed command → auto-rewrite to `rtk <cmd>` (`updatedInput`, zero round-trip — `rtk` still applied, so the token savings hold); `strict` is opt-in and denies instead; builtins/subshells pass untouched |
| `bash_command_gate` (commit gate) | Bash `git commit` | `MUSTARD_COMMIT_GATE_MODE` (default warn) | secrets staged / build broken |
| `bash_command_gate` (native-redirect) | Bash | hardcoded always-on | `grep`/`ls`/`cat`/`head`/`tail`/`find` → suggests Grep/Glob/Read (+ `[bash-windows-redirect]` sub-gate) |
| `scope_guard` | PreToolUse `Write`/`Edit`/`Task`/`Agent` | fail-open | production-file change outside an approved spec → deny `[scope-guard]` |
| `tool_use_counter` | SubagentStart/Stop | hard | Explore agents at 15 tool uses (warn at 12) |
| `skills_advisory` | Task | advisory | skills count >10 |

Bug in the hook itself (I/O error, timeout outside child process) fails open — only real sensor failures block.

### bash_command_gate Safety Rules (BG01–BG13)

Each rule has a stable ID surfaced in the deny reason (`[bash-safety BGnn]`): BG01 recursive force delete (`rm -rf`); BG02 force push (`git push --force`/`-f`; `--force-with-lease` allowed); BG03 hard reset; BG04 force clean (`git clean -f`); BG05 discard working-tree (`git checkout -- .`); BG06 restore all (`git restore .`); BG07 delete main/master branch; BG08 chmod 777; BG09 mkfs (Linux/macOS); BG10 raw disk write (`dd if=`); BG11 Windows `format <letter>:`; BG12 shutdown; BG13 reboot.

## Shared Memory Architecture

**Truth source**: per-spec append-only NDJSON under `.claude/spec/{name}/.events/` (and `wave-N-{role}/.events/`) — every event (incl. `pipeline.*`) routes there. There is no SQLite append path and no central `events.jsonl`.

**Persistent memory** is markdown, written atomically (`MarkdownStore`):

| Kind | Location | Writer |
|------|----------|--------|
| Knowledge patterns | `.claude/knowledge/{slug}.md` | `session_knowledge_observer` (hooks/session) |
| Decisions | `.claude/memory/decisions/{slug}.md` | `mustard-rt run memory decision` |
| Lessons | `.claude/memory/lessons/{slug}.md` | `mustard-rt run memory decision` |

Agent context is read via **views** in `mustard-rt run event-projections --view <name>` (each folds the per-spec NDJSON, not a database): `agent-visibility`, `pipeline-state`, `active-pipelines`, `session-summary`, `spec-tree`, `epic-summary`, `pr-metrics`.

### On-Demand Memory Queries

The automatic SessionStart injection is capped at `MEMORY_MAX_CHARS` (2000 chars), sourced from `.claude/knowledge/` + `.claude/memory/`. For deeper history: `mustard-rt run event-projections --view session-summary` / `--view pipeline-state --spec <name>` / `--view agent-visibility`.

**Use when**: exploring an area you have partial context on; checking a similar prior decision; resuming a spec after a session gap. **Don't use when**: the knowledge was already auto-injected at SessionStart.
