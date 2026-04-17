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

Mustard sets up a `.claude/` folder that turns Claude Code into a structured development pipeline with explicit phases (ANALYZE → PLAN → EXECUTE → REVIEW → CLOSE), wave-based agent dispatch, enforcement hooks, and token economy.

- **16 pipeline commands** — feature, bugfix, approve, complete, resume, scan, scan-format, git, maint, task, knowledge, skill, status, stats, metrics, review, plus the agent-prompt template
- **23 enforcement hooks** — bash safety, bash native redirect, file guard, registry enforcement, guard verify, auto-format, pre-compact, session cleanup, subagent tracker, RTK rewrite, session memory, review gate, metrics tracker, MCP budget, session knowledge, context budget, spec hygiene, output budget, tool-use counter, model routing gate, debug-loop guard, user-prompt hint, session-knowledge incremental
- **6 bundled skills** — design-craft, react-best-practices, senior-architect, skill-creator, commit-workflow, pipeline-execution
- **15 utility scripts** — subproject detection, entity registry sync, statusline, memory persist/write, diff context, knowledge update, metrics collect/report, security scan, pipeline verification, analyze validation, recipe matcher, skill generator
- **Token economy** — auto-installs [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) to reduce CLI-output tokens by 60–90%
- **Hook profiles & env overrides** — minimal/standard/strict profiles via `_lib/hook-env.js`; disable individual hooks with `MUSTARD_DISABLED_HOOKS`
- **Cursor IDE adapter** (experimental) — `mustard init --cursor` installs a Cursor-compatible hook adapter
- **Monorepo + single repo** — `sync-detect.js` auto-discovers subprojects and roles

## Quick Start

```bash
# Install globally
npm install -g mustard-claude

# Initialize your project
cd my-project
mustard init

# Open Claude Code and run /scan
```

After `/scan`, use `/feature`, `/bugfix`, or `/task` to work through structured pipelines.

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
2. RTK is auto-installed for token economy (60–90% savings on CLI outputs)
3. Inside Claude Code, run `/scan` to analyze your codebase
4. `/scan` generates guards, recipes, patterns, agents, and skills specific to your project
5. Use `/feature`, `/bugfix`, or `/task` to work through structured pipelines

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
- `commands/mustard/` — pipeline commands
- `hooks/` — enforcement hooks
- `skills/` — bundled skills
- `scripts/` — sync scripts
- `settings.json` — hook configuration

**Preserves** (user customizations):
- `CLAUDE.md` — orchestrator rules (populated by `/scan`)
- `pipeline-config.md` — agent dispatch config (populated by `/scan`)
- `entity-registry.json` — entity map (populated by sync-registry)
- `commands/*.md` — user commands outside `mustard/`
- `docs/`, `.agent-memory/`, `spec/`, `plans/`

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

```bash
mustard add template:dotnet-clean-arch
mustard add template:nextjs-app-router
```

### `mustard review`

| Option | Description |
|--------|-------------|
| `--pr <number>` | PR number to review (required) |
| `--ci` | CI mode: post as PR comment, exit 1 on critical issues |

**Requirements:** `gh` (GitHub CLI) and `claude` CLI.

```bash
# Interactive review
mustard review --pr 42

# CI mode (for GitHub Actions)
mustard review --ci --pr 42
```

## Pipeline Commands (inside Claude Code)

### Core Pipeline

| Command | Description |
|---------|-------------|
| `/scan` | Analyze codebase — generates guards, recipes, agents, skills |
| `/feature <name>` | Start feature pipeline (ANALYZE → PLAN → EXECUTE → REVIEW → CLOSE). Auto-detects Light / Extended-Light / Full scope. Prints full spec before asking for approval |
| `/bugfix <error>` | Autonomous bug fix (diagnose → fix → validate). Fast Path skips spec; Full Path writes and presents the spec before `/approve` |
| `/approve [--resume]` | Approve the active spec. With `--resume`, immediately chains into the `/resume` flow in the same session |
| `/resume` | Resume an interrupted pipeline from the last checkpoint |
| `/complete` | Finalize or cancel a pipeline |

### Operations

| Command | Description |
|---------|-------------|
| `/git <action>` | commit, push, sync, merge, deploy — handles monorepo git flow (`/git merge main` cascades from any branch) |
| `/maint <action>` | deps, validate, sync — maintenance utilities |
| `/status` | Consolidated git + pipeline + build + registry status |
| `/review [number\|url]` | Review a PR locally using the bundled review skill |

### Analysis & Delegation

| Command | Description |
|---------|-------------|
| `/task analyze <scope>` | Code exploration via Explore agent |
| `/task audit <domain> <scope>` | Quality audit (copy, design, a11y, i18n, consistency, api-contract) |
| `/task compare <criteria>` | Cross-subproject comparison (parallel explorers + consolidation) |
| `/task review <scope>` | Code review (SOLID, security, performance) |
| `/task refactor <scope>` | Plan + approve + implement refactoring (prints full plan before approval) |
| `/task docs <scope>` | Documentation generation |
| `/task implement <scope>` | Single-dispatch standardized implementation (low-cost, no audit gate) |

### Metrics & Knowledge

| Command | Description |
|---------|-------------|
| `/stats` | Pipeline metrics, token savings, performance |
| `/metrics` | Enforcement metrics report — hook hit rates, budget distributions, gate activity |
| `/knowledge notes [target]` | Manage project observations |
| `/knowledge audit` | Audit memory for duplicates |
| `/knowledge report daily\|weekly` | Progress reports from git data |

### Skills

| Command | Description |
|---------|-------------|
| `/skill list` | List installed skills |
| `/skill install <source>` | Install skill from local path or GitHub |
| `/skill create <name>` | Create new skill via skill-creator |
| `/skill optimize <name>` | Optimize skill triggering descriptions |

## What Gets Installed

```
.claude/
├── CLAUDE.md                          # Orchestrator rules (template)
├── pipeline-config.md                 # Agent dispatch config (template)
├── settings.json                      # Hooks + permissions + statusline
├── entity-registry.json               # Empty skeleton (populated by /scan)
├── commands/mustard/                  # Pipeline commands
│   ├── feature/SKILL.md               #   /feature — feature pipeline
│   ├── bugfix/SKILL.md                #   /bugfix — bug fix pipeline
│   ├── approve/SKILL.md               #   /approve [--resume] — approve spec
│   ├── complete/SKILL.md              #   /complete — finalize pipeline
│   ├── resume/SKILL.md                #   /resume — resume pipeline
│   ├── scan/SKILL.md                  #   /scan — analyze codebase
│   ├── scan-format/SKILL.md           #   /scan agent format rules
│   ├── git/SKILL.md                   #   /git — commit, push, merge, deploy
│   ├── maint/SKILL.md                 #   /maint — deps, validate, sync
│   ├── task/SKILL.md                  #   /task — delegated analysis/review/refactor
│   ├── knowledge/SKILL.md             #   /knowledge — notes, audit, reports
│   ├── skill/SKILL.md                 #   /skill — manage skills
│   ├── status/SKILL.md                #   /status — consolidated status
│   ├── stats/SKILL.md                 #   /stats — pipeline metrics
│   ├── metrics/SKILL.md               #   /metrics — enforcement metrics report
│   ├── review/SKILL.md                #   /review — PR review
│   └── templates/agent-prompt/        #   Agent prompt template
├── hooks/                             # Enforcement hooks (23)
│   ├── _lib/hook-env.js               #   Shared runtime controls (profiles, env overrides)
│   ├── rtk-rewrite.js                 #   Rewrites Bash commands through RTK
│   ├── bash-safety.js                 #   Blocks dangerous commands
│   ├── bash-native-redirect.js        #   Redirects grep/ls/cat/find → native tools
│   ├── file-guard.js                  #   Blocks sensitive file access
│   ├── enforce-registry.js            #   Blocks pipeline if no registry
│   ├── guard-verify.js                #   Validates architectural rules
│   ├── auto-format.js                 #   Auto-formats on write
│   ├── pre-compact.js                 #   Saves state before compaction
│   ├── session-cleanup.js             #   Cleans transient state on session end
│   ├── subagent-tracker.js            #   Tracks agent lifecycle, logs dispatch failures
│   ├── session-memory.js              #   Injects persistent memory on session start
│   ├── review-gate.js                 #   Pre-commit validation (fail-open)
│   ├── metrics-tracker.js             #   Tracks pipeline API calls, retries, gate saves
│   ├── mcp-budget.js                  #   Warns on excessive MCP tool counts
│   ├── session-knowledge.js           #   Extracts patterns from session before cleanup
│   ├── session-knowledge-inc.js       #   Incremental knowledge capture
│   ├── context-budget.js              #   Enforces context budget per agent
│   ├── spec-hygiene.js                #   Guards spec format + checkbox integrity
│   ├── output-budget.js               #   Caps agent return-size
│   ├── tool-use-counter.js            #   Caps Explore agents at 20 tool uses
│   ├── model-routing-gate.js          #   Blocks model upgrades vs routing table
│   ├── debug-loop-guard.js            #   Detects iteration anti-patterns
│   └── user-prompt-hint.js            #   Surfaces contextual hints on prompt input
├── scripts/                           # Utility scripts (15)
│   ├── sync-detect.js                 #   Detects subprojects + roles (SHA-256 incremental)
│   ├── sync-registry.js               #   Generates entity-registry.json
│   ├── statusline.js                  #   Claude Code statusline
│   ├── memory-persist.js              #   Persists decisions/lessons across sessions
│   ├── memory-write.js                #   Writes agent memory entries between waves
│   ├── diff-context.js                #   Generates git diff summary for agents
│   ├── knowledge-update.js            #   Updates project knowledge base
│   ├── metrics-collect.js             #   Collects pipeline metrics
│   ├── metrics-report.js              #   Renders enforcement metrics report
│   ├── security-scan.js               #   Scans for secrets / security misconfigs
│   ├── verify-pipeline.js             #   Runs build/test verification
│   ├── analyze-validation.js          #   Validates ANALYZE phase output
│   ├── recipe-match.js                #   Structured recipe matcher (entity + operation)
│   ├── skill-generator.js             #   Generates subproject pattern skills
│   └── _metrics-write.js              #   Internal metrics writer (used by hooks)
├── memory/                            # Persistent memory (auto-created)
│   ├── decisions.json                 #   Decisions across pipelines
│   └── lessons.json                   #   Lessons learned
├── .agent-memory/                     # Wave-to-wave agent handoff memory
├── .pipeline-states/                  # Active pipeline state + diff snapshots
├── spec/
│   ├── active/                        #   In-progress specs
│   └── completed/                     #   Archived specs
├── knowledge.json                     # Evolutionary knowledge base
├── metrics/                           # Pipeline metrics archive
├── adapters/cursor/                   # Cursor IDE adapter (experimental)
│   ├── README.md                      #   Setup and usage guide
│   └── adapter.js                     #   Cursor ↔ Claude Code hook protocol adapter
└── skills/                            # Bundled skills (6)
    ├── design-craft/                  #   UI design methodology
    ├── react-best-practices/          #   React/Next.js optimization (40+ rules)
    ├── senior-architect/              #   System architecture patterns
    ├── skill-creator/                 #   Create and optimize skills
    ├── commit-workflow/               #   Git commit strategy
    └── pipeline-execution/            #   Pipeline phases, waves, retries
```

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

After `/scan`, pipeline commands have full context to dispatch specialized agents.

## Pipeline Flow

```
/feature <name>
     │
     ▼
  ANALYZE — read registry + pipeline-config, determine layers, classify scope
     │
     ▼
  PLAN — create spec with tasks per agent, print spec inline for user review
     │
     ▼ (user approves — inline on Light, /approve on Full, /approve --resume to chain)
     │
  EXECUTE — dispatch agents per wave (DB+Backend ∥, Frontend after or parallel-safe)
     │
     ▼
  REVIEW — mandatory per-subproject review (SOLID, patterns, i18n, design, …)
     │
     ▼
  CLOSE — sync registry, move spec to completed/, cleanup state
```

### Scope detection

| Scope | Signals | Flow |
|-------|---------|------|
| **Light** | 1–2 layers, ≤5 files, known pattern, no new entity | ANALYZE → EXECUTE → CLOSE (one session) |
| **Extended Light** | Entity in registry + modification + ≤8 files, no new table/enum | Same as Light |
| **Full** | 3+ layers, 5+ files, new entity/CRUD, or new pattern | ANALYZE → PLAN → `/approve` → EXECUTE → CLOSE |

### Approval & resume shortcut

- `/approve` — prepares state and STOPS. User opens a new session and runs `/resume` (clean context).
- `/approve --resume` — prepares state and immediately continues to EXECUTE in the same session (skips the session hop; trades context freshness for convenience).

## Enforcement Hooks

Hooks are fail-open (any hook error exits 0 — pipeline is never blocked by a hook bug). They follow the PreToolUse / PostToolUse / Subagent / Session lifecycle.

### Cost-optimization hooks

| Hook | Matcher | Mode | Effect |
|------|---------|------|--------|
| `bash-native-redirect.js` | Bash | strict/warn/off | Blocks grep/ls/cat/head/tail/find — suggests Grep/Glob/Read |
| `model-routing-gate.js` | Task | strict/warn/off | Blocks model upgrades vs routing table |
| `tool-use-counter.js` | `.*` + Subagent | hard | Caps Explore agents at 20 tool uses (warn at 12) |
| `context-budget.js` | Task | warn | Enforces context budget per agent dispatch |
| `output-budget.js` | Subagent | warn | Caps agent return size |
| `mcp-budget.js` | startup | warn | Flags excessive MCP tool counts |
| `rtk-rewrite.js` | Bash | transparent | Routes CLI output through RTK for 60–90% token reduction |

### Safety & validation hooks

| Hook | Purpose |
|------|---------|
| `bash-safety.js` | Blocks dangerous commands (`rm -rf /`, force-push to main, etc.) |
| `file-guard.js` | Blocks access to `.env`, secrets, credentials |
| `enforce-registry.js` | Blocks pipeline commands if `entity-registry.json` is missing |
| `guard-verify.js` | Validates architectural rules during EXECUTE |
| `review-gate.js` | Pre-commit validation (fail-open) |
| `spec-hygiene.js` | Guards spec format + checkbox integrity |
| `debug-loop-guard.js` | Detects retry/iteration anti-patterns |

### Memory & telemetry hooks

| Hook | Purpose |
|------|---------|
| `session-memory.js` | Injects persistent memory on session start |
| `session-cleanup.js` | Cleans transient state on session end |
| `session-knowledge.js` | Extracts patterns from session before cleanup |
| `session-knowledge-inc.js` | Incremental knowledge capture |
| `pre-compact.js` | Saves state before Claude's auto-compaction |
| `subagent-tracker.js` | Tracks agent lifecycle, logs dispatch failures |
| `metrics-tracker.js` | Pipeline telemetry (API calls, retries, gate saves) |
| `auto-format.js` | Auto-formats on Write/Edit |
| `user-prompt-hint.js` | Surfaces contextual hints on prompt input |

### Environment overrides

```bash
# Change mode (default: strict for cost-optimization hooks)
MUSTARD_BASH_REDIRECT_MODE=warn
MUSTARD_MODEL_GATE_MODE=off

# Disable individual hooks
MUSTARD_DISABLED_HOOKS=bash-native-redirect,model-routing-gate

# Switch profile
MUSTARD_PROFILE=minimal   # minimal | standard (default) | strict
```

See `.claude/hooks/_lib/hook-env.js` for the full list.

## Token Economy

Mustard integrates [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) as core infrastructure:

- **Auto-install** — `mustard init` and `mustard update` install RTK silently if not present
- **Transparent hook** — `rtk-rewrite.js` (PreToolUse:Bash) wraps every Bash command through `rtk`, compressing output before it enters Claude's context
- **Fail-open** — if RTK is not available, the hook passes through with zero impact
- **Statusline** — real-time token savings displayed in the Claude Code status bar
- **Pipeline report** — `/complete` and `/stats` show total tokens saved

| Command type | Typical savings |
|--------------|-----------------|
| `git status/diff/log` | 75–80% |
| `npm test` / `cargo test` | 90–99% |
| `git add/commit/push` | 92% |
| Build output (`next build`, `tsc`, `cargo build`) | 80–90% |
| `ls` / `tree` / `grep` | 80% |

RTK only applies to Bash tool calls. Claude Code's built-in tools (Read, Grep, Glob) are already optimized and bypass the hook.

## Persistent Memory

Mustard maintains lightweight persistent memory across sessions:

- **Decisions** (`.claude/memory/decisions.json`) — architectural and implementation decisions
- **Lessons** (`.claude/memory/lessons.json`) — what went wrong and the correction applied
- **Knowledge base** (`.claude/knowledge.json`) — patterns, conventions, entities discovered across pipelines
- **Agent memory** (`.claude/.agent-memory/`) — wave-to-wave handoff summaries within a pipeline
- **Pipeline state** (`.claude/.pipeline-states/`) — active pipeline state + per-phase diff snapshots

Memory is automatically:
- **Injected** into each new session and into every dispatched agent
- **Capped** (50 entries memory / 200 entries knowledge — oldest pruned)
- **Never cleaned** by session cleanup — persists until manually removed

## CI Integration

Review PRs automatically in your CI pipeline:

```yaml
# GitHub Actions
- name: Mustard PR Review
  run: mustard review --ci --pr ${{ github.event.pull_request.number }}
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
|------|----------|
| **Backend** | .NET, Node.js (Express/Fastify/NestJS), Python (FastAPI/Django), Go, Rust, Java (Spring Boot) |
| **Frontend** | React, Next.js, Vue, Nuxt, Svelte, Angular |
| **Mobile** | Flutter/Dart, React Native |
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

# Run hook tests
node --test templates/hooks/__tests__/hooks.test.js

# Test locally
node bin/mustard.js init
```

## License

MIT
