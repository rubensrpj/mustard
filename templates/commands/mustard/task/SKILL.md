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
| `implement` | general-purpose | sonnet | Single-dispatch implementation with inline guards/patterns/recipes (low-cost, standardized) |

## L0 Enforcement

**CRITICAL**: Parent context does NOT read code, does NOT implement. ALL work happens in Task contexts.

**Note on `implement`**: the orchestrator may run targeted Greps against `.md` context files (`guards.md`, `patterns.md`, `recipes.md`) to inject standardization slices into the dispatched prompt. Those files are configuration docs, not application code — reading them in the parent is allowed. Source code reads still happen only inside the dispatched Task.

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
3. **APPROVE** — Print the ENTIRE plan returned by Task(Plan) verbatim inside a fenced markdown block (```` ```markdown ... ``` ````). Do NOT summarize or truncate — the user asked to read the complete plan before approving. Then `AskUserQuestion`: **"Approve and implement?"** / **"Adjust"** / **"Cancel"**.
4. **IMPLEMENT** — Task(general-purpose) to execute approved plan
5. **VALIDATE** — Run build/tests

> **Note on other `/task` actions:** only `refactor` has a plan-then-approve gate. `implement`, `analyze`, `audit`, `compare`, `review`, `docs` are single-dispatch by design (no plan to review). If you want a review gate before code changes, prefer `/feature` (Full scope) instead of `/task implement`.

### implement

1. **GREP SLICES** — Orchestrator runs targeted Greps against `{subproject}/.claude/commands/guards.md`, `patterns.md`, `recipes.md` for the scope keyword. Use `output_mode: content`, `-C 2`, `head_limit: 20` (cap ~500 tokens per file). Greps return small slices, not full files.
2. **DISPATCH** — Single `Task(general-purpose, sonnet)` with guards/patterns/recipe injected inline in the prompt, naming conventions explicit, and return format capped at 30 lines.
3. **BUILD** — Agent runs build/type-check at the end and reports the result.
4. **NO OVERHEAD** — No spec, no pipeline state, no review gate. Surgical.
5. **ON CONCERN** — If the agent returns CONCERN, orchestrator shows it to the user and offers either `/feature` Light (more gates) or an adjusted `implement` prompt.

## Implementation

```javascript
// analyze
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: `Analyze: ${scope}`,
  prompt: `
    # CODE ANALYSIS TASK
    ## Scope: ${scope}
    ## Instructions
    1. Use scoped Grep searches with path + pattern
    2. Read relevant files
    3. Document patterns found
    4. Report findings clearly
  `
})

// review
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `Review: ${scope}`,
  prompt: `
    # CODE REVIEW TASK
    ## Scope: ${scope}
    ## Checklist
    - [ ] SOLID principles
    - [ ] Error handling
    - [ ] Security concerns
    - [ ] Performance issues
    ## Output: [Severity] File:Line - Issue - Suggestion
  `
})

// docs
Task({
  subagent_type: "general-purpose",
  model: "sonnet",
  description: `Docs: ${scope}`,
  prompt: `
    # DOCUMENTATION TASK
    ## Scope: ${scope}
    ## Instructions
    1. Use scoped Grep to find relevant code
    2. Generate appropriate documentation
    3. Indicate where to save
  `
})

// refactor — Phase 1: Plan
Task({
  subagent_type: "Plan",
  model: "sonnet",
  description: `Plan refactor: ${scope}`,
  prompt: `# Plan refactoring for ${scope}...`
})

// refactor — Phase 2: Execute (after approval)
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `Execute refactor: ${scope}`,
  prompt: `# Execute approved plan...`
})

// audit
Task({
  subagent_type: "general-purpose",
  model: "sonnet",
  description: `Audit: ${scope}`,
  prompt: `
    # QUALITY AUDIT TASK
    ## Scope: ${scope}
    ## READ FIRST
    1. \`${subproject}/CLAUDE.md\` — guards, stack, key paths
    2. \`${subproject}/.claude/commands/guards.md\` — mandatory rules
    ## Domain: ${domain}
    ${domainChecklist}
    ## Output
    | Severity | File:Line | Issue | Recommendation |
    |----------|-----------|-------|----------------|
    ## Suggested Actions
    List concrete /task or pipeline commands to fix findings
  `
})

// implement — NEW ACTION
// Orchestrator runs targeted Greps first (each ≤500 tokens output)
const guards   = grep({path: `${sp}/.claude/commands/guards.md`,   pattern: keyword, output_mode: "content", "-C": 2, head_limit: 20});
const patterns = grep({path: `${sp}/.claude/commands/patterns.md`, pattern: keyword, output_mode: "content", "-C": 2, head_limit: 20});
const recipe   = grep({path: `${sp}/.claude/commands/recipes.md`,  pattern: keyword, output_mode: "content", "-C": 2, head_limit: 20});

// Single dispatch with everything inlined
Task({
  subagent_type: "general-purpose",
  model: "sonnet",
  description: `Implement: ${scope}`,
  prompt: `
    # IMPLEMENTATION TASK (standardized, low-cost)
    ## Scope: ${scope}

    ## Guards (inline — do not re-read)
    ${guards}

    ## Patterns to follow
    ${patterns}

    ## Recipe
    ${recipe}

    ## Naming conventions
    - PascalCase for classes/components
    - camelCase for variables/functions
    - snake_case for DB columns
    - kebab-case for files/URLs

    ## Return format
    - ≤30 lines
    - Sections: Files Changed (bullet list), Build result, Status (DONE/CONCERN/BLOCKED)
    - Do NOT paste file contents
  `
})

// compare — Phase 1: Parallel exploration
subprojects.forEach(sp => Task({
  subagent_type: "Explore",
  model: "haiku",
  description: `Compare scan: ${sp.name} — ${criteria}`,
  prompt: `# COMPARISON SCAN\n## Criteria: ${criteria}\n## Subproject: ${sp.name}\nCollect relevant data and report findings.`
}))
// compare — Phase 2: Consolidation
Task({
  subagent_type: "Plan",
  model: "sonnet",
  description: `Consolidate comparison: ${criteria}`,
  prompt: `# CONSOLIDATION\n## Explorer Results:\n${explorerResults}\nIdentify discrepancies and recommend actions.`
})
```

## Domain Checklists (for `audit`)

Orchestrator infers domain from scope keywords. Multiple domains can be combined.

| Domain | Keywords | Checklist |
|--------|----------|-----------|
| `copy` | copy, text, wording, tone, marketing | Tone consistency, grammar, placeholder text, marketing claims accuracy, CTA clarity |
| `design` | design, tokens, colors, typography, UI | Token usage, component reuse, visual hierarchy, spacing consistency, dark/light parity |
| `a11y` | accessibility, a11y, aria, contrast | ARIA labels, contrast ratios, keyboard navigation, screen reader support, focus management |
| `i18n` | i18n, translation, locale, language | Missing keys across locales, hardcoded strings, parameter consistency, pluralization |
| `consistency` | consistency, naming, structure, patterns | Naming conventions, file structure, pattern adherence across modules |
| `api-contract` | api, contract, endpoint, dto | DTO completeness, status codes, error response format, endpoint naming, versioning |

Default domain (if ambiguous): `consistency`.

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
/task implement "add logout button to header"
/task implement "create GET /api/users endpoint"
```

Replace `{subproject}` with actual subproject name. Single repo: omit the subproject argument.

## When to use implement vs /feature vs refactor

- `implement` — 1-3 arquivos, pattern conhecido, resultado verificável por build. Baixo custo, sem auditoria.
- `/feature` Light — mudanças estruturadas com spec auditável e review gate. Custo médio.
- `refactor` — reorganização sem mudança funcional (split, rename, extract). Tem fase de Plan separada.
