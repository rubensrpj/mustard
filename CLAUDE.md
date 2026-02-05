# CLAUDE.md

Instructions for Claude Code when working with this repository.

## Project

Mustard is a CLI that generates `.claude/` folders for Claude Code projects. It creates prompts, commands, hooks, and rules.

**Key concepts:**
- "Agents" are prompts loaded into `Task(general-purpose)` - custom subagent types don't work
- Only 4 native `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`
- Enforcement via JavaScript hooks

## Build & Run

```bash
cd cli && npm install
cd cli && npm run build
cd cli && npm test

# Initialize a project
node cli/bin/mustard.js init

# Update existing project
node cli/bin/mustard.js update

# Sync prompts/context with current code
node cli/bin/mustard.js sync
```

## Structure

```
mustard/
├── cli/
│   ├── bin/mustard.js
│   └── src/
│       ├── commands/     # init.ts, update.ts, sync.ts
│       ├── scanners/     # stack.ts, structure.ts, dependencies.ts
│       ├── analyzers/    # semantic.ts, llm.ts
│       ├── generators/   # claude-md, prompts, commands, hooks, registry
│       └── services/     # ollama.ts, grepai.ts
│
└── claude/               # Templates (copied to target .claude/)
    ├── CLAUDE.md
    ├── prompts/          # 8 agent prompts
    ├── commands/mustard/ # Pipeline commands
    ├── core/             # Enforcement rules
    ├── hooks/            # enforce-grepai.js, enforce-pipeline.js
    └── scripts/          # statusline.js
```

## CLI Flow

```
mustard init
    → scanProject() - detect stacks
    → semanticAnalyzer() - grepai patterns (optional)
    → llmAnalyzer() - Ollama analysis (optional)
    → generateAll() - create .claude/ files

mustard update
    → backup existing .claude/
    → regenerate core files only
    → preserve: CLAUDE.md, prompts/, context/, docs/

mustard sync
    → scanProject() - re-detect stacks
    → semanticAnalyzer() - discover entities
    → merge prompts (auto section only)
    → regenerate context/, entity-registry.json
```

## Prompts (Agents)

| Prompt | Model | Purpose |
|--------|-------|---------|
| orchestrator | opus | Coordinates pipeline |
| backend | opus | APIs, services |
| frontend | opus | Components, hooks |
| database | opus | Schema, migrations |
| bugfix | opus | Bug analysis |
| review | opus | QA, SOLID |
| report | sonnet | Commit reports |
| naming | - | Conventions reference |

## Stacks Detected

TypeScript/JS, C#, Python, Java, Go, Rust, React, Next.js, .NET, FastAPI, Django, Drizzle, Prisma, TypeORM
