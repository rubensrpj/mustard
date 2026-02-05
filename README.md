<p align="center">
  <img src="assets/logo.svg" width="120" alt="Mustard">
</p>

<h1 align="center">Mustard</h1>

<p align="center">
  <em>The perfect sauce for your Claude Code</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-3.1.0-yellow?style=for-the-badge" alt="Version">
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

- **8 specialized prompts** for `Task(general-purpose)` delegation
- **Pipeline commands** for features and bugfixes
- **Enforcement hooks** (grepai, pipeline confirmation)
- **Stack detection** and auto-generated CLAUDE.md

## Quick Start

```bash
cd my-project
node path/to/mustard/cli/bin/mustard.js init
```

The CLI will:
1. Detect stacks (React, .NET, Python, etc.)
2. Analyze code with Ollama (optional)
3. Generate `.claude/` structure

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
| `core/*.md` | `context/*` |
| `scripts/*.js` | `docs/*` |

### `mustard sync`

Syncs prompts and context with current codebase state. Uses markers to preserve user customizations.

```bash
mustard sync [options]

Options:
  --prompts      Only sync prompts
  --context      Only sync context files
  --registry     Only sync entity registry
  --no-ollama    Skip LLM analysis
  --no-grepai    Skip semantic analysis
  -f, --force    Skip confirmation
  -v, --verbose  Detailed output
```

| Synced | Preserved |
|--------|-----------|
| `prompts/*.md` (auto section) | User content in prompts |
| `context/*.md` | `CLAUDE.md` |
| `entity-registry.json` | `commands/*` |

## Structure

```
mustard/
├── cli/
│   ├── bin/mustard.js
│   └── src/
│       ├── commands/       # init, update, sync
│       ├── scanners/       # stack, structure, dependencies
│       ├── analyzers/      # semantic, llm
│       ├── generators/     # claude-md, prompts, commands, hooks
│       └── services/       # ollama, grepai
│
└── claude/                 # Templates (copied to .claude/)
    ├── CLAUDE.md
    ├── prompts/            # 8 agent prompts
    ├── commands/mustard/   # Pipeline commands
    ├── core/               # Enforcement, pipeline rules
    ├── hooks/              # enforce-grepai.js, enforce-pipeline.js
    └── scripts/            # statusline.js
```

## Prompts

Claude Code only accepts 4 `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`.

Mustard "agents" are prompts loaded into `Task(general-purpose)`:

| Prompt | Model | Purpose |
|--------|-------|---------|
| orchestrator | opus | Coordinates pipelines |
| backend | opus | APIs, services |
| frontend | opus | Components, hooks |
| database | opus | Schema, migrations |
| bugfix | opus | Bug analysis and fix |
| review | opus | QA, SOLID validation |
| report | sonnet | Commit reports |
| naming | - | Naming conventions reference |

## Pipeline Commands

| Command | Description |
|---------|-------------|
| `/mtd-pipeline-feature` | Start feature pipeline |
| `/mtd-pipeline-bugfix` | Start bugfix pipeline |
| `/mtd-pipeline-approve` | Approve spec |
| `/mtd-pipeline-complete` | Finalize |
| `/mtd-pipeline-resume` | Resume active pipeline |

### Task Commands (L0 Universal Delegation)

| Command | Description |
|---------|-------------|
| `/mtd-task-analyze` | 🔍 Code analysis via Task(Explore) |
| `/mtd-task-review` | 🔎 Code review via Task(general-purpose) |
| `/mtd-task-refactor` | 📋⚙️ Refactoring via Task(Plan) → Task(general-purpose) |
| `/mtd-task-docs` | 📊 Documentation via Task(general-purpose) |

### Other Commands

| Command | Description |
|---------|-------------|
| `/mtd-validate-build` | Build + type-check |
| `/mtd-validate-status` | Project status |
| `/mtd-git-commit` | Simple commit |
| `/mtd-git-push` | Commit and push |
| `/mtd-sync-registry` | Update entity registry |

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
cd cli
npm install
npm run build
npm test
```

## License

MIT
