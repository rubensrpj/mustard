# CLAUDE.md

Instructions for Claude Code when working with this repository.

## Project

Mustard is a CLI that generates `.claude/` folders for Claude Code projects. It creates prompts, commands, hooks, and rules.

**Key concepts:**

- "Agents" are prompts loaded into `Task(general-purpose)` - custom subagent types don't work
- Only 4 native `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`
- Enforcement via JavaScript hooks
- **Universal Delegation**: All code activities must be delegated via Task (separate context)
- **Context per Agent**: Each agent loads context from `context/shared/` + `context/{agent}/`
- **Compiled context at skill invocation**: `/feature` and `/bugfix` commands compile contexts before starting
- **Agent Teams** (experimental): Alternative to Task subagents for complex multi-layer features

## L0 Rule - Universal Delegation

**CRITICAL:** The parent context (main) serves ONLY for:

- Receiving user requests
- Coordinating delegations via Task tool
- Presenting final results

**ALL** activities involving code MUST be delegated:

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
    ├── CLAUDE.md
    ├── prompts/             # 8 agent prompts (agnostic)
    ├── context/             # Context files per agent
    │   ├── shared/          # Common context (all agents)
    │   ├── backend/         # Backend-specific patterns
    │   ├── frontend/        # Frontend-specific patterns
    │   ├── database/        # Database-specific patterns
    │   └── ...
    ├── commands/mustard/    # Pipeline commands
    ├── core/                # Enforcement rules
    ├── hooks/               # enforce-grepai.js, enforce-pipeline.js
    └── scripts/             # statusline.js
```

## Context per Agent (v2.5)

Prompts are **agnostic** - project-specific patterns live in context files:

```text
context/
├── shared/       # All agents load this
├── backend/      # Only Backend Specialist loads
├── frontend/     # Only Frontend Specialist loads
├── database/     # Only Database Specialist loads
├── bugfix/       # Only Bugfix Specialist loads
├── review/       # Only Review Specialist loads
├── orchestrator/ # Only Orchestrator loads
└── team-lead/    # Only Team Lead loads (Agent Teams mode)
```

**Flow:**

1. User invokes `/feature` or `/bugfix` skill
2. Skill compiles contexts for all agents (git-based caching)
3. Agent is called with compiled context ready
4. Compiled context saved to `prompts/{agent}.context.md`

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

| Prompt | Model | Context Folders |
|--------|-------|-----------------|
| team-lead | opus | shared + team-lead (Agent Teams) |
| orchestrator | opus | shared + orchestrator |
| backend | opus | shared + backend |
| frontend | opus | shared + frontend |
| database | opus | shared + database |
| bugfix | opus | shared + bugfix |
| review | opus | shared + review |
| report | sonnet | (uses git log) |
| naming | - | Reference only |

## Commands

### Pipeline (Task Mode)

- `/feature` - Start feature pipeline
- `/bugfix` - Start bugfix pipeline
- `/approve` - Approve spec
- `/complete` - Finalize pipeline
- `/resume` - Resume active pipeline

### Pipeline (Agent Teams Mode - Experimental)

- `/feature-team` - Feature pipeline with Agent Teams (parallel)
- `/bugfix-team` - Bugfix pipeline with competing hypotheses

### Task (L0 Delegation)

- `/task-analyze` - Code analysis via Task(Explore)
- `/task-review` - Code review via Task(general-purpose)
- `/task-refactor` - Refactoring via Task(Plan) -> Task(general-purpose)
- `/task-docs` - Documentation via Task(general-purpose)

## Stacks Detected

TypeScript/JS, C#, Python, Java, Go, Rust, React, Next.js, .NET, FastAPI, Django, Drizzle, Prisma, TypeORM
