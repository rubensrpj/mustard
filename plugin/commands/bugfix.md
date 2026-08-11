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

**Runnable evidence goes in `.claude/scratch/`.** When two hypotheses are settled faster by RUNNING something than by arguing it — a shell probe, a data fixture, a `mustard-rt run …` call — write it there and run it. That prefix is carved out of branch protection (`work_branch_gate`), so it is writable on a bare integration base BEFORE any unit exists: the write is allowed, no branch is cut, and a pending marker survives for the first real edit. The seeded `.claude/.gitignore` ignores `scratch/`, so scratch never reaches a diff and never joins the unit. **Its limit:** cargo does not compile files under `.claude/`, so evidence that must COMPILE inside a crate cannot live there — no carve-out can make a throwaway Rust integration test work in scratch. For that case open the unit early and write the throwaway inside it; §3 carries the diagnosis into the spec either way, so opening early costs nothing.

**Root-cause cache** (in-memory): `sha256(bugDescription|affectedFiles)` + a content hash; reused on a Structural retry when the hash matches and the failure stays inside `affectedFiles`.

## 2. ASSESS

1-2 files, clear root cause → **Fast Path** (skip PLAN; canonical emitted scope: `lean`). 3+ files, unclear/cross-layer → **Full Path** (brief spec; canonical emitted scope: `full`). **PROMOTE to `/feature`** when the fix becomes feature work — a wide rename, an API/contract change, a UX change, a sweep across subprojects. This can fire mid-pipeline: hand off the moment DIAGNOSE/EXECUTE reveals the true scope (the `change-log.md` records what surfaced).

## 3. Full Path spec

**Assemble the material FIRST — then draft. DIAGNOSE's output is an INPUT to the spec, never something retyped into it afterwards:** what the hand does not retype is simply lost, and the root cause is exactly what gets lost. **Order, said out loud: the base gate — and therefore the unit's `{base}_{slug}` branch — comes BEFORE this write.** `.claude/.cache/` is NOT the §1 carve-out (`is_harness_carve_out` covers `.claude/plans/` and `.claude/scratch/`, nothing else), so this is an ordinary write: with the branch already cut it lands normally, and from a bare integration base it is REFUSED and the flow dead-ends here with nothing materialised. Write everything §1 established into one JSON file, `.claude/.cache/spec-material.json` — `definitions` (a term this conversation settled + what it means HERE) · `decisions` (a choice + the REASON it was taken) · `findings` (a verified statement + the `file` and `line` it was checked at — the located root cause lands here, and so does a hypothesis the diagnosis REFUTED). Then pass it:

`mustard-rt run spec-draft --intent "<symptom>" --slug <the unit name the base gate reported> --scope full --lang <bcp47> --material .claude/.cache/spec-material.json`

Exact schema, the per-kind refusals and the FAIL-CLOSED contract (an unknown key or a half-entry aborts the draft rather than degrading to an empty channel): `/feature` §2.2 — this flow uses the same channel, it simply never used to. Nothing established → omit `--material` and the draft is byte-identical to one written before the channel existed. The three sections it writes (`## Definitions` / `## Decisions` / `## Evidence`) are written by the drafter and never by hand.

**Weight follows the diagnosis.** A root cause already DEMONSTRATED — the finding rides in with its `file:line` — drafts the MINIMAL spec: `## Contexto` + `## Acceptance Criteria` + `## Limites`, and nothing else. The discovery sections are dropped because they no longer have work to do: `## Causa raiz` would restate what `## Evidence` already carries with its file and line, and `## Plano` would narrate a fix the tasks already name. A cause still ARGUED (competing hypotheses, cross-layer trace) keeps both. Resolve Lang via cascade (`meta.json#lang` → `mustard.json#specLang` → ask once → persist). Lean either way, per `${CLAUDE_PLUGIN_ROOT}/refs/feature/spec-language.md`; PRD layer = `## Contexto` + `## Acceptance Criteria`, Plano layer = `## Causa raiz` + `## Plano` + `## Limites`. No divider/PRD-subsection headings. MUST include ≥1 AC: a reproduction command that exits non-zero before the fix, 0 after.

Once the slug exists, run `mustard-rt run digest-adherence-finalize --spec {slug}` (fire-and-forget telemetry; never blocks). Print the spec, then *"Run `/mustard:spec` to approve and proceed to EXECUTE."*

## 4. EXECUTE

All prompts via `agent-prompt-render --emit ref` — never hand-craft; stub mechanics: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`. Dispatch each with its role's `subagent_type` (`impl`→`general-purpose`, `review`→`mustard:mustard-review`; the DIAGNOSE Explore already ran read-only). Browser/UI-layer bug → append to the render's `--task-text`: `First Read ${CLAUDE_PLUGIN_ROOT}/refs/stack-templates/browser-debug.md and follow its instrumentation protocol.` Validate: build/type-check passes, no regression (max 3 iterations).

## 5. Failure routing

**Transient** → retry once. **Resolvable** (≤3-line patch, no new reads) → patch + retry (counts as 1). **Structural** → check the cache; hash matches AND failure doesn't point elsewhere → reuse the cached summary, else re-Explore (does NOT count against the 2-retry cap). Escalation statuses (`CONCERN`/`BLOCKED`/`PARTIAL`/`DEFERRED`) → `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Escalation Statuses`.

## 6. QA + CLOSE

`pipeline.stage: QaReview` → `qa-run`. Pass → CLOSE; fail → return failing AC (max 3 QA iterations). Then `mustard-rt run scan` if the codebase changed materially.

## Inviolable

- NEVER hand-craft an agent prompt — always `agent-prompt-render`.
- Fast Path Explore capped ≤10 tool uses; escalate to Full Path on >5 files.
