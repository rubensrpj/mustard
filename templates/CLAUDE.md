# Mustard - Instructions for Claude

> Agent framework and pipeline for Claude Code.
> **Version 2.4** - Auto-generated context, Memory MCP search in agents, improved CLI.

---

## 0. PIPELINE - ALWAYS CHECK

> **BEFORE ANY RESPONSE:** Check if there is an active pipeline.

### When Starting an Interaction

```javascript
// ALWAYS execute at the start
mcp__memory__search_nodes({ query: "pipeline phase" })
```

| Result | Action |
|--------|--------|
| No pipeline | Free analysis, but code edits require /feature or /bugfix |
| Pipeline in "explore" | Continue exploration or present spec for approval |
| Pipeline in "implement" | Edits allowed, follow spec |

### Automatic Intent Detection

| Request Type | Pipeline Required? |
|--------------|-------------------|
| "How does X work?" | NO - free analysis |
| "Where is Y?" | NO - free analysis |
| "Explain Z" | NO - free analysis |
| "Add field X" | YES - /feature |
| "Fix error Y" | YES - /bugfix |
| "Refactor Z" | YES - /feature |

---

## 1. ENFORCEMENT L0 - READ FIRST

> **ABSOLUTE RULE:** Main Claude does NOT implement code. ALWAYS delegates.

### When Receiving a Request:

1. **IDENTIFY** task type
2. **SELECT** appropriate agent/prompt
3. **DELEGATE** via Task tool with native `subagent_type`
4. **NEVER** start writing code directly

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
| Reports | `general-purpose` | sonnet | `prompts/report.md` |

### Self-Check

**Before using Write, Edit, or Bash (to create code):**

> Am I inside an agent (Task)?
> If NO → STOP and delegate.

---

## 2. Claude Code Native Types

Claude Code accepts **only 4 types** of subagent_type:

| Native Type | Description | Mustard Usage |
|-------------|-------------|---------------|
| `Explore` | Quick codebase exploration | Analysis phase |
| `Plan` | Implementation planning | Complex specs |
| `general-purpose` | Implementation, bug fixes, reviews | **MAIN** |
| `Bash` | Terminal commands | Git, builds |

### How It Works

Mustard "agents" are **prompts** that load specialized instructions inside a `Task(general-purpose)`:

```javascript
// BEFORE (doesn't work)
Task({ subagent_type: "orchestrator", ... })  // X

// AFTER (works)
Task({
  subagent_type: "general-purpose",
  model: "opus",
  prompt: `
    # You are the ORCHESTRATOR
    [content from prompts/orchestrator.md]

    # TASK
    ${description}
  `
})  // OK
```

---

## 3. Agents as Prompts

| Role | subagent_type | Model | Prompt File |
|------|---------------|-------|-------------|
| Orchestrator | `general-purpose` | opus | `prompts/orchestrator.md` |
| Explorer | `Explore` | haiku | (native - no prompt) |
| Backend | `general-purpose` | opus | `prompts/backend.md` |
| Frontend | `general-purpose` | opus | `prompts/frontend.md` |
| Database | `general-purpose` | opus | `prompts/database.md` |
| Bugfix | `general-purpose` | opus | `prompts/bugfix.md` |
| Review | `general-purpose` | opus | `prompts/review.md` |
| Report | `general-purpose` | sonnet | `prompts/report.md` |

---

## 4. Available Commands

### Pipeline

| Command | Description |
|---------|-------------|
| `/feature <name>` | Single entry point for features |
| `/bugfix <error>` | Single entry point for bugs |
| `/approve` | Approve spec and enable implementation |
| `/complete` | Finalize pipeline (after validation) |
| `/resume` | Resume active pipeline |

### Git

| Command | Description |
|---------|-------------|
| `/commit` | Simple commit |
| `/commit-push` | Commit and push |
| `/merge-main` | Merge to main |

### Validation

| Command | Description |
|---------|-------------|
| `/validate` | Build + type-check |
| `/status` | Consolidated status |
| `/scan` | Project reconnaissance |

### Sync

| Command | Description |
|---------|-------------|
| `/sync-registry` | Update Entity Registry |
| `/sync-types` | Regenerate TypeScript types |
| `/install-deps` | Install dependencies |
| `/sync-context` | Load project context |

### Reports

| Command | Description |
|---------|-------------|
| `/report-daily` | Daily commit report |
| `/report-weekly` | Weekly consolidated report |

### Task Commands (L0 Universal Delegation)

| Command | Emoji | Description |
|---------|-------|-------------|
| `/task-analyze <scope>` | 🔍 | Code analysis via Task(Explore) |
| `/task-review <scope>` | 🔎 | Code review via Task(general-purpose) |
| `/task-refactor <scope>` | 📋⚙️ | Refactoring via Task(Plan) → Task(general-purpose) |
| `/task-docs <scope>` | 📊 | Documentation via Task(general-purpose) |

> **IMPORTANT:** These commands ensure that ALL code activity is delegated to a separate context (Task), keeping the main (parent) context clean.

---

## 5. Required Single Pipeline

```
/feature or /bugfix → EXPLORE → SPEC → [APPROVE] → IMPLEMENT → REVIEW → COMPLETE
```

See full details in [core/pipeline.md](./core/pipeline.md).

---

## 6. Decision Tree

```
Request
    ↓
Is it a bug? ──YES──→ /bugfix
    │
   NO
    ↓
Is it a new feature? ──YES──→ /feature
    │
   NO
    ↓
Task(general-purpose) with specific prompt
```

---

## 7. Complete Enforcement (L0-L9)

| Level | Rule | Description |
|-------|------|-------------|
| L0 | Universal Delegation | ALL code activity MUST be delegated via Task (separate context) |
| L1 | grepai | Prefer grepai for semantic search |
| L2 | Pipeline | Pipeline required for features/bugs |
| L3 | Patterns | Naming, soft delete, multi-tenancy |
| L4 | Type-check | Frontend must pass type-check |
| L5 | Build | Backend must compile |
| L6 | Registry | Sync registry after creating entities |
| L7 | DbContext | Service does NOT access DbContext directly |
| L8 | Repository | Service only injects OWN Repository |
| L9 | ISP | Prefer segregated interfaces (SOLID) |

See details in [core/enforcement.md](./core/enforcement.md).

---

## 8. Search Rules

**ALWAYS use grepai** for semantic search:
```javascript
grepai_search({ query: "..." })
grepai_trace_callers({ symbol: "..." })
grepai_trace_callees({ symbol: "..." })
```

**ALWAYS use memory MCP** for pipeline context:
```javascript
mcp__memory__search_nodes({ query: "pipeline phase" })
mcp__memory__open_nodes({ names: ["Pipeline:name"] })
```

**FORBIDDEN** to use Grep/Glob - hook `enforce-grepai.js` blocks automatically.

### Why grepai?

| Tool | Problem |
|------|---------|
| Grep | Simple text search, many false positives |
| Glob | Only finds by file name |
| grepai | Semantic search, understands context and intent |

---

## 9. Correct Usage Example

### Calling Orchestrator for a Feature

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Orchestrate Invoice feature",
  prompt: `
# You are the ORCHESTRATOR

## Identity
You coordinate the development pipeline. You do NOT implement code - you delegate.

## Required Pipeline
1. EXPLORE: Use Task(subagent_type="Explore") to analyze
2. SPEC: Create spec in spec/active/{name}/spec.md
3. IMPLEMENT: Use Task(general-purpose) for each layer
4. REVIEW: Use Task(general-purpose) with review prompt
5. COMPLETE: Update registry

## TASK
Implement feature: Invoice
  `
})
```

### Calling Explorer (native)

```javascript
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: "Explore Invoice requirements",
  prompt: "Analyze requirements to implement Invoice entity. Map existing similar files."
})
```

### Calling Backend Specialist

```javascript
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Backend Invoice implementation",
  prompt: `
# You are the BACKEND SPECIALIST

## Responsibilities
- Implement endpoints/APIs
- Create services and business logic
- Follow project patterns

## Rules
- L7: Service does NOT access DbContext directly
- L8: Service only injects OWN Repository

## TASK
Implement backend module for Invoice according to spec.
  `
})
```

---

## 10. Project Context (v2.4)

### Auto-Generated Context by CLI

The CLI automatically generates context files in `.claude/context/`:

```
.claude/context/
├── README.md             # Folder documentation
├── architecture.md       # AUTO: Type, stacks, layers
├── patterns.md           # AUTO: Detected patterns
└── naming.md             # AUTO: Naming conventions
```

### User Files (Optional)

You can add custom files (flat, no subfolders):

```
.claude/context/
├── project-spec.md       # Project specification
├── business-rules.md     # Business rules
├── tips.md               # Tips for Claude
├── service-example.md    # Service example
├── component-example.md  # Component example
└── hook-example.md       # Hook example
```

### Rules

| Rule | Description |
|------|-------------|
| Markdown only | Only `.md` files are loaded |
| Max 500 lines | Larger files are truncated |
| Max 20 files | Total file limit |
| Refresh 24h | Auto-refresh if context > 24h |

### Entity Types in Memory MCP

| Entity | Description |
|--------|-------------|
| `ProjectContext:current` | Project metadata |
| `UserContext:{filename}` | Files from context/ |
| `EntityRegistry:current` | Cache of entity-registry.json |
| `EnforcementRules:current` | Rules L0-L9 |
| `CodePattern:{type}` | Patterns discovered via grepai |

### Using Context (Agents)

All agent prompts now automatically search for context:

```javascript
// Search context before implementing
const context = await mcp__memory__search_nodes({
  query: "UserContext architecture CodePattern service"
});

// Open specific entities
if (context.entities?.length) {
  const details = await mcp__memory__open_nodes({
    names: context.entities.map(e => e.name)
  });
  // Use found examples and patterns
}
```

### Benefits

| Metric | Impact |
|--------|--------|
| Tokens per feature | ~60% less (less exploration) |
| Rework | Reduces (follows patterns) |
| Quality | Improves (real examples) |
| Consistency | Uniform code |

---

## 11. Memory MCP - Pipeline Persistence

Pipeline state is persisted via **memory MCP**, not via files.

### Structure in Knowledge Graph

```
Pipeline:{name}
├── type: "pipeline"
├── observations:
│   ├── "phase: explore|implement|completed"
│   ├── "started: {ISO_DATE}"
│   ├── "objective: {description}"
│   └── "files: {list}"
└── relations:
    └── has_spec → Spec:{name}

Spec:{name}
├── type: "spec"
└── observations:
    ├── "## Objective\n..."
    ├── "## Files\n..."
    └── "## Checklist\n☐ Backend ☐ Frontend"
```

### Common Operations

```javascript
// Create pipeline (/feature)
mcp__memory__create_entities({
  entities: [{
    name: "Pipeline:add-email",
    entityType: "pipeline",
    observations: [
      "phase: explore",
      "started: 2026-02-05",
      "objective: Add email to Customer"
    ]
  }]
})

// Approve (/approve)
mcp__memory__add_observations({
  observations: [{
    entityName: "Pipeline:add-email",
    contents: ["phase: implement", "approved: 2026-02-05"]
  }]
})

// Search for active
mcp__memory__search_nodes({ query: "pipeline phase explore implement" })

// Finalize (/complete)
mcp__memory__delete_entities({
  entityNames: ["Pipeline:add-email", "Spec:add-email"]
})
```

---

## 12. Enforcement Hooks

### enforce-pipeline.js (L0+L2)

- **Trigger:** Edit/Write on code files
- **Action:** Asks for confirmation, Claude checks memory MCP
- **Exceptions:** .md, .json, .yaml, .claude/, mustard/, spec/

### enforce-grepai.js (L1)

- **Trigger:** Grep/Glob
- **Action:** BLOCKS with message to use grepai
- **No exceptions**

---

## 13. Links

### Core

- [Enforcement L0-L9](./core/enforcement.md)
- [Naming Conventions](./core/naming-conventions.md)
- [Entity Registry Spec](./core/entity-registry-spec.md)
- [Pipeline](./core/pipeline.md)

### Prompts

- [Prompts Index](./prompts/_index.md)
- [Backend](./prompts/backend.md)
- [Frontend](./prompts/frontend.md)
- [Database](./prompts/database.md)

### Commands - Pipeline

- [feature](./commands/mustard/feature.md)
- [bugfix](./commands/mustard/bugfix.md)
- [approve](./commands/mustard/approve.md)
- [complete](./commands/mustard/complete.md)
- [resume](./commands/mustard/resume.md)

### Commands - Other

- [sync-registry](./commands/mustard/sync-registry.md)
- [install-deps](./commands/mustard/install-deps.md)
- [sync-context](./commands/mustard/sync-context.md)
- [report-daily](./commands/mustard/report-daily.md)
- [report-weekly](./commands/mustard/report-weekly.md)

### Context

- [context/README.md](./context/README.md)

### Hooks

- [enforce-pipeline.js](./hooks/enforce-pipeline.js)
- [enforce-grepai.js](./hooks/enforce-grepai.js)
