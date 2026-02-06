<p align="center">
  <img src="assets/logo.svg" width="120" alt="Mustard">
</p>

<h1 align="center">Mustard</h1>

<p align="center">
  <em>The perfect sauce for your Claude Code</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-2.5.0-yellow?style=for-the-badge" alt="Version">
  <img src="https://img.shields.io/badge/node-%3E%3D18-green?style=for-the-badge&logo=node.js" alt="Node">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="License">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Claude_Code-Ready-blueviolet?style=flat-square&logo=anthropic" alt="Claude Code">
  <img src="https://img.shields.io/badge/.NET-supported-512BD4?style=flat-square&logo=dotnet" alt=".NET">
  <img src="https://img.shields.io/badge/React-supported-61DAFB?style=flat-square&logo=react" alt="React">
  <img src="https://img.shields.io/badge/Python-supported-3776AB?style=flat-square&logo=python" alt="Python">
</p>

---

## What is Mustard?

Mustard generates a `.claude/` folder with prompts, commands, and rules for Claude Code:

- **8 agnostic prompts** for `Task(general-purpose)` delegation
- **Context per agent** - each agent loads its specific context folder
- **Auto-compiled context** - agents verify git and compile context on-demand
- **Pipeline commands** for features and bugfixes
- **Enforcement hooks** (grepai, pipeline confirmation)
- **Stack detection** and auto-generated CLAUDE.md

## What's New in v2.5

- **Agent Teams support** (experimental): True parallel execution for complex features
  - `/feature-team` and `/bugfix-team` commands
  - Team Lead coordinates Database, Backend, Frontend, and Review teammates
- **Mandatory Pipeline Invocation**: Skills compile contexts before starting
- **Simplified agent prompts**: Context loading moved to skill commands

## Installation

### Global Installation

```bash
# Using npm
npm install -g mustard-claude

# Using pnpm
pnpm add -g mustard-claude
```

### Run Without Installing

```bash
# Using npx
npx mustard-claude init

# Using pnpx
pnpx mustard-claude init
```

## Quick Start

```bash
cd my-project
mustard init
```

The CLI will:

1. Detect stacks (React, .NET, Python, etc.)
2. Analyze code with Ollama (optional)
3. Generate `.claude/` structure with context folders

## Context per Agent

Prompts are **agnostic** - they don't contain project-specific code. Instead, each agent loads context from dedicated folders:

```text
.claude/context/
├── shared/       # All agents load this
├── backend/      # Only Backend Specialist loads
├── frontend/     # Only Frontend Specialist loads
├── database/     # Only Database Specialist loads
├── bugfix/       # Only Bugfix Specialist loads
├── review/       # Only Review Specialist loads
└── orchestrator/ # Only Orchestrator loads
```

**How it works (v2.5):**

1. User invokes `/feature` or `/bugfix` skill
2. Skill compiles contexts for all agents (git-based caching)
3. Agent is called with compiled context ready
4. Compiled context saved to `prompts/{agent}.context.md`

**Benefits:**

- Prompts work for any stack
- Easy to customize per project
- Clear separation: agent logic vs. project patterns
- Automatic recompilation when context changes

## Commands

### `mustard init`

```bash
mustard init [options]

Options:
  -f, --force      Overwrite existing .claude/
  -y, --yes        Skip confirmations
  --no-ollama      Skip LLM analysis
  --no-grepai      Skip semantic analysis
  -v, --verbose    Detailed output
```

### `mustard update`

Updates core files while preserving customizations.

```bash
mustard update [options]

Options:
  -f, --force          Skip backup
  --include-claude-md  Also update CLAUDE.md
```

| Updated | Preserved |
|---------|-----------|
| `commands/mustard/*.md` | `CLAUDE.md` |
| `hooks/*.js` | `prompts/*.md` |
| `core/*.md` | `context/**/*.md` (user files) |
| `scripts/*.js` | `docs/*` |

## Structure

```text
.claude/
├── CLAUDE.md               # Project instructions
├── prompts/                # 8 agnostic agent prompts
│   ├── orchestrator.md
│   ├── orchestrator.context.md  # Auto-compiled context
│   ├── backend.md
│   ├── backend.context.md       # Auto-compiled context
│   ├── frontend.md
│   ├── database.md
│   ├── bugfix.md
│   ├── review.md
│   ├── report.md
│   └── naming.md
├── context/                # Context source files (editable)
│   ├── shared/             # Common (all agents)
│   │   └── conventions.md
│   ├── backend/            # Backend-specific
│   │   └── patterns.md
│   ├── frontend/           # Frontend-specific
│   │   └── patterns.md
│   └── database/           # Database-specific
│       └── patterns.md
├── commands/mustard/       # Pipeline commands
├── core/                   # Enforcement, pipeline rules
├── hooks/                  # JavaScript hooks
└── entity-registry.json    # Entity mappings
```

## Prompts

Claude Code only accepts 4 `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`.

Mustard "agents" are prompts loaded into `Task(general-purpose)`:

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
| naming | - | Naming conventions reference |

## Pipeline Commands (Task Mode)

| Command | Description |
|---------|-------------|
| `/feature` | Start feature pipeline |
| `/bugfix` | Start bugfix pipeline |
| `/approve` | Approve spec |
| `/complete` | Finalize |
| `/resume` | Resume active pipeline |

### Agent Teams Mode (Experimental)

| Command | Description |
|---------|-------------|
| `/feature-team` | Feature pipeline with parallel teammates |
| `/bugfix-team` | Bugfix pipeline with competing hypotheses |

Agent Teams require `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` in `.claude/settings.json`.

### Task Commands (L0 Universal Delegation)

| Command | Description |
|---------|-------------|
| `/task-analyze` | Code analysis via Task(Explore) |
| `/task-review` | Code review via Task(general-purpose) |
| `/task-refactor` | Refactoring via Task(Plan) -> Task(general-purpose) |
| `/task-docs` | Documentation via Task(general-purpose) |

### Other Commands

| Command | Description |
|---------|-------------|
| `/validate` | Build + type-check |
| `/status` | Project status |
| `/commit` | Simple commit |
| `/commit-push` | Commit and push |
| `/sync-registry` | Update entity registry |

## Enforcement Hooks

| Hook | Trigger | Action |
|------|---------|--------|
| `enforce-grepai.js` | Grep, Glob | Blocks (suggests grepai) |
| `enforce-pipeline.js` | Edit, Write | **Hybrid mode**: Blocks source code, allows configs |

### L0 Universal Delegation

All code activities MUST be delegated via Task tool (separate context window).
The parent context only coordinates and presents results.

## Supported Stacks

| Language | Frameworks |
|----------|------------|
| TypeScript/JS | React, Next.js, Node, Express |
| C# | .NET, ASP.NET Core |
| Python | FastAPI, Django, Flask |
| Java | Spring Boot |
| Go | Gin, Echo |
| Rust | Actix, Axum |
| ORMs | Drizzle, Prisma, TypeORM |

## Optional Dependencies

| Tool | Purpose |
|------|---------|
| **Ollama** | LLM-generated CLAUDE.md |
| **grepai** | Semantic code search |
| **memory MCP** | Pipeline persistence |

Without these, the CLI uses default templates.

## Development

```bash
npm install
npm run build
npm test

# Run locally without installing
node bin/mustard.js init
```

## Publishing

```bash
npm version patch   # or minor/major
npm publish
```

## License

MIT
