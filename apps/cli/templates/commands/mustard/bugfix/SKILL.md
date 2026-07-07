---
name: mustard-bugfix
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Autonomous diagnose + fix pipeline for an error, bug, or broken behavior — zero context-switch. Weak fallback only: use when the router did not engage and the user reports an error, bug, or broken behavior.
source: manual
---
<!-- mustard:generated -->
# /bugfix - Bug Fix Pipeline

**Iron law: NO fix before the cause is located and reproduced.**

`/bugfix <error-description>` — search for newest docs before any change.

## Rationalizations that don't fly

| Excuse | Answer |
|--------|--------|
| "the fix is obvious, skip DIAGNOSE" | an obvious fix at the wrong layer breeds the next bug; locate the root cause first |
| "I can't reproduce it, but the change looks right" | without a reproduction there is no proof — the AC must exit non-zero before the fix and 0 after |
| "it grew to 4 files but it's still just a fix" | 3+ files / unclear impact → Full Path (brief spec); a contract/UX change or wide sweep → PROMOTE to `/feature` |
| "grep found nothing, so I'll rewrite the area" | a symptom with no quotable token is concept-only → digest; a `miss` is not "absent" |
| "the code reads fine, the user must be wrong" | a runtime symptom cannot be refuted by static reading — reproduce before disputing |

**Red flags** — catch yourself thinking any of these and return to the flow: *"Let me patch it and see if the user confirms."* · *"The reproduction can come after the merge."* · *"I'm fixing where the symptom appears, not where it originates."* · *"Third retry of the same patch with no new hypothesis."*

## Procedure

### 1. Hygiene + ANALYZE

→ `../../../refs/feature/spec-hygiene.md`. (No stage emit here — there is no spec yet; `spec-draft` backfills the `ANALYZE` marker when the slug is born.) Ensure `mustard-rt run scan` has produced `.claude/grain.model.json`.

**Triage the symptom FIRST — pick the locating tool by what the symptom hands you (→ `../../../refs/locating-code.md`).** A bug almost always carries a LITERAL anchor: the error / exception message, a field or type name, a `file:line` from a stack trace, an HTTP status, a log line. When it does → **`grep`/`glob` that token directly** (exact, instant, complete — the rule's literal branch), then go straight to DIAGNOSE and do NOT run the digest: a concept query over a literal symptom returns a concept *neighbourhood* (it comes back *amplo demais*) — never pay for retrieval the symptom already pinpointed. Use the digest below ONLY when the symptom is CONCEPT-only — a behaviour with no quotable token ("import broken", "total wrong", "slow") where the code's vocabulary may diverge from the report.

**(Concept-only path) Research with `mustard-rt run feature --intent "<lapidated code-shaped terms + the user's content words>"`** (the scan digest — locate first, then read). **Lapidate the bug into code-shaped terms yourself**: strip the glue (content words only), translate into the code's vocabulary, shape it how code NAMES things — verbs infinitive (`create`/`fix`), collection nouns plural (`receivables`/`titles`) — so terms hit the **EXACT** tier, not `stem` (where the noise lives). ONE call, **pure deterministic** (no model call), matching the **distinct union**. Then **prune by provenance** (`anchorsDetail` shows each anchor's matched terms — drop the tangential, keep the central) and read only the survivors. On a `weak`/`none` result the digest returns a `candidates` array (the repo's real vocabulary) — sharpen your translation and re-call, or fall back to direct Glob+Grep. Each query feeds `lexicon-suggest`, so a confirmed bridge becomes deterministic over time.

The flow goes straight from the pruned digest anchors to DIAGNOSE — no validation layer in between. When the digest's `concerns` show ≥2 (multiple distinct symptoms), DIAGNOSE + fix each concern separately, scoped to its OWN anchors, instead of tangling them.

**DIAGNOSE.** Dispatch Explore (`≤20 tool uses, ≤3 full file reads`) with the `diagnose` skill, prompt rendered via `agent-prompt-render --role explore --task-text ... --emit ref` (spec-less — the compiled explore contract rides along; pass the 2-line stub stdout verbatim as the Task prompt, the PreToolUse hook expands it). Scoped Greps for the symptom; trace callers/callees; return root cause + 1-line explanation.

**Root-cause cache** (in-memory): `rootCauseHash = sha256(bugDescription + '|' + affectedFiles)` + `affectedFilesHash = sha256(contents)`. Reused on Structural retry when hash matches + failure rationale stays inside `affectedFiles`.

### 2. ASSESS

1-2 files, clear root cause → **Fast Path** (skip PLAN). 3+ files, unclear impact, cross-layer → **Full Path** (brief spec).

**PROMOTE to `/feature` (Full scope)** when the fix stops being a bug fix and becomes feature work: a wide cross-cutting rename, an API/contract change, a UX change, or a sweep across many files / multiple subprojects. The lean bugfix mould is the wrong shape for that — STOP and re-enter via `/mustard:feature` instead of forcing a refactor through a bugfix spec. This can fire mid-pipeline: if DIAGNOSE or EXECUTE reveals the true scope only then, hand off to `/feature` at that moment (the spec's `change-log.md` already records what surfaced).

### 3. Full Path Spec

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → ask once → persist to `meta.json`). Lean — `## Contexto` + `## Acceptance Criteria` = PRD layer; `## Causa raiz` + `## Plano` + `## Limites` = Plano layer. NO divider headings, NO PRD subsections. MUST include ≥1 AC: reproduction command (exits non-zero before fix, exit 0 after). → `../../../refs/feature/spec-language.md`.

Once the spec exists and has a slug, run `mustard-rt run digest-adherence-finalize --spec {slug}`. Fire-and-forget telemetry: it folds the session's events into one `analyze.digest.summary` attributed to the spec; it never blocks — continue immediately. The Fast Path has no spec, so it never emits this summary.

Print spec verbatim, then *"Run `/mustard:spec` to approve and proceed to EXECUTE."*

### 4. EXECUTE

All agent prompts via `mustard-rt run agent-prompt-render --emit ref` (NEVER hand-craft; the 2-line stub stdout IS the Task prompt — the PreToolUse hook expands it; the subagent's context is the spec section + anchors). Dispatch each with its role's `subagent_type` (`impl`→`general-purpose`, `review`→`mustard-review`); the DIAGNOSE Explore already runs read-only. `role=ui` → append `Read .claude/refs/stack-templates/browser-debug.md before instrumenting.` to `{context_extras}`.

Validate: build/type-check passes, no regression (max 3 iterations).

### 5. Failure Routing + Escalation

**Transient** → retry once. **Resolvable** (≤3-line patch, no new reads) → patch + retry (counts as 1). **Structural** → check cache; hash matches AND failure doesn't point elsewhere → reuse cached summary; else re-Explore (does NOT count against 2-retry cap). Escalations: `CONCERN` → `## Concerns`; `BLOCKED` → STOP + AskUserQuestion; `PARTIAL` → granular retry (max 2); `DEFERRED` → note + confirm. → `.claude/pipeline-config.md § Escalation Statuses`.

### 6. QA + CLOSE

`pipeline.stage: QaReview` → `qa-run`. pass → CLOSE; fail → return failing AC (max 3 QA iterations). Then `mustard-rt run scan` if the codebase changed materially (refresh the model).

## INVIOLABLE RULES

- NEVER ask "can you show?" / "which file?" / "how to fix?" — find, trace, propose + implement.
- NEVER hand-craft an agent prompt — always `agent-prompt-render`.
- Fast Path Explore capped: ≤10 tool uses, escalate to Full Path on >5 files.
