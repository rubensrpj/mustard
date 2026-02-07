# CLAUDE.md

Instructions for Claude Code when working with this repository.

## Project

Mustard is a CLI that generates `.claude/` folders for Claude Code projects. It creates prompts, commands, hooks, and rules.

**Key concepts:**

- "Agents" are prompts loaded into `Task(general-purpose)` - custom subagent types don't work
- Only 4 native `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`
- Enforcement via JavaScript hooks (`PreToolUse` with `Skill` matcher)
- **Universal Delegation**: All code activities must be delegated via Task (separate context)
- **Modular Context**: Each agent has `README.md` + `{agent}.core.md` with explicit identity
- **Auto-sync Scripts**: `sync-detect.js`, `sync-compile.js`, `sync-registry.js`
- **Namespaced Commands**: All commands use `mustard:` prefix (e.g., `/mustard:feature`)

## L0 Rule - Universal Delegation

**CRITICAL:** The parent context (main) serves ONLY for:

- Receiving user requests
- Coordinating delegations via Task tool
- Presenting final results

**ALL** activities involving code MUST be delegated:

| Activity | Task Type |
|----------|-----------|
| Code exploration | `Task(Explore)` |
| Planning | `Task(Plan)` |
| Backend/APIs | `Task(general-purpose)` |
| Frontend/UI | `Task(general-purpose)` |
| Database | `Task(general-purpose)` |
| Bugfix | `Task(general-purpose)` |
| Code Review | `Task(general-purpose)` |
| Documentation | `Task(general-purpose)` |

## Build & Run

```bash
npm install
npm run build
npm test

# Initialize a project
node bin/mustard.js init

# Update existing project
node bin/mustard.js update
```

## Structure

```text
mustard/
├── bin/mustard.js           # CLI entry point
├── src/                     # TypeScript source
│   ├── commands/            # init.ts, update.ts
│   ├── scanners/            # stack.ts, structure.ts, dependencies.ts
│   ├── analyzers/           # semantic.ts, llm.ts
│   ├── generators/          # claude-md, prompts, commands, hooks, registry
│   └── services/            # ollama.ts, grepai.ts
├── dist/                    # Compiled JavaScript
└── templates/               # Templates (copied to target .claude/)
    ├── CLAUDE.md            # Minimal orchestrator rules
    ├── prompts/             # Stub prompts (reference .core.md)
    ├── context/             # Modular context per agent
    │   ├── shared/          # Common context (all agents)
    │   ├── backend/         # README.md + backend.core.md
    │   ├── frontend/        # README.md + frontend.core.md
    │   ├── database/        # README.md + database.core.md
    │   ├── bugfix/          # README.md + bugfix.core.md
    │   ├── review/          # README.md + review.core.md
    │   └── orchestrator/    # README.md + orchestrator.core.md
    ├── commands/mustard/    # Pipeline commands (namespaced)
    ├── scripts/             # Sync scripts
    ├── core/                # Enforcement rules
    └── hooks/               # JavaScript hooks
```

## Context Architecture (v3.0)

Each agent has **modular context** with explicit identity:

```text
context/{agent}/
├── README.md        # Extensibility guide (how to add custom context)
└── {agent}.core.md  # Identity + Responsibilities + Workflow + Return Format
```

### .core.md Structure

| Section | Purpose |
|---------|---------|
| **Identity** | "You are the Backend Specialist" |
| **Responsibilities** | What the agent implements/doesn't implement |
| **Prerequisites** | Validations before accepting work |
| **Checklist** | Step-by-step workflow |
| **Return Format** | Standardized response format |
| **Naming Conventions** | PascalCase, snake_case, kebab-case rules |
| **Rules** | Explicit DO/DO NOT |

### Sync Flow

1. User invokes `/mustard:feature` or `/mustard:bugfix`
2. `sync-detect.js` discovers subprojects (monorepo)
3. `sync-compile.js` compiles contexts with SHA256 caching
4. Agent receives compiled `{agent}.context.md`
5. Skip recompilation if content hash unchanged

## CLI Flow

```text
mustard init
    -> scanProject() - detect stacks
    -> semanticAnalyzer() - grepai patterns (optional)
    -> llmAnalyzer() - Ollama analysis (optional)
    -> generateAll() - create .claude/ files + context structure

mustard update
    -> backup existing .claude/
    -> regenerate core files only
    -> preserve: CLAUDE.md, prompts/, context/*.md (user files)
```

## Prompts (Agents)

| Prompt | Model | Context |
|--------|-------|---------|
| orchestrator | opus | orchestrator.core.md |
| backend | opus | backend.core.md |
| frontend | opus | frontend.core.md |
| database | opus | database.core.md |
| bugfix | opus | bugfix.core.md |
| review | opus | review.core.md |

## Commands

### Pipeline

- `/mustard:feature` - Start feature pipeline
- `/mustard:bugfix` - Start bugfix pipeline
- `/mustard:approve` - Approve spec
- `/mustard:complete` - Finalize pipeline
- `/mustard:resume` - Resume active pipeline

### Task (L0 Delegation)

- `/mustard:task-analyze` - Code analysis via Task(Explore)
- `/mustard:task-review` - Code review via Task(general-purpose)
- `/mustard:task-refactor` - Refactoring via Task(Plan) -> Task(general-purpose)
- `/mustard:task-docs` - Documentation via Task(general-purpose)

### Git

- `/mustard:commit` - Simple commit
- `/mustard:commit-push` - Commit and push
- `/mustard:merge-main` - Merge to main

### Sync

- `/mustard:sync-registry` - Update entity registry
- `/mustard:sync-context` - Compile agent contexts
- `/mustard:validate` - Build + type-check
- `/mustard:status` - Project status

## Enforcement Hooks

Hooks are registered in `templates/settings.json`:

| Hook | Matcher | Behavior |
|------|---------|----------|
| `enforce-registry.js` | `Skill` | **BLOCKS** if registry missing |
| `enforce-context.js` | `Skill` | **WARNS** (advisory) |
| `enforce-grepai.js` | `Grep/Glob` | **BLOCKS** search without path |
| `enforce-pipeline.js` | `Edit/Write` | **REMINDS** about pipeline |

### Pre-Pipeline Validation Flow

```text
User: /mustard:feature add-login
         │
         ▼
    enforce-registry.js
    - Registry exists? (BLOCK if not)
    - Version >= 3.x? (BLOCK if not)
         │
         ▼
    enforce-context.js
    - Contexts compiled? (WARN if not)
         │
         ▼
    Pipeline starts...
```

## Sync Scripts

### sync-detect.js

Auto-discovers subprojects in monorepos:

- Detection patterns: `.NET`, `React`, `Drizzle`, etc.
- Output: JSON with subprojects, agents, paths

### sync-compile.js

Compiles contexts with git-aware caching:

1. Copies subproject commands to `context/{agent}/cmd-{file}`
2. Concatenates `.md` files → `{agent}.context.md`
3. Computes SHA256 hash
4. Skips if hash unchanged

### sync-registry.js

Generates `entity-registry.json` v3.1:

- Scans Drizzle schemas (`pgTable`, `pgEnum`)
- Scans .NET entities (`DbSet`, `class T`)
- Outputs `_patterns`, `_enums`, entity refs/subs

## Stacks Detected

TypeScript/JS, C#, Python, Java, Go, Rust, React, Next.js, .NET, FastAPI, Django, Drizzle, Prisma, TypeORM
