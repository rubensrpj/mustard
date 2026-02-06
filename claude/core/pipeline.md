# Mustard Pipeline

> Single mandatory pipeline for features and bugfixes.
> **v2.3** - Auto context-loading, new entity types in memory MCP.

## Overview

All implementation work MUST follow the pipeline:

```
ENTRY → EXPLORE → SPEC → IMPLEMENT → REVIEW → COMPLETE
```

## Complete Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                        ENTRY                                        │
│                       /feature or /bugfix                           │
└───────────────────────────┬─────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│  PHASE 1: EXPLORE                                                   │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Task(subagent_type="Explore", model="haiku")               │    │
│  │  - grepai_search for semantic search                        │    │
│  │  - Mapping of affected files                                │    │
│  │  - Identification of existing patterns                      │    │
│  │  - Returns: compact synthesis                               │    │
│  └─────────────────────────────────────────────────────────────┘    │
└───────────────────────────┬─────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│  PHASE 2: SPEC                                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Task(general-purpose) + orchestrator.md                    │    │
│  │  - Creates spec/active/{date}-{name}/spec.md                │    │
│  │  - Lists all tasks                                          │    │
│  │  - PRESENTS to user                                         │    │
│  │  - AWAITS approval                                          │    │
│  └─────────────────────────────────────────────────────────────┘    │
└───────────────────────────┬─────────────────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              ▼                           ▼
        [APPROVED]                   [ITERATE]
              │                           │
              │                    (back to PHASE 1)
              ▼
┌─────────────────────────────────────────────────────────────────────┐
│  PHASE 3: IMPLEMENT                                                 │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐        │
│  │  database.md   │  │  backend.md    │  │  frontend.md   │        │
│  │  (opus)        │  │  (opus)        │  │  (sonnet)      │        │
│  │                │  │                │  │                │        │
│  │  Schema        │  │  Endpoints     │  │  Components    │        │
│  │  Migrations    │  │  Services      │  │  Hooks         │        │
│  │  Seeds         │  │  DTOs          │  │  Pages         │        │
│  └───────┬────────┘  └───────┬────────┘  └───────┬────────┘        │
│          │                   │                   │                  │
│          └───────────────────┴───────────────────┘                  │
│                              │                                      │
│               Task(general-purpose) + specialized prompt            │
│                    (parallel when possible)                         │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│  PHASE 4: REVIEW                                                    │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  Task(general-purpose) + review.md (opus)                   │    │
│  │  - Checks naming (L3)                                       │    │
│  │  - Checks structure                                         │    │
│  │  - Checks integration                                       │    │
│  │  - Checks SOLID (L7/L8)                                     │    │
│  │  - Runs /validate (L4/L5)                                   │    │
│  │  - APPROVES or REJECTS                                      │    │
│  └─────────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
              ┌─────────────────┴─────────────────┐
              ▼                                   ▼
        [APPROVED]                           [REJECT]
              │                                   │
              │                            (back to PHASE 3)
              ▼                            (with feedback)
┌─────────────────────────────────────────────────────────────────────┐
│  PHASE 5: COMPLETE                                                  │
│  - Updates entity-registry.json (if new entity) - L6               │
│  - Moves spec to spec/completed/                                    │
│  - Reports success                                                  │
│  - Suggests /commit-push                                            │
└─────────────────────────────────────────────────────────────────────┘
```

## Detailed Phases

### PHASE 1: EXPLORE

**Type:** `Task(subagent_type="Explore")` (native)

**Objective:** Understand context and map files.

**Tools:**
- `grepai_search` - semantic search
- `grepai_trace_callers` - who calls
- `grepai_trace_callees` - what it calls
- `Read` - read files

> ⛔ **Glob/Grep are FORBIDDEN** - use only grepai (L1)

**Output:**
```markdown
## Exploration: {objective}

### Relevant Files
| File | Action |
|------|--------|
| {path} | create/modify |

### Patterns
- {pattern}

### References
- {similar file}
```

### PHASE 2: SPEC

**Type:** `Task(general-purpose)` + orchestrator.md

**Objective:** Document what will be done and get approval.

**Output:** File in `spec/active/{date}-{name}/spec.md`

**Format:**
```markdown
# Spec: {Name}

## Date: {YYYY-MM-DD}
## Status: active

## Summary
{description}

## Files
- [ ] {file}: {action}

## Tasks
1. [ ] {task}
```

### PHASE 3: IMPLEMENT

**Types:** `Task(general-purpose)` + specialized prompts

**Objective:** Implement according to spec.

**Order:**
1. Database first (if there's schema) - database.md
2. Backend can start after schema - backend.md
3. Frontend can start after endpoints (or parallel with mocks) - frontend.md

### PHASE 4: REVIEW

**Type:** `Task(general-purpose)` + review.md

**Objective:** Ensure quality and integration.

**Checklist:**
- [ ] Correct naming (L3)
- [ ] Correct structure
- [ ] Patterns followed (L3)
- [ ] Integration OK
- [ ] Build passes (L4/L5)
- [ ] SOLID OK (L7/L8)

### PHASE 5: COMPLETE

**Objective:** Finalize and document.

**Actions:**
- Update registry (L6)
- Move spec to completed
- Report success

## Rules

1. **NEVER skip phases**
2. **ALWAYS get approval in PHASE 2**
3. **ALWAYS go through review in PHASE 4**
4. **ALWAYS use Task tool** (L0 enforcement)
5. **ALWAYS use native types** (Explore, general-purpose)

---

## Memory MCP - Entity Types

### Context Entities (Loaded at pipeline start)

| Entity Type | Name | Description |
|-------------|------|-------------|
| `project-context` | `ProjectContext:current` | Project metadata (stacks, naming) |
| `entity-registry` | `EntityRegistry:current` | Cache of entity-registry.json |
| `enforcement` | `EnforcementRules:current` | L0-L9 rules |
| `user-context` | `UserContext:{filename}` | Files from .claude/context/*.md |
| `code-pattern` | `CodePattern:{type}` | Patterns discovered via grepai |

### Pipeline Entities

| Entity Type | Name | Description |
|-------------|------|-------------|
| `pipeline` | `Pipeline:{name}` | Active pipeline state |
| `spec` | `Spec:{name}` | Approved specification |

### ProjectContext Structure

```javascript
{
  name: "ProjectContext:current",
  entityType: "project-context",
  observations: [
    "type: monorepo",
    "stacks: dotnet:9.0, react:19.x, drizzle:0.44.x",
    "naming.classes: PascalCase",
    "naming.files: {ts:camelCase, tsx:camelCase}",
    "naming.folders: plural",
    "loaded: 2026-02-05T10:00:00Z"
  ]
}
```

### UserContext Structure

```javascript
{
  name: "UserContext:architecture",
  entityType: "user-context",
  observations: [
    "file: .claude/context/architecture.md",
    "title: Architecture",
    "content: ## Layers\n- Database (Drizzle)\n- Backend (.NET)\n..."
  ]
}
```

### CodePattern Structure

```javascript
{
  name: "CodePattern:service",
  entityType: "code-pattern",
  observations: [
    "type: service",
    "file: Backend/Modules/Contract/Services/ContractService.cs",
    "pattern: Repository + UnitOfWork, IContractService",
    "sample: public class ContractService(IContractRepository repo)..."
  ]
}
```

### EntityRegistry Structure

```javascript
{
  name: "EntityRegistry:current",
  entityType: "entity-registry",
  observations: [
    "version: 2.1",
    "patterns.db: Database/src/schema/{kebab}.ts",
    "patterns.be: Backend/Modules/{PascalPlural}/",
    "patterns.fe: Frontend/src/features/{kebab-plural}/",
    "entities: Contract,Partner,Invoice,User,Company"
  ]
}
```

### EnforcementRules Structure

```javascript
{
  name: "EnforcementRules:current",
  entityType: "enforcement",
  observations: [
    "L0: Delegation - Main Claude does NOT implement code",
    "L1: grepai - Prefer semantic search",
    "L7: DbContext - Service does NOT access DbContext directly",
    // ... all rules
  ]
}
```

### Relations

```
ProjectContext:current
├── has_registry → EntityRegistry:current
├── has_rules → EnforcementRules:current
├── has_context → UserContext:architecture
├── has_context → UserContext:business-rules
├── has_context → UserContext:tips
├── has_pattern → CodePattern:service
├── has_pattern → CodePattern:repository
└── has_pattern → CodePattern:component

Pipeline:{name}
└── has_spec → Spec:{name}
```

---

## Context Loading Flow

```
/feature or /bugfix
         │
         ▼
┌─────────────────────────────────────────────────────────┐
│  PHASE 0: LOAD CONTEXT (if missing or > 24h)            │
│                                                         │
│  1. Glob .claude/context/*.md → UserContext:*           │
│  2. Read CLAUDE.md → ProjectContext:current             │
│  3. Read entity-registry.json → EntityRegistry:current  │
│  4. Read enforcement.md → EnforcementRules:current      │
│  5. grepai discovery → CodePattern:*                    │
│  6. Create relations                                    │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
              [Normal pipeline continues...]
```

---

## Using Context During Implementation

```javascript
// Search for relevant context
const context = await mcp__memory__search_nodes({
  query: "UserContext architecture CodePattern service"
});

// Open specific entities
const details = await mcp__memory__open_nodes({
  names: ["UserContext:architecture", "CodePattern:service"]
});

// Context available instantly!
```

---

## See Also

- [/sync-context](../commands/sync-context.md) - Manually load context
- [/feature](../commands/feature.md) - Feature pipeline
- [/bugfix](../commands/bugfix.md) - Bugfix pipeline
- [context/README.md](../context/README.md) - How to create context files
