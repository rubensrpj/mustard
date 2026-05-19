# CLAUDE.md

Instructions for Claude Code when working with this repository.

> **Em português simples (1 parágrafo):** Mustard é uma "configuração pronta" para Claude Code. Quando você roda `mustard init` num projeto, ele cria a pasta `.claude/` com tudo que a IA precisa para trabalhar como um sênior: pipeline em fases (pesquisa → plano → execução → QA → fechamento), regras automáticas que evitam erros comuns (não rodar `rm -rf`, não passar de 40% da janela de contexto, não esquecer de testar, etc.), e um sistema que aprende com cada sessão. Esta página é a "primeira leitura" que a IA faz ao abrir o projeto — por isso é técnica de propósito.

## Project

Mustard is a CLI that generates `.claude/` folders for Claude Code projects. It creates prompts, commands, hooks, and rules. The repo is a **pnpm + Cargo monorepo**: the CLI lives in `packages/cli/` (npm package `mustard-claude`), and a companion Tauri 2 + React 19 desktop dashboard lives in `apps/dashboard/`. The root `.claude/` is the monorepo orchestrator.

**Key concepts:**

- "Agents" are prompts loaded into `Task(general-purpose)` - custom subagent types don't work
- Only 4 native `subagent_type` values: `Explore`, `Plan`, `general-purpose`, `Bash`
- Enforcement via JavaScript hooks (`PreToolUse`/`PostToolUse`/`SessionStart`/`PreCompact`/`SessionEnd`/`SubagentStart`/`SubagentStop`/`UserPromptSubmit`)
- **Universal Delegation**: All code activities must be delegated via Task (separate context)
- **Skill+Recipe-driven context**: agents auto-load skills by description; recipes inject 90% skeletons by entity+operation
- **Auto-sync Scripts**: `sync-detect.js`, `sync-registry.js`
- **Namespaced Commands**: All commands use `mustard:` prefix (e.g., `/mustard:feature`)
- **Canonical methodology mapping**: ANALYZE↔Research, PLAN↔Spec+Plan, EXECUTE↔Implement (cf. [GitHub Spec Kit](https://github.com/github/spec-kit) RPI loop)

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

This repo is a **pnpm + Cargo monorepo**. The CLI lives in `packages/cli/`; the Tauri dashboard in `apps/dashboard/`.

```bash
# Install all workspace deps (run at repo root)
pnpm install

# Build / test the CLI
pnpm --filter mustard-claude build
pnpm --filter mustard-claude test

# Build the dashboard
pnpm --filter mustard-dashboard build

# Initialize a project (run from packages/cli/)
bun packages/cli/bin/mustard.js init

# Update existing project
bun packages/cli/bin/mustard.js update
```

## Structure

```text
mustard/                         # monorepo root — pnpm-workspace.yaml + Cargo.toml
├── .claude/                     # root orchestrator (pipelines, hooks, registry)
├── packages/
│   └── cli/                     # the Mustard CLI (npm pkg: mustard-claude)
│       ├── bin/mustard.js       # CLI entry point
│       ├── src/                 # TypeScript source
│       │   ├── commands/        # init.ts, update.ts, config.ts, review.ts, add.ts
│       │   ├── scanners/        # stack.ts, structure.ts, dependencies.ts
│       │   ├── generators/      # claude-md, prompts, commands, hooks, registry
│       │   └── services/        # npm.ts
│       ├── dist/                # Compiled JavaScript
│       ├── tests/               # unit / integration / bench
│       └── templates/           # Templates (copied to target .claude/)
│           ├── CLAUDE.md        # Orchestrator rules (auto-loaded by Claude Code)
│           ├── settings.json    # Hook wiring + permissions + env modes
│           ├── pipeline-config.md
│           ├── commands/mustard/ # 18 namespaced slash commands
│           ├── skills/          # 7 foundation skills (karpathy, design-craft, etc.)
│           ├── refs/            # Progressive-disclosure refs (loaded on demand)
│           ├── recipes/         # Structured recipes (90% skeletons)
│           ├── context/qa/      # QA agent core context (only static .core.md kept)
│           ├── scripts/         # 28 utility scripts (sync-*, event-projections, qa-run, etc.)
│           └── hooks/           # 31 JavaScript hooks (fail-open, no npm deps)
│               └── _lib/        # Shared runtime: hook-env.js, harness-event.js, metrics-emit.js
└── apps/
    └── dashboard/               # Tauri 2 + React 19 desktop dashboard (mustard-dashboard)
        ├── src/                 # React/TypeScript UI
        └── src-tauri/           # Rust backend
```

## Context Architecture

Mustard uses **skill+recipe-driven context loading** — agents receive context lazily, not from monolithic files.

### Loading sources (in order of preference)

| Source | Where | Loaded |
|---|---|---|
| **Project root rules** | `{root}/CLAUDE.md` | Auto, every session |
| **Subproject guards** | `{subproject}/CLAUDE.md` | Auto when working in subproject |
| **Foundation skills** | `templates/skills/{name}/SKILL.md` | Auto via skill description match |
| **Subproject patterns** | `{subproject}/.claude/skills/` | Auto via skill description match |
| **Recipes (structured)** | `.claude/recipes/{operation}.json` | Matched by `recipe-match.js --entity --operation` |
| **Refs (progressive)** | `templates/refs/{cmd}/*.md` | Read on-demand by commands |
| **Stack/Modules** | `{subproject}/.claude/commands/{stack,patterns,guards,recipes,notes}.md` | Read on-demand |
| **Entity registry** | `.claude/entity-registry.json` | Grep by entity name |
| **QA core** | `templates/context/qa/qa.core.md` | Loaded by `/mustard:qa` |

### Methodology mapping (PRD ↔ Mustard)

| PRD term | Mustard phase | Reference |
|---|---|---|
| Research | ANALYZE | [GitHub Spec Kit](https://github.com/github/spec-kit) |
| Spec + Plan | PLAN | [Martin Fowler — SDD-3-tools](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html) |
| Implement | EXECUTE | — |
| Acceptance | QA (Wave 10) | runnable AC commands |
| Close | CLOSE | sync registry + move spec |

### Sync flow (auto-discovery, monorepo-aware)

1. User invokes `/mustard:feature` or `/mustard:bugfix`
2. `sync-detect.js` discovers subprojects + roles
3. `sync-registry.js` scans entities (Drizzle/EF/Prisma/TypeORM/etc.)
4. Pipeline reads `entity-registry.json` for known entities
5. SHA256 hash skips recompilation when content unchanged

## CLI Flow

```text
mustard init
    -> scanProject() - detect stacks
    -> generateAll() - create .claude/ files + context structure
    -> generateMustardJson() - git flow config (interactive)

mustard update
    -> backup existing .claude/
    -> regenerate core files only
    -> preserve: CLAUDE.md, prompts/, context/*.md, mustard.json (user files)
```

## Model routing (`model-routing-gate.js`)

Models are auto-selected by intent. Upgrades blocked, downgrades allowed (opt-in via env).

| Intent | Model | Why |
|---|---|---|
| Explore (mechanical search) | haiku | cheap, fast, no reasoning needed |
| Plan | opus | bad plan = bad implementation |
| Feature pipeline (any) | opus | quality-first |
| Bugfix pipeline | opus | diagnosis needs deep reasoning |
| Default | sonnet | safe baseline |

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

### Git (reads `mustard.json` for branch flow)

- `/mustard:git sync` - Pull parent branch into current
- `/mustard:git commit` - Simple commit
- `/mustard:git push` - Sync + commit + push
- `/mustard:git merge` - Promote to parent (local ff-only, no PRs)
- `/mustard:git merge main` - Promote dev → main (explicit)
- `/mustard:review [number|url]` - Review PR via Claude code-review

### Sync

- `/mustard:sync-registry` - Update entity registry
- `/mustard:sync-context` - Compile agent contexts
- `/mustard:validate` - Build + type-check
- `/mustard:status` - Project status

## Enforcement Hooks (highlights)

31 hooks wired in `templates/settings.json`. Highlights below — full list at `templates/settings.json` and behavioral docs at `templates/pipeline-config.md`.

| Hook | Matcher | Behavior |
|------|---------|----------|
| `bash-native-redirect.js` | `Bash` | **BLOCKS** grep/ls/cat/head/tail/find → native tools |
| `bash-safety.js` | `Bash` | **BLOCKS** rm -rf, mkfs, dd, credentials access |
| `model-routing-gate.js` | `Task` | **BLOCKS** upgrades vs routing table (downgrades allowed) |
| `tool-use-counter.js` | `.*` + Subagent | **BLOCKS** Explore agents at 15-20 tool uses (warn at 12) |
| `context-budget.js` | `Task` | **BLOCKS** Task prompts >per-role budget (Explore 10K chars, review 12K, general 30K); advisory >40% model window (Dumb Zone) |
| `output-budget.js` | `Task` | **WARNS** when agent return >per-role line cap (advisory) |
| `close-gate.js` | `Write\|Edit` to pipeline-states | **BLOCKS** CLOSE if build/lint/test/QA fail or checklist incomplete |
| `enforce-registry.js` | `Skill` | **BLOCKS** /feature, /bugfix if registry missing |
| `spec-size-gate.js` | `Write\|Edit` | **WARNS** specs >500 lines (strict block opt-in) |
| `skill-validate-gate.js` | `Write\|Edit` | **VALIDATES** skill YAML frontmatter |
| `review-gate.js` | `Bash git commit` | **WARNS** secrets staged or build broken |
| `auto-format.js` | `Write\|Edit` (PostToolUse) | Auto-formats by extension (Prettier/Black/etc.) |
| `checklist-auto-mark.js` | `Write\|Edit` (PostToolUse) | Auto-marks Checklist items when matching file edited |
| `memory-auto-extract.js` | `SessionEnd` | **EXTRACTS** Decisões não-óbvias from active specs → `memory/decisions.json` |
| `session-knowledge.js`/`-inc` | `SessionEnd` / `PostToolUse(Task)` | **EXTRACTS** patterns from pipeline-states; throttled 3/h, idempotency 24h |
| `session-memory.js` | `SessionStart` | **INJECTS** knowledge.json + cross-session timeline |

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
    Pipeline starts...
```

## Sync Scripts

### sync-detect.js

Auto-discovers subprojects in monorepos:

- Detection patterns: `.NET`, `React`, `Drizzle`, etc.
- Output: JSON with subprojects, agents, paths

### sync-registry.js

Generates `entity-registry.json` v3.1:

- Scans Drizzle schemas (`pgTable`, `pgEnum`)
- Scans .NET entities (`DbSet`, `class T`)
- Outputs `_patterns`, `_enums`, entity refs/subs

### security-scan.js

Scans for secrets, env exposure, and security misconfigurations:

- Detects leaked credentials, API keys, tokens
- Checks `.env` exposure and insecure patterns
- Reports findings with severity levels

### verify-pipeline.js

Runs build/test verification for the active pipeline:

- Executes build and test commands
- Reports pass/fail status per subproject
- Used during pipeline EXECUTE/CLOSE phases

## Project Structure

Monorepo (`pnpm-workspace.yaml`: `packages/*`, `apps/*` + `Cargo.toml` workspace).

| Subproject | Path | Technology | CLAUDE.md |
|------------|------|------------|-----------|
| CLI | `packages/cli/` | TypeScript / Bun — `mustard-claude` npm package; hooks, scripts, commands under `templates/` | [templates](./packages/cli/templates/CLAUDE.md) |
| Dashboard | `apps/dashboard/` | Tauri 2 + React 19 + Tailwind 4 desktop app — `mustard-dashboard` | [dashboard](./apps/dashboard/CLAUDE.md) |

## Entity Registry

**CRITICAL:** Before searching for ANY entity, read `.claude/entity-registry.json` first.

## Ignore Paths

Never search in:
- `node_modules/`, `.next/`, `bin/`, `obj/`, `dist/`, `migrations/`

## Stacks Detected

TypeScript/JS, C#, Python, Java, Go, Rust, React, Next.js, .NET, FastAPI, Django, Drizzle, Prisma, TypeORM
