---
name: mustard-feature
description: Use when the user runs /feature or asks to add, create, implement, or enhance a new feature. Starts the feature pipeline (ANALYZE → PLAN → optional inline EXECUTE for Light scope).
source: manual
---
<!-- mustard:generated -->
# /feature - Feature Pipeline

`/feature <feature-name>` — search for newest docs before any change. Heavy lifting delegates to `mustard-rt`; orchestrator routes phases + emits events.

## Action

### 1. Hygiene + ANALYZE

→ `../../../refs/feature/spec-hygiene.md`. Emit `pipeline.stage: Analyze`. Run `sync-detect` (and `sync-registry` if `hashChanged: true`). Grep `entity-registry.json` per entity (never read the full JSON). Read `.claude/pipeline-config.md` once.

Scope: light (1-2 layers, ≤5 files, known pattern) | extended-light (entity in registry + modifies existing + ≤8 files) | full (3+ layers, new entity/pattern). MAX 5 reads in ANALYZE. Skip Explore when entity is in registry.

End: `rtk mustard-rt run analyze-validation --spec .claude/spec/{spec}/spec.md` — append `issues[]` to `## Concerns` on `ok: false` (non-blocking).

### 2. PLAN

→ `../../../refs/feature/spec-language.md` (header translation, narrative rules, Component Contract). → `../../../refs/feature/wave-decomposition.md`.

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → AskUserQuestion once → persist to `meta.json`). **Concern Coverage Audit**: every concrete user critique from the conversation must map to covered by wave/task | non-goal justified | surfaced for decision. Orphaned items block the AskUserQuestion. Full scope: wave decomposition when `file_count ≥ 6 OR layer_count ≥ 3 OR independent_subbehaviors ≥ 3` — `mustard-rt run wave-scaffold --spec-dir <dir> --plan <plan.json>`.

Write `.claude/spec/{date}-{name}/spec.md` two-layer: `## PRD` → `## Contexto`, `## Usuários/Stakeholders`, `## Métrica de sucesso`, `## Não-Objetivos`, `## Critérios de Aceitação`; `## Plano` → `## Informações da Entidade`, `## Arquivos`, optional `## Component Contract` (UI only), `## Tarefas`, `## Dependências`, `## Limites`.

Emit `pipeline.scope` + `pipeline.stage: Plan`. Print spec verbatim + `wave-tree`. AskUserQuestion: **"Approve and implement?"** / **"Adjust (give feedback)"** / **"Save for later (stop)"**.

### 3. Light/Extended-Light EXECUTE (inline)

User chooses "Approve and implement now": emit `pipeline.stage: Execute` → `exec-rewave-check` (decomposed → use wave-1 spec) → `dependency-precheck` (block on missing externals) → dispatch agents via `agent-prompt-render` (NEVER hand-craft; all agents of a wave → one message) → per-wave validate + `memory agent` + `write-back --kind injected` → REVIEW per subproject (sonnet for re-reviews, `review-result` emit, max 2 fix loops) → QA (`qa-run`; pass → CLOSE; fail → return failing AC; skip → warn + allow CLOSE; max 3 QA iterations).

Escalations: `CONCERN` → `## Concerns`, continue. `BLOCKED` → STOP + AskUserQuestion. `PARTIAL` → granular retry (max 2). `DEFERRED` → note + confirm. → `../../../refs/feature/ac-cross-shell.md`.

## INVIOLABLE RULES

- NEVER read more files after Explore returns. MAX 5 reads in ANALYZE.
- NEVER skip `analyze-validation` or `dependency-precheck`.
- NEVER hand-craft agent prompts — always `agent-prompt-render`.
- ALWAYS Grep `entity-registry.json` per entity, never read the full JSON.
- ALWAYS emit `pipeline.scope` + `pipeline.stage` at each transition.
