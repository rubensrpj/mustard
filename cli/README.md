# Mustard CLI v2.0

Framework-agnostic CLI for setting up and updating Claude Code projects.

## Features

- **Stack Detection**: Automatically detects languages and frameworks
- **Semantic Analysis**: Uses grepai for intelligent code understanding
- **LLM-Powered**: Leverages Ollama for context-aware file generation
- **Template Fallback**: Works without external dependencies
- **Safe Updates**: Update core files without losing customizations

## Installation

### Global Installation

Install globally to use the `mustard` command anywhere:

```bash
# Using npm
npm install -g mustard-claude

# Using pnpm
pnpm add -g mustard-claude
```

### Run Without Installing

Use `npx` or `pnpx` to run without a global installation:

```bash
# Using npx (npm)
npx mustard-claude init
npx mustard-claude update
npx mustard-claude sync

# Using pnpx (pnpm)
pnpx mustard-claude init
pnpx mustard-claude update
pnpx mustard-claude sync
```

## Commands

### `mustard init` - Initialize New Project

Creates a complete `.claude/` structure for your project.

```bash
# Navigate to your project
cd my-project

# Initialize .claude/ structure
mustard init

# With options
mustard init --force       # Overwrite existing .claude/
mustard init --yes         # Skip confirmation prompts
mustard init --no-ollama   # Skip Ollama (use templates)
mustard init --no-grepai   # Skip semantic analysis
mustard init --verbose     # Show detailed output
```

### `mustard update` - Update Existing Project

Updates Mustard core files while preserving your customizations.

```bash
# Update core files (preserves CLAUDE.md and context/)
mustard update

# With options
mustard update --force          # Skip backup and confirmation
mustard update --include-claude-md  # Also update CLAUDE.md
mustard update --no-ollama      # Skip Ollama analysis
mustard update --no-grepai      # Skip semantic analysis
mustard update --verbose        # Show detailed output
```

#### What Gets Updated vs Preserved

| Updated (Core) | Preserved (Client) |
|----------------|-------------------|
| `commands/*.md` | `CLAUDE.md` |
| `prompts/*.md` | `context/*.md` (except README) |
| `hooks/*.js` | `context/examples/*` |
| `core/*.md` | `docs/*` |
| `scripts/*.js` | |
| `settings.json` (merged) | |
| `entity-registry.json` | |

## What It Does

1. **Scans** your project to detect:
   - Languages (TypeScript, C#, Python, Java, Go, Rust)
   - Frameworks (React, Next.js, .NET, FastAPI, etc.)
   - Architecture patterns (MVC, Clean, Feature-based)
   - Naming conventions

2. **Analyzes** code semantically (with grepai):
   - Finds services, repositories, endpoints
   - Discovers entities/models
   - Maps call graphs

3. **Generates** a customized `.claude/` structure:
   - `CLAUDE.md` - Main instructions file
   - `prompts/` - Specialized agent prompts
   - `commands/` - Pipeline commands
   - `hooks/` - Enforcement hooks
   - `entity-registry.json` - Entity mapping

## Supported Stacks

| Language | Frameworks |
|----------|------------|
| TypeScript/JavaScript | React, Next.js, Node, Express |
| C# | .NET, ASP.NET Core, FastEndpoints |
| Python | FastAPI, Django, Flask |
| Java | Spring Boot, Quarkus |
| Go | Gin, Echo, Fiber |
| Rust | Actix, Axum |

## Dependencies

### Required

- Node.js 18+

### Optional (Enhanced Features)

- **Ollama** - For LLM-powered analysis and generation
  - Install: https://ollama.ai
  - Run: `ollama pull llama3.2`

- **grepai** - For semantic code search
  - Provides better entity discovery
  - Enables call graph tracing

## Generated Structure

```text
.claude/
├── CLAUDE.md           # Main instructions
├── entity-registry.json # Entity mapping
├── settings.json       # Claude Code settings
├── prompts/
│   ├── _index.md
│   ├── orchestrator.md
│   ├── backend.md      # If backend detected
│   ├── frontend.md     # If frontend detected
│   ├── database.md     # If ORM detected
│   ├── bugfix.md
│   └── review.md
├── commands/
│   ├── feature.md
│   ├── bugfix.md
│   ├── approve.md
│   ├── complete.md
│   ├── resume.md
│   ├── commit.md
│   ├── commit-push.md
│   ├── validate.md
│   ├── status.md
│   ├── sync-registry.md
│   └── install-deps.md
├── hooks/
│   ├── enforce-pipeline.js
│   └── enforce-grepai.js
├── core/
│   ├── enforcement.md
│   ├── naming-conventions.md
│   └── pipeline.md
├── scripts/
│   └── statusline.js
└── context/
    ├── README.md
    ├── architecture.md
    ├── patterns.md
    ├── naming.md
    └── examples/
```

## Commands After Setup

| Command | Description |
|---------|-------------|
| `/feature <name>` | Start a new feature pipeline |
| `/bugfix <error>` | Start a bugfix pipeline |
| `/approve` | Approve spec for implementation |
| `/complete` | Complete current pipeline |
| `/commit` | Create a commit |
| `/validate` | Run build/type-check |
| `/status` | Show project status |

## Development

```bash
# Clone
git clone <repo>
cd mustard/cli

# Install dependencies
npm install

# Build
npm run build

# Run locally
node bin/mustard.js init
node bin/mustard.js update

# Test with npx
npx . init
```

## License

MIT
