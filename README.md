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
  <img src="https://img.shields.io/badge/Monorepo-supported-green?style=flat-square" alt="Monorepo">
  <img src="https://img.shields.io/badge/Single_Repo-supported-green?style=flat-square" alt="Single Repo">
</p>

---

## What is Mustard?

Mustard sets up a `.claude/` folder that turns Claude Code into a structured development pipeline:

- **14 pipeline skills** — feature, bugfix, scan, resume, approve, complete, git, maint, task, knowledge, skill, status, scan-format, agent-prompt template
- **8 enforcement hooks** — bash safety, file guard, registry validation, guard verification, auto-format, pre-compact, session cleanup, subagent tracking
- **6 bundled skills** — design-craft, react-best-practices, senior-architect, skill-creator, commit-workflow, pipeline-execution
- **3 sync scripts** — subproject detection, entity registry sync, statusline
- **Monorepo + single repo** — works with any project structure

## How It Works

1. `mustard init` copies the `.claude/` structure into your project
2. Inside Claude Code, run `/scan` to analyze your codebase
3. `/scan` generates guards, recipes, patterns, agents, and skills specific to your project
4. Use `/feature`, `/bugfix`, `/task` to work through structured pipelines

The CLI is a **one-time setup tool**. All intelligence lives in the skills and hooks inside `.claude/`.

## Installation

### Prerequisites

- **Node.js** >= 18.0.0

### Install

```bash
# Global
npm install -g mustard-claude

# Or run without installing
npx mustard-claude init
```

### Initialize a Project

```bash
cd my-project
mustard init
```

That's it. Open Claude Code and run `/scan`.

## CLI Commands

```
mustard init [options]     Copy .claude/ structure into current project
mustard update [options]   Update Mustard core files (preserves user customizations)
mustard auto-update        Check npm for newer version and install
```

### `mustard init`

| Option | Description |
|--------|-------------|
| `-f, --force` | Overwrite existing `.claude/` without backup |
| `-y, --yes` | Skip confirmation prompts (merge mode: skip existing files) |

**Behavior:**
- If `.claude/` doesn't exist → copies all templates
- If `.claude/` exists → asks: backup & overwrite, merge (skip existing), or cancel
- Merge mode preserves all existing files and only adds new ones

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
│   └── templates/agent-prompt/SKILL.md #  Agent prompt template
├── hooks/                             # Enforcement hooks
│   ├── bash-safety.js                 #   Blocks dangerous commands
│   ├── file-guard.js                  #   Blocks sensitive file access
│   ├── enforce-registry.js            #   Blocks pipeline if no registry
│   ├── guard-verify.js                #   Validates architectural rules
│   ├── auto-format.js                 #   Auto-formats on write
│   ├── pre-compact.js                 #   Saves state before compaction
│   ├── session-cleanup.js             #   Cleans up on session end
│   ├── subagent-tracker.js            #   Tracks agent lifecycle
│   └── __tests__/hooks.test.js        #   Hook tests
├── scripts/
│   ├── sync-detect.js                 #   Detects subprojects + roles
│   ├── sync-registry.js               #   Generates entity-registry.json
│   └── statusline.js                  #   Claude Code statusline
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

### Operations

| Command | Description |
|---------|-------------|
| `/git <action>` | commit, push, merge, deploy (handles monorepo) |
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
  EXECUTE — dispatch agents per wave (DB+Backend ∥, Frontend after)
     │
     ▼
  REVIEW — mandatory review per subproject (SOLID, patterns, i18n, ...)
     │
     ▼
  CLOSE — sync registry, move spec, cleanup state
```

**Light scope** (≤5 files, known pattern): ANALYZE → EXECUTE → CLOSE in one session.
**Full scope** (3+ layers, new entity): ANALYZE → PLAN → `/approve` → new session → `/resume` → CLOSE.

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

## Development

```bash
git clone https://github.com/Competi/mustard.git
cd mustard
npm install
npm run build

# Test locally
node bin/mustard.js init
```

## License

MIT
