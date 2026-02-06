# Enforcement Rules

> Mandatory rules for Mustard v3.0 (stack-agnostic).
> Pipeline enforcement via memory MCP.

## Enforcement Matrix

| Level | Rule | Description | Details in |
|-------|------|-------------|------------|
| L0 | Delegation | Main Claude does NOT implement code | This file |
| L1 | grepai | Prefer grepai for semantic search | This file |
| L2 | Pipeline | Pipeline mandatory for features/bugs | This file |
| L3 | Naming | Follow naming conventions | `prompts/naming.md` |
| L4 | Validation | Code must pass static validation | `prompts/review.md` |
| L5 | Build | Project must compile/run | `prompts/review.md` |
| L6 | Registry | Sync registry after creating entities | `commands/sync-registry.md` |

---

## L0 - Mandatory Delegation

### Rule L0

> ⛔ Main Claude does NOT implement code. ALWAYS delegate via Task.

### Self-Check

Before using **Write**, **Edit**, or **Bash** (for code):

```text
Question: "Am I inside an agent (Task)?"

If YES → Continue
If NO → STOP and DELEGATE
```

### Delegation Map

| Request | subagent_type | model | Prompt |
|---------|---------------|-------|--------|
| Bug fix | `general-purpose` | opus | `prompts/bugfix.md` |
| New feature | `general-purpose` | opus | `prompts/orchestrator.md` |
| Backend | `general-purpose` | opus | `prompts/backend.md` |
| Frontend | `general-purpose` | opus | `prompts/frontend.md` |
| Database | `general-purpose` | opus | `prompts/database.md` |
| QA/Review | `general-purpose` | opus | `prompts/review.md` |
| Explore | `Explore` | haiku | (native) |

### Correct L0 Example

```text
User: "Add email field to Person"

Main Claude:
1. Identify: It's an entity modification (feature)
2. Delegate: Task(subagent_type="general-purpose", model="opus", prompt="[orchestrator]...")
3. Return: Result to user
```

### Incorrect L0 Example

```text
User: "Add email field to Person"

Main Claude:
1. Read Person.cs
2. START EDITING ← ⛔ L0 VIOLATION
```

---

## L1 - Mandatory grepai

### Rule L1

> ⛔ **Grep and Glob are BLOCKED by the `enforce-grepai.js` hook.**
> Use ONLY grepai for ALL code searches.

### Hook L1

```text
mustard/claude/hooks/enforce-grepai.js
```

- **Trigger:** Attempt to use Grep or Glob
- **Action:** BLOCKS with permissionDecision: "deny"
- **No exceptions**

### Correct L1 Usage

```javascript
// Semantic search
grepai_search({ query: "authentication flow" })

// Call tracing
grepai_trace_callers({ symbol: "SaveContract" })
grepai_trace_callees({ symbol: "ValidateUser" })
grepai_trace_graph({ symbol: "ProcessPayment", depth: 2 })
```

### BLOCKED L1 Usage

```bash
# ⛔ AUTOMATICALLY BLOCKED by hook
grep -r "authentication" .  # ⛔ BLOCKED
Glob("**/*.tsx")            # ⛔ BLOCKED
```

### Why grepai?

| Tool | Problem |
|------|---------|
| Grep | Simple text search, many false positives |
| Glob | Only finds by filename |
| grepai | Semantic search, understands context and intent |

---

## L2 - Mandatory Pipeline (Memory MCP)

### Rule L2

> Every feature/bugfix goes through the pipeline, with state persisted via memory MCP.

### Hook L2

```text
mustard/claude/hooks/enforce-pipeline.js
```

- **Trigger:** Edit/Write on code files
- **Action:** Asks for confirmation (ask), Claude checks memory MCP
- **Exceptions:** .md, .json, .yaml, .claude/, mustard/, spec/

### Pipeline Phases

```text
explore → (spec approved) → implement → (validation) → completed
```

| Phase | Action | Edits Allowed |
|-------|--------|---------------|
| explore | Analysis, create spec | ❌ Code blocked |
| implement | Implementation | ✅ Code allowed |
| completed | Pipeline finished | (new pipeline needed) |

### Verification via Memory MCP

```javascript
// When receiving message
const result = await mcp__memory__search_nodes({
  query: "pipeline phase"
});

// Check phase
if (result.entities.length === 0) {
  // No pipeline → free analysis, edits blocked
}
if (result.entities[0].observations.includes("phase: implement")) {
  // Edits allowed
}
```

### Pipeline Commands

| Command | Phase | Action |
|---------|-------|--------|
| `/feature` | → explore | Creates pipeline in memory |
| `/approve` | explore → implement | Enables edits |
| `/validate` | implement | Checks build/type-check |
| `/complete` | implement → done | Cleans pipeline |
| `/resume` | (any) | Loads context |

### Forbidden in L2

- Edit code without active pipeline
- Edit code in "explore" phase
- Skip spec approval

---

## L3 - Naming Conventions

### Rule L3

> Every implementation MUST follow project naming conventions.

**Full details in:** [prompts/naming.md](../prompts/naming.md)

### Quick Summary

| Type | Pattern | Example |
|------|---------|---------|
| Entity | PascalCase singular | `Contract` |
| DB Table | snake_case plural | `contracts` |
| Endpoint | kebab-case | `/api/contracts` |
| Hook | use + camelCase | `useContracts` |

---

## L4 - Validation

### Rule L4

> Code must pass static validation (lint, type-check).

**Details in:** [prompts/review.md](../prompts/review.md)

Validation command depends on project stack:

| Stack | Command |
|-------|---------|
| TypeScript | `tsc --noEmit` or `pnpm type-check` |
| Python | `mypy` or `pyright` |
| Go | `go vet` |
| Rust | `cargo check` |

---

## L5 - Build

### Rule L5

> Project must compile/run without errors.

**Details in:** [prompts/review.md](../prompts/review.md)

Build command depends on stack:

| Stack | Command |
|-------|---------|
| .NET | `dotnet build` |
| Node.js | `npm run build` or `pnpm build` |
| Python | `python -m py_compile` |
| Go | `go build` |
| Rust | `cargo build` |

---

## L6 - Entity Registry

### Rule L6

> After creating/modifying entities, update the registry.

```bash
/sync-registry
```

**Details in:** [entity-registry-spec.md](./entity-registry-spec.md)

---

## Visual Summary

```text
L0: Delegation ──────────────────────────────────────────────────┐
L1: grepai ───────────────────────────────────────────────────────┤ ENGINE
L2: Pipeline ─────────────────────────────────────────────────────┤ (core/)
                                                                  │
L3: Naming ───────────────────────────────────────────────────────┤
L4: Validation ───────────────────────────────────────────────────┤ CONVENTIONS
L5: Build ────────────────────────────────────────────────────────┤ (prompts/)
L6: Registry ─────────────────────────────────────────────────────┘
```

---

## See Also

- [prompts/naming.md](../prompts/naming.md) - Naming conventions (L3)
- [prompts/review.md](../prompts/review.md) - Validation and build (L4/L5)
- [prompts/backend.md](../prompts/backend.md) - Backend architecture patterns
- [pipeline.md](./pipeline.md) - Complete pipeline
- [entity-registry-spec.md](./entity-registry-spec.md) - Registry spec (L6)
