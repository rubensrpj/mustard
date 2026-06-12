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

→ `../../../refs/feature/spec-hygiene.md`. (No stage emit here — there is no spec yet; `spec-draft` backfills the `ANALYZE` marker when the slug is born.) Ensure `mustard-rt run scan` has produced `.claude/grain.model.json`; research with `mustard-rt run feature --intent "<bug>"` (the scan digest — no source-reading) and read only its anchors.

**DIAGNOSE.** Dispatch Explore (`≤20 tool uses, ≤3 full file reads`) with the `diagnose` skill. Scoped Greps for the symptom; trace callers/callees; return root cause + 1-line explanation.

**Root-cause cache** (in-memory): `rootCauseHash = sha256(bugDescription + '|' + affectedFiles)` + `affectedFilesHash = sha256(contents)`. Reused on Structural retry when hash matches + failure rationale stays inside `affectedFiles`.

### 2. ASSESS

1-2 files, clear root cause → **Fast Path** (skip PLAN). 3+ files, unclear impact, cross-layer → **Full Path** (brief spec).

**PROMOTE to `/feature` (Full scope)** when the fix stops being a bug fix and becomes feature work: a wide cross-cutting rename, an API/contract change, a UX change, or a sweep across many files / multiple subprojects. The lean bugfix mould is the wrong shape for that — STOP and re-enter via `/mustard:feature` instead of forcing a refactor through a bugfix spec. This can fire mid-pipeline: if DIAGNOSE or EXECUTE reveals the true scope only then, hand off to `/feature` at that moment (the spec's `change-log.md` already records what surfaced).

### 3. Full Path Spec

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → ask once → persist to `meta.json`). Lean — `## Contexto` + `## Acceptance Criteria` = PRD layer; `## Causa raiz` + `## Plano` + `## Limites` = Plano layer. NO divider headings, NO PRD subsections. MUST include ≥1 AC: reproduction command (exits non-zero before fix, exit 0 after). → `../../../refs/feature/spec-language.md`.

Once the spec exists and has a slug, run `mustard-rt run digest-adherence-finalize --spec {slug}`. Fire-and-forget telemetry: it folds the session's events into one `analyze.digest.summary` attributed to the spec; it never blocks — continue immediately. The Fast Path has no spec, so it never emits this summary.

Print spec verbatim, then *"Run `/mustard:spec` to approve and proceed to EXECUTE."*

### 4. EXECUTE

All agent prompts via `mustard-rt run agent-prompt-render` (NEVER hand-craft; the subagent's context is the spec section + anchors). Dispatch each with its role's `subagent_type` (`impl`→`general-purpose`, `review`→`mustard-review`); the DIAGNOSE Explore already runs read-only. `role=ui` → append `Read .claude/refs/stack-templates/browser-debug.md before instrumenting.` to `{context_extras}`.

Validate: build/type-check passes, no regression (max 3 iterations).

### 5. Failure Routing + Escalation

**Transient** → retry once. **Resolvable** (≤3-line patch, no new reads) → patch + retry (counts as 1). **Structural** → check cache; hash matches AND failure doesn't point elsewhere → reuse cached summary; else re-Explore (does NOT count against 2-retry cap). Escalations: `CONCERN` → `## Concerns`; `BLOCKED` → STOP + AskUserQuestion; `PARTIAL` → granular retry (max 2); `DEFERRED` → note + confirm. → `.claude/pipeline-config.md § Escalation Statuses`.

### 6. QA + CLOSE

`pipeline.stage: QaReview` → `qa-run`. pass → CLOSE; fail → return failing AC (max 3 QA iterations). Then `mustard-rt run scan` if the codebase changed materially (refresh the model).

## INVIOLABLE RULES

- NEVER ask "can you show?" / "which file?" / "how to fix?" — find, trace, propose + implement.
- NEVER hand-craft an agent prompt — always `agent-prompt-render`.
- Fast Path Explore capped: ≤10 tool uses, escalate to Full Path on >5 files.
