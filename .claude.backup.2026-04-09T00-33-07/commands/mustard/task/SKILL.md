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
```

Replace `{subproject}` with actual subproject name. Single repo: omit the subproject argument.
