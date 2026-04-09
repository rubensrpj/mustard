# /task — Implementation Examples

Reference patterns for each action type. Read only when implementing a new action or debugging dispatch logic.

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
