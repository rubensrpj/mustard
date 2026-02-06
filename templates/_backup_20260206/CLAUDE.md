# Mustard - Instructions for Claude

> Agent framework and pipeline for Claude Code.
> **Version 2.5** - Agent Teams support, mandatory pipeline invocation.

---

## 0. MANDATORY PIPELINE INVOCATION (L-1)

> **CRITICAL:** When user requests code changes, you MUST invoke the appropriate skill FIRST.

### Before Responding to Code Change Requests

**Step 1:** Detect if request involves code changes:

| Intent | Examples |
|--------|----------|
| New feature | "Add X", "Create Y", "Implement Z" |
| Bug fix | "Fix X", "Error Y", "Not working" |
| Refactor | "Refactor X", "Rename Y", "Move Z" |

**Step 2:** If code change detected, invoke the skill IMMEDIATELY:

```text
For features/refactors: Use Skill tool with skill: "mustard:feature"
For bug fixes: Use Skill tool with skill: "mustard:bugfix"
```

**Step 3:** Do NOT analyze, explore, or plan before invoking the skill.

### Why This Matters

- The skill compiles contexts (git-based caching)
- The skill creates the pipeline in memory MCP
- The skill ensures proper delegation
- Without the skill, contexts are not loaded

### Exceptions (No Pipeline Needed)

| Request Type | Action |
|--------------|--------|
| "How does X work?" | Free analysis |
| "Where is Y?" | Free exploration |
| "Explain Z" | Free explanation |
| Questions about code | Free analysis |

---

## 1. PIPELINE STATE CHECK

> **AFTER invoking a skill**, check pipeline state.

### When Starting an Interaction

```javascript
// Check pipeline state
mcp__memory__search_nodes({ query: "pipeline phase" })
```

| Result | Action |
|--------|--------|
| No pipeline | Invoke /feature or /bugfix skill first |
| Pipeline in "explore" | Continue exploration or present spec |
| Pipeline in "implement" | Edits allowed, follow spec |

---

## 2. ENFORCEMENT L0 - DELEGATION

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

## 3. Claude Code Native Types

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

## 4. Agents as Prompts

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

## 5. Available Commands

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

## 6. Required Single Pipeline

```
/feature or /bugfix → EXPLORE → SPEC → [APPROVE] → IMPLEMENT → REVIEW → COMPLETE
```

See full details in [core/pipeline.md](./core/pipeline.md).

---

## 7. Decision Tree

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

## 8. Complete Enforcement (L0-L9)

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

## 9. Search Rules

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

## 10. Correct Usage Example

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

## 11. Project Context

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

## 12. Memory MCP - Pipeline Persistence

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

## 13. Agent Teams (Experimental)

> Alternative to Task subagents for complex, multi-layer features.
> Uses Claude Code's experimental Agent Teams feature.

### Enable Agent Teams

Add to `.claude/settings.json`:

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

### Team Commands

| Command | Description |
|---------|-------------|
| `/feature-team <name>` | Feature pipeline with Agent Teams |
| `/bugfix-team <error>` | Bugfix pipeline with competing hypotheses |

### When to Use Teams vs Tasks

| Use Agent Teams | Use Task Subagents |
|-----------------|-------------------|
| Multi-layer features | Single-layer changes |
| Complex coordination needed | Simple delegation |
| Competing hypotheses (bugfix) | Known root cause |
| True parallelism needed | Sequential is OK |
| Higher token budget OK | Token cost matters |

### Team Roles

| Role | Prompt | Description |
|------|--------|-------------|
| Team Lead | `prompts/team-lead.md` | Spawns and coordinates teammates |
| Database | `prompts/database.md` | Schema and migrations (as teammate) |
| Backend | `prompts/backend.md` | APIs and services (as teammate) |
| Frontend | `prompts/frontend.md` | Components and hooks (as teammate) |
| Review | `prompts/review.md` | Quality validation (as teammate) |

### Team Pipeline

```text
/feature-team <name>
     │
     ▼
 TEAM LEAD (you, in delegate mode)
     │
     ├── Spawn Database Teammate
     ├── Spawn Backend Teammate
     ├── Spawn Frontend Teammate
     │
     ▼
 SHARED TASK LIST (with dependencies)
     │
     ▼
 Spawn Review Teammate
     │
     ▼
 TEAM CLEANUP
```

### Key Differences from Task Mode

| Aspect | Task Mode | Agent Teams |
|--------|-----------|-------------|
| Context | Shared session | Independent per teammate |
| Communication | Report to parent | Peer-to-peer messaging |
| Parallelism | Sequential Tasks | True parallel execution |
| Token Cost | Lower | Higher |

### Limitations

- No session resumption with in-process teammates
- Task status can lag
- Shutdown can be slow
- One team per session
- Higher token cost

See [feature-team.md](./commands/mustard/feature-team.md) for full details.

---

## 14. Enforcement Hooks

### enforce-pipeline.js (L0+L2)

- **Trigger:** Edit/Write on code files
- **Action:** Asks for confirmation, Claude checks memory MCP
- **Exceptions:** .md, .json, .yaml, .claude/, mustard/, spec/

### enforce-grepai.js (L1)

- **Trigger:** Grep/Glob
- **Action:** BLOCKS with message to use grepai
- **No exceptions**

---

## 15. Links

### Core

- [Enforcement L0-L9](./core/enforcement.md)
- [Naming Conventions](./core/naming-conventions.md)
- [Entity Registry Spec](./core/entity-registry-spec.md)
- [Pipeline](./core/pipeline.md)

### Prompts

- [Prompts Index](./prompts/_index.md)
- [Team Lead](./prompts/team-lead.md)
- [Backend](./prompts/backend.md)
- [Frontend](./prompts/frontend.md)
- [Database](./prompts/database.md)

### Commands - Pipeline

- [feature](./commands/mustard/feature.md)
- [bugfix](./commands/mustard/bugfix.md)
- [approve](./commands/mustard/approve.md)
- [complete](./commands/mustard/complete.md)
- [resume](./commands/mustard/resume.md)

### Commands - Agent Teams

- [feature-team](./commands/mustard/feature-team.md)
- [bugfix-team](./commands/mustard/bugfix-team.md)

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
