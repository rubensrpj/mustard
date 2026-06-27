---
name: mustard-task
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Lean delegated code task (analyze, audit, compare, review, docs, refactor, implement) via separate Task contexts (L0 Universal Delegation). Weak fallback only: use when the router did not engage and the user asks for a delegated code task.
source: manual
---
<!-- mustard:generated -->
# /task - Delegated Task Execution

## Trigger

`/task <action> <scope>`

| Action | Agent | Description |
|--------|-------|-------------|
| `analyze` | Explore | Code exploration / pattern analysis |
| `audit` | general-purpose | Quality audit with domain checklist |
| `compare` | parallel explorers → Plan | Cross-subproject alignment |
| `review` | mustard-review | SOLID / security / perf |
| `docs` | general-purpose | Documentation generation |
| `refactor` | Plan → general-purpose | Plan + approve + implement |
| `implement` | general-purpose | Single-dispatch with inline slices |

## L0 Enforcement

Parent NEVER reads source, NEVER implements. All work inside Task contexts. The agent prompt is **always** produced by `mustard-rt run agent-prompt-render` — NEVER hand-assembled (same inviolable rule as `/feature` and `/tactical-fix`). The subproject `## Guards` ride in as `{guards_summary}`; the relevance-sliced domain glossary (the subproject `CLAUDE.md`, plus a `CONTEXT.md` when one exists) rides in as `{context_md}` — both filled by the renderer, never hand-Grepped into the prompt string.

## Research + Prompt rendering (mandatory)

`/task` is spec-less, so there is no wave plan and no `dispatch-plan` — but spec-less is **not** context-less. LOCATE first — **triage by what the scope hands you (→ `../../../refs/locating-code.md`): a LITERAL token (exact symbol, error string, file glob) → `grep`/`glob` it directly and skip the digest (and its Sonnet validator); a CONCEPT whose vocabulary may diverge → the scan digest** (the same step `/feature` and `/bugfix` run). Then render each action's prompt with `agent-prompt-render`. **Dispatching without locating sends the agent in blind — the single most common reason a `/task` returns nothing useful.** Render fail-opens on every empty placeholder, so a spec-less invocation is safe.

```bash
# 1. LOCATE via the scan digest — NEVER dispatch blind. Returns anchors (~12 real files),
#    reason (strong|weak|none|generated_only), stacks. LAPIDATE the request into code-shaped terms
#    YOURSELF: strip the glue (prepositions/articles — content words only), translate into the code's
#    vocabulary, and shape it how code NAMES things — verbs infinitive (create/list/update), collection
#    nouns plural (clients/contracts/receivables); when unsure, include both forms (the digest dedups).
#    Code-shaped terms hit EXACT, not stem (where the noise lives). ONE call, pure deterministic.
mustard-rt run feature --intent "<lapidated code-shaped terms + the user's content words>"
#    Prune, then read ONLY the surviving anchors: anchorsDetail shows each anchor's matched terms —
#    drop the tangential (a seeder on `pagos`), keep the central (route/form/datatable). On weak/none
#    the digest returns a `candidates` array (the REAL code vocabulary) — sharpen your translation and
#    re-call, or fall back to Glob+Grep. Each query feeds lexicon-suggest (bridge → deterministic).

# 2. Render the dispatch prompt — fold the located anchor paths into --task-text so the agent
#    starts from them instead of searching the repo from zero.
mustard-rt run agent-prompt-render --spec {scope} --role {action} \
  --subproject {subproject} \
  --task-text "<the action's task> — start from these anchors: <anchor file list>" \
  --mode first --emit ref
```

Pass the `agent-prompt-render` **stdout verbatim** as the Task `prompt` — with `--emit ref` that stdout is a 2-line stub the PreToolUse hook expands to the full prompt at dispatch, so the full text never transits your context. `{guards_summary}` (subproject `## Guards`) and `{reference_files}` are filled by the renderer — do not duplicate them in the prompt. Spec-less, so the action's work + the located anchors ride in via `--task-text`.

**Validate the digest FIRST (AI step — digest path only; SKIP when you located a literal via `grep`).** Right after step 1, run the shared digest-validator (**`../../../refs/digest-validate.md`**): `mustard-rt run digest-validate-render --intent "<the user's request>" --emit ref` → with `--emit ref` it writes the full ~9.5 KB prompt to a `.dispatch/` file and prints a 2-line stub (`MUSTARD-PROMPT-REF: ...`); pass that stub stdout VERBATIM as the Task prompt to `model: sonnet` (the PreToolUse hook expands it at dispatch — NEVER read the `.dispatch/` file in the parent; same mechanism as the scan Guards step) → `{route, scope, dropped, concerns, centralFound, requeryBridges}`. This is the lean retrieval-quality guard (no route/scope ceremony on `/task` — you are already on the lean path); act on these only:
- **`centralFound=false` → RE-QUERY FIRST, then PERSIST on confirmation**, before dispatching: the central concept missed, so the anchors point at the WRONG flow (a `strong` reason is not trustworthy). Re-run `mustard-rt run feature --intent "<all codeTerms across requeryBridges, joined>"` and dispatch on ITS anchors. **If that re-query's `report.reason` is `strong`, the bridge is confirmed** → write `requeryBridges` (`{userWord, codeTerms}` shape) to a temp file and `mustard-rt run lexicon-enrich --apply <file>` so the next identical query is deterministic — no LLM (one-line `auto-bridged: …` note). Do NOT persist when the re-query is not `strong`. (`centralFound=true` or absent → proceed.)
- **`dropped`** → drop those anchors (incidental / far-layer lexical matches), never read them.
- **`concerns` (≥2)** → render + dispatch ONE action per concern, each scoped to its OWN anchors, instead of one mixed dispatch.
Empty render / validator down → fall through to the flat pruned anchors. Pass the user's actual request as `--intent` (never a bare term list — see the INTENT-hygiene rule there).

## Flow

Each action picks `--role` + `subagent_type`, renders via `agent-prompt-render`, then dispatches (agents inherit the session model — no model selection):

- **analyze** — `--role explore`, `subagent_type: Explore` → report.
- **review** — `--role review`, `subagent_type: mustard-review` (read-only) → report.
- **docs** — `--role docs`, `subagent_type: general-purpose` → report.
- **audit** — load `improve-codebase-architecture` → `--role audit`, `subagent_type: general-purpose`; pass the domain checklist as the task via `--task-text "<checklist>"` (the renderer folds it into `## TASK` — no hand-appending) → severity-classified report.
- **compare** — one explorer per subproject in PARALLEL (single message), each rendered with its own `--subproject` (`--role explore`) → Task(Plan) merges + surfaces discrepancies.
- **refactor** — load `improve-codebase-architecture` → render `--role plan` (Plan) → print plan verbatim → AskUserQuestion (Approve/Adjust/Cancel) → render `--role implement` (general-purpose) → validate.
- **implement** — render `--role implement` (general-purpose), return cap 30 lines → agent runs build/type-check. ON CONCERN → surface + offer `/feature` Light.

→ See `../../../refs/task/task-prompts.md` for the per-action render invocations.

Persistent tracking is **N/A** — `/task` is spec-less by design. Promote to `/feature` Light or `/tactical-fix` if a tracked spec is needed.

## Dispatch resilience

A Task dispatch can fail with a **transient infra error** (`Tool result missing due to internal error`) — that is the Agent tool, NOT a located-files problem. When the digest came back `strong`, the anchors are ALREADY located, so a failed dispatch must **never strand the run**:

1. **Retry the dispatch ONCE** (same rendered prompt).
2. If it persists, **proceed from the located anchors** instead of re-routing from zero:
   - read-only action (`analyze`/`review`/`audit`) → read the handful of anchor files directly and report (reading ≤ a few located files in the parent is allowed — L0 forbids *implementing* in the parent, not reading to answer);
   - mutating action → dispatch `implement` straight away, folding the anchor paths into `--task-text` (the next dispatch is independent of the failed one).
3. On a `strong` digest the `analyze`/Explore **mapping pass is often redundant** for concentrated work (a few files, known pattern) — skip it and go straight to `implement` with anchors in `--task-text`. Keep the Explore pass only when the action genuinely needs to MAP an unfamiliar region to graft from (e.g. a `compare`/reuse task where the source pattern must be understood before grafting).

## Domain Checklists (audit)

`copy` (tone/grammar/placeholders/CTA), `design` (tokens/reuse/hierarchy/parity), `a11y` (ARIA/contrast/keyboard/focus), `i18n` (missing keys/hardcoded/plurals), `consistency` (naming/structure/adherence), `api-contract` (DTOs/status codes/errors/versioning). Default when ambiguous: `consistency`.

## Analysis → Action

After `audit`/`compare`: parse severity, map each CRITICAL/WARNING to `/task refactor` or Pipeline, present structured list with estimated scope. Do NOT auto-execute — user picks.

`implement` → 1-3 files, known pattern, build-verifiable (low cost). `/feature` Light → spec + review gate (medium cost). `refactor` → reorganization without functional change.

## Lexicon feedback (end of run)

`/task` has no close, so feed the self-learning dictionary HERE — especially when the digest came back `weak`/`none` and you located the files by **other means** (Glob/Grep). Pure data + gated; fail-open (no `pt-en` pair / no candidates → skip).

```bash
mustard-rt run lexicon-suggest   # `candidates` (re-query bridges) + `locationCandidates` (found OUTSIDE the digest)
```

For each `candidates` `{missed, bridged}` accept the confirmed bridge: `--accept {missed}={bridged}`. For each `locationCandidates` `{missed, files}` open the file, pick the code term, and `--accept {missed}={codeTerm}` — only when the mapping is clear (a wrong bridge poisons future queries). Gated (the code term must be a real mined term), idempotent. This makes the next `/task`, `/feature` or `/bugfix` find it deterministically, no LLM.
