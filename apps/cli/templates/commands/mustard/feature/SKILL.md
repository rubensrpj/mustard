# /feature - Feature Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/feature <feature-name>`

## Description

Starts the pipeline to implement a feature or enhancement. Self-contained: ANALYZE → PLAN phases. Light scope may include EXECUTE inline.

## Action

### Spec Hygiene (automatic, before ANALYZE)

Audit specs in `active/` before starting — steps 1-5: scan, verify completed/cancelled, handle in-progress via AskUserQuestion, no-active path.

→ See `../../../refs/feature/spec-hygiene.md`

### Diff Context (automatic, cross-phase)

At the start of **PLAN** and **EXECUTE** only, run `mustard-rt run diff-context --subproject {subproject_path} --phase {plan|execute}`. Save to `.claude/.pipeline-states/{specName}.diff-{subproject}.md`. Prepend the subproject diff to every subagent prompt in those phases (`## Current Git State\n{diff}\n\n## Your Task\n...`). Skip header if diff empty/missing. Never dispatch without attempting interpolation. ANALYZE phase intentionally skips this step (diff always empty pre-work) and emits the `analyze-diff-skip` telemetry metric instead.

### Context Slice (automatic, per-wave snapshot)

In the **EXECUTE** phase, as part of the per-wave snapshot (re-run only on a wave transition, alongside the diff-context refresh): produce the relevance-filtered glossary slice that fills the `{context_md}` placeholder of the agent-prompt template.

1. Locate the project's `CONTEXT.md` (built by the `grill-with-docs` skill). Also pass any sibling `CONTEXT.md` files and a `CONTEXT-MAP.md` if present — `context-slice` accepts repeated `--context` flags and expands a map.
2. Run `mustard-rt run context-slice --context {CONTEXT.md} --spec {operational_spec} > .claude/.pipeline-states/{specName}.context-md.md`.
3. Fill `{context_md}` in every subagent prompt of the wave with the contents of that snapshot file.
4. **Graceful degrade:** if no `CONTEXT.md` exists, the script emits an empty slice — leave `{context_md}` empty. Never block the dispatch.

The slice is stable for the whole pipeline (the spec does not change mid-run), so it lives in the PREFIX-STABLE block and caches across every dispatch of the wave. Re-run the snapshot only when the wave changes.

### ANALYZE Phase

**Phase marker (first action, before any Grep):** Run `mustard-rt run emit-phase --spec {spec-name} --to ANALYZE`. ANALYZE runs in the parent before any pipeline-state file exists, so `pipeline-phase.js` cannot see it — this is the only point that knows ANALYZE started. Idempotent (script skips if already emitted for this spec) and fail-open.

**Auto-sync (silent):** Run `mustard-rt run sync-detect`. If output shows any subproject with `hashChanged: true`, then run `mustard-rt run sync-registry`. Otherwise skip sync-registry entirely.

1. Read `.claude/pipeline-config.md` — agents, wave transitions, model selection
2. Grep `entity-registry.json` for the specific entity name — NEVER read the full JSON
3. Determine layers from signals:

| Signal | Layers |
|--------|--------|
| New field/column/relation | DB (+Backend/FE if visible) |
| New endpoint, business logic | Backend (+FE if visible) |
| New screen/component | Frontend (+Backend if new endpoint) |
| New CRUD / sub-entity | DB + Backend + Frontend |
| Refactoring, bug fix | Root cause layer(s) |

When in doubt → `AskUserQuestion`: "Which layers?"

#### Scope Detection

| Signal | → Scope |
|--------|---------|
| 1-2 layers, ≤5 files, known pattern, no new entity | **Light** |
| Entity in registry + modification (add field/column/endpoint/behavior) + ≤8 files, no new entity/table/enum | **Extended Light** |
| 3+ layers, 5+ files, new entity/CRUD, new pattern | **Full** |

Any **Full** signal → Full. All **Light** or **Extended Light** → skip PLAN. Record scope (`light`, `extended-light`, or `full`) for PLAN phase branching.

**Extended Light** = Light flow (skip PLAN, inline EXECUTE). Constraints: entity exists in `entity-registry.json` (Grep confirms), modifies existing entity (no new entity/table/enum/module), ≤8 files, ≤3 layers. Any failed condition or >8 files surfacing during ANALYZE → reclassify as Full.

Light/Extended Light scope CAN use Task(Explore) ONCE with ≤10 tool uses. Prefer Grep/Glob direct. >5 files on Light during ANALYZE → reclassify Extended Light (if entity in registry) or Full.

#### Explore (conditional, budget-capped)

**File budget: MAX 5 reads total in ANALYZE phase (excludes registry/pipeline-config)**

**Path A — SKIP Explore agent** (DEFAULT when entity exists in registry): Entity in registry → skip Explore agent. Read 2-3 reference files directly. Go straight to PLAN.

**Path B — Explore agent ("medium")** (ONLY for genuinely new entities/patterns): Entity NOT in registry AND new CRUD/entity → use Explore agent. **Explorer cap: ≤20 tool uses, ≤3 full file reads.** After Explore returns → go straight to PLAN, ZERO additional reads. NEVER duplicate reads the Explore agent already performed.

**HARD RULE:** If you already understand the change, STOP reading and write the spec. More reads ≠ better spec.

#### Compact Advisory

After ANALYZE, if heavy exploration (>8 file reads, >3 Grep rounds, or multiple Explore agents):
Suggest: _"Analysis complete. Context is heavy — consider `/compact` before proceeding to implementation, then `/resume`."_ Advisory only.

### Decomposition Rule (Wave 7)

When ANALYZE surfaces >5 files, >3 architectural layers, or multiple independent sub-behaviors: STOP, decompose into child specs (2-5 children, each ≤5 files, ≤2 layers). Link via `mustard-rt run spec-link`. Parent enters `COORDINATE` phase until all children reach CLOSE.

→ See `../../../refs/feature/wave-decomposition.md`

### End of ANALYZE — Validation

Run: `rtk mustard-rt run analyze-validation --spec .claude/spec/active/{specName}/spec.md`
If output `ok: false`, append each `issues[]` entry to the spec under `## Concerns` (non-blocking). Continue to PLAN regardless.

### PLAN Phase

#### Grill Opt-In (Full scope only — first PLAN action)

**Only when scope is Full.** Before any other PLAN step, run `AskUserQuestion`:

> "Escrever a spec direto, ou grelhar o plano antes (`grill-with-docs`)?"
> Options: **"Escrever direto"** / **"Grelhar o plano"**.

- **"Grelhar o plano"** → invoke `Skill(grill-with-docs)` BEFORE drafting the spec. The skill runs its own relentless interview against the project's domain model and maintains `CONTEXT.md` on its own — `/feature` does not write, slice, or read `CONTEXT.md` here. Only after the grilling session concludes, continue to "Spec Language Resolution" and draft the spec.
- **"Escrever direto"** → proceed straight to "Spec Language Resolution" with no grilling.

**Light / Extended Light scope:** NEVER grill — do NOT show this question. Skip directly to the Light Scope flow.

**The skill is NOT adapted.** `/feature` only triggers `Skill(grill-with-docs)`; Matt's verbatim skill content (`templates/skills/grill-with-docs/`) stays untouched — no Mustard-specific edits, no wrapper. The skill manages `CONTEXT.md`/ADRs entirely on its own per its own instructions.

#### Spec Language Resolution

Cascade (stop at first hit): (1) spec header `### Lang: pt|en`, (2) `.claude/mustard.json#specLang`, (3) `AskUserQuestion` ÚNICA — `"Spec language: pt | en?"` (persist to mustard.json). Write resolved value as `### Lang: pt|en` after `### Checkpoint`.

**HARD RULES:** Lang=pt → ALL `## ` body headings in PT (Boundaries→Limites, Root cause→Causa raiz, Plan→Plano, Concerns→Preocupações, Acceptance Criteria→Critérios de Aceitação, Non-Goals→Não-Objetivos). Lang=en → keep headers EN. Source code (identifiers, all comment forms, log/error messages, AC `Command:`) always EN regardless of Lang. Pre-existing comments NOT translated (karpathy §3 surgical). Exceptions always EN: status/phase/scope values, shell commands, filenames, the `### Lang:` line.

→ See `../../../refs/feature/spec-language.md` for full Header Translation Table.

#### Wave Decomposition Pre-Check (Full scope only)

Check whether the work should be decomposed into waves before writing a single spec. Signals: fileCount, layerCount, newEntityCount, knowledgeMatches. Runs `mustard-rt run scope-decompose` + `mustard-rt run wave-dependency`. Produces wave-plan.md + per-wave spec.md if decompose=true.

→ See `../../../refs/feature/wave-decomposition.md`

#### Roadmap Auto-Scaffold (automatic)

When `scope-decompose` returns `reason: "roadmap-signal"` and `roadmapMatches` contains a path to `.claude/plans/*.md`:

1. Read that plans file.
2. Extract the waves table (rows matching `^\|\s*(W?\d+|Wave\s*\d+)\s*\|`).
3. **Auto-create** under the spec dir:
   - `wave-plan.md` (table copied/adapted from the plans file, status column initialized to `queued` for all)
   - `wave-1-{role}/spec.md` — full detail (Status: draft, narrative copied)
   - `wave-N-{role}/spec.md` for N=2..total — skeleton only (Status: queued, Title + 1-line summary)
4. **Emit pipeline scope event** — a wave plan has no root `spec.md`, so this scaffold is the only place its initial pipeline state is born. Emit two events:
   ```bash
   mustard-rt run emit-pipeline --kind scope --spec {spec-name} --payload "{\"scope\":\"full\",\"lang\":\"{lang}\",\"model\":\"opus\",\"is_wave_plan\":true,\"total_waves\":{wave-count}}"
   mustard-rt run emit-pipeline --kind status --spec {spec-name} --payload "{\"from\":null,\"to\":\"draft\"}"
   ```
   `/mustard:approve` § Step 3b reads `pipeline_state_for_spec` from SQLite — no JSON file is written here.
5. **No AskUserQuestion** — proceed silently per the agnostic auto-detection contract.

#### Full Scope

The spec is a **SINGLE file** organized in two named layers — `## PRD` (the *what & why*) at the top, `## Plano` (the *how*) at the bottom. Both are `##`-level **divider headings**; the subsections under them stay at `##` level (parsers anchor on `## Contexto`, `## Arquivos`, `## Tarefas`, `## Critérios de Aceitação`, `## Limites` — never demote them to `###`). The Acceptance Criteria sit at the boundary between the layers: they are the verifiable *what*, so they close the PRD layer.

1. Create `.claude/spec/active/{date}-{name}/spec.md` with this layout (Lang=pt headings shown; Lang=en uses the EN column of `../../../refs/feature/spec-language.md § Header Translation Table`):

   ```text
   # {Title}
   ### Status / Phase / Scope / Checkpoint / Lang headers

   ## PRD                       ← divider — the "what & why"
   ## Contexto                  ← narrative briefing
   ## Usuários/Stakeholders     ← who is affected / who asked
   ## Métrica de sucesso        ← how success is measured
   ## Não-Objetivos             ← explicit out-of-scope
   ## Critérios de Aceitação    ← boundary: verifiable "what"

   ## Plano                     ← divider — the "how"
   ## Informações da Entidade
   ## Arquivos
   ## Component Contract        ← UI specs only (see CONDITIONAL below)
   ## Tarefas
   ## Dependências
   ## Limites
   ```

   - **PRD layer** (`## PRD` divider, then):
     - `## Contexto` — heading **exactly** `## Contexto` (Lang=pt) or `## Context` (Lang=en) — never substitute with synonyms (Sintoma, Symptom, Description, Background). Body is **narrative prose, 4-8 lines**: how the system should work + what changed + how the gap violates expectation + observable impact on user/business. NO tables, NO line numbers, NO method names, NO bullets. **MUST follow** `../../../refs/feature/spec-language.md § Contexto Narrative Rules` (good/bad examples there).
     - `## Usuários/Stakeholders` — 1-3 lines: who is affected by this change and who requested it. Plain language, no jargon.
     - `## Métrica de sucesso` — 1-3 lines: how you will know the feature succeeded (observable outcome, not implementation detail).
     - `## Não-Objetivos` — bullet list of what this spec deliberately does NOT do.
     - **MANDATORY: `## Acceptance Criteria` section** (Wave 10) — 3-8 binary, executable items: `- [ ] AC-1: {description} — Command: \`{exact command}\``. Each: exit 0 = pass; runnable from project root; focus on observable behavior (build, endpoint, test). Include `Testable, binary (pass/fail) criteria. Each MUST be executable and independent.` header line. Sits last in the PRD layer — it is the verifiable *what*.
   - **Plano layer** (`## Plano` divider, then):
     - `## Informações da Entidade`, `## Arquivos`, `## Tarefas`, `## Dependências`, `## Limites`.
     - Tasks organized by `### {Agent} Agent (Wave {N})`
     - 3-8 checkboxed steps per agent, decomposed by operation type (NOT by file)
     - Mark `(parallel-safe)` on frontend tasks with no dependency on new backend endpoints
   - **CONDITIONAL: `## Component Contract` section (UI specs only)** — append between `## Arquivos` and `## Tarefas` (inside the Plano layer) when ANALYZE detects component creation/refactoring (new `*.tsx|*.vue|*.svelte|*.dart|*.swift` widget/View, or props/variants change). Template + rationale at `../../../refs/feature/spec-language.md § Component Contract`. **Skip for non-UI work** — adding this section to backend/database specs is bloat.
2. Add checkpoint fields: `Status: draft`, `Phase: PLAN`, `Scope: full`, `Checkpoint: {now}`
3. Emit pipeline events for Full scope spec:
   ```bash
   mustard-rt run emit-pipeline --kind scope --spec {spec-name} --payload "{\"scope\":\"full\",\"lang\":\"{lang}\",\"model\":\"{model}\",\"is_wave_plan\":false}"
   mustard-rt run emit-pipeline --kind status --spec {spec-name} --payload "{\"from\":null,\"to\":\"draft\"}"
   ```
4. Elegance Check: 3+ files or complex logic → "Is there a more elegant approach?"
5. **Present full spec to user:** Read spec file and print ENTIRE contents verbatim in a fenced markdown block. Add 1-line change summary (WHAT + WHY). Then `AskUserQuestion`: **"Approve and implement?"** / **"Adjust (give feedback)"** / **"Save for later (stop)"**.

#### Wave Tree (end of PLAN)

Run `mustard-rt run wave-tree --spec-dir .claude/spec/active/{spec-name}` and print the output inline immediately before the AskUserQuestion. Fail-open (warn, do not block PLAN).

#### Light Scope

Light keeps the same two-layer shape but **lean** — a thin PRD layer and a thin Plano layer. Do NOT add bureaucracy: no Usuários/Stakeholders, no Não-Objetivos, no Entity Info, no Dependencies sections. The two dividers cost one line each and keep Light specs consistent with Full.

1. Create `.claude/spec/active/{date}-{name}/spec.md` with compact format — headers: `# Enhancement: {name}`, `### Status: draft | Phase: PLAN | Scope: light`, `### Checkpoint: {ISO}`, `### Lang: {pt|en}`, then:
   - **PRD layer** — `## PRD` divider, then `## Contexto` (Lang=pt) or `## Context` (Lang=en) — heading EXACT, body **narrative prose 3-6 lines** (how the system should work + what's the gap + user/business impact; NO line numbers/method names/tables — see `../../../refs/feature/spec-language.md § Contexto Narrative Rules`), then `## Métrica de sucesso` (1 line — the single observable outcome that proves it worked), then `## Acceptance Criteria` (1-3 items, `- [ ] AC-1: {description} — Command: \`{exact command}\``; at least AC-1 must verify the feature works).
   - **Plano layer** — `## Plano` divider, then `## Summary` (1-2 lines, technical synthesis), `## Checklist` → `### {Agent} Agent` (steps + build/type-check), `## Files (~{N})` (paths).
2. Emit pipeline events for Light scope spec:
   ```bash
   mustard-rt run emit-pipeline --kind scope --spec {spec-name} --payload "{\"scope\":\"light\",\"lang\":\"{lang}\",\"model\":\"sonnet\",\"is_wave_plan\":false}"
   mustard-rt run emit-pipeline --kind status --spec {spec-name} --payload "{\"from\":null,\"to\":\"draft\"}"
   ```
3. **Present full spec to user:** Print ENTIRE contents verbatim in fenced markdown block. Then `AskUserQuestion`: **"Approve and implement now"** / **"Approve for later"** / **"Adjust"**.

#### Spec Boundaries

Add `## Boundaries` section before writing tasks: list only paths intentionally touched (exact files, directories, globs). Out-of-boundary edits surface `[BOUNDARY WARNING]` — treat as signal, not error to suppress.

### Pre-EXECUTE Existence Gate (Full scope only)

Dispatch 1 Haiku Task(Explore) to verify work is still needed. Pre-check via `rtk git diff --stat` first (skip if <10 insertions/deletions). Decision: all-no → transparent; mixed → mark done tasks [x], re-dispatch for remaining; all-yes → mandatory AskUserQuestion before closing as already-implemented.

→ See `../../../refs/feature/existence-gate.md`

### EXECUTE Phase (Light scope — same session)

When user chooses "Approve and implement now":
0. **Pre-EXECUTE Rewave Check:** Run `mustard-rt run exec-rewave-check --spec .claude/spec/active/{spec-name}/spec.md`. Parse JSON output. If `action: "decomposed"`, the spec was just split into N waves — proceed using wave-1's spec (`wave-1-{role}/spec.md`) instead of the original. If `action: "keep-single"` or `"skip"`, continue with the original spec normally. Silent operation — no AskUserQuestion.
1. Update spec: `Status: implementing`, `Phase: EXECUTE`. Every agent prompt MUST include: `Return format cap: ≤50 lines. Apply compact Return Format from .claude/pipeline-config.md strictly.`
2. Emit status transition to implementing:
   ```bash
   mustard-rt run emit-pipeline --kind status --spec {spec-name} --payload "{\"from\":\"draft\",\"to\":\"implementing\"}"
   ```
3. Read `.claude/pipeline-config.md` for agent config. Grep `entity-registry.json` for specific entity block only
4. Match recipes by title via Grep on `{subproject}/.claude/commands/recipes.md` — do NOT read full file
4b. **Structured Recipe (if available):** Run `mustard-rt run recipe-match --entity {entity} --operation {operation} --subproject {subproject_path}`. If non-empty JSON, inject into agent prompt as `{recipe_context}`. Gives agent a 90%-complete skeleton.
5. Identify relevant skills for `{recommended_skills}`: **prepend `karpathy-guidelines`** for code-editing agents (impl/backend/frontend/database/bugfix); skip karpathy for Explore and Review. Then list task-relevant skill names. See `.claude/refs/agent-prompt/agent-prompt.md § How to fill {recommended_skills}`.
6. Dispatch agents (wave rules: DB+Backend parallel, Frontend after Backend UNLESS `(parallel-safe)`)
7. Wave transitions between waves (from `.claude/pipeline-config.md`)
8. On return: validate (build/type-check). The `checklist-auto-mark.js` hook already marked Checklist items as the agent edited matching files (silent, no tool call). If any item didn't auto-mark (no file pista in the item text), close-gate at CLOSE will surface it.
8b. **Agent Memory:** `mustard-rt run memory agent --json '{"agent_type":"{type}","wave":{N},"pipeline":"{spec-name}","summary":"{what}","details":{...}}'` — one per agent. Skip if single-wave pipeline.

#### Escalation Status Handling

After each agent returns, check for escalation before advancing:

- **Internal error** — re-dispatch sequentially (not parallel). Max 1 Internal retry per agent
- `CONCERN` — record verbatim under `## Concerns`; continue to next step
- `BLOCKED` — stop immediately; AskUserQuestion with exact blocker; do NOT retry or advance
- `PARTIAL` — apply Granular Retry Protocol from last completed step; do NOT restart from step 1
- `DEFERRED` — note in spec with agent justification; ask user if deferred item is load-bearing before closing

If two or more agents in same wave return `CONCERN`, surface all concerns together before starting next wave. See `.claude/pipeline-config.md` Escalation Statuses and Diagnostic Failure Routing.

9. **REVIEW** — dispatch review agent per affected subproject (guards + relevant skills, 7-category checklist). REJECTED → see `resume/SKILL.md § Fix Loop Dispatch Protocol` (max 2 loops). Re-reviews always use `model: "sonnet"`. After the verdict is consolidated, for each reviewed subproject run `mustard-rt run review-result --spec {specName} --verdict {approved|rejected} --critical {N} --subproject {subproject}` — emits the `review` metric surfaced in `/stats` Verification. Fail-open.
10. All passed + APPROVED → run QA Phase (Wave 10, see below) → on QA `pass`/`skip` → CLOSE flow inline (sync registry, move spec, cleanup state)
11. Failed → max 2 retries, then STOP + report

#### Failure Routing

Classify before retrying: (1) **Transient?** → retry once immediately. (2) **Resolvable?** (≤3-line patch, no new reads) → apply patch, retry (counts as retry 1). (3) **Structural?** (spec assumed false) → re-analyze 1-2 files, update spec, re-dispatch — does NOT count against 2-retry cap. Retry cap applies to Transient + Resolvable only.

### QA Phase (Wave 10)

After all EXECUTE tasks complete: (1) emit phase transition via `mustard-rt run emit-phase --spec {specName} --to QA`. (2) Run `mustard-rt run qa-run --spec {specName}`. (3) `overall=pass` → update `## Acceptance Criteria` checkboxes, then emit `mustard-rt run emit-phase --spec {specName} --to CLOSE` (triggers `close-gate`) → CLOSE; `overall=fail` → return failing AC list to implementation agent, re-run; `overall=skip` (no AC) → warn + allow CLOSE. Max 3 QA iterations — then `AskUserQuestion`: "QA has failed 3 times. Choose: (a) Fix manually and retry, (b) Relax the AC, (c) Abort pipeline."

Update `## Acceptance Criteria` checkboxes: `[x]` passed, `[ ]` failed. Visual: `[v] ANALYZE  [v] PLAN  [v] EXECUTE  [>] QA  [ ] CLOSE`

## Visual Output

Progress: `[v] ANALYZE  [>] PLAN  [ ] EXECUTE  [ ] QA  [ ] CLOSE` — add `[LIGHT]` or `[FULL]` scope tag after progress line.

## Spec Layout

Progressive disclosure for specs: default single `spec.md` (Light/Full ≤200 lines). When >200 lines, extract autonomous sections to `spec-references/{section}.md` in same dir; spec.md keeps `→ See spec-references/{section}.md` pointers. Hard block at 500 (env `MUSTARD_SPEC_SIZE_MODE`, default `warn` — logs `[SPEC-SIZE]`; `strict` blocks PLAN write). Wave plans (`wave-plan.md` + per-wave `spec.md` dirs) are the canonical multi-file form. Reference: Anthropic progressive disclosure (skill-creator).

### Acceptance Criteria — Cross-Shell Pattern

Write AC commands in portable form: prefer `node -e "..."` for multi-step assertions, `bash -c '...'` for shell pipes, or single commands where exit code is the verdict. Avoid raw bash syntax (`for`, `test`, `[...]`) — cmd.exe silently mishandles it on Windows.

→ See `../../../refs/feature/ac-cross-shell.md`

## Rules
- This command is self-contained — reads `.claude/pipeline-config.md` directly
- NEVER implement code in Full scope — only PLAN. EXECUTE via `/approve` + `/resume` (or `/approve --resume` to chain inline)
- NEVER launch Explore agent when entity already exists in registry — read 2-3 files directly
- NEVER read additional files after Explore agent returns — its output is final
- NEVER exceed 5 file reads in ANALYZE phase (registry + pipeline-config are free)
- Light scope + user chose "implement now" → proceed to EXECUTE inline
- ALWAYS read `.claude/pipeline-config.md` for agent/wave/model info
- ALWAYS create pipeline state at PLAN phase
- ALWAYS record `scope` in spec header AND pipeline state
- ALWAYS go straight to PLAN once you understand the change — more reads ≠ better spec
- Light scope inline implement follows same dispatch rules as `/resume` (template, waves, retries)
- Context budget: Grep entity-registry (not full read), Grep recipes (not full read), line-by-line checkbox updates
- Wave decomposition is opt-in via signals (knowledge matches, layer/file/entity counts) — never force waves on small scopes
- If wave decomposition is approved, single-spec Full Scope flow is skipped — waves execute sequentially via `/resume`
ULTRATHINK
