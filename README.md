<p align="center">
  <img src="assets/logo.svg" width="120" alt="Mustard">
</p>

<h1 align="center">Mustard</h1>

<p align="center">
  <em>The perfect sauce for your Claude Code</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-3.0.0-yellow?style=for-the-badge" alt="Version">
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

- **6 specialist prompts** with explicit identity and workflow (`.core.md` files)
- **Modular context** - `README.md` + `{agent}.core.md` per agent
- **Auto-sync scripts** - git-aware context compilation with SHA256 caching
- **Pipeline commands** for features and bugfixes (namespaced: `/mustard:*`)
- **Enforcement hooks** (grepai, pipeline, registry validation)
- **Monorepo support** - auto-detection of subprojects

## What's New in v3.0

### Breaking Changes

- **Namespaced commands**: All commands now use `mustard:` prefix
  - `/feature` → `/mustard:feature`
  - `/bugfix` → `/mustard:bugfix`
  - `/commit` → `/mustard:commit`
  - etc.

### Removed Features

- **Agent Teams** (`/feature-team`, `/bugfix-team`) - experimental feature discontinued
- **Checkpoint** (`/checkpoint`) - replaced by Context Reset
- **Compile Context** (`/compile-context`) - now automatic via hooks

### New Architecture

- **Modular context**: `patterns.md` → `README.md` + `{agent}.core.md`
- **Explicit agent identity**: Each specialist has defined responsibilities and return format
- **Auto-sync scripts**: `sync-detect.js`, `sync-compile.js`, `sync-registry.js`
- **Simplified templates**: 90% reduction in template size (externalized to compiled context)
- **Hook modernization**: `UserPromptSubmit` → `PreToolUse` with `Skill` matcher

### New Features

- **Backend operational commands**: `backend-run`, `backend-stop`, `backend-restart`, `backend-logs`
- **Design Principles skill**: Jony Ive-level UI guidelines
- **Entity Registry v3.1**: Includes `_patterns` and `_enums`

## Installation

### Prerequisites

- **Node.js** >= 18.0.0
- **Package Manager**: npm, pnpm, yarn, or bun

### Install Mustard

#### Option 1: Global Installation

```bash
# npm
npm install -g mustard-claude

# pnpm
pnpm add -g mustard-claude

# yarn
yarn global add mustard-claude

# bun
bun add -g mustard-claude
```

#### Option 2: Run Without Installing

```bash
# npx (npm)
npx mustard-claude init

# pnpx (pnpm)
pnpx mustard-claude init

# yarn dlx
yarn dlx mustard-claude init

# bunx
bunx mustard-claude init
```

---

## Optional Dependencies

Mustard works without these tools, but they enhance functionality:

| Tool | Purpose | Required |
|------|---------|----------|
| [Ollama](https://ollama.com) | LLM analysis + grepai embeddings | No |
| [grepai](https://github.com/yoanbernabeu/grepai) | Semantic code search | No |
| [Memory MCP](https://github.com/doobidoo/mcp-memory-service) | Pipeline persistence | No |

### 1. Ollama Installation

Ollama provides local LLM capabilities. Required if you want:

- Personalized CLAUDE.md generation (`mustard init --ollama`)
- grepai semantic embeddings

#### macOS

Download and install from: [ollama.com/download/Ollama.dmg](https://ollama.com/download/Ollama.dmg)

#### Windows

Download and install from: [ollama.com/download/OllamaSetup.exe](https://ollama.com/download/OllamaSetup.exe)

#### Linux

```bash
curl -fsSL https://ollama.com/install.sh | sh
```

#### Docker

**CPU-only:**

```bash
docker run -d -v ollama:/root/.ollama -p 11434:11434 --name ollama ollama/ollama
```

**With NVIDIA GPU:**

```bash
# Install NVIDIA Container Toolkit first
sudo apt-get install -y nvidia-container-toolkit
sudo nvidia-ctk runtime configure --runtime=docker
sudo systemctl restart docker

# Run with GPU support
docker run -d --gpus=all -v ollama:/root/.ollama -p 11434:11434 --name ollama ollama/ollama
```

**With AMD GPU:**

```bash
docker run -d --device /dev/kfd --device /dev/dri -v ollama:/root/.ollama -p 11434:11434 --name ollama ollama/ollama:rocm
```

#### Pull Required Models

```bash
# For Mustard LLM analysis
ollama pull llama3.2

# For grepai embeddings (required if using grepai)
ollama pull nomic-embed-text
```

#### Verify Ollama Installation

```bash
ollama list
# Should show downloaded models
```

---

### 2. grepai Installation

grepai provides semantic code search. **Requires Ollama** for embeddings.

#### macOS (Homebrew)

```bash
brew install yoanbernabeu/tap/grepai
```

#### Linux/macOS (Script)

```bash
curl -sSL https://raw.githubusercontent.com/yoanbernabeu/grepai/main/install.sh | sh
```

#### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/yoanbernabeu/grepai/main/install.ps1 | iex
```

#### Setup grepai in Your Project

```bash
cd your-project

# Initialize (creates .grepai folder)
grepai init

# Start the indexing daemon (keeps index up-to-date)
grepai watch

# Test semantic search
grepai search "authentication flow"

# Trace function calls
grepai trace callers "Login"
```

---

### 3. Memory MCP Installation

Memory MCP provides persistent memory for Claude across sessions.

#### Install via pip

```bash
pip install mcp-memory-service
```

#### Quick Setup (Claude Desktop)

```bash
python -m mcp_memory_service.scripts.installation.install --quick
```

#### Manual Configuration

Add to Claude Desktop config:

- **macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows:** `%APPDATA%\Claude\claude_desktop_config.json`
- **Linux:** `~/.config/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "memory": {
      "command": "memory",
      "args": ["server"]
    }
  }
}
```

Restart Claude Desktop after configuration.

---

## Verify Installation

After installing dependencies, verify your setup:

```bash
# Check Node.js version
node --version

# Check Ollama (optional)
ollama list

# Check grepai (optional)
grepai --version

# Initialize Mustard
cd your-project
mustard init --ollama  # Use --ollama flag to enable LLM analysis
```

## System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| RAM | 8 GB | 16 GB |
| Storage | 10 GB | 20 GB |
| Node.js | 18.0.0 | 20+ |

### RAM for Ollama Models

| Model Size | RAM Required |
|------------|--------------|
| 7B params | 8 GB |
| 13B params | 16 GB |
| 33B params | 32 GB |

---

## Quick Start

```bash
cd my-project
mustard init
```

The CLI will:

1. Detect stacks (React, .NET, Python, etc.)
2. Analyze code with Ollama (optional)
3. Generate `.claude/` structure with modular context

## Context Architecture

Each agent has **modular context** with explicit identity:

```text
.claude/context/
├── shared/              # All agents load this
├── backend/
│   ├── README.md        # Extensibility guide
│   └── backend.core.md  # Identity + Responsibilities + Workflow
├── frontend/
│   ├── README.md
│   └── frontend.core.md
├── database/
│   ├── README.md
│   └── database.core.md
├── bugfix/
│   ├── README.md
│   └── bugfix.core.md
├── review/
│   ├── README.md
│   └── review.core.md
└── orchestrator/
    ├── README.md
    └── orchestrator.core.md
```

### `.core.md` Structure

Each specialist has explicit sections:

| Section | Purpose |
|---------|---------|
| **Identity** | "You are the Backend Specialist" |
| **Responsibilities** | What the agent implements/doesn't implement |
| **Prerequisites** | Validations before accepting work |
| **Checklist** | Step-by-step workflow |
| **Return Format** | Standardized response format |
| **Naming Conventions** | PascalCase, snake_case, kebab-case rules |
| **Rules** | Explicit DO/DO NOT |

### How Context Works

1. User invokes `/mustard:feature` or `/mustard:bugfix`
2. `sync-detect.js` discovers subprojects (monorepo)
3. `sync-compile.js` compiles contexts with SHA256 caching
4. Agent receives compiled `{agent}.context.md`
5. Skip recompilation if content hash unchanged

## Commands

### CLI Commands

```bash
mustard init [options]

Options:
  -f, --force      Overwrite existing .claude/
  -y, --yes        Skip confirmations
  --no-ollama      Skip LLM analysis
  --no-grepai      Skip semantic analysis
  -v, --verbose    Detailed output
```

```bash
mustard update [options]

Options:
  -f, --force          Skip backup
  --include-claude-md  Also update CLAUDE.md
```

### Pipeline Commands

| Command | Description |
|---------|-------------|
| `/mustard:feature <name>` | Start feature pipeline |
| `/mustard:bugfix <error>` | Start bugfix pipeline |
| `/mustard:approve` | Approve spec |
| `/mustard:complete` | Finalize pipeline |
| `/mustard:resume` | Resume active pipeline |

### Task Commands (L0 Delegation)

| Command | Description |
|---------|-------------|
| `/mustard:task-analyze` | Code analysis via Task(Explore) |
| `/mustard:task-review` | Code review via Task(general-purpose) |
| `/mustard:task-refactor` | Refactoring via Task(Plan) → Task(general-purpose) |
| `/mustard:task-docs` | Documentation via Task(general-purpose) |

### Git Commands

| Command | Description |
|---------|-------------|
| `/mustard:commit` | Simple commit |
| `/mustard:commit-push` | Commit and push |
| `/mustard:merge-main` | Merge to main |

### Sync Commands

| Command | Description |
|---------|-------------|
| `/mustard:sync-registry` | Update entity registry |
| `/mustard:sync-context` | Compile agent contexts |
| `/mustard:validate` | Build + type-check |
| `/mustard:status` | Project status |

## Structure

```text
.claude/
├── CLAUDE.md               # Minimal orchestrator rules
├── prompts/                # Stub prompts (reference .core.md)
│   ├── orchestrator.md
│   ├── backend.md
│   ├── frontend.md
│   ├── database.md
│   ├── bugfix.md
│   └── review.md
├── context/                # Modular context (editable)
│   ├── shared/
│   ├── backend/
│   │   ├── README.md
│   │   └── backend.core.md
│   ├── frontend/
│   ├── database/
│   ├── bugfix/
│   ├── review/
│   └── orchestrator/
├── commands/mustard/       # Pipeline commands
├── scripts/                # Sync scripts
│   ├── sync-detect.js
│   ├── sync-compile.js
│   └── sync-registry.js
├── core/                   # Enforcement rules
├── hooks/                  # JavaScript hooks
└── entity-registry.json    # Entity mappings v3.1
```

## Prompts (Agents)

Claude Code only accepts 4 `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`.

Mustard "agents" are prompts loaded into `Task(general-purpose)`:

| Prompt | Model | Context |
|--------|-------|---------|
| orchestrator | opus | orchestrator.core.md |
| backend | opus | backend.core.md |
| frontend | opus | frontend.core.md |
| database | opus | database.core.md |
| bugfix | opus | bugfix.core.md |
| review | opus | review.core.md |

## Sync Scripts

### sync-detect.js

Auto-discovers subprojects in monorepos:

```javascript
// Detection patterns
"backend": [/.NET/, /dotnet/, /FastEndpoints/]
"frontend": [/React/, /Next\.js/, /Vue/]
"database": [/Drizzle/, /Prisma/, /PostgreSQL/]
```

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
- Detects relationships and patterns
- Outputs `_patterns`, `_enums`, entity refs/subs

## Enforcement Hooks

| Hook | Matcher | Behavior |
|------|---------|----------|
| `enforce-registry.js` | `Skill` | **BLOCKS** if registry missing |
| `enforce-context.js` | `Skill` | **WARNS** (advisory) |
| `enforce-grepai.js` | `Grep/Glob` | **BLOCKS** search without path |
| `enforce-pipeline.js` | `Edit/Write` | **REMINDS** about pipeline |

### Pre-Pipeline Validation

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

## Migration from v2.x

1. **Update command invocations**:
   ```bash
   # Before
   /feature add-login

   # After
   /mustard:feature add-login
   ```

2. **Regenerate registry**:
   ```bash
   /mustard:sync-registry --force
   ```

3. **Recompile contexts**:
   ```bash
   /mustard:sync-context
   ```

4. **Note removed features**:
   - Agent Teams (`/feature-team`, `/bugfix-team`) - removed
   - Checkpoint (`/checkpoint`) - use Context Reset instead

## Supported Stacks

| Language | Frameworks |
|----------|------------|
| TypeScript/JS | React, Next.js, Node, Express |
| C# | .NET, ASP.NET Core, FastEndpoints |
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
| **Memory MCP** | Pipeline persistence |

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
