---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Autonomous diagnose + fix pipeline for an error, bug, or broken behavior — zero context-switch. Weak fallback only: use when the router did not engage and the user reports an error, bug, or broken behavior.
user-invocable: false
source: manual
---
<!-- mustard:generated -->
# /bugfix — Bug Fix Pipeline

**Iron law: NO fix before the cause is located and reproduced.** `/bugfix <error-description>` — search for newest docs before any change. NEVER ask "which file?" / "how to fix?" — find, trace, propose, implement.

## 1. Hygiene + ANALYZE

Run `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-hygiene.md`; ensure `mustard-rt run scan` has produced `.claude/grain.model.json`. (No stage emit yet — `spec-draft` backfills the `ANALYZE` marker when the slug is born.)

**Locate by what the symptom hands you** (`${CLAUDE_PLUGIN_ROOT}/refs/locating-code.md` owns triage / query-shaping / reading anchors): a LITERAL anchor (error message, symbol, `file:line`, log line) → `grep`/`glob` it directly, straight to DIAGNOSE; a CONCEPT-only symptom (no quotable token) → the digest `mustard-rt run feature --intent "…"`, then READ its anchors.

**DIAGNOSE.** Dispatch Explore (`≤15 tool uses (warn 12), ≤3 full reads`), prompt rendered via `agent-prompt-render --role explore --task-text … --emit ref` (spec-less; pass the stub verbatim). Scoped Greps for the symptom; trace callers/callees; return root cause + 1-line explanation. When ≥2 distinct symptoms surface, DIAGNOSE + fix each separately, scoped to its own anchors.

**Root-cause cache** (in-memory): `sha256(bugDescription|affectedFiles)` + a content hash; reused on a Structural retry when the hash matches and the failure stays inside `affectedFiles`.

## 2. ASSESS

1-2 files, clear root cause → **Fast Path** (skip PLAN; canonical emitted scope: `lean`). 3+ files, unclear/cross-layer → **Full Path** (brief spec; canonical emitted scope: `full`). **PROMOTE to `/feature`** when the fix becomes feature work — a wide rename, an API/contract change, a UX change, a sweep across subprojects. This can fire mid-pipeline: hand off the moment DIAGNOSE/EXECUTE reveals the true scope (the `change-log.md` records what surfaced).

## 3. Full Path spec

Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → ask once → persist). Lean, per `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`: `## Contexto` + `## Acceptance Criteria` (PRD layer); `## Causa raiz` + `## Plano` + `## Limites` (Plano layer). No divider/PRD-subsection headings. MUST include ≥1 AC: a reproduction command that exits non-zero before the fix, 0 after.

Once the slug exists, run `mustard-rt run digest-adherence-finalize --spec {slug}` (fire-and-forget telemetry; never blocks). Print the spec, then *"Run `/mustard:spec` to approve and proceed to EXECUTE."*

## 4. EXECUTE

All prompts via `agent-prompt-render --emit ref` — never hand-craft; stub mechanics: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`. Dispatch each with its role's `subagent_type` (`impl`→`general-purpose`, `review`→`mustard-review`; the DIAGNOSE Explore already ran read-only). Browser/UI-layer bug → append to the render's `--task-text`: `First Read ${CLAUDE_PLUGIN_ROOT}/refs/stack-templates/browser-debug.md and follow its instrumentation protocol.` Validate: build/type-check passes, no regression (max 3 iterations).

## 5. Failure routing

**Transient** → retry once. **Resolvable** (≤3-line patch, no new reads) → patch + retry (counts as 1). **Structural** → check the cache; hash matches AND failure doesn't point elsewhere → reuse the cached summary, else re-Explore (does NOT count against the 2-retry cap). Escalation statuses (`CONCERN`/`BLOCKED`/`PARTIAL`/`DEFERRED`) → `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Escalation Statuses`.

## 6. QA + CLOSE

`pipeline.stage: QaReview` → `qa-run`. Pass → CLOSE; fail → return failing AC (max 3 QA iterations). Then `mustard-rt run scan` if the codebase changed materially.

## Inviolable

- NEVER hand-craft an agent prompt — always `agent-prompt-render`.
- Fast Path Explore capped ≤10 tool uses; escalate to Full Path on >5 files.
