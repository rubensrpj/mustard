# Enforcement Rules

> Mustard v3.0 (stack-agnostic)

## Enforcement Levels

| Level | Rule | Description | Details |
|-------|------|-------------|---------|
| L0 | Delegation | Main Claude does NOT implement code | This file |
| L1 | grepai | Prefer grepai for semantic search | This file |
| L2 | Pipeline | Pipeline required for features/bugs | This file |
| L3 | Naming | Follow naming conventions | Inline in each agent `*.core.md` |
| L4 | Validation | Code must pass static validation | `prompts/review.md` |
| L5 | Build | Project must compile | `prompts/review.md` |
| L6 | Registry | Sync registry after creating entities | This file |

## Details

### L0 - Delegation
Main Claude coordinates but does not implement. Always delegates via Task tool.

### L1 - grepai
Use grepai for semantic search instead of Grep/Glob when possible.

### L2 - Pipeline
Features and bugfixes must follow the pipeline: Explore → Spec → Implement → Review.

### L3 - Naming
Naming conventions are inline in each agent's `context/{agent}/{agent}.core.md`.

### L4/L5 - Validation & Build
Validation and build commands depend on the project stack. See [prompts/review.md](../prompts/review.md).

### L6 - Registry
After creating/modifying entities, run `/sync-registry`.
