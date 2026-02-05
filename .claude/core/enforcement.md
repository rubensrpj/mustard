# Enforcement Rules

## Enforcement Levels

| Level | Rule | Description |
|-------|------|-------------|
| L0 | Delegation | Main Claude does NOT implement code |
| L1 | grepai | Prefer grepai for semantic search |
| L2 | Pipeline | Pipeline required for features/bugs |
| L3 | Patterns | Follow naming conventions |
| L4 | Type-check | Code must pass type-check |
| L5 | Build | Project must compile |
| L6 | Registry | Sync registry after creating entities |

## Details

### L0 - Delegation
Main Claude coordinates but does not implement. Always delegates via Task tool.

### L1 - grepai
Use grepai for semantic search instead of Grep/Glob when possible.

### L2 - Pipeline
Features and bugfixes must follow the pipeline: Explore -> Spec -> Implement -> Review.
