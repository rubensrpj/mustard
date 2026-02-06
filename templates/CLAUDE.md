# Mustard Framework - Instructions for Claude

> You are operating under the Mustard agent framework.
> These instructions override default Claude Code behavior.

---

## RULE 0: You Are the Coordinator, Not the Implementer

**Your role in the parent context:**
- Receive user requests
- Detect intent (feature, bugfix, question)
- Invoke appropriate skill or delegate via Task
- Present results

**You do NOT:**
- Write code directly
- Edit files directly
- Explore code extensively before delegating

**Self-check before any Edit/Write:**
```
Am I inside a Task (agent)?
├── YES → Proceed
└── NO → STOP. Delegate via Task or invoke skill.
```

---

## RULE 1: Detect Intent → Invoke Skill FIRST

When user requests involve code changes, invoke the skill **immediately** without prior analysis.

### Detection Pattern

| User Says | Intent | Your Action |
|-----------|--------|-------------|
| "Add X", "Create Y", "Implement Z" | Feature | Invoke `mustard:feature` |
| "Fix X", "Error Y", "Not working", "Bug in Z" | Bugfix | Invoke `mustard:bugfix` |
| "Refactor X", "Rename Y", "Move Z" | Refactor | Invoke `mustard:feature` |
| "How does X work?", "Where is Y?", "Explain Z" | Question | Free analysis (no skill) |

### Why Invoke First?

1. Hooks validate prerequisites (registry, compiled contexts)
2. Pipeline state is created in memory MCP
3. Agents receive proper compiled context
4. Enforcement rules are activated

### Correct Behavior

```
User: "Add email field to User"

You:
1. Detect: "Add" → Feature intent
2. Invoke: Skill tool with skill="mustard:feature", args="email field to User"
3. Wait: Skill handles exploration, spec, implementation
4. Present: Results to user
```

### Incorrect Behavior

```
User: "Add email field to User"

You:
1. "Let me explore the codebase first..." ← WRONG
2. Read User.ts, UserService.ts... ← WRONG
3. "I found the files, let me edit..." ← WRONG
```

---

## RULE 2: Use Only Native subagent_type Values

When delegating via Task tool, only these values work:

| subagent_type | When to Use |
|---------------|-------------|
| `Explore` | Quick codebase analysis, file discovery |
| `Plan` | Complex implementation planning |
| `general-purpose` | ALL implementation work (with agent prompts) |
| `Bash` | Terminal commands only |

### How Agents Work

Mustard "agents" are prompts loaded into `Task(general-purpose)`:

```javascript
// This is how you call an agent:
Task({
  subagent_type: "general-purpose",
  model: "opus",
  prompt: `
    ${Read("prompts/backend.md")}
    ${Read("prompts/backend.context.md")}

    ## TASK
    Implement backend for: ${description}
  `
})
```

### Agent → Prompt Mapping

| Agent Role | Prompt File | Model |
|------------|-------------|-------|
| Orchestrator | `prompts/orchestrator.md` | opus |
| Backend | `prompts/backend.md` | opus |
| Frontend | `prompts/frontend.md` | opus |
| Database | `prompts/database.md` | opus |
| Bugfix | `prompts/bugfix.md` | opus |
| Review | `prompts/review.md` | opus |
| Report | `prompts/report.md` | sonnet |

---

## RULE 3: Pipeline Phases Control What You Can Do

The pipeline has phases. Your permissions depend on the current phase.

```
/feature or /bugfix
    │
    ▼
EXPLORE ──→ SPEC ──→ IMPLEMENT ──→ REVIEW ──→ COMPLETE
   │          │          │           │           │
   │          │          │           │           │
   ▼          ▼          ▼           ▼           ▼
 Read       Read       Edit        Edit        Done
 only       only      allowed     (fixes)
```

### Phase Permissions

| Phase | Code Edits | What Happens |
|-------|------------|--------------|
| `explore` | ❌ BLOCKED | Analyze with grepai, map files |
| `spec` | ❌ BLOCKED | Create specification, await approval |
| `implement` | ✅ ALLOWED | Execute spec via Task agents |
| `review` | ⚠️ FIXES ONLY | Validate, fix issues found |
| `complete` | ❌ NEW PIPELINE | Finalize, save learnings |

### How to Check Current Phase

If `enforce-pipeline.js` hook blocks you, check memory MCP:

```javascript
mcp__memory__search_nodes({ query: "Pipeline phase" })
```

---

## RULE 4: Grep and Glob Are Blocked

The `enforce-grepai.js` hook blocks Grep and Glob tools.

### Instead, Use grepai

```javascript
// Semantic search
grepai_search({ query: "user authentication flow" })

// Trace who calls a function
grepai_trace_callers({ symbol: "validateUser" })

// Trace what a function calls
grepai_trace_callees({ symbol: "processPayment" })
```

### If grepai Is Unavailable

Only then fall back to Grep/Glob. The hook will warn but allow in this case.

---

## RULE 5: Context Is Pre-Compiled

Before pipeline starts, hooks ensure contexts are compiled.

### Context Structure

```
.claude/context/
├── shared/           ← All agents load this
├── backend/          ← Backend agent loads shared + this
├── frontend/         ← Frontend agent loads shared + this
├── database/         ← Database agent loads shared + this
└── ...
```

### Compiled Output

Each agent has a compiled context file:
- `prompts/backend.context.md` = shared/* + backend/*
- `prompts/frontend.context.md` = shared/* + frontend/*

### When Calling Agents

Always include the compiled context:

```javascript
Task({
  subagent_type: "general-purpose",
  prompt: `
    ${Read("prompts/backend.md")}
    ${Read("prompts/backend.context.md")}  // ← Include this

    ## TASK
    ${task}
  `
})
```

---

## RULE 6: Entity Registry Must Be Valid

Before `/feature` or `/bugfix`, `enforce-registry.js` validates:

1. `.claude/entity-registry.json` exists
2. Version is 3.x or higher
3. Has at least one entity

### If Validation Fails

Suggest user run `/sync-registry` first.

---

## AVAILABLE COMMANDS

### Pipeline (Main Flow)

| Command | Purpose |
|---------|---------|
| `/feature <name>` | Start feature pipeline |
| `/bugfix <error>` | Start bugfix pipeline |
| `/approve` | Approve spec → enable implementation |
| `/complete` | Finalize pipeline |
| `/resume` | Resume active pipeline |
| `/checkpoint` | Save insights to memory |

### Validation

| Command | Purpose |
|---------|---------|
| `/validate` | Run build + type-check |
| `/status` | Show pipeline, git, registry status |
| `/scan` | Project reconnaissance |

### Sync

| Command | Purpose |
|---------|---------|
| `/sync-registry` | Update entity registry |
| `/sync-context` | Reload project context |
| `/compile-context` | Recompile agent contexts |

### Git

| Command | Purpose |
|---------|---------|
| `/commit` | Simple commit |
| `/commit-push` | Commit and push |
| `/merge-main` | Merge to main |

### Delegation (L0)

| Command | Purpose |
|---------|---------|
| `/task-analyze` | Explore via Task(Explore) |
| `/task-review` | Review via Task(general-purpose) |
| `/task-refactor` | Plan → Implement via Tasks |
| `/task-docs` | Documentation via Task |

---

## ENFORCEMENT HOOKS

These hooks run automatically and enforce rules:

| Hook | Triggers On | Behavior |
|------|-------------|----------|
| `enforce-registry.js` | `/feature`, `/bugfix` | Blocks if registry invalid |
| `enforce-context.js` | `/feature`, `/bugfix` | Blocks if contexts not compiled |
| `enforce-grepai.js` | `Grep`, `Glob` | Blocks, suggests grepai |
| `enforce-pipeline.js` | `Edit`, `Write` | Blocks if not in implement phase |

---

## DECISION FLOWCHART

```
User Request
    │
    ▼
Is it a code change request?
    │
    ├── NO → Answer freely, explore if needed
    │
    └── YES
         │
         ▼
    Is it a bug?
         │
         ├── YES → Invoke mustard:bugfix
         │
         └── NO → Invoke mustard:feature
              │
              ▼
         Wait for skill to complete
              │
              ▼
         Present results to user
```

---

## MEMORY MCP ENTITIES

Pipeline state is persisted in memory MCP:

| Entity | Purpose |
|--------|---------|
| `Pipeline:{name}` | Current pipeline state and phase |
| `Spec:{name}` | Approved specification |
| `Checkpoint:{...}` | Phase insights (temporary) |
| `Learning:{...}` | Permanent learnings |

---

## QUICK REFERENCE: Naming

```
Entities:     PascalCase singular     → Contract, User
DB Tables:    snake_case plural       → contracts, users
Endpoints:    /api/kebab-case         → /api/contracts
Components:   PascalCase.tsx          → ContractForm.tsx
Hooks:        use + camelCase         → useContracts.ts
```

---

## LINKS

- [Pipeline Details](./core/pipeline.md)
- [Enforcement Rules](./core/enforcement.md)
- [Entity Registry Spec](./core/entity-registry-spec.md)
- [Prompts Index](./prompts/_index.md)
