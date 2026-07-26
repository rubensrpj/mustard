---
description: An internal flow — dispatched by the orchestrator router (CLAUDE.md § Intent Routing), not chosen directly by the user. Lean delegated code task (analyze, audit, compare, review, docs, refactor, implement) via separate Task contexts (L0 Universal Delegation). Weak fallback only: use when the router did not engage and the user asks for a delegated code task.
user-invocable: false
source: manual
---
<!-- mustard:generated -->
# /task — Delegated Task Execution

**Iron law: ONE layer only — the moment it grows to two, it is a `/feature`.** Spec-less by design; promote to `/feature` Light or `/tactical-fix` if tracking is needed. The parent NEVER reads source and NEVER implements — all work runs inside Task contexts (L0).

`/task <action> <scope>`

| Action | `--role` | `subagent_type` |
|--------|----------|-----------------|
| `analyze` | `explore` | `Explore` (read-only) |
| `audit` | `explore` | `Explore` (read-only) |
| `compare` | `explore` ×N → `plan` | `Explore` parallel → `Plan` |
| `review` | `review` | `mustard:mustard-review` (read-only) |
| `docs` | `impl` | `general-purpose` |
| `refactor` | `plan` → `impl` | `Plan` → `general-purpose` |
| `implement` | `impl` | `general-purpose` |

Roles are the render's canonical vocabulary (`explore`, `plan`, `impl`, `review`) — an unknown role falls through to the impl contract, so never pass the action name as `--role`.

## LOCATE → render → dispatch

Spec-less is not context-less. **Locate first** (`${CLAUDE_PLUGIN_ROOT}/refs/locating-code.md` owns how to triage, shape the query, and read anchors): a LITERAL token → `grep`/`glob`; a CONCEPT → the digest `mustard-rt run feature --intent "…"`, then READ the anchors it points to. Dispatching blind is the top cause of an empty `/task`.

The agent prompt is **always** produced by `agent-prompt-render` — NEVER hand-assembled. `{guards_summary}` (subproject `## Guards`) and `{context_md}` (relevance-sliced glossary) are filled by the renderer. Render each action, folding the anchors into `--task-text` so the agent starts from them:

```bash
mustard-rt run agent-prompt-render --role {role} \
  --subproject {subproject} \
  --task-text "<the action's work> — start from these anchors: <paths>" \
  --mode first --emit ref
```

Pass the stdout **verbatim** as the Task `prompt` — never hand-assemble, never read the `.dispatch/` file; stub mechanics: `${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md`. Swap `--mode granular|fix-loop` on a retry. When the digest's `concerns` show ≥2, render + dispatch ONE action per concern, each scoped to its own anchors.

**Per-action:** `audit` folds its checklist (§ Audit checklists) into `--task-text`. `refactor` is two-phase — render `plan`, print verbatim, AskUserQuestion (Approve/Adjust/Cancel), then render `impl`. `compare` dispatches one `explore` per subproject in one parallel message → `Plan` merges. `implement` returns ≤30 lines + runs build/type-check; ON CONCERN → offer `/feature` Light.

## Dispatch resilience

A dispatch can fail with a transient infra error (`Tool result missing…`) — the Agent tool, not a locate problem. Retry once; if it persists, proceed from the anchors: read-only actions read them directly and report, mutating actions dispatch `implement` from them. On a `strong` digest, skip the Explore mapping pass and go straight to `implement`.

## Audit checklists

`copy` · `design` · `a11y` · `i18n` · `consistency` · `api-contract`. Default `consistency`. After `audit`/`compare`: map each CRITICAL/WARNING to `/task refactor` or a pipeline; present the list, user picks — never auto-execute.

## Lexicon feedback (end of run)

`/task` has no close, so persist a confirmed vocabulary bridge HERE — especially when you located by other means after a `weak`/`none` digest:

```bash
mustard-rt run equivalence-learn --term <missed-concept> --tokens <code-terms>
```

Only when the mapping is clear (you opened the file and the code term names the concept) — a wrong bridge poisons future queries. Writes the learned overlay that re-scans never wipe.
