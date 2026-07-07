---
name: pipeline-execution
description: Pipeline phases, dispatch rules, wave system, validate, retry. Use when running /feature, /resume, /approve or any pipeline phase requiring dispatch/wave context.
tags: [plan, any]
appliesTo: []
scope: [plan, code-editing]
metadata:
  generated_by: foundation
disable-model-invocation: true
source: manual
---

# Pipeline Execution Detail

> Phases, role rules, dispatch mechanics, validation, bugfix paths. Loaded on-demand.

**Iron law: code lands in the layer's existing shape — the subproject's `## Guards` and its `{role}-pattern` molds are LAW for the diff, not context.** A diff that violates a Guard or sidesteps an applicable mold is WRONG even if it compiles, passes tests, and works — quality is judged on shape, not just behavior.

## Rationalizations that don't fly

| Excuse | Answer |
|--------|--------|
| "it works — I'll align it with the pattern later" | later never ships; the mold exists so code lands right the FIRST time |
| "it's a small helper, no mold applies" | small helpers rot into parallel conventions — put it where the mold says it lives |
| "the local pattern is outdated; I'll write it the modern way" | divergence is the OWNER's call — flag it in your report, never impose it in the diff |
| "that Guard doesn't apply to my case" | if you can't justify WHY in one line of your report, it applies |
| "tests pass, so quality is covered" | tests check behavior; Guards and molds keep the codebase ONE codebase |

**Red flags** — stop if you catch yourself: *"I'm inventing a new folder/naming scheme mid-task."* · *"I'm copying a pattern from another project, not this layer."* · *"I'm justifying a Guard violation in a code comment instead of my report."*

## Pipeline Feature

### ANALYZE Phase (collapses old SYNC+UNDERSTAND+SCOPE+EXPLORE)

1. **MODEL:** `mustard-rt run scan` (produce/refresh `.claude/grain.model.json`).
2. Research via the scan digest — NEVER read the repo or the model whole. Run `mustard-rt run feature --intent "{request}"`; the insumos carry the matched slices/contracts/hubs + the anchor files. `miss: true` → no repo-vocabulary precedent (re-query with repo terms; treat true net-new as design). Otherwise infer layers from the matched slices.
3. Read ONLY the `anchors` the insumos point to (~12 real files), then ask `mustard-rt run scan spec` per unit.

| Signal                       | Layers                               |
| ---------------------------- | ------------------------------------ |
| New field/column/relation    | DB (+ Backend/FE if visible)         |
| New endpoint, business logic | Backend (+ FE if visible)            |
| New screen/component         | Frontend (+ Backend if new endpoint) |
| New CRUD / sub-entity        | DB + Backend + Frontend              |
| Refactoring, bug fix         | Root cause layer(s)                  |

When in doubt → `AskUserQuestion`: "Which layers?"

**Scope Detection:**

| Signal                                             | → Scope   |
| -------------------------------------------------- | --------- |
| 1-2 layers, ≤5 files, known pattern, no new entity | **Light** |
| 3+ layers, 5+ files, new entity/CRUD, new pattern  | **Full**  |

**Explore (conditional):**

- Entity in registry → SKIP Explore, read 2-3 reference files directly
- Entity NOT in registry → Explore agent ("medium"), then straight to PLAN
- **MAX 5 file reads in ANALYZE** (registry/pipeline-config are free)

### PLAN Phase (collapses old SPEC)

Create `.claude/spec/{date}-{name}/spec.md`:

- **Full scope:** Summary, Entity Info, Files, Tasks (by wave), Dependencies. Each wave's PLAN must declare its target files — `wave-scaffold` seeds that wave's trackable checklist from them into the wave's `meta.json` (one `{label, path, done: false}` item per file). The wave-plan parent is a coordination doc and carries no checklist.
- **Light scope:** Summary (1-2 lines), Checklist (tasks by agent, no waves).

Lifecycle state (stage/phase/scope/checkpoint) lives ONLY in the `meta.json` sidecar — never write `Status:` / `Phase:` / `Scope:` / `Checkpoint:` header lines into the markdown; the spec.md stays pure narrative.
Create `.claude/.pipeline-states/{spec-name}.json`.

**Light Scope → Inline Path:** When `/feature` detects Light scope and user approves inline, EXECUTE runs in same session. No PLAN phase needed.

### EXECUTE Phase (collapses old IMPLEMENT+VALIDATE+REVIEW)

**1. Skills Auto-Loading:**

Agents auto-load relevant skills from `{subproject}/.claude/skills/` based on task description.
The subproject's curated Guards are injected inline by the renderer (`## GUARDS`); there is no `{recommended_skills}` hint block (generated skills were removed).

**1b. Relevance gate for spec memory — run ONCE, before the first dispatch round (SKIP when `.claude/spec/{specName}/memory/` is empty — the common case):**

Irrelevant memory principles measurably degrade a subagent's reasoning (distractors compound with reasoning depth), so the rendered `## SPEC MEMORY` block must carry only what pertains to THIS spec — filtered by **relevance, never trimmed to a count**. When the spec has `memory/*.md` principle files:

1. Read each principle's frontmatter `name` + `description` (a handful of small files — read directly, it is cheap).
2. Dispatch a throwaway precision judge — `Task(subagent_type: general-purpose, model: haiku)`, read-only — passing the spec goal (from `spec.md`) plus the candidate `name — description` list **inline**. Instruction: *"Return ONLY the names whose principle is relevant to this spec's work, one per line. When unsure, EXCLUDE."* (Haiku here is a deliberate exception to the inherit-session-model rule: a cheap relevance judge over a short list, not pipeline work — RT itself stays LLM-free and byte-stable, so the judge lives here in the orchestration layer.)
3. Write the approved names to `.claude/spec/{specName}/.memory-approved`, one per line. **Write the file even when the judge approves nothing** (an empty file) — that means "none relevant → inject no memory", which is honoured, not a fallback.

The renderer inside `wave-advance` reads `.memory-approved` and injects EXACTLY that set (`select_spec_memory_by_names`). **No file** → the deterministic recall matcher runs (relevance-ranked, uncapped) as the ungated fallback. The gate is per-spec and idempotent — re-run it only if the spec's `memory/` set changes.

**2. Plan Waves — routed by Rust, the LLM relays:**

Do NOT read `wave-plan.md` or decide the wave order by hand. Run:

```bash
mustard-rt run wave-advance --spec {specName}
```

It returns the **current dispatch round** as a deterministic JSON array — every wave of the first dependency level whose waves lack `pipeline.wave.complete`; once every impl wave is complete it returns the **review round** (one `role: review` / `mustard-review` item per touched subproject, prompts rendered); `[]` only after every touched subproject carries a `review.result`. Each item is `{wave, role, subproject, subagent_type, prompt}` with the `prompt` **already rendered**:

- Items returned together have no dependency between them → dispatch them together in ONE message (multiple `<invoke>` blocks). Re-run `wave-advance` after the round completes — a higher level starts ONLY after every lower-level wave completes.
- NEVER nest dispatch — nesting breaks parallel execution.
- `resume-bootstrap` decides the **stage**; `wave-advance` decides the **wave routing + render** (`dispatch-plan` remains as an inspection view of the full DAG/levels). The orchestrator is a relay over the array, not a planner.

**3. Dispatch Agent:**

For each item, pass its `prompt` **verbatim** to the Task `prompt` (it was already rendered by `agent-prompt-render` inside `wave-advance` — never hand-assembled; it arrives as a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands at dispatch — never read the `.dispatch/` file in the parent, that pays the full prompt back into your context) with the item's **`subagent_type`** (the tool picks it per role: read-only roles run tool-restricted — `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`, so they physically cannot write; writing roles → `general-purpose`). Never pick the agent by hand. The rendered template carries the role contract + boundary + return cap inline, plus the spec's project section + its anchors.

**4. Validate:**

- Build passes (backend: `dotnet build`, frontend: `pnpm build`, mobile: `fvm flutter analyze`)
- Zero critical Guard/mold violations (the iron law above — shape, not just behavior)
- Checklist marking is automatic: the `checklist-auto-mark` hook runs after every Edit/Write (incremental, silent). Wave specs are meta-first: the hook flips the matching item to `done: true` in the wave's `meta.json#checklist` (matched by the item's `path`/basename) and emits a `checklist.item.marked` NDJSON event — the checklist seeded from the wave's target files is auto-markable by construction. Markdown `## Checklist` sections (Light scope, or legacy specs without a meta checklist) are still marked in place — give each item a file hint: include the file basename in the item text (e.g. `- [ ] Validate UserService.cs`) or append a target arrow (e.g. `- [ ] Validate input → src/Services/UserService.cs`). Items without any hint won't auto-mark; close-gate will surface them at CLOSE.
- Any failure → retry (max 2/agent), then STOP + replan

**6. Review (MANDATORY — NEVER skip):**
Review enters the same `wave-advance` loop: once every impl wave completes, `wave-advance` returns one rendered `role: review` item (`mustard-review`) per affected subproject — dispatch them like any round; after each returns, the orchestrator records the verdict with `mustard-rt run review-result --spec {specName} --verdict approved|rejected [--critical N] --subproject {sub}`. The review agent reads `{subproject}/CLAUDE.md` — the `## Guards` section carries the subproject's DO/DON'T rules — and runs the full 7-category checklist:

1. **SOLID** — SRP, OCP, LSP, ISP, DIP
2. **Design System** — tokens, typography, spacing, components, icons, theme
3. **Patterns** — the subproject's `## Guards` AND its `{role}-pattern` molds; a violation of either is CRITICAL, never a style note
4. **i18n** — all strings localized, all locale files updated
5. **Integration** — types synced, no orphans, no circular deps
6. **Build** — compiles/analyzes clean
7. **Elegance** — simplest solution, no over-engineering

APPROVED (zero CRITICAL) → CLOSE. REJECTED (any CRITICAL) → fix agent dispatched (max 2 fix loops), then re-review.

**7. Capabilities (OPTIONAL — only when this feature created/changed a user-visible behaviour):**
Most small specs touch no capability — skip this. When the feature DID add or change one, author/update its durable capability doc: `mustard-rt run capability create --slug {slug} --title "{title}"`, then edit `.claude/capabilities/{slug}.md` (its `### Requirement:` / `#### Scenario:` blocks with when/then[/command] + `## Covers` entity links), and link it in the spec's `## Capabilities` section as a `- [[cap.{slug}]]` bullet. On CLOSE the merge folds each linked doc back (adds the `spec.*` backlink + emits `capability.declared`). Absent section = no-op.

### CLOSE Phase (collapses old COMPLETE)

1. `mustard-rt run scan` (refresh `grain.model.json` if the codebase changed)
2. Checklist must already be fully done from EXECUTE — `close-gate` consolidates every wave's `meta.json#checklist` (markdown `## Checklist` is the legacy fallback) and blocks CLOSE while any item is unmarked. Never write lifecycle headers (`Status:` / `Phase:`) into the markdown — `meta.json` is synced by the close itself (step 3).
3. Run `mustard-rt run close-orchestrate --spec {name}`. When `overall == pass` it **auto-chains the finalize in-process** (flips the spec straight to `completed`, emits + verifies `pipeline.complete`, syncs `meta.json` to Close/Completed); the LLM does not call `complete-spec` itself. When `overall == fail` it is report-only — fix the failing gate and re-run. (A close lands straight on `completed` — there is no follow-up grace window; follow-up work goes into a separate linked sub-spec. No filesystem move — spec dir stays at `.claude/spec/{name}/`.)
4. Output with agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══`

### Replan Protocol

When: agent FAILED structurally, retry exhausted, user reports unexpected behavior, review REJECTED with architectural concern.
Steps: update spec → summarize failure → Explore → rewrite tasks → re-approve → resume EXECUTE.

## Role Rules

> See `.claude/pipeline-config.md § Role Rules` for role boundaries and validation rules.

## Pipeline Bugfix

### Fast Path (1-2 files, clear cause)

ANALYZE → FIX → VALIDATE → CLOSE. No spec needed.

### Full Path (3+ files, unclear impact)

ANALYZE → PLAN → APPROVE → FIX → VALIDATE → CLOSE.

### Decision

Explore returns clear root cause in 1-2 files → Fast Path. Otherwise → Full Path.
