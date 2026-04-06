# /task - Delegated Task Execution

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

> Delegates code tasks via **separate Task contexts** (L0 Universal Delegation).

## Trigger

`/task <action> <scope>`

## Actions

| Action | Agent | Model | Description |
|--------|-------|-------|-------------|
| `analyze` | Explore | haiku | Code exploration and pattern analysis |
| `audit` | general-purpose | sonnet | Quality audit with domain checklist (copy, design, a11y, i18n, consistency, api-contract) |
| `compare` | parallel explorers → Plan | haiku + sonnet | Cross-subproject comparison and alignment |
| `review` | general-purpose | opus | Code quality review (SOLID, security, perf) |
| `docs` | general-purpose | sonnet | Documentation generation |
| `refactor` | Plan → general-purpose | sonnet/opus | Plan + approve + implement refactoring |

## L0 Enforcement

**CRITICAL**: Parent context does NOT read code, does NOT implement. ALL work happens in Task contexts.

## Flow

### analyze / review / docs

1. **DELEGATE** — Create Task with scope
2. **REPORT** — Present findings to user

### audit

1. **DELEGATE** — Task(general-purpose, sonnet) with compiled layers + domain checklist
2. **REPORT** — Present findings with severity classification (CRITICAL / WARNING / NOTE)
3. **SUGGEST** — Propose actionable next steps (`/task refactor`, pipeline Enhancement, etc.)

### compare

1. **DISCOVER** — Identify target subprojects (from scope or all detected)
2. **PARALLEL** — Launch one explorer per subproject with comparison criteria
3. **CONSOLIDATE** — Launch Plan agent (sonnet) to merge findings and identify discrepancies
4. **REPORT** — Present unified comparison with actionable recommendations

### refactor (updated)

1. **ASSESS** — 3+ files or cross-layer → Plan mode first
2. **PLAN** — Task(Plan) to analyze and propose strategy
3. **APPROVE** — Present plan, wait for user approval
4. **IMPLEMENT** — Task(general-purpose) to execute approved plan
5. **VALIDATE** — Run build/tests

## Implementation

See `references/implementation-examples.md` for Task dispatch patterns per action type.

## Domain Checklists (for `audit`)

See `references/domain-checklists.md` for domain inference rules and checklist items.

## Scope Escalation

When scope touches **schema + API + UI** (3+ layers) or requires **multiple waves of implementation**:
- **Suggest `/feature` instead of `/task`** — pipeline Feature manages state formally (ANALYZE→PLAN→EXECUTE→CLOSE)
- `/task` is for **single-action delegations**, not multi-wave implementation
- If user insists on `/task`, proceed but warn: "No state recovery — if agents fail, progress is lost"

## Parallel Dispatch Rules

When dispatching multiple agents (compare, audit multi-subproject):
- **Independent waves MUST run in parallel** — dispatch all in a single message with multiple Task invocations
- **If ≥2 agents fail simultaneously**: re-dispatch **sequentially** (one at a time), not parallel — avoids the race condition that likely caused the batch failure
- **Report failures before re-dispatch** — tell user which agents failed and offer selective retry

## Analysis → Action Bridge

After receiving results from `audit` or `compare`:

1. **Parse severity** — count CRITICAL / WARNING / NOTE
2. **Map to intents** — for each CRITICAL/WARNING finding, suggest a concrete command:
   - Missing i18n key → `/task refactor "add missing i18n keys for {locale}"`
   - Design token violation → `/task refactor "replace hardcoded values with tokens in {file}"`
   - Pattern deviation → Pipeline Enhancement for the affected module
   - Cross-subproject mismatch → `/task compare` with narrower scope, then Pipeline Feature
3. **Present to user** — structured list with estimated scope (Light/Full)
4. **Do NOT auto-execute** — user must approve or pick which actions to take

## Examples

```bash
/task analyze authentication flow
/task audit "copy quality" {frontend-subproject}
/task audit "design token usage" {mobile-subproject}
/task audit "i18n consistency" {mobile-subproject}
/task audit "api-contract alignment" {api-subproject}
/task compare "design token naming"
/task compare "i18n key coverage"
/task review "Contract entity"
/task docs "API endpoints"
/task refactor "extract PaymentService"
```

Replace `{subproject}` with actual subproject name. Single repo: omit the subproject argument.
