<p align="center">
  <img src="assets/logo.svg" width="120" alt="Mustard">
</p>

<h1 align="center">Mustard</h1>

<p align="center">
  <em>The perfect sauce for your Claude Code</em>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/mustard-claude"><img src="https://img.shields.io/npm/v/mustard-claude?style=for-the-badge&color=yellow&label=npm" alt="npm"></a>
  <img src="https://img.shields.io/badge/node-%3E%3D18-green?style=for-the-badge&logo=node.js" alt="Node">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="License">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Claude_Code-Ready-blueviolet?style=flat-square&logo=anthropic" alt="Claude Code">
  <img src="https://img.shields.io/badge/Monorepo-supported-green?style=flat-square" alt="Monorepo">
  <img src="https://img.shields.io/badge/Single_Repo-supported-green?style=flat-square" alt="Single Repo">
</p>

---

## What is Mustard?

Mustard sets up a `.claude/` folder that turns Claude Code into a structured development pipeline:

- **15 pipeline skills** — feature, bugfix, scan, resume, approve, complete, git, maint, task, knowledge, skill, status, scan-format, agent-prompt template, stats
- **14 enforcement hooks** — bash safety, file guard, registry validation, guard verification, auto-format, pre-compact, session cleanup, subagent tracking, RTK rewrite, session memory, review gate, metrics tracker, MCP budget, session knowledge
- **6 bundled skills** — design-craft, react-best-practices, senior-architect, skill-creator, commit-workflow, pipeline-execution
- **10 utility scripts** — subproject detection, entity registry sync, statusline, memory persistence, diff context, knowledge base, metrics collection, security scan, pipeline verification
- **Token economy** — auto-installs [RTK](https://github.com/rtk-ai/rtk) to reduce token consumption by 60-90% on CLI outputs
- **Hook profiles** — minimal/standard/strict profiles via `_lib/hook-env.js`, env-based hook disabling
- **Cursor IDE adapter** (experimental) — `mustard init --cursor` installs a Cursor-compatible hook adapter
- **Monorepo + single repo** — works with any project structure

## Quick Start

```bash
# Install globally
npm install -g mustard-claude

# Initialize your project
cd my-project
mustard init

# Open Claude Code and run /scan
```

That's it. After `/scan`, use `/feature`, `/bugfix`, `/task` to work through structured pipelines.

## Installation

### Prerequisites

- **Node.js** >= 18.0.0
- **Claude Code** CLI or IDE extension
- **RTK** (auto-installed) — [Rust Token Killer](https://github.com/rtk-ai/rtk) for token economy

### Option 1: Global Install (recommended)

```bash
npm install -g mustard-claude
```

After install, the `mustard` command is available globally:

```bash
mustard init
mustard update
mustard auto-update
```

### Option 2: Run Without Installing

```bash
npx mustard-claude init
npx mustard-claude update
npx mustard-claude auto-update
```

### Verify Installation

```bash
mustard --version
```

## How It Works

1. `mustard init` copies the `.claude/` structure into your project
2. RTK is auto-installed for token economy (60-90% savings on CLI outputs)
3. Inside Claude Code, run `/scan` to analyze your codebase
4. `/scan` generates guards, recipes, patterns, agents, and skills specific to your project
5. Use `/feature`, `/bugfix`, `/task` to work through structured pipelines

The CLI is a **one-time setup tool**. All intelligence lives in the skills and hooks inside `.claude/`.

## CLI Commands

| Command | Description |
|---------|-------------|
| `mustard init` | Copy `.claude/` structure into current project |
| `mustard update` | Update core files (preserves user customizations) |
| `mustard auto-update` | Check npm for newer version and install |
| `mustard add <template>` | Install a community template |
| `mustard review --pr <N>` | Review a pull request (local or CI mode) |
| `mustard --version` | Show installed version |
| `mustard --help` | Show help |

### `mustard init`

| Option | Description |
|--------|-------------|
| `-f, --force` | Overwrite existing `.claude/` without backup |
| `-y, --yes` | Skip confirmation prompts (merge mode: skip existing files) |
| `--cursor` | Install Cursor IDE adapter at `.cursor/hooks/adapter.js` |

**Behavior:**
- If `.claude/` doesn't exist → copies all templates
- If `.claude/` exists → asks: backup & overwrite, merge (skip existing), or cancel
- Merge mode preserves all existing files and only adds new ones
- Auto-installs RTK if not present (silent, never blocks on failure)

### `mustard update`

| Option | Description |
|--------|-------------|
| `-f, --force` | Skip backup and confirmation |

**Recreates** (from latest templates):
- `commands/mustard/` — pipeline skills
- `hooks/` — enforcement hooks
- `skills/` — bundled skills
- `scripts/` — sync scripts
- `settings.json` — hook configuration

**Preserves** (user customizations):
- `CLAUDE.md` — orchestrator rules (populated by `/scan`)
- `pipeline-config.md` — agent dispatch config (populated by `/scan`)
- `entity-registry.json` — entity map (populated by sync-registry)
- `commands/*.md` — user commands outside `mustard/`
- `docs/`, `agent-memory/`, `spec/`, `plans/`

### `mustard auto-update`

| Option | Description |
|--------|-------------|
| `--check-only` | Only check for updates, do not install |
| `-y, --yes` | Skip confirmation prompts |

### `mustard add`

| Option | Description |
|--------|-------------|
| `-f, --force` | Overwrite existing files |

**Sources:**
- GitHub: `github.com/mustard-templates/{name}`
- npm: `mustard-template-{name}`

**Usage:**
```bash
mustard add template:dotnet-clean-arch
mustard add template:nextjs-app-router
```

### `mustard review`

| Option | Description |
|--------|-------------|
| `--pr <number>` | PR number to review (required) |
| `--ci` | CI mode: post as PR comment, exit 1 on critical issues |

**Requirements:** `gh` (GitHub CLI) and `claude` CLI must be installed.

**Usage:**
```bash
# Interactive review
mustard review --pr 42

# CI mode (for GitHub Actions)
mustard review --ci --pr 42
```

## What Gets Installed

```
.claude/
├── CLAUDE.md                          # Orchestrator rules (template)
├── pipeline-config.md                 # Agent dispatch config (template)
├── settings.json                      # Hooks + permissions + statusline
├── entity-registry.json               # Empty skeleton (populated by /scan)
├── commands/mustard/                  # Pipeline skills
│   ├── feature/SKILL.md               #   /feature — feature pipeline
│   ├── bugfix/SKILL.md                #   /bugfix — bug fix pipeline
│   ├── approve/SKILL.md               #   /approve — approve spec
│   ├── complete/SKILL.md              #   /complete — finalize pipeline
│   ├── resume/SKILL.md                #   /resume — resume pipeline
│   ├── scan/SKILL.md                  #   /scan — analyze codebase
│   ├── scan-format/SKILL.md           #   /scan agent format rules
│   ├── git/SKILL.md                   #   /git — commit, push, merge, deploy
│   ├── maint/SKILL.md                 #   /maint — deps, validate, sync
│   ├── task/SKILL.md                  #   /task — delegated analysis/review
│   ├── knowledge/SKILL.md             #   /knowledge — notes, audit, reports
│   ├── skill/SKILL.md                 #   /skill — manage skills
│   ├── status/SKILL.md                #   /status — project status
│   ├── stats/SKILL.md                 #   /stats — pipeline metrics
│   └── templates/agent-prompt/SKILL.md #  Agent prompt template
├── hooks/                             # Enforcement hooks
│   ├── _lib/hook-env.js               #   Shared runtime controls (profiles, env overrides)
│   ├── rtk-rewrite.js                 #   Rewrites Bash commands through RTK
│   ├── bash-safety.js                 #   Blocks dangerous commands
│   ├── file-guard.js                  #   Blocks sensitive file access
│   ├── enforce-registry.js            #   Blocks pipeline if no registry
│   ├── guard-verify.js                #   Validates architectural rules
│   ├── auto-format.js                 #   Auto-formats on write
│   ├── pre-compact.js                 #   Saves state before compaction
│   ├── session-cleanup.js             #   Cleans up on session end
│   ├── subagent-tracker.js            #   Tracks agent lifecycle
│   ├── session-memory.js              #   Injects persistent memory on session start
│   ├── review-gate.js                 #   Pre-commit validation (fail-open)
│   ├── metrics-tracker.js             #   Tracks pipeline API calls and retries
│   ├── mcp-budget.js                  #   Warns about excessive MCP tool counts
│   ├── session-knowledge.js           #   Extracts patterns from session before cleanup
│   └── __tests__/hooks.test.js        #   Hook tests
├── scripts/
│   ├── sync-detect.js                 #   Detects subprojects + roles
│   ├── sync-registry.js               #   Generates entity-registry.json
│   ├── statusline.js                  #   Claude Code statusline
│   ├── memory-persist.js              #   Persists decisions/lessons across sessions
│   ├── memory-write.js                #   Writes agent memory entries
│   ├── diff-context.js                #   Generates git diff summary for agents
│   ├── knowledge-update.js            #   Updates project knowledge base
│   ├── metrics-collect.js             #   Collects and displays pipeline metrics
│   ├── security-scan.js               #   Scans for secrets and security misconfigs
│   └── verify-pipeline.js             #   Runs build/test verification for pipeline
├── memory/                            # Persistent memory (auto-created)
│   ├── decisions.json                 #   Decisions across pipelines
│   └── lessons.json                   #   Lessons learned
├── knowledge.json                     # Evolutionary knowledge base
├── metrics/                           # Pipeline metrics archive
├── adapters/cursor/                   # Cursor IDE adapter (experimental)
│   ├── README.md                      #   Setup and usage guide
│   └── adapter.js                     #   Translates Cursor ↔ Claude Code hook protocol
└── skills/                            # Bundled skills
    ├── design-craft/                  #   UI design methodology
    ├── react-best-practices/          #   React/Next.js optimization (40+ rules)
    ├── senior-architect/              #   System architecture patterns
    ├── skill-creator/                 #   Create and optimize skills
    ├── commit-workflow/               #   Git commit strategy
    └── pipeline-execution/            #   Pipeline orchestration
```

## Pipeline Commands (inside Claude Code)

### Core Pipeline

| Command | Description |
|---------|-------------|
| `/scan` | Analyze codebase — generates guards, recipes, agents, skills |
| `/feature <name>` | Start feature pipeline (ANALYZE → PLAN → EXECUTE → CLOSE) |
| `/bugfix <error>` | Autonomous bug fix (diagnose → fix → validate) |
| `/approve` | Approve spec for implementation |
| `/resume` | Resume interrupted pipeline |
| `/complete` | Finalize or cancel pipeline |
| `/stats` | Show pipeline metrics and token savings |

### Operations

| Command | Description |
|---------|-------------|
| `/git <action>` | commit, push, merge, deploy (handles monorepo; `/git merge main` cascades from any branch) |
| `/maint <action>` | deps, validate, sync |
| `/status` | Git + pipeline + build + registry status |

### Analysis & Delegation

| Command | Description |
|---------|-------------|
| `/task analyze <scope>` | Code exploration (Explore agent) |
| `/task audit <domain> <scope>` | Quality audit (copy, design, a11y, i18n, api-contract) |
| `/task compare <criteria>` | Cross-subproject comparison |
| `/task review <scope>` | Code review (SOLID, security, perf) |
| `/task refactor <scope>` | Plan + approve + implement refactoring |
| `/task docs <scope>` | Documentation generation |

### Knowledge

| Command | Description |
|---------|-------------|
| `/knowledge notes [target]` | Manage project observations |
| `/knowledge audit` | Audit memory for duplicates |
| `/knowledge report daily/weekly` | Progress reports from git data |

### Skills

| Command | Description |
|---------|-------------|
| `/skill list` | List installed skills |
| `/skill install <source>` | Install from local path or GitHub |
| `/skill create <name>` | Create new skill via skill-creator |
| `/skill optimize <name>` | Optimize skill triggering |

## How `/scan` Works

`/scan` is the most important command. It runs inside Claude Code and:

1. **Detects subprojects** — reads git submodules or scans for `CLAUDE.md` files
2. **Incremental detection** — compares source hashes to skip unchanged subprojects
3. **Launches analysis agents** — one per subproject, in parallel
4. **Generates per-subproject**:
   - `{subproject}/CLAUDE.md` — stack, commands, guards
   - `{subproject}/.claude/commands/` — guards, recipes, patterns, modules
   - `{subproject}/.claude/skills/` — granular pattern skills
   - `.claude/agents/{subproject}-impl.md` — implementation agent
   - `.claude/agents/{subproject}-explorer.md` — read-only explorer
5. **Updates root files** — `CLAUDE.md`, `pipeline-config.md`, `entity-registry.json`

After `/scan`, the pipeline commands (`/feature`, `/bugfix`) have full context to dispatch specialized agents.

## Pipeline Flow

```
/feature <name>
     │
     ▼
  ANALYZE — read registry + pipeline-config, determine layers
     │
     ▼
  PLAN — create spec with tasks per agent (Light: inline, Full: /approve)
     │
     ▼
  EXECUTE — dispatch agents per wave (DB+Backend ∥, Frontend after or parallel if safe)
     │
     ▼
  REVIEW — mandatory review per subproject (SOLID, patterns, i18n, ...)
     │
     ▼
  CLOSE — sync registry, move spec, cleanup state
```

**Light scope** (≤5 files, known pattern): ANALYZE → EXECUTE → CLOSE in one session.
**Full scope** (3+ layers, new entity): ANALYZE → PLAN → `/approve` → new session → `/resume` → CLOSE.

## Token Economy

Mustard integrates [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) as core infrastructure to reduce token consumption:

- **Auto-install** — `mustard init` and `mustard update` install RTK silently if not present
- **Transparent hook** — a `PreToolUse` hook rewrites every Bash command through `rtk`, compressing output before it reaches Claude's context
- **Fail-open** — if RTK is not available, the hook passes through with zero impact
- **Statusline** — real-time token savings displayed in the Claude Code status bar
- **Pipeline report** — `/complete` shows total tokens saved at the end of each pipeline

| Command Type | Token Savings |
|-------------|--------------|
| `git status/diff/log` | 75-80% |
| `npm test` / `cargo test` | 90-99% |
| `git add/commit/push` | 92% |
| Build output | 80-90% |
| `ls` / `tree` / `grep` | 80% |

RTK only applies to Bash tool calls. Claude Code's built-in tools (Read, Grep, Glob) are already optimized and bypass the hook.

## Persistent Memory

Mustard maintains lightweight persistent memory across sessions:

- **Decisions** — architectural and implementation decisions are saved to `.claude/memory/decisions.json`
- **Lessons** — what went wrong and the correction applied, saved to `.claude/memory/lessons.json`
- **Knowledge base** — patterns, conventions, and entities discovered across pipelines, saved to `.claude/knowledge.json`

Memory is automatically:
- **Injected** into each new session and every dispatched agent
- **Capped** at 50 entries (memory) / 200 entries (knowledge) — oldest pruned
- **Never cleaned** by session cleanup — persists until manually removed

This gives 80% of the benefit of a database-backed system with zero infrastructure.

## CI Integration

Review PRs automatically in your CI pipeline:

```bash
# In GitHub Actions
mustard review --ci --pr ${{ github.event.pull_request.number }}
```

CI mode:
- Fetches PR diff via `gh pr diff`
- Runs Claude review with project guards and rules
- Posts review as PR comment
- Exits with code 1 if CRITICAL issues found (fails the build)

Requires `gh` and `claude` CLI in the CI environment.

## Supported Projects

Mustard is **framework-agnostic**. The CLI just copies templates. `/scan` handles detection:

| Type | Examples |
|------|---------|
| **Backend** | .NET, Node.js (Express/Fastify), Python (FastAPI/Django), Go, Rust, Java |
| **Frontend** | React, Next.js, Vue, Nuxt, Svelte, Angular |
| **Mobile** | Flutter/Dart |
| **Database** | Drizzle, Prisma, EF Core, TypeORM |
| **Monorepo** | Any combination of the above |
| **Single repo** | Any single project |

## Updating

### Update Mustard CLI

```bash
# Check if there's a new version
mustard auto-update --check-only

# Update to latest
mustard auto-update

# Or manually
npm install -g mustard-claude@latest
```

### Update Project Templates

After updating the CLI, update your project's `.claude/` files:

```bash
cd my-project
mustard update
```

This recreates core files (hooks, skills, scripts, commands) while preserving your customizations.

## Development

```bash
git clone https://github.com/rubensrpj/mustard.git
cd mustard
npm install
npm run build

# Test locally
node bin/mustard.js init
```

## License

MIT
