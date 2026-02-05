# Enforcement Rules

## Enforcement Levels

| Level | Rule | Description |
|-------|------|-------------|
| L0 | Universal Delegation | ALL activities MUST be delegated via Task (separate context) |
| L1 | grepai | Prefer grepai for semantic search |
| L2 | Pipeline | Pipeline required for features/bugs |
| L3 | Patterns | Follow naming conventions |
| L4 | Type-check | Code must pass type-check |
| L5 | Build | Project must compile |
| L6 | Registry | Sync registry after creating entities |

## Details

### L0 - Universal Delegation (CRITICAL)

**The main context (parent) ONLY serves to:**
- Receive user requests
- Coordinate delegations via Task tool
- Present final results
- Manage pipeline state

**ALL activities involving code MUST be delegated:**

| Activity | Task Type | Emoji |
|----------|-----------|-------|
| Code exploration | `Task(Explore)` | 🔍 |
| Planning | `Task(Plan)` | 📋 |
| Backend/APIs | `Task(general-purpose)` | ⚙️ |
| Frontend/UI | `Task(general-purpose)` | 🎨 |
| Database | `Task(general-purpose)` | 🗄️ |
| Bugfix | `Task(general-purpose)` | 🐛 |
| Code Review | `Task(general-purpose)` | 🔎 |
| Documentation | `Task(general-purpose)` | 📊 |

**Hybrid Hook Enforcement:**
- **BLOCKS**: Source code files (`.ts`, `.js`, `.tsx`, `.jsx`, `.cs`, `.py`, etc.)
- **ALLOWS with advisory**: Configs, docs, and `.claude/` files

### L1 - grepai
Use grepai for semantic search instead of Grep/Glob when possible.

### L2 - Pipeline
Features and bugfixes must follow the pipeline: Explore -> Spec -> Implement -> Review.
