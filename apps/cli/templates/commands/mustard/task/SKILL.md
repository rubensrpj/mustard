---
name: mustard-task
description: Use when the user runs /task or asks for a delegated code task (analyze, audit, compare, review, docs, refactor, implement). Delegates each action via separate Task contexts (L0 Universal Delegation).
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

## Prompt rendering (mandatory)

`/task` is spec-less, so there is no wave plan and no `dispatch-plan`. Render each action's prompt directly with `agent-prompt-render`, choosing `--role` from the action and `--subproject` from the scope. Render fail-opens on every empty placeholder, so a spec-less invocation is safe.

```bash
# 1. Slice the subproject CLAUDE.md for the scope (cached, relevance-filtered — never the whole file).
#    If a domain glossary exists, append: --context {subproject}/CONTEXT.md
mustard-rt run context-slice --spec {scope} \
  --context-claude-md {subproject}/CLAUDE.md

# 2. Render the dispatch prompt (one process call → Task-ready string on stdout).
mustard-rt run agent-prompt-render --spec {scope} --role {action} \
  --subproject {subproject} --task-text "<the action's task>" --mode first [--budget-tokens 4000]
```

Pass the `agent-prompt-render` **stdout verbatim** as the Task `prompt`. `{guards_summary}` (subproject `## Guards`), `{context_md}` (the `context-slice` output above) and `{reference_files}` are filled by the renderer — do not duplicate them in the prompt. Spec-less, so the action's work rides in via `--task-text`.

## Flow

Each action picks `--role` + `subagent_type`, renders via `agent-prompt-render`, then dispatches (agents inherit the session model — no model selection):

- **analyze** — `--role explore`, `subagent_type: Explore` → report.
- **review** — `--role review`, `subagent_type: mustard-review` (read-only) → report.
- **docs** — `--role docs`, `subagent_type: general-purpose` → report.
- **audit** — load `improve-codebase-architecture` → `--role audit`, `subagent_type: general-purpose`; pass the domain checklist as the task via `--task-text "<checklist>"` (the renderer folds it into `## TASK` — no hand-appending) → severity-classified report.
- **compare** — one explorer per subproject in PARALLEL (single message), each rendered with its own `--subproject` (`--role explore`) → Task(Plan) merges + surfaces discrepancies.
- **refactor** — load `improve-codebase-architecture` → render `--role plan` (Plan) → print plan verbatim → AskUserQuestion (Approve/Adjust/Cancel) → render `--role implement` (general-purpose) → validate.
- **implement** — render `--role implement` (general-purpose) with `--budget-tokens 4000`, return cap 30 lines → agent runs build/type-check. ON CONCERN → surface + offer `/feature` Light.

→ See `../../../refs/task/task-prompts.md` for the per-action render invocations.

Persistent tracking is **N/A** — `/task` is spec-less by design. Promote to `/feature` Light or `/tactical-fix` if a tracked spec is needed.

## Domain Checklists (audit)

`copy` (tone/grammar/placeholders/CTA), `design` (tokens/reuse/hierarchy/parity), `a11y` (ARIA/contrast/keyboard/focus), `i18n` (missing keys/hardcoded/plurals), `consistency` (naming/structure/adherence), `api-contract` (DTOs/status codes/errors/versioning). Default when ambiguous: `consistency`.

## Analysis → Action

After `audit`/`compare`: parse severity, map each CRITICAL/WARNING to `/task refactor` or Pipeline, present structured list with estimated scope. Do NOT auto-execute — user picks.

`implement` → 1-3 files, known pattern, build-verifiable (low cost). `/feature` Light → spec + review gate (medium cost). `refactor` → reorganization without functional change.
