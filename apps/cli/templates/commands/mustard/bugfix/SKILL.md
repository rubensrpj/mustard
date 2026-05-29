---
name: mustard-bugfix
description: Use when the user runs /bugfix or reports an error, bug, broken behavior, or asks to fix something. Autonomous diagnose + fix pipeline — zero context-switch.
source: manual
---
<!-- mustard:generated -->
# /bugfix - Bug Fix Pipeline

`/bugfix <error-description>` — search for newest docs before any change.

## Procedure

### 1. Hygiene + ANALYZE

→ `../../../refs/feature/spec-hygiene.md`. Emit `pipeline.stage: Analyze`. Run `sync-detect` (and `sync-registry` if `hashChanged: true`).

**DIAGNOSE.** Dispatch Explore (`≤20 tool uses, ≤3 full file reads`) with the `diagnose` skill. Scoped Greps for the symptom; trace callers/callees; return root cause + 1-line explanation.

**Root-cause cache** (in-memory): `rootCauseHash = sha256(bugDescription + '|' + affectedFiles)` + `affectedFilesHash = sha256(contents)`. Reused on Structural retry when hash matches + failure rationale stays inside `affectedFiles`.

### 2. ASSESS

1-2 files, clear root cause → **Fast Path** (skip PLAN). 3+ files, unclear impact, cross-layer → **Full Path** (brief spec).

### 3. Full Path Spec

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → ask once → persist to `meta.json`). Lean — `## Contexto` + `## Acceptance Criteria` = PRD layer; `## Causa raiz` + `## Plano` + `## Limites` = Plano layer. NO divider headings, NO PRD subsections. MUST include ≥1 AC: reproduction command (exits non-zero before fix, exit 0 after). → `../../../refs/feature/spec-language.md`.

Print spec verbatim, then *"Run `/mustard:spec` to approve and proceed to EXECUTE."*

### 4. EXECUTE

All agent prompts via `mustard-rt run agent-prompt-render` (NEVER hand-craft). Required `{recommended_skills}` start: `karpathy-guidelines, diagnose`. `role=ui` → append `Read .claude/refs/stack-templates/browser-debug.md before instrumenting.` to `{context_extras}`.

Validate: build/type-check passes, no regression (max 3 iterations). Full Path: `mustard-rt run write-back --spec {spec} --kind injected`.

### 5. Failure Routing + Escalation

**Transient** → retry once. **Resolvable** (≤3-line patch, no new reads) → patch + retry (counts as 1). **Structural** → check cache; hash matches AND failure doesn't point elsewhere → reuse cached summary; else re-Explore (does NOT count against 2-retry cap). Escalations: `CONCERN` → `## Concerns`; `BLOCKED` → STOP + AskUserQuestion; `PARTIAL` → granular retry (max 2); `DEFERRED` → note + confirm. → `.claude/pipeline-config.md § Escalation Statuses`.

### 6. QA + CLOSE

`pipeline.stage: QaReview` → `qa-run`. pass → CLOSE; fail → return failing AC (max 3 QA iterations). Then `sync-registry` if entities changed.

## INVIOLABLE RULES

- NEVER ask "can you show?" / "which file?" / "how to fix?" — find, trace, propose + implement.
- NEVER hand-craft an agent prompt — always `agent-prompt-render`.
- Fast Path Explore capped: ≤10 tool uses, escalate to Full Path on >5 files.
