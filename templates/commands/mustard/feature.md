# /feature - Feature Pipeline

> Single entry point for implementing new features.
> **v2.6** - Prerequisites enforced via hooks + Context Reset support.

## Usage

```
/feature <name>
/feature Invoice
/feature "Stripe Integration"
```

## Prerequisites (Enforced by Hooks)

Before this command runs, hooks validate:

1. **Entity Registry** - `.claude/entity-registry.json` must exist and be valid (v3.x)
2. **Compiled Contexts** - All agent contexts must be compiled and up-to-date

If prerequisites fail, you'll be prompted to run:

- `/sync-registry` - Update entity registry
- `/compile-context` - Compile agent contexts

## What It Does

1. **Creates pipeline** in memory MCP
2. **Explores** requirements via grepai + Task(Explore)
3. **Creates spec** in memory MCP
4. **Awaits approval** (/approve)
5. **Implements** via Task(general-purpose)
6. **Validates** via /validate
7. **Completes** via /complete

## Pipeline (Native Types)

```
/feature <name>
     │
     ▼
┌────────────────────────────────┐
│  Task(general-purpose)         │
│  + orchestrator.md prompt      │
│  model: opus                   │
└──────────────┬─────────────────┘
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
Task(Explore) → SPEC → APPROVE
               │
     ┌─────────┼─────────┐
     ▼         ▼         ▼
 database   backend   frontend
 (general)  (general) (general)
     │         │         │
     └─────────┴─────────┘
               │
               ▼
         review (general)
               │
               ▼
          COMPLETED
```

## Implementation

### Phase 1: Create Pipeline in Memory MCP

```javascript
// 1. Create pipeline entity
mcp__memory__create_entities({
  entities: [{
    name: `Pipeline:${name}`,
    entityType: "pipeline",
    observations: [
      "phase: explore",
      `started: ${new Date().toISOString()}`,
      `objective: ${userDescription}`
    ]
  }]
})

// 2. Explore with grepai (now with context loaded!)
grepai_search({ query: `${name} entity implementation` })
grepai_trace_callers({ symbol: `${relatedEntity}` })

// 3. Search for related context
const userContext = await mcp__memory__search_nodes({
  query: `UserContext ${name}`
});
// Context instantly available for analysis
```

### Phase 2: Create Spec

```javascript
// 3. Create spec as entity
mcp__memory__create_entities({
  entities: [{
    name: `Spec:${name}`,
    entityType: "spec",
    observations: [
      "## Objective\n" + objective,
      "## Files\n" + files.join('\n'),
      "## Approach\n" + steps.join('\n'),
      "## Checklist\n□ Database\n□ Backend\n□ Frontend"
    ]
  }]
})

// 4. Create relation
mcp__memory__create_relations({
  relations: [{
    from: `Pipeline:${name}`,
    to: `Spec:${name}`,
    relationType: "has_spec"
  }]
})
```

### Phase 3: Orchestrate Implementation

```javascript
// After /approve, execute via Task
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: `Feature: ${name}`,
  prompt: `
# You are the ORCHESTRATOR

## Active Pipeline
Name: ${name}
Phase: implement (approved)
Objective: ${objective}

## Delegation Rules
- Database: Task(general-purpose, model: opus) + database.md prompt
- Backend: Task(general-purpose, model: opus) + backend.md prompt
- Frontend: Task(general-purpose, model: opus) + frontend.md prompt

## ENFORCEMENT
- L0: You do NOT implement code directly - delegate
- L2: Follow approved spec
- L3: Ensure patterns (naming, soft delete, tenant_id)

## TASK
Implement according to approved spec.
  `
})
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `<name>` | Feature name | `Invoice`, `"User Auth"` |

## Examples

```bash
# New entity
/feature Invoice

# Feature with description
/feature "Add email field to Person"

# Integration
/feature "Payment gateway integration"
```

## Output

### During Execution

```
Orchestrator: Starting pipeline for Invoice
Task(Explore): Analyzing requirements...
Spec created: spec/active/2026-02-04-invoice/spec.md

Awaiting approval...
```

### After Approval

```
Task(general-purpose): Database - Creating schema...
Task(general-purpose): Backend - Implementing module...
Task(general-purpose): Frontend - Creating CRUD...
Task(general-purpose): Review - Reviewing...

✅ Feature Invoice implemented successfully!

Files created:
- src/schema/invoice.ts
- Modules/Invoice/...
- src/features/invoice/...
```

## Generated Spec

```markdown
# Spec: Invoice

## Date: 2026-02-04
## Status: active

## Summary
Create Invoice entity with items...

## Files

### Database
- [ ] src/schema/invoice.ts

### Backend
- [ ] Modules/Invoice/...

### Frontend
- [ ] src/features/invoice/...

## Tasks
1. [ ] Create schema
2. [ ] Generate migration
3. [ ] Create endpoints
...
```

## Related Commands

| Phase | Command | Description |
|-------|---------|-------------|
| Start | `/feature` | Creates pipeline, explores, creates spec |
| Approval | `/approve` | Enables implement phase |
| Validation | `/validate` | Build + type-check |
| End | `/complete` | Finalizes and cleans pipeline |
| Resume | `/resume` | Resumes existing pipeline |

## Notes

- **Prerequisites enforced** via hooks (registry + contexts)
- **Always** creates spec before implementing
- **Always** awaits approval (/approve)
- Pipeline persisted via **memory MCP** (not files)
- Only **one active pipeline** at a time
- **Uses only native types**: Explore, general-purpose
- **Uses grepai** for search (Grep/Glob blocked)
- Compiled context available in `prompts/{agent}.context.md`

## See Also

- [/approve](./approve.md) - Approve spec
- [/complete](./complete.md) - Finalize pipeline
- [/resume](./resume.md) - Resume pipeline
- [/bugfix](./bugfix.md) - Pipeline for bugs
- [/sync-context](./sync-context.md) - Manually load context
- [context/README.md](../context/README.md) - How to create context files
