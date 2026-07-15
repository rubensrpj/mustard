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

> Phases, role rules, dispatch mechanics, validation, bugfix paths. Loaded on-demand. Rationale: `docs/TEMPLATE-RATIONALE.md`.

Law: code lands in the layer's existing shape — the subproject's `## Guards` and its `{role}-pattern` molds are LAW for the diff. A diff that violates either is wrong even if it compiles and passes tests; divergence from a local pattern is the owner's call — flag it in the report, never impose it in the diff. Red flags to stop on: inventing a folder/naming scheme mid-task; copying a pattern from another project; justifying a Guard violation in a code comment instead of the report.

## Pipeline Feature

### ANALYZE

1. Model: `mustard-rt run scan` (produce/refresh `.claude/grain.model.json`).
2. Research via the digest, never the repo whole: `mustard-rt run feature --intent "{request}"`. `miss: true` → re-query with repo vocabulary; treat true net-new as design. Infer layers from the matched slices.
3. Read ONLY the `anchors` (~12 real files), then `mustard-rt run scan spec` per unit.
4. Explore: entity in registry → skip Explore, read 2-3 reference files; not in registry → one Explore ("medium") then PLAN. Max 5 file reads in ANALYZE (registry/pipeline-config are free). In doubt about layers → one AskUserQuestion.

| Signal | Layers |
| --- | --- |
| New field/column/relation | DB (+ Backend/FE if visible) |
| New endpoint, business logic | Backend (+ FE if visible) |
| New screen/component | Frontend (+ Backend if new endpoint) |
| New CRUD / sub-entity | DB + Backend + Frontend |
| Refactoring, bug fix | Root cause layer(s) |

| Scope signal | Scope |
| --- | --- |
| 1-2 layers, ≤5 files, known pattern, no new entity | Light |
| 3+ layers, 5+ files, new entity/CRUD, new pattern | Full |

### PLAN

- Full: spec at `.claude/spec/{date}-{name}/spec.md` with Summary, Entity Info, Files, Tasks (by wave), Dependencies. Each wave's PLAN declares its target files — `wave-scaffold` seeds that wave's checklist from them into the wave's `meta.json` (`{label, path, done:false}` per file). The wave-plan parent is coordination only, no checklist.
- Light: Summary (1-2 lines) + Checklist (tasks by agent, no waves). Light + user approval → EXECUTE inline in the same session, no PLAN phase.
- Lifecycle state (stage/phase/scope/checkpoint) lives ONLY in `meta.json` — never `Status:`/`Phase:`/`Scope:` headers in the markdown.

### EXECUTE

1. Skills auto-load from `{subproject}/.claude/skills/` by task; the renderer injects the subproject's Guards inline (`## GUARDS`).
2. Spec-memory relevance gate — once per spec, before the first dispatch round; skip when `.claude/spec/{spec}/memory/` is empty:
   - Read each principle's frontmatter `name` + `description` directly.
   - Dispatch a throwaway read-only judge — `Task(general-purpose, model: haiku)` — with the spec goal + the `name — description` list inline: "Return ONLY the names relevant to this spec's work, one per line. When unsure, EXCLUDE." (Haiku is a deliberate exception to inherit-session-model: a cheap relevance judge, not pipeline work.)
   - Write the approved names to `.claude/spec/{spec}/.memory-approved` — write it even when empty (empty = "inject none", honoured). No file → the deterministic recall matcher is the fallback. Re-run only if `memory/` changes.
3. Wave routing is Rust's, the LLM relays: `mustard-rt run wave-advance --spec {spec}` returns the current dispatch round as deterministic JSON (`{wave, role, subproject, subagent_type, prompt}` per item, prompt already rendered). Items in one round have no dependency → dispatch together in ONE message. Re-run after the round completes; a higher level starts only after every lower-level wave completes. Never nest dispatch. Once impl waves are done it returns the review round; `[]` only after every touched subproject has a `review.result`. `resume-bootstrap` decides the stage; `wave-advance` the routing — the orchestrator is a relay, not a planner.
4. Dispatch: pass each item's `prompt` verbatim to Task with its `subagent_type` (read-only roles are tool-restricted: `explore`→Explore, `review`/`qa`→mustard-review, `guards`→mustard-guards; writing roles → general-purpose). The prompt arrives as a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands at dispatch — never read the `.dispatch/` file in the parent. Never pick the agent by hand.
5. Validate:
   - Build passes (backend `dotnet build`, frontend `pnpm build`, mobile `fvm flutter analyze` — per detected stack).
   - Zero Guard/mold violations (the law above — shape, not just behavior).
   - Checklist marking is automatic: the `checklist-auto-mark` hook flips the matching wave `meta.json#checklist` item by `path`/basename and emits `checklist.item.marked`. Markdown `## Checklist` (Light/legacy) still marks in place when the item carries a file hint (basename in the text, or `→ path`); unhinted items surface at CLOSE.
   - Failure → retry (max 2 per agent), then STOP + replan.
6. Review — mandatory, never skipped: `wave-advance` returns one rendered `role: review` item per touched subproject; after each returns, record `mustard-rt run review-result --spec {spec} --verdict approved|rejected [--critical N] --subproject {sub}`. The reviewer runs the 7 categories: SOLID · Design System (tokens/typography/spacing) · Patterns (Guards + molds — a violation is CRITICAL, never a style note) · i18n · Integration (types synced, no orphans/cycles) · Build · Elegance (simplest solution). Zero CRITICAL → CLOSE; any CRITICAL → fix agent (max 2 loops), re-review.
7. Capabilities — only when the feature created/changed a user-visible behaviour (most specs: skip): `mustard-rt run capability create --slug {slug} --title "{title}"`, edit `.claude/capabilities/{slug}.md` (`### Requirement:` / `#### Scenario:` when/then blocks + `## Covers`), link `- [[cap.{slug}]]` in the spec's `## Capabilities`. CLOSE folds it back. Absent section = no-op.

### CLOSE

1. `mustard-rt run scan` if the codebase changed.
2. Checklist must be fully done — `close-gate` consolidates every wave's `meta.json#checklist` (markdown is the legacy fallback) and blocks while any item is unmarked.
3. `mustard-rt run close-orchestrate --spec {name}`: `overall == pass` auto-chains the finalize in-process (spec → `completed`, emits + verifies `pipeline.complete`, syncs `meta.json`) — the LLM does not call `complete-spec`. `fail` is report-only: fix the gate, re-run. No follow-up window, no filesystem move — follow-up work is a separate linked sub-spec.
4. Output: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══`

### Replan Protocol

When an agent failed structurally, retries are exhausted, the user reports unexpected behavior, or review rejected with an architectural concern: update spec → summarize failure → Explore → rewrite tasks → re-approve → resume EXECUTE.

## Role Rules

> `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Role Rules` — boundaries and validation per role.

## Pipeline Bugfix

- Fast path (clear cause, 1-2 files): ANALYZE → FIX → VALIDATE → CLOSE. No spec.
- Full path (3+ files or unclear impact): ANALYZE → PLAN → APPROVE → FIX → VALIDATE → CLOSE.
- Decision: Explore returns a clear root cause in 1-2 files → fast; otherwise full.
