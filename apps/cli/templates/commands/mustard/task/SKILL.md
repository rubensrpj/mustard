---
name: mustard-task
description: Use when the user runs /task or asks for a delegated code task (analyze, audit, compare, review, docs, refactor, implement). Delegates each action via separate Task contexts (L0 Universal Delegation).
source: manual
---
<!-- mustard:generated -->
# /task - Delegated Task Execution

## Trigger

`/task <action> <scope>`

| Action | Agent | Model | Description |
|--------|-------|-------|-------------|
| `analyze` | Explore | sonnet | Code exploration / pattern analysis |
| `audit` | general-purpose | sonnet | Quality audit with domain checklist |
| `compare` | parallel explorers → Plan | sonnet | Cross-subproject alignment |
| `review` | general-purpose | opus | SOLID / security / perf |
| `docs` | general-purpose | sonnet | Documentation generation |
| `refactor` | Plan → general-purpose | sonnet/opus | Plan + approve + implement |
| `implement` | general-purpose | sonnet | Single-dispatch with inline slices |

## L0 Enforcement

Parent NEVER reads source, NEVER implements. All work inside Task contexts. Orchestrator MAY Grep `.md` config files (`guards.md`, `patterns.md`) to inject standardization slices.

## Flow

- **analyze / review / docs** — delegate → report.
- **audit** — load `improve-codebase-architecture` → Task(general-purpose, sonnet) with checklist → severity-classified report.
- **compare** — one explorer per subproject (PARALLEL, single message) → Task(Plan, sonnet) merges + surfaces discrepancies.
- **refactor** — load `improve-codebase-architecture` → Task(Plan) → print plan verbatim → AskUserQuestion (Approve/Adjust/Cancel) → Task(general-purpose) → validate.
- **implement** — Greps `{subproject}/.claude/commands/{guards,patterns}.md` (`-C 2`, `head_limit 20`) → single `Task(general-purpose, sonnet)` with slices inline, return cap 30 lines → agent runs build/type-check. ON CONCERN → surface + offer `/feature` Light.

→ See `../../../refs/task/task-prompts.md` for prompt templates.

Graph write-back is **N/A** — `/task` is spec-less by design. Promote to `/feature` Light or `/tactical-fix` if persistent backlinks needed.

## Domain Checklists (audit)

`copy` (tone/grammar/placeholders/CTA), `design` (tokens/reuse/hierarchy/parity), `a11y` (ARIA/contrast/keyboard/focus), `i18n` (missing keys/hardcoded/plurals), `consistency` (naming/structure/adherence), `api-contract` (DTOs/status codes/errors/versioning). Default when ambiguous: `consistency`.

## Analysis → Action

After `audit`/`compare`: parse severity, map each CRITICAL/WARNING to `/task refactor` or Pipeline, present structured list with estimated scope. Do NOT auto-execute — user picks.

`implement` → 1-3 files, known pattern, build-verifiable (low cost). `/feature` Light → spec + review gate (medium cost). `refactor` → reorganization without functional change.
